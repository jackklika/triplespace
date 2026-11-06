//! `triplespace sync`: a provider's dump into its mirror partition (0002 §8.4), through the
//! adapter its registry entry names (0091 §4). Every Wikibase provider names
//! `scatter-adapter-wikibase`, parameterised by the entry; no other adapter is built yet.
//!
//! A first pass over the dump comes when the provider has key-map rules (0091 §2), since
//! a value pointing at `Q9` can only become `domain:example.org` once every item is
//! indexed, or when the dump is XML: its stored JSON has no `datatype` on snaks, so the
//! pass collects the property types from the dump's own property pages (as `adopt` does,
//! 0035 §3), and its redirects, so that a merge into a mapped item becomes an alias of the
//! key (0009 A13). A JSON dump of a provider without key maps is read once.
//!
//! Every reject is printed to stderr (`reject Q48: …`) and, with `--rejects`, written to a
//! tab-separated file: the job record keeps only the first 100 (0010 §9).

use std::cell::Cell;
use std::collections::VecDeque;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::rc::Rc;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_adapter_wikibase::{
    DumpEntry, DumpPropertyTypes, Entities, Entries, Index, KeyMappedAdapter, KeyRules, Outcome,
};
use scatter_ingest::sync::{SyncItem, run_sync_in};
use scatter_ingest::write::Attestation;
use scatter_ingest::{IngestError, Prepared, run_bootstrap};
use scatter_providers::{Provider, Registry};
use scatter_wikibase_changeset::{JobHeader, Mode, Upstream, WikibaseAdapter};
use triplespace_api_ingest::follow::{KIND_PROVIDER, KIND_SOURCE, record_load};

use crate::follow::tenant_registry;

use crate::common::{connect, farm_of, now, store};

/// `sync`.
#[derive(Debug, Args)]
pub struct Sync {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The provider slug (`internetdomains`, `wikidata`, …).
    #[arg(long, conflicts_with_all = ["tenant", "source"])]
    pub provider: Option<String>,
    /// The tenant whose entity source to load (0078 §4; 0092 §4), with `--source`.
    #[arg(long, requires = "source")]
    pub tenant: Option<String>,
    /// The entity source's name, with `--tenant`.
    #[arg(long, requires = "tenant")]
    pub source: Option<String>,
    /// The dump: a Wikibase JSON dump or a MediaWiki XML dump (`.json`, `.xml`, `.gz`, `.bz2`).
    #[arg(long)]
    pub dump: PathBuf,
    /// The dump's identity, the source version (`20261001`); the file name by default.
    #[arg(long)]
    pub version: Option<String>,
    /// The dump is complete: entities it does not carry are tombstoned afterwards.
    #[arg(long)]
    pub snapshot: bool,
    /// The most entities a snapshot sweep may tombstone before the job fails instead.
    #[arg(long, default_value_t = 1000)]
    pub threshold: usize,
    /// Entities per unit of work.
    #[arg(long, default_value_t = 200)]
    pub batch: usize,
    /// Writes every reject to this file, one `upstream ID<TAB>reason` line each.
    #[arg(long)]
    pub rejects: Option<PathBuf>,
    /// Loads through the ordinary sync even where bootstrap mode applies (an empty
    /// instance mirror, no key map, a JSON dump; 0013 §9).
    #[arg(long)]
    pub no_bootstrap: bool,
}

/// What a dump yields for the sync: its items in dump order, read as the sync asks for
/// them, so a dump of any size is held one entity at a time (`run_sync_in` takes any
/// iterator). The first pass, when there is one, has already run; it keeps the key index
/// and the property types, not the entities.
pub struct SyncItems {
    entries: Entries<Box<dyn BufRead + Send>>,
    adapter: KeyMappedAdapter,
    types: DumpPropertyTypes,
    /// Whether the first pass counted the unreadable entries, so this one skips them.
    counted: bool,
    /// Items made from one entry and not yet handed over: a reject and its state.
    pending: VecDeque<SyncItem>,
    unreadable: Rc<Cell<u64>>,
    done: bool,
}

