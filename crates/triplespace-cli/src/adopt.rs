//! `triplespace adopt` (0035): a frozen Wikibase's full-history MediaWiki XML dump becomes
//! the tenant's `local` graph, revision by revision (A12), or, with `--current-only`, as
//! each entity's newest revision. Its document pages come in the same job, into `pages`
//! ([`crate::adopt_pages`], A13). Two passes over the dump: a survey for the floors, the
//! accounts and whether the dump carries history, then the adoption job itself.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use scatter_actors::actor::ActorRecord;
use scatter_actors::key::ActorKey;
use scatter_adapter_wikibase::adoption::{
    AdoptedRevision, ENTITY_MODELS, Survey, adopt_history, adopt_page,
};
use scatter_adapter_wikibase::xml_dump::XmlDump;
use scatter_ingest::adopt::{Adopted, AdoptedItem, Adoption, Floors, run_adoption};
use scatter_ingest::write::Attestation;
use scatter_providers::Registry;
use scatter_wikibase_model::id::EntityId;
use triplespace_projections::actors::payload;

use crate::common::{connect, draft, farm_of, issuer_of, now, store};

/// `adopt`.
#[derive(Debug, Args)]
#[allow(clippy::struct_excessive_bools)] // command-line switches
pub struct Adopt {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
    /// The tenant slug.
    #[arg(long)]
    pub tenant: String,
    /// The MediaWiki XML dump of the frozen source (`.xml`, `.xml.gz`, `.xml.bz2`): a
    /// full-history export (`dumpBackup.php --full`) unless `--current-only`.
    #[arg(long)]
    pub dump: PathBuf,
    /// Adopt each entity's newest revision only, under a fresh revision ID, leaving the
    /// history to a later backfill (0035 §7, A12).
    #[arg(long)]
    pub current_only: bool,
    /// The source's base URL, `https://librarybase.org/`.
    #[arg(long)]
    pub source: String,
    /// The dump's identity, the source version (`librarybase-20260928.xml.gz`).
    #[arg(long)]
    pub version: Option<String>,
    /// The operator declares the source frozen (required).
    #[arg(long)]
    pub frozen: bool,
    /// The source's `wb_id_counters`, `item=350000,property=1200`; the dump's highest
    /// IDs otherwise, with a warning (0035 §4).
    #[arg(long)]
    pub counters: Option<String>,
    /// The highest log ID the source consumed (not in the dump).
    #[arg(long, default_value_t = 0)]
    pub log_floor: u64,
    /// Entity-source prefixes the source used, `wikidata=WD` (wikibase-compat §5.1).
    #[arg(long = "entity-source")]
    pub entity_sources: Vec<String>,
    /// The subsidiary running the job, `librarybase:2`; the owner's own key by default.
    #[arg(long)]
    pub subsidiary: Option<String>,
    /// Revisions per unit of work.
    #[arg(long, default_value_t = 200)]
    pub batch: usize,
    /// Leave the source's talk pages unconverted, deferred as before 0090 §10.
    #[arg(long)]
    pub skip_talk: bool,
    /// Skip the source's accounts.
    #[arg(long)]
    pub no_accounts: bool,
}

fn parse_pairs(text: &str) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part
            .split_once('=')
            .with_context(|| format!("`{part}` is not `name=value`"))?;
        out.insert(k.trim().to_string(), v.trim().to_string());
    }
    Ok(out)
}