impl SyncItems {
    /// Entities the dump reader or the adapter could not read so far: all of them once
    /// the iterator is exhausted.
    pub fn unreadable(&self) -> u64 {
        self.unreadable.get()
    }

    /// A handle on [`Self::unreadable`] that outlives the iterator, for reading the count
    /// after the sync has consumed it.
    pub fn unreadable_counter(&self) -> Rc<Cell<u64>> {
        Rc::clone(&self.unreadable)
    }

    fn unreadable_one(&self) {
        self.unreadable.set(self.unreadable.get() + 1);
    }

    /// The items one dump entry makes, queued in `pending`.
    fn read(&mut self, entry: Result<DumpEntry, scatter_adapter_wikibase::DumpError>) {
        let mut parsed = match entry {
            Ok(DumpEntry::Entity(p)) => *p,
            Ok(DumpEntry::Redirect(r)) => {
                let upstream = Upstream::revision(r.revid, Some(&r.timestamp));
                match self
                    .adapter
                    .sync_redirect(r.from.as_str(), r.to.as_str(), upstream)
                {
                    Ok(Some((from, to, upstream))) => {
                        self.pending
                            .push_back(SyncItem::Redirected { from, to, upstream });
                    }
                    Ok(None) => {}
                    Err(e) => {
                        self.unreadable_one();
                        eprintln!("adapter error: {} → {}: {e}", r.from, r.to);
                    }
                }
                return;
            }
            Err(e) => {
                // Counted by the first pass when there was one.
                if !self.counted {
                    self.unreadable_one();
                    eprintln!("dump error: {e}");
                }
                return;
            }
        };
        self.types.apply(&mut parsed.entity);
        match self.adapter.sync_item(parsed) {
            Ok(Outcome::State {
                entity,
                upstream,
                upstream_id,
                upstream_aliases,
            }) => self.pending.push_back(SyncItem::State {
                entity,
                upstream,
                upstream_id,
                upstream_aliases,
            }),
            Ok(Outcome::Rejected {
                entity,
                upstream,
                upstream_id,
                reason,
            }) => {
                self.pending.push_back(SyncItem::Rejected {
                    upstream_id,
                    reason,
                });
                self.pending.push_back(SyncItem::State {
                    entity,
                    upstream,
                    upstream_id: None,
                    upstream_aliases: Vec::new(),
                });
            }
            Err(e) => {
                self.unreadable_one();
                eprintln!("adapter error: {e}");
            }
        }
    }
}

impl Iterator for SyncItems {
    type Item = SyncItem;

    fn next(&mut self) -> Option<SyncItem> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Some(item);
            }
            if self.done {
                return None;
            }
            if let Some(entry) = self.entries.next() {
                self.read(entry);
            } else {
                self.done = true;
                if let Some(line) = self.types.untyped_summary() {
                    eprintln!("  {line}");
                }
            }
        }
    }
}

/// Reads a Wikibase provider's dump through `scatter-adapter-wikibase` with the provider's
/// key-map rules: two passes when the rules or the format need them, one otherwise
/// (0091 §4). The first pass runs here; the second runs as the items are taken.
pub fn prepare_wikibase(
    provider: &Provider,
    registry: &'static Registry,
    dump: &std::path::Path,
) -> Result<SyncItems> {
    let rules = KeyRules::from_provider(provider)?;
    let inner = WikibaseAdapter::with_registry(&provider.slug, registry)
        .with_context(|| format!("provider `{}` is not registered", provider.slug))?;
    let xml = Entities::open(dump).context("open dump")?.is_xml();
    let mut index = Index::default();
    let mut types = DumpPropertyTypes::default();
    let mut unreadable = 0u64;
    let two_passes = !rules.is_empty() || xml;
    if two_passes {
        first_pass(dump, &rules, &mut index, &mut types, &mut unreadable)?;
    }
    index.finish();
    if !rules.is_empty() {
        eprintln!(
            "  {} mapped to keys, {} keys in conflict, {} redirects",
            index.mapped(),
            index.conflicts().count(),
            index.redirects()
        );
    }
    Ok(SyncItems {
        entries: Entities::open(dump).context("open dump")?.entries(),
        adapter: KeyMappedAdapter::new(inner, rules, index),
        types,
        counted: two_passes,
        pending: VecDeque::new(),
        unreadable: Rc::new(Cell::new(unreadable)),
        done: false,
    })
}

/// The first pass: the key index, the property types and the redirects.
fn first_pass(
    dump: &std::path::Path,
    rules: &KeyRules,
    index: &mut Index,
    types: &mut DumpPropertyTypes,
    unreadable: &mut u64,
) -> Result<()> {
    let mut seen = 0u64;
    eprintln!("indexing {}", dump.display());
    for entry in Entities::open(dump).context("open dump")?.entries() {
        match entry {
            Ok(DumpEntry::Entity(p)) => {
                seen += 1;
                // A reject here is reported on the second pass.
                let _ = index.observe(&p.entity, rules);
                types.observe(&p.entity);
            }
            Ok(DumpEntry::Redirect(r)) => index.observe_redirect(r.from.as_str(), r.to.as_str()),
            Err(e) => {
                *unreadable += 1;
                eprintln!("dump error: {e}");
            }
        }
    }
    eprintln!("  {seen} entities, {} property types", types.len());
    Ok(())
}

/// What the sync's items are watched for on their way in: every reject, printed to stderr
/// and, with a path, written as an `upstream ID<TAB>reason` line (the job record keeps only
/// the first 100), and the newest upstream time, where the load ended.
pub struct Watch {
    rejects: Option<std::io::BufWriter<std::fs::File>>,
    /// The first error writing the rejects file: an iterator cannot return it.
    error: Option<std::io::Error>,
    latest: Option<u64>,
}

impl Watch {
    /// Creates the rejects file, if one is named.
    pub fn new(rejects: Option<&std::path::Path>) -> Result<Self> {
        let rejects = rejects
            .map(|p| {
                std::fs::File::create(p)
                    .map(std::io::BufWriter::new)
                    .with_context(|| format!("create {}", p.display()))
            })
            .transpose()?;
        Ok(Self {
            rejects,
            error: None,
            latest: None,
        })
    }

    /// Notes one item.
    pub fn observe(&mut self, item: &SyncItem) {
        match item {
            SyncItem::Rejected {
                upstream_id,
                reason,
            } => {
                eprintln!("reject {upstream_id}: {reason}");
                if let Some(f) = self.rejects.as_mut()
                    && self.error.is_none()
                {
                    let reason = reason.replace(['\t', '\n'], " ");
                    if let Err(e) = writeln!(f, "{upstream_id}\t{reason}") {
                        self.error = Some(e);
                    }
                }
            }
            SyncItem::State { upstream, .. }
            | SyncItem::Deleted { upstream, .. }
            | SyncItem::Redirected { upstream, .. } => {
                if let Some(t) = upstream
                    .time
                    .as_deref()
                    .and_then(scatter_adapter_mediawiki::recentchanges::parse_iso)
                {
                    self.latest = self.latest.max(Some(t));
                }
            }
        }
    }

    /// Flushes the rejects file and returns the newest upstream time, epoch seconds.
    pub fn finish(mut self) -> Result<Option<u64>> {
        if let Some(e) = self.error.take() {
            return Err(e).context("write the rejects file");
        }
        if let Some(mut f) = self.rejects.take() {
            f.flush().context("write the rejects file")?;
        }
        Ok(self.latest)
    }
}

/// A registry provider's mirror, or a tenant's source in the tenant's table of names:
/// the registry, the slug and the tenant (`""` for the instance).
async fn target(
    client: &tokio_postgres::Client,
    args: &Sync,
) -> Result<(&'static Registry, &'static Provider, String)> {
    let (registry, slug, tenant) = match (&args.provider, &args.tenant, &args.source) {
        (Some(p), None, None) => (Registry::default_registry(), p.as_str(), String::new()),
        (None, Some(t), Some(s)) => (tenant_registry(client, t).await?, s.as_str(), t.clone()),
        _ => bail!("name a provider with --provider, or a source with --tenant and --source"),
    };
    let provider = registry.by_slug(slug).with_context(|| {
        if tenant.is_empty() {
            format!("provider `{slug}` is not registered")
        } else {
            format!("`{tenant}` declares no source `{slug}`")
        }
    })?;
    Ok((registry, provider, tenant))
}