/// Runs the adoption.
#[allow(clippy::too_many_lines)]
pub async fn run(args: Adopt) -> Result<()> {
    if !args.frozen {
        bail!(
            "adoption is a one-way door: declare the source frozen with --frozen once its final dump is taken (0035 §2)"
        );
    }
    let registry = Registry::default_registry();
    let sources: BTreeMap<String, String> = args
        .entity_sources
        .iter()
        .map(|s| parse_pairs(s))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    // Pass 1: the survey.
    eprintln!("surveying {}", args.dump.display());
    let mut survey = Survey::default();
    for page in XmlDump::open(&args.dump).context("open dump")? {
        let page = page.context("read dump")?;
        survey.observe(&page);
        if survey.pages % 50_000 == 0 {
            eprintln!("  {} pages, {} entities", survey.pages, survey.entities);
        }
    }
    eprintln!(
        "  {} pages, {} entities, {} redirects, {} accounts; highest page {}, revision {}",
        survey.pages,
        survey.entities,
        survey.redirects,
        survey.accounts.len(),
        survey.max_page_id,
        survey.max_revid
    );
    eprintln!(
        "  {} revisions, {} pages with more than one, {} with revisions deleted on the source; {} other pages",
        survey.revisions, survey.pages_with_history, survey.history_gaps, survey.other_pages
    );
    if !args.current_only && survey.current_state_dump() {
        bail!(
            "the dump is a current-state export: every page has one revision while revisions name parents. Take a full-history dump (dumpBackup.php --full), or pass --current-only (0035 §2, §7)"
        );
    }
    let mut floors = Floors {
        entity: survey.max_entity.clone(),
        page: survey.max_page_id,
        revision: survey.max_revid,
        log: args.log_floor,
        user: survey.max_user_id(),
    };
    match &args.counters {
        Some(text) => {
            for (t, n) in parse_pairs(text)? {
                let n: u64 = n.parse().with_context(|| format!("counter `{t}`"))?;
                floors.entity.insert(t, n);
            }
        }
        None => eprintln!(
            "warning: no --counters; entity floors come from the dump's highest IDs, so numbers the source allocated and deleted may be reused (0035 §4)"
        ),
    }

    // The tenant's issuer code, which its accounts carry (0079 §6), and the farm.
    let client = connect(&args.database).await?;
    let issuer = issuer_of(&client, &args.tenant).await.with_context(|| {
        format!(
            "run `instance create` or `tenant create` for `{}` first",
            args.tenant
        )
    })?;
    let farm = farm_of(&client).await?;
    let owner: Option<String> = client
        .query_opt(
            "SELECT m.actor_key FROM view.membership m WHERE m.tenant = $1 AND m.\"group\" = 'owner' ORDER BY m.actor_key LIMIT 1",
            &[&args.tenant],
        )
        .await?
        .map(|r| r.get(0));
    let actor = match (&args.subsidiary, owner) {
        (Some(s), _) => ActorKey::parse(s)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .to_string(),
        (None, Some(o)) => o,
        (None, None) => bail!("no owner found for `{}`; pass --subsidiary", args.tenant),
    };
    if !args.no_accounts {
        let held: Vec<(String, String)> = client
            .query(
                "SELECT actor_key, name FROM view.actor
                 WHERE tenant = $1 AND kind IN ('registered', 'bot') AND name IS NOT NULL",
                &[&args.tenant],
            )
            .await?
            .iter()
            .map(|r| (r.get(0), r.get(1)))
            .collect();
        let adopted: Vec<(String, String)> = survey
            .accounts()
            .iter()
            .map(|a| (ActorKey::numeric(&issuer, a.id).to_string(), a.name.clone()))
            .collect();
        let clashes = name_clashes(&held, &adopted);
        if !clashes.is_empty() {
            bail!(
                "the dump's accounts disagree with accounts the tenant already has, so adoption would give two accounts one name or rename one (0035 §5):\n  {}\nThe owner must be created under the source's own user ID and name (`instance create --owner <user_id> --owner-name <name>`, 07 §2.5)",
                clashes.join("\n  ")
            );
        }
    }
    let t = now();
    let accounts = if args.no_accounts {
        Vec::new()
    } else {
        survey
            .accounts()
            .iter()
            .map(|a| {
                draft(
                    payload::ACTOR,
                    &ActorKey::numeric(&issuer, a.id).to_string(),
                    &serde_json::to_value(ActorRecord::registered(&a.name))?,
                    &actor,
                    Some("adopted account"),
                    t,
                )
            })
            .collect::<Result<Vec<_>>>()?
    };
    let adoption = Adoption {
        issuer: issuer.clone(),
        source: args.source.clone(),
        version: args.version.clone().unwrap_or_else(|| {
            args.dump
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        }),
        adapter_version: Some(format!(
            "{} {}",
            scatter_adapter_wikibase::NAME,
            scatter_adapter_wikibase::VERSION
        )),
        frozen: true,
        history: !args.current_only,
        current_state_dump: survey.current_state_dump(),
        floors,
        accounts,
        property_types: survey.property_types.clone(),
    };
    if survey.property_types.is_empty() {
        eprintln!(
            "warning: the dump defines no properties, so adopted snaks keep no datatype and identifiers are not indexed"
        );
    }

    // Pass 2: the job.
    let (store, pipeline) = store(&args.database, &farm)?;
    let mut read_errors = 0u64;
    let mut unreadable_revisions = 0u64;
    let mut seen = 0u64;
    let mut untyped: BTreeMap<EntityId, u64> = BTreeMap::new();
    let mut pages_read = 0u64;
    let mut deferred: BTreeMap<String, u64> = BTreeMap::new();
    let mut ignored: BTreeMap<String, u64> = BTreeMap::new();
    let mut talk_pages = 0u64;
    let tenant = issuer.clone();
    let current_only = args.current_only;
    let revisions = XmlDump::open(&args.dump)
        .context("open dump")?
        .flat_map(|page| {
            let page = match page {
                Ok(page) => page,
                Err(e) => {
                    read_errors += 1;
                    eprintln!("dump error: {e}");
                    return Vec::new();
                }
            };
            let entity_page = page
                .revisions
                .last()
                .and_then(|r| r.model.as_deref())
                .is_some_and(|m| ENTITY_MODELS.contains(&m));
            if !entity_page {
                // Talk pages are converted after the job, through the thread writer
                // (0090 §10; `adopt_talk`), once their subjects and accounts are here.
                if !args.skip_talk && crate::adopt_talk::is_talk_page(&page) {
                    talk_pages += 1;
                    return Vec::new();
                }
                return match crate::adopt_pages::read_page(&page, &tenant, current_only) {
                    crate::adopt_pages::PageRead::Adopt(revs) => {
                        pages_read += 1;
                        revs.into_iter().map(AdoptedItem::Page).collect()
                    }
                    crate::adopt_pages::PageRead::Deferred(why) => {
                        eprintln!("deferring page {} (`{}`): {why}", page.id, page.title);
                        *deferred.entry(why).or_default() += 1;
                        Vec::new()
                    }
                    crate::adopt_pages::PageRead::Ignored(why) => {
                        *ignored.entry(why).or_default() += 1;
                        Vec::new()
                    }
                };
            }
            let read = if current_only {
                adopt_page(&page, &sources, registry, &survey.property_types)
                    .map(|a| a.into_iter().collect::<Vec<_>>())
            } else {
                adopt_history(&page, &sources, registry, &survey.property_types).map(|a| {
                    a.map(|a| {
                        for (revid, why) in &a.unreadable {
                            unreadable_revisions += 1;
                            eprintln!(
                                "skipping revision {revid} of page {} (`{}`): {why}",
                                page.id, page.title
                            );
                        }
                        a.revisions
                    })
                    .unwrap_or_default()
                })
            };
            match read {
                Ok(revisions) => {
                    if !revisions.is_empty() {
                        seen += 1;
                        if seen.is_multiple_of(10_000) {
                            eprintln!("  {seen} entities read");
                        }
                    }
                    revisions
                        .into_iter()
                        .map(|a| {
                            for (p, n) in &a.untyped_snaks {
                                *untyped.entry(p.clone()).or_default() += n;
                            }
                            AdoptedItem::Entity(adopted(a, &tenant, current_only))
                        })
                        .collect()
                }
                Err(e) => {
                    read_errors += 1;
                    eprintln!("skipping page {} (`{}`): {e}", page.id, page.title);
                    Vec::new()
                }
            }
        });
    let outcome = run_adoption(
        &store,
        &pipeline,
        &args.tenant,
        Attestation::by(&actor),
        adoption,
        revisions,
        registry,
        t,
        args.batch.max(1),
    )
    .await
    .context("adoption")?;
    println!(
        "adoption job {} finished ({})",
        outcome.job_id,
        if args.current_only {
            "current state only"
        } else {
            "full history"
        }
    );
    println!(
        "  adopted    {} revisions of {seen} entities",
        outcome.counts.adopted
    );
    println!("  unchanged  {}", outcome.counts.unchanged);
    println!("  rejected   {}", outcome.counts.rejected);
    println!("  accounts   {}", outcome.accounts);
    if read_errors > 0 {
        println!("  unreadable {read_errors} pages (see stderr)");
    }
    if unreadable_revisions > 0 {
        println!("  unreadable {unreadable_revisions} revisions (see stderr)");
    }
    println!("  pages      {pages_read} document pages");
    let deferred_total: u64 = deferred.values().sum();
    if deferred_total > 0 {
        println!("  deferred   {deferred_total} pages (0035 A13):");
        for (why, n) in &deferred {
            println!("    {n} × {why}");
        }
    }
    let ignored_total: u64 = ignored.values().sum();
    if ignored_total > 0 {
        println!("  ignored    {ignored_total} pages the instance does not use (0095 A1):");
        for (why, n) in &ignored {
            println!("    {n} × {why}");
        }
    }
    if !untyped.is_empty() {
        let total: u64 = untyped.values().sum();
        let by_property: Vec<String> = untyped.iter().map(|(p, n)| format!("{p}×{n}")).collect();
        println!(
            "  untyped    {total} snaks on {} properties the dump does not define: {}",
            untyped.len(),
            by_property.join(", ")
        );
    }
    for r in outcome.rejects.iter().take(20) {
        println!("  reject #{}: {}", r.line, r.reason);
    }

    // Pass 3: the talk pages, comment by comment (0090 §10).
    if talk_pages > 0 {
        eprintln!("converting {talk_pages} talk pages");
        let site = scatter_talk::Site::english();
        let ctx = crate::adopt_talk::TalkContext {
            tenant: &args.tenant,
            issuer: &issuer,
            source: &args.source,
            accounts: &survey.accounts,
            actor: &actor,
            job: Some(outcome.job_id),
            site: &site,
        };
        let pages = XmlDump::open(&args.dump)
            .context("open dump")?
            .filter_map(Result::ok)
            .filter(crate::adopt_talk::is_talk_page);
        let talk = crate::adopt_talk::convert_all(&store, &pipeline, &ctx, pages, now()).await?;
        println!(
            "  talk       {} pages converted into {} threads, {} posts; {} already present",
            talk.pages, talk.threads, talk.posts, talk.present
        );
        let n: u64 = talk.deferred.values().sum();
        if n > 0 {
            println!("  deferred   {n} talk pages:");
            for (why, k) in &talk.deferred {
                println!("    {k} × {why}");
            }
        }
    }
    Ok(())
}