/// Runs the sync.
pub async fn run(args: Sync) -> Result<()> {
    let client = connect(&args.database).await?;
    let (registry, provider, tenant) = target(&client, &args).await?;
    let slug = provider.slug.as_str();
    match provider.adapter.as_deref() {
        Some(scatter_adapter_wikibase::NAME) => {}
        Some(other) => bail!("`{slug}` is read by `{other}`, which this build does not include"),
        None => bail!("`{slug}` names no adapter"),
    }
    let mut watch = Watch::new(args.rejects.as_deref())?;
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let version = args.version.clone().unwrap_or_else(|| {
        args.dump
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let mode = if args.snapshot {
        Mode::Snapshot
    } else {
        Mode::Upsert
    };
    let mut header = if tenant.is_empty() {
        JobHeader::mirror(slug, &version, mode)
    } else {
        JobHeader::source(slug, &version, mode)
    };
    header.adapter_version = Some(format!(
        "{} {}",
        scatter_adapter_wikibase::NAME,
        scatter_adapter_wikibase::VERSION
    ));
    let attestation = Attestation::by(&farm.operator().to_string());
    let (outcome, unreadable, latest) = if bootstraps(&client, &args, provider, &tenant).await? {
        load_bootstrapped(
            &store,
            &pipeline,
            &args,
            provider,
            registry,
            header,
            attestation,
            &mut watch,
        )
        .await?
    } else {
        let items = prepare_wikibase(provider, registry, &args.dump)?;
        let unreadable = items.unreadable_counter();
        let outcome = run_sync_in(
            &store,
            &pipeline,
            &tenant,
            header,
            attestation,
            items.inspect(|item| watch.observe(item)),
            args.threshold,
            registry,
            now(),
            args.batch.max(1),
        )
        .await
        .context("sync")?;
        (outcome, unreadable.get(), None)
    };
    // Flushes the rejects file; on the ordinary path it also saw every state's time.
    let latest = latest.or(watch.finish()?);
    // Where the load ended (0092 §5), for a follower to continue from.
    if let Some(latest) = latest {
        let kind = if tenant.is_empty() {
            KIND_PROVIDER
        } else {
            KIND_SOURCE
        };
        record_load(&client, &tenant, kind, slug, latest)
            .await
            .context("record where the load ended")?;
        println!(
            "  loaded to  {}",
            scatter_adapter_mediawiki::recentchanges::iso(latest)
        );
    }
    println!("sync job {} finished", outcome.job_id);
    println!("  created    {}", outcome.counts.created);
    println!("  updated    {}", outcome.counts.merged);
    println!("  unchanged  {}", outcome.counts.unchanged);
    println!("  rejected   {}", outcome.counts.rejected);
    println!("  tombstoned {}", outcome.counts.tombstoned);
    if outcome.counts.cleared > 0 {
        println!("  cleared    {}", outcome.counts.cleared);
    }
    if outcome.counts.other > 0 {
        println!("  redirected {}", outcome.counts.other);
    }
    if outcome.swept > 0 {
        println!("  swept      {}", outcome.swept);
    }
    if unreadable > 0 {
        println!("  unreadable {unreadable} (see stderr)");
    }
    Ok(())
}

/// A bootstrap load (0013 §9): the dump prepared on worker threads
/// ([`crate::bootstrap::prepared`]) and appended in blocks by
/// [`scatter_ingest::run_bootstrap`]. Returns the outcome, the unreadable count and where the
/// load ends.
#[allow(clippy::too_many_arguments)]
async fn load_bootstrapped(
    store: &triplespace_api_ingest::PgIngest,
    pipeline: &scatter_projection::Pipeline<triplespace_api_ingest::PgIngest>,
    args: &Sync,
    provider: &Provider,
    registry: &'static Registry,
    mut header: JobHeader,
    attestation: Attestation,
    watch: &mut Watch,
) -> Result<(scatter_ingest::SyncOutcome, u64, Option<u64>)> {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    // The job says how it loaded, for the operator and for `view.job` (05 §3.8).
    header
        .params
        .insert("bootstrap".into(), serde_json::Value::Bool(true));
    let workers = std::thread::available_parallelism()
        .map_or(4, std::num::NonZero::get)
        .saturating_sub(2)
        .max(1);
    eprintln!(
        "bootstrap: the mirror is empty; loading in blocks, prepared on {workers} threads (0013 §9)"
    );
    let inner = WikibaseAdapter::with_registry(&provider.slug, registry)
        .with_context(|| format!("provider `{}` is not registered", provider.slug))?;
    let mut index = Index::default();
    index.finish();
    let adapter = Arc::new(KeyMappedAdapter::new(
        inner,
        KeyRules::from_provider(provider)?,
        index,
    ));
    let tally = Arc::new(crate::bootstrap::Tally::default());
    let outcome = run_bootstrap(
        store,
        pipeline,
        header,
        attestation,
        registry,
        now(),
        args.batch.max(1),
        |prepare| {
            // The projections' per-record work runs on the workers too (0013 A74).
            let prepare = prepare.warmed_by(Arc::new(triplespace_projections::common::warm));
            let prepared: Box<dyn Iterator<Item = Result<Prepared, IngestError>>> =
                match crate::bootstrap::prepared(&args.dump, &adapter, prepare, workers, &tally) {
                    Ok(ordered) => Box::new(ordered),
                    Err(e) => Box::new(std::iter::once(Err(e))),
                };
            prepared.inspect(|p| {
                if let Ok(Prepared::Rejected {
                    upstream_id,
                    reason,
                }) = p
                {
                    watch.observe(&SyncItem::Rejected {
                        upstream_id: upstream_id.clone(),
                        reason: reason.clone(),
                    });
                }
            })
        },
    )
    .await
    .context("bootstrap")?;
    let latest = Some(tally.latest.load(Ordering::Relaxed)).filter(|t| *t > 0);
    Ok((outcome, tally.unreadable.load(Ordering::Relaxed), latest))
}

/// Whether this load is bootstrap mode's (0013 §9, A70): an instance mirror whose partition is
/// still empty, a provider with no key map (its subjects are their own keys), a JSON dump
/// (lines parse independently), and `--no-bootstrap` not given.
async fn bootstraps(
    client: &tokio_postgres::Client,
    args: &Sync,
    provider: &Provider,
    tenant: &str,
) -> Result<bool> {
    if args.no_bootstrap || !tenant.is_empty() || !KeyRules::from_provider(provider)?.is_empty() {
        return Ok(false);
    }
    if Entities::open(&args.dump).context("open dump")?.is_xml() {
        return Ok(false);
    }
    let row = client
        .query_opt(
            "SELECT next_offset FROM log.partition WHERE tenant IS NULL AND name = $1",
            &[&format!("mirror/{}", provider.slug)],
        )
        .await?;
    Ok(row.is_some_and(|r| r.get::<_, i64>(0) == 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::entity::EntityType;
    use scatter_wikibase_model::id::EntityId;
    use scatter_wikibase_model::statement::SnakKind;
    use scatter_wikibase_model::value::{DataType, DataValue};

    /// What the sync would be handed, read to the end, and the unreadable count then.
    struct Prepared {
        items: Vec<SyncItem>,
        unreadable: u64,
    }

    fn collect(mut items: SyncItems) -> Prepared {
        let collected: Vec<SyncItem> = items.by_ref().collect();
        Prepared {
            items: collected,
            unreadable: items.unreadable(),
        }
    }

    fn prepare(slug: &str, dump: &std::path::Path) -> Result<Prepared> {
        let provider = Registry::default_registry().by_slug(slug).unwrap();
        prepare_wikibase(provider, Registry::default_registry(), dump).map(collect)
    }

    fn prepare_internetdomains(dump: &std::path::Path) -> Result<Prepared> {
        prepare("internetdomains", dump)
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../triplespace-server/tests/fixtures")
            .join(name)
    }

    fn snaks_of(
        entity: &scatter_wikibase_model::entity::Entity,
    ) -> Vec<&scatter_wikibase_model::statement::Snak> {
        entity
            .statements
            .values()
            .flatten()
            .flat_map(|s| {
                std::iter::once(&s.mainsnak)
                    .chain(s.qualifiers.values().flatten())
                    .chain(s.references.iter().flat_map(|r| r.snaks.values().flatten()))
            })
            .collect()
    }

    /// Any Wikibase provider syncs through the one adapter (0091 §1, §4): Librarybase's
    /// XML sample, with no key map, is mirrored under `LB` with every snak typed from the
    /// dump's own properties, and nothing is rejected.
    #[test]
    fn a_wikibase_without_a_key_map_mirrors_under_its_prefix() {
        let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/samples/librarybase/dump/librarybase-sample.xml.gz");
        let prepared = prepare("librarybase", &sample).unwrap();
        assert_eq!(prepared.unreadable, 0);
        assert!(prepared.items.len() > 100, "{}", prepared.items.len());
        for item in &prepared.items {
            let SyncItem::State {
                entity,
                upstream,
                upstream_id,
                ..
            } = item
            else {
                panic!("only states: {item:?}");
            };
            assert!(entity.id.as_str().starts_with("LB"), "{}", entity.id);
            assert!(upstream.revid.is_some());
            assert!(upstream_id.is_none());
            for snak in snaks_of(entity) {
                assert!(snak.property.as_str().starts_with("LBP"));
                assert!(
                    snak.datatype.is_some(),
                    "{}: untyped {}",
                    entity.id,
                    snak.property
                );
            }
        }
    }

    /// A JSON dump of a provider without key maps is read in one pass (0091 §4): its snaks
    /// carry their `datatype`, and there is no index to build.
    #[test]
    fn a_json_dump_without_a_key_map_is_read_once() {
        let prepared = prepare("librarybase", &fixture("internetdomains.json")).unwrap();
        assert_eq!(prepared.unreadable, 0);
        let ids: Vec<String> = prepared
            .items
            .iter()
            .filter_map(|i| match i {
                SyncItem::State { entity, .. } => Some(entity.id.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(ids.len(), 4);
        assert!(
            ids.iter()
                .all(|id| id.starts_with("LBQ") || id.starts_with("LBP")),
            "{ids:?}"
        );
    }

    /// The items are read as the sync takes them, not before: after the first item of a
    /// one-pass JSON dump, the rest of the dump is still unread.
    #[test]
    fn a_dump_is_read_as_the_sync_takes_its_items() {
        let provider = Registry::default_registry().by_slug("librarybase").unwrap();
        let mut items = prepare_wikibase(
            provider,
            Registry::default_registry(),
            &fixture("internetdomains.json"),
        )
        .unwrap();
        assert!(items.next().is_some());
        assert!(
            !items.done,
            "the dump is not read to the end for the first item"
        );
        assert!(items.pending.is_empty());
        let rest = items.by_ref().count();
        assert_eq!(rest, 3);
        assert!(items.done);
        assert!(items.next().is_none());
    }

    /// Wikidata items whose values are a lexeme's sense or form (0066 §4) mirror like any
    /// other: the values become `WDL…-S…` and `WDL…-F…`, and the snak hashes Wikidata
    /// wrote for them recompute the same. Two real statements of the 2026-10-26 dump.
    #[test]
    fn values_naming_a_sense_or_a_form_mirror_with_their_hashes() {
        let sense = r#"{"mainsnak":{"snaktype":"value","property":"P1549","datavalue":{"value":{"text":"Belgier","language":"de"},"type":"monolingualtext"},"datatype":"monolingualtext"},"type":"statement","qualifiers":{"P518":[{"snaktype":"value","property":"P518","hash":"735874569aa9bd9762a6bf8dea8d3b3faae7d12b","datavalue":{"value":{"entity-type":"item","numeric-id":499327,"id":"Q499327"},"type":"wikibase-entityid"},"datatype":"wikibase-item"}],"P7018":[{"snaktype":"value","property":"P7018","hash":"12e61a1c81f9fc906805e081c6112561e268e93b","datavalue":{"value":{"entity-type":"sense","id":"L499143-S1"},"type":"wikibase-entityid"},"datatype":"wikibase-sense"}]},"qualifiers-order":["P518","P7018"],"id":"Q31$a09fa2e0-4ba8-6bfa-f1af-4ed688f6fd04","rank":"normal"}"#;
        let form = r#"{"mainsnak":{"snaktype":"value","property":"P8017","datavalue":{"value":{"entity-type":"form","id":"L252247-F2"},"type":"wikibase-entityid"},"datatype":"wikibase-form"},"type":"statement","id":"Q33866$c2e8a983-4dca-9d9d-46a6-c02b4a249e34","rank":"normal"}"#;
        let item = |id: &str, p: &str, statement: &str| {
            format!(
                r#"{{"type":"item","id":"{id}","labels":{{}},"descriptions":{{}},"aliases":{{}},"claims":{{"{p}":[{statement}]}},"sitelinks":{{}},"lastrevid":1,"modified":"2026-10-25T00:00:00Z"}}"#
            )
        };
        let dump = format!(
            "[\n{},\n{}\n]\n",
            item("Q31", "P1549", sense),
            item("Q33866", "P8017", form)
        );
        let path =
            std::env::temp_dir().join(format!("ts-lexeme-parts-{}.json", std::process::id()));
        std::fs::write(&path, dump).unwrap();
        let prepared = prepare("wikidata", &path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(prepared.unreadable, 0);
        let hasher = scatter_wikibase_model::Hasher::mirrored_from("WD");
        let mut values = Vec::new();
        for item in prepared.items {
            let SyncItem::State { entity, .. } = item else {
                panic!("only states: {item:?}");
            };
            let mut entity = *entity;
            let mismatches = hasher.reconcile(&mut entity);
            assert!(
                mismatches.is_empty(),
                "{}: {:?}",
                entity.id,
                mismatches.by_kind()
            );
            for snak in snaks_of(&entity) {
                if let SnakKind::Value(DataValue::EntityId(v)) = &snak.kind
                    && v.id.lexeme_part().is_some()
                {
                    values.push((v.entity_type.clone(), v.id.to_string()));
                }
            }
        }
        assert_eq!(
            values,
            [
                ("sense".to_string(), "WDL499143-S1".to_string()),
                ("form".to_string(), "WDL252247-F2".to_string()),
            ]
        );
    }

    /// The XML fixture is the JSON one as Wikibase stores it: no `datatype` on any snak.
    /// Typed from the dump's own properties, it reads to the same mirror states.
    #[test]
    fn an_xml_dump_syncs_like_the_json_dump_of_the_same_wiki() {
        let json = prepare_internetdomains(&fixture("internetdomains.json")).unwrap();
        let xml = prepare_internetdomains(&fixture("internetdomains.xml")).unwrap();
        assert_eq!((json.unreadable, xml.unreadable), (0, 0));
        assert_eq!(xml.items.len(), 4);
        assert_eq!(xml.items, json.items);
    }

    /// The link from wikipedia.org to example.org, both mapped, is a Domain value; the
    /// adapter can only see that once the snak is typed `wikibase-item`.
    #[test]
    fn an_xml_link_to_a_mapped_item_becomes_a_domain_value() {
        let xml = prepare_internetdomains(&fixture("internetdomains.xml")).unwrap();
        let wikipedia = xml
            .items
            .iter()
            .find_map(|i| match i {
                SyncItem::State {
                    entity,
                    upstream_id: Some(q),
                    ..
                } if q == "Q11" => Some(entity),
                _ => None,
            })
            .expect("Q11 is mapped");
        assert_eq!(wikipedia.entity_type, EntityType::Domain);
        let p8 = EntityId::parse("XDP8").unwrap();
        let snak = &wikipedia.statements[&p8][0].mainsnak;
        assert_eq!(snak.datatype, Some(DataType::WikibaseDomain));
        let SnakKind::Value(DataValue::EntityId(v)) = &snak.kind else {
            panic!("an entity value");
        };
        assert_eq!(v.id.as_str(), "domain:example.org");
        // Every snak of every state carries a datatype.
        for item in &xml.items {
            if let SyncItem::State { entity, .. } = item {
                for s in entity.statements.values().flatten() {
                    for snak in std::iter::once(&s.mainsnak)
                        .chain(s.qualifiers.values().flatten())
                        .chain(s.references.iter().flat_map(|r| r.snaks.values().flatten()))
                    {
                        assert!(
                            snak.datatype.is_some(),
                            "{}: untyped {}",
                            entity.id,
                            snak.property
                        );
                    }
                }
            }
        }
    }

    /// The XML fixture with three more pages: an unmapped registrar (Q20), Q12 merged into
    /// wikipedia.org (Q11) and Q21 merged into the registrar, and an item whose identity
    /// value is not a domain (Q22).
    fn merged_dump() -> std::path::PathBuf {
        let base = std::fs::read_to_string(fixture("internetdomains.xml")).unwrap();
        let page = |id: u64, title: &str, model: &str, text: &str| {
            format!(
                "  <page>\n    <title>{title}</title>\n    <ns>120</ns>\n    <id>{id}</id>\n    <revision>\n      <id>{}</id>\n      <timestamp>2026-10-02T00:00:00Z</timestamp>\n      <contributor><username>Fixture</username><id>1</id></contributor>\n      <model>{model}</model>\n      <format>application/json</format>\n      <text xml:space=\"preserve\">{text}</text>\n    </revision>\n  </page>\n",
                100 + id
            )
        };
        let extra = [
            page(20, "Item:Q20", "wikibase-item", r#"{"type":"item","id":"Q20","labels":{"en":{"language":"en","value":"Registrar"}},"descriptions":[],"aliases":[],"claims":[],"sitelinks":[]}"#),
            page(12, "Item:Q12", "wikibase-item", r#"{"entity":"Q12","redirect":"Q11"}"#),
            page(21, "Item:Q21", "wikibase-item", r#"{"entity":"Q21","redirect":"Q20"}"#),
            page(22, "Item:Q22", "wikibase-item", r#"{"type":"item","id":"Q22","labels":[],"descriptions":[],"aliases":[],"claims":{"P1":[{"mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"news.example/us","type":"string"}},"type":"statement","rank":"normal","id":"Q22$00000000-0000-0000-0000-0000000000C1"}]},"sitelinks":[]}"#),
        ]
        .concat();
        let xml = base.replace("</mediawiki>", &format!("{extra}</mediawiki>"));
        let path = std::env::temp_dir().join(format!("ts-merged-{}.xml", std::process::id()));
        std::fs::write(&path, xml).unwrap();
        path
    }

    #[test]
    fn redirects_become_aliases_or_mirror_redirects_and_rejects_are_all_reported() {
        let dump = merged_dump();
        let prepared = prepare_internetdomains(&dump).unwrap();
        std::fs::remove_file(&dump).unwrap();
        let wikipedia = prepared
            .items
            .iter()
            .find_map(|i| match i {
                SyncItem::State {
                    upstream_id: Some(q),
                    upstream_aliases,
                    ..
                } if q == "Q11" => Some(upstream_aliases),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            wikipedia,
            &["Q12".to_string()],
            "a merge into a mapped item"
        );
        let redirects: Vec<(String, String)> = prepared
            .items
            .iter()
            .filter_map(|i| match i {
                SyncItem::Redirected { from, to, .. } => Some((from.to_string(), to.to_string())),
                _ => None,
            })
            .collect();
        assert_eq!(redirects, [("XDQ21".to_string(), "XDQ20".to_string())]);

        let file = std::env::temp_dir().join(format!("ts-rejects-{}.tsv", std::process::id()));
        let mut watch = Watch::new(Some(&file)).unwrap();
        for item in &prepared.items {
            watch.observe(item);
        }
        assert_eq!(
            watch.finish().unwrap(),
            scatter_adapter_mediawiki::recentchanges::parse_iso("2026-10-02T00:00:00Z"),
            "the newest upstream time among the items"
        );
        let tsv = std::fs::read_to_string(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        let lines: Vec<&str> = tsv.lines().collect();
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].starts_with("Q22\t`news.example/us` is not a valid domain key"),
            "{tsv}"
        );
    }
}