/// The job's form of an adapted revision: under `--current-only` the job's own revision
/// (0035 §3), otherwise the source's, attributed to its author (A12).
fn adopted(a: AdoptedRevision, issuer: &str, current_only: bool) -> Adopted {
    if current_only && let Some(entity) = a.entity {
        return Adopted::current(entity, a.source_revid, &a.source_time, a.source_pageid);
    }
    let author = crate::adopt_pages::author(a.contributor, issuer);
    Adopted {
        id: a.id,
        entity: a.entity,
        redirect: a.redirect,
        source_revid: a.source_revid,
        source_parentid: a.source_parentid,
        source_time: a.source_time,
        source_pageid: a.source_pageid,
        minor: a.minor,
        withheld: a.withheld,
        comment: a.comment,
        author,
    }
}

/// Where the source's accounts disagree with the tenant's (0035 §5, A16): a source account
/// whose name another key already holds, or a key already held under another name. Account
/// names are unique on a tenant, as on MediaWiki, and the adoption leaves an account it finds
/// present as it is (07 §2.5), so either case would leave the tenant with two accounts of one
/// name or the source's account under the wrong name. `held` and `adopted` are
/// `(actor key, name)` pairs.
fn name_clashes(held: &[(String, String)], adopted: &[(String, String)]) -> Vec<String> {
    let mut out = Vec::new();
    for (key, name) in adopted {
        for (k, n) in held {
            if n == name && k != key {
                out.push(format!(
                    "`{name}` is {key} in the dump, but the tenant's {k} has that name"
                ));
            } else if k == key && n != name {
                out.push(format!(
                    "{key} is `{name}` in the dump, but the tenant has it as `{n}`"
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod name_clash_tests {
    use super::name_clashes;

    fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter()
            .map(|(k, n)| ((*k).to_string(), (*n).to_string()))
            .collect()
    }

    // 0035 §5, A16
    #[test]
    fn an_owner_under_another_id_with_a_source_name_is_refused() {
        let held = pairs(&[("t:3", "Librarybase")]);
        let adopted = pairs(&[("t:1", "Librarybase")]);
        assert_eq!(name_clashes(&held, &adopted).len(), 1);
    }

    #[test]
    fn an_owner_under_a_source_id_with_another_name_is_refused() {
        let held = pairs(&[("t:3", "Harej")]);
        let adopted = pairs(&[("t:3", "Librarybase")]);
        assert_eq!(name_clashes(&held, &adopted).len(), 1);
    }

    #[test]
    fn the_owner_as_the_source_has_it_and_a_rerun_pass() {
        let held = pairs(&[("t:3", "Harej"), ("t:1", "Librarybase")]);
        let adopted = pairs(&[("t:1", "Librarybase"), ("t:3", "Harej")]);
        assert!(name_clashes(&held, &adopted).is_empty());
    }
}
