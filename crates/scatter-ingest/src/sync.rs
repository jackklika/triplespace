//! A mirror sync (0002 §8.4; 0012 §2.2; 0006 §2): an instance job over a provider's
//! mirror partition. Each state the adapter hands over is checked against the version
//! cursor — an older revision, or the same content under the same mapping, means
//! `unchanged` — and otherwise written as a
//! `put` carrying `prev_upstream`, `first_seen`, both sizes and the `changes` summary.
//! Upstream snak and reference hashes are recomputed; a differing one is kept and counted
//! (the hash guard). In `snapshot` mode the entities the job did not see are tombstoned
//! once it completes, unless more than the threshold would go.
//!
//! A keyed entity is never tombstoned (0009 §9; architecture 05 §2.2, §4.2): an upstream
//! deletion, or its absence from a snapshot, clears the mirror's contribution with a `put`
//! of the empty state, as it does for every entity of a provider registered
//! `deletion = clear`. A key-mapped sync also keeps the mapping honest: when an upstream
//! item the job saw no longer owns the key it owned before (its identity value changed,
//! or it stopped being mapped), that key is cleared; and when an item becomes mapped, the
//! foreign item it was (`XDQ9`) is tombstoned, since a mapped item's upstream ID never
//! appears as a subject. An upstream redirect is written once; a later sync that sees the
//! same redirect leaves it.
//!
//! A mirror record's revision ID is the upstream revision's (0013 §6), and an upstream
//! revision belongs to one page, so a `put` claims it only the first time it is recorded.
//! A state re-written at a revision already recorded, because the mapping moved rather
//! than the item (an alias added, a key gained or lost, a value pointing at a moved
//! item), carries no `revid`; its `version` is `{revid}+{job version}`.

use std::collections::{BTreeMap, BTreeSet};

use scatter_log::cbor::Value;
use scatter_projection::{Budget, Pipeline, par_map};
use scatter_providers::{Deletion, Provider, Registry, split_revid};
use scatter_wikibase_changeset::{Changes, Graph, JobHeader, Mode, Operation, Upstream};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::{HashMismatches, Hasher};
use scatter_wikibase_model::id::{EntityId, IdForm};

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::IngestStore;
use crate::write::{Attestation, Request, write_to};

/// What an adapter hands the sync, with IDs already in stored form.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncItem {
    /// An entity's current upstream state.
    State {
        /// The state.
        entity: Box<Entity>,
        /// Its upstream version.
        upstream: Upstream,
        /// The provider's own item ID, for a key-mapped subject (0009 §9).
        upstream_id: Option<String>,
        /// The provider's IDs of items upstream merged into `upstream_id`, recorded as
        /// aliases of the key (0009 A13). Empty for an unmapped subject.
        upstream_aliases: Vec<String>,
    },
    /// Upstream deleted the entity.
    Deleted {
        /// The entity.
        id: EntityId,
        /// The deleting version.
        upstream: Upstream,
    },
    /// Upstream merged `from` into `to`.
    Redirected {
        /// The retired ID.
        from: EntityId,
        /// The target.
        to: EntityId,
        /// The merging version.
        upstream: Upstream,
    },
    /// The adapter could not shape an upstream entity as intended and reports it (0009
    /// §9: an invalid or duplicate identity value). Nothing is written for this item;
    /// an adapter that still has a state to write hands that over as a separate `State`.
    Rejected {
        /// The upstream ID.
        upstream_id: String,
        /// Why.
        reason: String,
    },
}

/// What a sync produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncOutcome {
    /// The job ID.
    pub job_id: u64,
    /// The counts on the finish record.
    pub counts: Counts,
    /// Entities the `snapshot` sweep tombstoned.
    pub swept: usize,
}

/// The upstream revision a recorded state stands for: its `revid`, or, for a state
/// re-written at a revision already recorded, the revision its `version` names
/// (`56+20261004`).
fn revision_of(upstream: &Upstream) -> Option<u64> {
    upstream
        .revid
        .or_else(|| upstream.version.as_deref()?.split_once('+')?.0.parse().ok())
}

/// The upstream revision the newest record of `id` stands for.
async fn recorded_revision<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    partition: u64,
    graph_name: &str,
    id: &EntityId,
) -> Result<Option<u64>, IngestError> {
    let Some(cursor) = store.cursor(cx, tenant, id, graph_name).await? else {
        return Ok(None);
    };
    Ok(previous(store, cx, partition, cursor.offset)
        .await?
        .and_then(|p| revision_of(&p.upstream)))
}

/// Whether a state carries nothing: what a cleared keyed entity holds.
fn is_cleared(entity: &Entity) -> bool {
    entity.labels.is_empty()
        && entity.descriptions.is_empty()
        && entity.aliases.is_empty()
        && entity.statements.is_empty()
        && entity.sitelinks.is_empty()
}

/// Whether an upstream deletion of `id` clears the mirror's contribution with an empty
/// `put` rather than a `tombstone`: always for a keyed entity, and for every entity of a
/// provider registered `deletion = clear` (architecture 05 §2.1, §2.2). A source's
/// entity is in the colon shape but is a foreign entity (0078 §2), so it is tombstoned.
fn clears(provider: &Provider, id: &EntityId) -> bool {
    (id.form() == IdForm::Keyed && !provider.source) || provider.deletion == Deletion::Clear
}

/// The header key an entity's mirror records are filed under: the ID, or a keyed
/// entity's surrogate when it has one (`None` before its first record).
async fn key_of<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    id: &EntityId,
) -> Result<Option<String>, IngestError> {
    if id.form() != IdForm::Keyed {
        return Ok(Some(id.as_str().to_string()));
    }
    let Some((keyed_type, key)) = id.keyed_parts() else {
        return Ok(None);
    };
    // A source's entity is in the colon shape with no surrogate (0078 §2).
    match store.surrogate(cx, keyed_type, key).await? {
        Some(n) => Ok(Some(format!("{keyed_type}#{n}"))),
        None => Ok(Some(id.as_str().to_string())),
    }
}

/// Whether a state is not written: an older revision than the one recorded, or the same
/// state under the same mapping. A state that differs at a revision already recorded (the
/// mapping moved: an alias, a key, a value pointing at a moved item) is written, but
/// cannot claim that revision again ([`put_of`]).
fn unchanged_state(
    prev: Option<&Previous>,
    upstream: &Upstream,
    upstream_id: Option<&String>,
    upstream_aliases: &[String],
    entity: &Entity,
) -> bool {
    let Some(p) = prev else {
        return false;
    };
    let older = matches!(
        (upstream.revid, revision_of(&p.upstream)),
        (Some(a), Some(b)) if a < b
    );
    older
        || (p.upstream_id.as_ref() == upstream_id
            && p.upstream_aliases == upstream_aliases
            && p.entity.to_canonical_json() == entity.to_canonical_json())
}

/// The `put` that writes a state: against the previous one at the cursor, if any, and at
/// no revision `recorded` already stands for (the highest the entity's records claimed,
/// or the state's own), which it then records as a version instead (0092 §2).
#[allow(clippy::too_many_arguments)]
fn put_of(
    entity: Entity,
    upstream: Upstream,
    upstream_id: Option<String>,
    upstream_aliases: Vec<String>,
    prev: Option<&Previous>,
    cursor: Option<&crate::Cursor>,
    recorded: Option<u64>,
    now: u64,
    source_version: &str,
) -> Operation {
    let upstream = match (upstream.revid, recorded) {
        (Some(r), Some(seen)) if r <= seen => Upstream {
            revid: None,
            time: upstream.time,
            version: Some(format!("{r}+{source_version}")),
        },
        _ => upstream,
    };
    let size = entity.to_canonical_json().len() as u64;
    let (prev_upstream, first_seen, prev_size, changes) = match (prev, cursor) {
        (Some(p), Some(c)) => (
            Some(p.upstream.clone()),
            Some(p.first_seen.unwrap_or(c.synced_at)),
            Some(p.size.unwrap_or(c.size)),
            Some(Changes::between(Some(&p.entity), &entity)),
        ),
        _ => (None, Some(now), None, Some(Changes::between(None, &entity))),
    };
    Operation::Put {
        id: entity.id.clone(),
        entity,
        upstream,
        prev_upstream,
        first_seen,
        size: Some(size),
        prev_size,
        changes,
        delta: None,
        upstream_id,
        upstream_aliases,
    }
}

/// Whether a unit of items is written as a block ([`BlockUnit`]; 0013 A76): every subject
/// its own key and named once, no state mapped from another upstream ID, and no deletion
/// that clears. A unit of one item gains nothing from it. The caller also requires the
/// provider to own no keys, so that no mapping moves.
fn is_block_unit(unit: &[SyncItem], provider: &Provider) -> bool {
    if unit.len() < 2 {
        return false;
    }
    let mut subjects = BTreeSet::new();
    unit.iter().all(|item| match item {
        SyncItem::State {
            entity,
            upstream_id,
            ..
        } => {
            upstream_id.is_none()
                && entity.id.form() != IdForm::Keyed
                && subjects.insert(entity.id.clone())
        }
        SyncItem::Deleted { id, .. } => {
            id.form() != IdForm::Keyed && !clears(provider, id) && subjects.insert(id.clone())
        }
        SyncItem::Redirected { from, .. } => {
            from.form() != IdForm::Keyed && subjects.insert(from.clone())
        }
        SyncItem::Rejected { .. } => true,
    })
}

/// A unit's items to write as a block, their hashes reconciled: states, deletions and
/// redirects, each subject named once ([`is_block_unit`]).
#[derive(Default)]
struct BlockUnit {
    items: Vec<SyncItem>,
}

impl BlockUnit {
    /// The operations the unit writes, in item order, decided as the item-by-item path
    /// decides each ([`unchanged_state`], [`put_of`]) from what is read for the whole unit
    /// at once: the subjects' cursors, the records at them and the revisions their keys
    /// have claimed. Since no two items name the same subject, what one writes is nothing
    /// another reads. The states are reconciled with the provider's hashes, decoded
    /// against their previous records and decided on every core ([`par_map`]); the hash
    /// mismatches come back for the job. Items that write nothing are counted unchanged.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    async fn decide<S: IngestStore>(
        self,
        store: &S,
        cx: &mut S::Cx,
        tenant: &str,
        partition: u64,
        graph_name: &str,
        now: u64,
        source_version: &str,
        hasher: &Hasher,
        counts: &mut Counts,
    ) -> Result<(Vec<Operation>, Vec<HashMismatches>), IngestError> {
        let ids: Vec<EntityId> = self
            .items
            .iter()
            .filter_map(|item| match item {
                SyncItem::State { entity, .. } => Some(entity.id.clone()),
                SyncItem::Deleted { id, .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        let cursors = store.cursors(cx, tenant, &ids, graph_name).await?;
        let offsets: Vec<u64> = self
            .items
            .iter()
            .filter_map(|item| match item {
                SyncItem::State { entity, .. } => cursors.get(&entity.id).map(|c| c.offset),
                _ => None,
            })
            .collect();
        let records = store.read_many(cx, partition, &offsets).await?;
        let keys: Vec<String> = self
            .items
            .iter()
            .filter_map(|item| match item {
                SyncItem::State { entity, .. } => Some(entity.id.as_str().to_string()),
                _ => None,
            })
            .collect();
        let claimed = store.highest_revids(cx, partition, &keys).await?;
        // Each state on its own: reconciled, its previous state decoded, decided.
        let decided = par_map(self.items, |item| -> Result<Decided, IngestError> {
            let SyncItem::State {
                entity,
                upstream,
                upstream_id,
                upstream_aliases,
            } = item
            else {
                return Ok(Decided::Other(item));
            };
            let mut entity = *entity;
            let mismatches = hasher.reconcile(&mut entity);
            let cursor = cursors.get(&entity.id);
            let prev = match cursor.and_then(|c| records.get(&c.offset)) {
                Some(record) => as_previous(operation_in(record)?),
                None => None,
            };
            if unchanged_state(
                prev.as_ref(),
                &upstream,
                upstream_id.as_ref(),
                &upstream_aliases,
                &entity,
            ) {
                return Ok(Decided::Unchanged(mismatches));
            }
            let recorded = prev.as_ref().and_then(|p| revision_of(&p.upstream)).max(
                claimed
                    .get(entity.id.as_str())
                    .map(|&global| split_revid(global).1),
            );
            Ok(Decided::Put(
                Box::new(put_of(
                    entity,
                    upstream,
                    upstream_id,
                    upstream_aliases,
                    prev.as_ref(),
                    cursor,
                    recorded,
                    now,
                    source_version,
                )),
                mismatches,
            ))
        });
        let mut ops = Vec::with_capacity(decided.len());
        let mut mismatches = Vec::new();
        for d in decided {
            match d? {
                Decided::Unchanged(m) => {
                    counts.unchanged += 1;
                    mismatches.push(m);
                }
                Decided::Put(op, m) => {
                    ops.push(*op);
                    mismatches.push(m);
                }
                Decided::Other(SyncItem::Deleted { id, upstream }) => {
                    if cursors.contains_key(&id) {
                        ops.push(Operation::Tombstone { id, upstream });
                    } else {
                        counts.unchanged += 1;
                    }
                }
                Decided::Other(SyncItem::Redirected { from, to, upstream }) => {
                    // Rare enough in a dump to look up one at a time.
                    if already_redirected(store, cx, partition, &from, &to).await? {
                        counts.unchanged += 1;
                    } else {
                        ops.push(Operation::Redirect {
                            from,
                            to,
                            upstream: Some(upstream),
                        });
                    }
                }
                Decided::Other(_) => {}
            }
        }
        Ok((ops, mismatches))
    }
}

/// What [`BlockUnit::decide`] makes of one item on a worker thread.
enum Decided {
    /// A state already recorded, with its hash mismatches.
    Unchanged(HashMismatches),
    /// A state's `put`, with its hash mismatches.
    Put(Box<Operation>, HashMismatches),
    /// A deletion or a redirect, decided afterwards on the unit's connection.
    Other(SyncItem),
}

/// The previous `put` at a cursor.
struct Previous {
    entity: Entity,
    upstream: Upstream,
    first_seen: Option<u64>,
    size: Option<u64>,
    upstream_id: Option<String>,
    upstream_aliases: Vec<String>,
}

/// The operation recorded at an offset, if the record is there and is one.
async fn operation_at<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    partition: u64,
    offset: u64,
) -> Result<Option<Operation>, IngestError> {
    let Some(record) = store.read(cx, partition, offset).await? else {
        return Ok(None);
    };
    operation_in(&record)
}

/// The operation a record carries, if it is one and its content is not erased.
fn operation_in(record: &scatter_log::record::Record) -> Result<Option<Operation>, IngestError> {
    let Some(content) = record.body().content().value().map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    if content == Value::Null {
        return Ok(None);
    }
    Ok(Some(
        scatter_log::cbor::from_value(content).map_err(|e| e.to_string())?,
    ))
}

/// The previous state an operation at a cursor stands for: a `put`'s.
fn as_previous(op: Option<Operation>) -> Option<Previous> {
    match op {
        Some(Operation::Put {
            entity,
            upstream,
            first_seen,
            size,
            upstream_id,
            upstream_aliases,
            ..
        }) => Some(Previous {
            entity,
            upstream,
            first_seen,
            size,
            upstream_id,
            upstream_aliases,
        }),
        _ => None,
    }
}

/// The previous `put` at a cursor, if the record is there and is one.
async fn previous<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    partition: u64,
    offset: u64,
) -> Result<Option<Previous>, IngestError> {
    Ok(as_previous(
        operation_at(store, cx, partition, offset).await?,
    ))
}

/// The `put` of the empty state that clears `id`'s contribution to the mirror, or `None`
/// when there is nothing to clear: no record, or a state already cleared.
async fn clearing<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    tenant: &str,
    partition: u64,
    graph_name: &str,
    id: &EntityId,
    upstream: Upstream,
) -> Result<Option<Operation>, IngestError> {
    let Some(cursor) = store.cursor(cx, tenant, id, graph_name).await? else {
        return Ok(None);
    };
    let Some(prev) = previous(store, cx, partition, cursor.offset).await? else {
        return Ok(None);
    };
    if is_cleared(&prev.entity) && prev.upstream_id.is_none() {
        return Ok(None);
    }
    let entity = Entity::new(id.clone(), prev.entity.entity_type.clone());
    let changes = Changes::between(Some(&prev.entity), &entity);
    Ok(Some(Operation::Put {
        id: id.clone(),
        size: Some(entity.to_canonical_json().len() as u64),
        entity,
        upstream,
        prev_upstream: Some(prev.upstream),
        first_seen: Some(prev.first_seen.unwrap_or(cursor.synced_at)),
        prev_size: Some(prev.size.unwrap_or(cursor.size)),
        changes: Some(changes),
        delta: None,
        upstream_id: None,
        upstream_aliases: Vec::new(),
    }))
}

/// Whether the newest record under `from` is already this redirect.
async fn already_redirected<S: IngestStore>(
    store: &S,
    cx: &mut S::Cx,
    partition: u64,
    from: &EntityId,
    to: &EntityId,
) -> Result<bool, IngestError> {
    let Some((offset, _)) = store.latest_for_key(cx, partition, from.as_str()).await? else {
        return Ok(false);
    };
    Ok(matches!(
        operation_at(store, cx, partition, offset).await?,
        Some(Operation::Redirect { to: t, .. }) if &t == to
    ))
}

/// Runs a sync of `header.graph` (a `mirror/{slug}`) in `header.mode` (`upsert` or
/// `snapshot`), `batch` items per unit of work. `threshold` bounds the `snapshot` sweep.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_sync<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    header: JobHeader,
    attestation: Attestation,
    items: impl IntoIterator<Item = SyncItem>,
    threshold: usize,
    registry: &Registry,
    now: u64,
    batch: usize,
) -> Result<SyncOutcome, IngestError> {
    run_sync_in(
        store,
        pipeline,
        "",
        header,
        attestation,
        items,
        threshold,
        registry,
        now,
        batch,
    )
    .await
}

/// [`run_sync`] for a graph a tenant owns: a sync into a tenant's entity source writes
/// `source/{name}` (0078 §4), whose records, job and cursors are the tenant's, with the
/// tenant's table of names as `registry` ([`Registry::with_sources`]). `tenant` is `""`
/// for a provider's mirror.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_sync_in<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    tenant: &str,
    header: JobHeader,
    attestation: Attestation,
    items: impl IntoIterator<Item = SyncItem>,
    threshold: usize,
    registry: &Registry,
    now: u64,
    batch: usize,
) -> Result<SyncOutcome, IngestError> {
    let graph = header.graph();
    let (graph, slug) = match &graph {
        Some(g @ Graph::Mirror(slug)) if tenant.is_empty() => (g.clone(), slug.clone()),
        Some(g @ Graph::Source(name)) if !tenant.is_empty() => (g.clone(), name.clone()),
        _ => {
            return Err(IngestError::Adapter(format!(
                "a sync writes a mirror graph of the instance or a source graph of a tenant, not `{}` for `{tenant}`",
                header.graph
            )));
        }
    };
    let mode = header.mode.unwrap_or(Mode::Upsert);
    if mode == Mode::Adopt {
        return Err(IngestError::Adapter(
            "a sync is `upsert` or `snapshot`".into(),
        ));
    }
    let provider = registry
        .by_slug(&slug)
        .ok_or_else(|| IngestError::UnknownProvider(slug.clone()))?;
    let hasher = Hasher::mirrored_from(&provider.code);
    let graph_name = graph.name();
    // The provider's own form of a foreign ID (`XDQ9` → `Q9`), and back.
    let upstream_of = |id: &EntityId| -> Option<String> {
        (id.form() != IdForm::Local)
            .then(|| registry.parse_foreign_id(id.as_str()).ok())
            .flatten()
            .map(|f| f.upstream_id())
    };
    let foreign_of = |upstream: &str| -> Option<EntityId> {
        registry
            .from_upstream(provider, upstream)
            .ok()
            .and_then(|f| EntityId::parse(&f.to_string()).ok())
    };
    // The job's version, for the records it writes without an upstream state of their
    // own: clears and the sweep's tombstones.
    let job_version = Upstream::version(header.source_version().unwrap_or(""));

    let mut cx = store.begin().await?;
    let partition = store
        .partition(&mut cx, tenant, &graph_name)
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: tenant.to_string(),
            graph: graph_name.clone(),
        })?;
    // Who owned which key before this job (0009 §9).
    let owners_before = store.keyed_owners(&mut cx, &graph_name).await?;
    let mut job = Job::start(
        store,
        pipeline,
        &mut cx,
        tenant,
        header.clone(),
        attestation,
        now,
    )
    .await?;
    let mut request = Request::new(tenant, job.record_attestation(), now);
    request.budget = Budget::NONE;
    let mut counts = Counts::default();
    let mut seen: BTreeSet<EntityId> = BTreeSet::new();
    // Upstream items the job saw in any form, the keys mapped items claimed, and who
    // owns each key now: what the clearing of moved keys is decided from.
    let mut upstream_seen: BTreeSet<String> = BTreeSet::new();
    let mut claimed: BTreeSet<EntityId> = BTreeSet::new();
    let mut owner_now: BTreeMap<String, EntityId> = BTreeMap::new();
    // The foreign entities in the mirror, loaded the first time a mapped item arrives:
    // whether a newly mapped item's foreign form has to be tombstoned.
    let mut foreign_present: Option<BTreeSet<EntityId>> = None;
    // Rejects are numbered in the order they arrive; a sync has no batch lines.
    let mut rejected_line = 1usize;

    // A unit of `batch` items at a time, each its own unit of work. A unit of the common
    // kind is read, decided and written as a block (0013 A76); any other is written item
    // by item, below, which a block writes the same records as.
    let mut items = items.into_iter();
    let mut first_unit = true;
    loop {
        let unit: Vec<SyncItem> = items.by_ref().take(batch.max(1)).collect();
        if unit.is_empty() {
            break;
        }
        if !first_unit {
            store.commit(cx).await?;
            cx = store.begin().await?;
        }
        first_unit = false;
        if owners_before.is_empty() && is_block_unit(&unit, provider) {
            let mut block = BlockUnit::default();
            for item in unit {
                match item {
                    SyncItem::State {
                        entity,
                        upstream,
                        upstream_id,
                        mut upstream_aliases,
                    } => {
                        upstream_aliases.sort();
                        upstream_aliases.dedup();
                        seen.insert(entity.id.clone());
                        if let Some(u) = upstream_of(&entity.id) {
                            upstream_seen.insert(u);
                        }
                        block.items.push(SyncItem::State {
                            entity,
                            upstream,
                            upstream_id,
                            upstream_aliases,
                        });
                    }
                    SyncItem::Deleted { ref id, .. }
                    | SyncItem::Redirected { from: ref id, .. } => {
                        seen.insert(id.clone());
                        if let Some(u) = upstream_of(id) {
                            upstream_seen.insert(u);
                        }
                        if let Some(present) = foreign_present.as_mut() {
                            present.remove(id);
                        }
                        block.items.push(item);
                    }
                    SyncItem::Rejected {
                        upstream_id,
                        reason,
                    } => {
                        counts.rejected += 1;
                        job.reject(Reject {
                            line: rejected_line,
                            match_key: None,
                            reason: format!("{upstream_id}: {reason}"),
                        });
                        rejected_line += 1;
                    }
                }
            }
            let source_version = header.source_version().unwrap_or("").to_string();
            let (ops, mismatches) = block
                .decide(
                    store,
                    &mut cx,
                    tenant,
                    partition,
                    &graph_name,
                    now,
                    &source_version,
                    &hasher,
                    &mut counts,
                )
                .await?;
            for m in &mismatches {
                for (kind, n) in m.by_kind() {
                    job.mismatch(kind, *n);
                }
            }
            crate::write::write_block(
                store, pipeline, &mut cx, &request, partition, &ops, &graph, registry,
            )
            .await?;
            for op in &ops {
                count(op, &mut counts);
            }
            continue;
        }
        for item in unit {
            let mut ops: Vec<Operation> = Vec::new();
            match item {
                SyncItem::State {
                    entity,
                    upstream,
                    upstream_id,
                    mut upstream_aliases,
                } => {
                    let mut entity = *entity;
                    let mismatches = hasher.reconcile(&mut entity);
                    for (kind, n) in mismatches.by_kind() {
                        job.mismatch(kind, *n);
                    }
                    upstream_aliases.sort();
                    upstream_aliases.dedup();
                    seen.insert(entity.id.clone());
                    // The subjects the same upstream item was recorded under before, when the
                    // mapping moved it: a foreign item now mapped, a key it owned.
                    let mut moved_from: Vec<EntityId> = Vec::new();
                    if let Some(u) = upstream_of(&entity.id) {
                        if let Some(key) = owners_before.get(&u) {
                            moved_from.push(key.clone());
                        }
                        upstream_seen.insert(u);
                    }
                    if let Some(u) = &upstream_id {
                        upstream_seen.insert(u.clone());
                        upstream_seen.extend(upstream_aliases.iter().cloned());
                        owner_now.insert(u.clone(), entity.id.clone());
                        claimed.insert(entity.id.clone());
                        // A mapped item's upstream ID never appears as a subject (05 §4.1):
                        // the foreign items it and its merged aliases were are retired.
                        if foreign_present.is_none() {
                            foreign_present = Some(
                                store
                                    .entities_of_graph(&mut cx, tenant, &graph_name)
                                    .await?
                                    .into_iter()
                                    .filter(|id| id.form() == IdForm::Foreign)
                                    .collect(),
                            );
                        }
                        let present = foreign_present.as_mut().expect("loaded");
                        if let Some(own) = foreign_of(u)
                            && present.contains(&own)
                        {
                            moved_from.push(own);
                        }
                        if let Some(key) = owners_before.get(u)
                            && key != &entity.id
                        {
                            moved_from.push(key.clone());
                        }
                        for retired in std::iter::once(u)
                            .chain(upstream_aliases.iter())
                            .filter_map(|u| foreign_of(u))
                        {
                            // The job's version, not the state's: a mirror record's
                            // revision ID comes from its upstream revision, which belongs
                            // to the state's own `put`.
                            if present.remove(&retired) {
                                ops.push(Operation::Tombstone {
                                    id: retired,
                                    upstream: job_version.clone(),
                                });
                            }
                        }
                    }
                    let cursor = store
                        .cursor(&mut cx, tenant, &entity.id, &graph_name)
                        .await?;
                    let prev = match &cursor {
                        Some(c) => previous(store, &mut cx, partition, c.offset).await?,
                        None => None,
                    };
                    if unchanged_state(
                        prev.as_ref(),
                        &upstream,
                        upstream_id.as_ref(),
                        &upstream_aliases,
                        &entity,
                    ) {
                        counts.unchanged += 1;
                    } else {
                        let mut recorded = prev.as_ref().and_then(|p| revision_of(&p.upstream));
                        // A tombstone or a redirect at the cursor hides the revisions the
                        // entity's earlier states claimed; an undelete brings the entity
                        // back at one of them (0092 §2), which no record may claim twice.
                        if let Some(key) = key_of(store, &mut cx, &entity.id).await? {
                            let claimed = store
                                .highest_revid(&mut cx, partition, &key)
                                .await?
                                .map(|global| split_revid(global).1);
                            recorded = recorded.max(claimed);
                        }
                        for old in &moved_from {
                            let r = recorded_revision(
                                store,
                                &mut cx,
                                tenant,
                                partition,
                                &graph_name,
                                old,
                            )
                            .await?;
                            recorded = recorded.max(r);
                        }
                        // The state first, then the retirements it causes.
                        ops.insert(
                            0,
                            put_of(
                                entity,
                                upstream,
                                upstream_id,
                                upstream_aliases,
                                prev.as_ref(),
                                cursor.as_ref(),
                                recorded,
                                now,
                                header.source_version().unwrap_or(""),
                            ),
                        );
                    }
                }
                SyncItem::Deleted { id, upstream } => {
                    seen.insert(id.clone());
                    if let Some(u) = upstream_of(&id) {
                        upstream_seen.insert(u);
                    }
                    if let Some(present) = foreign_present.as_mut() {
                        present.remove(&id);
                    }
                    if clears(provider, &id) {
                        match clearing(
                            store,
                            &mut cx,
                            tenant,
                            partition,
                            &graph_name,
                            &id,
                            upstream,
                        )
                        .await?
                        {
                            Some(op) => ops.push(op),
                            None => counts.unchanged += 1,
                        }
                    } else if store
                        .cursor(&mut cx, tenant, &id, &graph_name)
                        .await?
                        .is_some()
                    {
                        ops.push(Operation::Tombstone { id, upstream });
                    } else {
                        counts.unchanged += 1;
                    }
                }
                SyncItem::Redirected { from, to, upstream } => {
                    seen.insert(from.clone());
                    if let Some(u) = upstream_of(&from) {
                        upstream_seen.insert(u);
                    }
                    if let Some(present) = foreign_present.as_mut() {
                        present.remove(&from);
                    }
                    if already_redirected(store, &mut cx, partition, &from, &to).await? {
                        counts.unchanged += 1;
                    } else {
                        ops.push(Operation::Redirect {
                            from,
                            to,
                            upstream: Some(upstream),
                        });
                    }
                }
                SyncItem::Rejected {
                    upstream_id,
                    reason,
                } => {
                    counts.rejected += 1;
                    job.reject(Reject {
                        line: rejected_line,
                        match_key: None,
                        reason: format!("{upstream_id}: {reason}"),
                    });
                    rejected_line += 1;
                }
            }
            for op in ops {
                write_counted(
                    store,
                    pipeline,
                    &mut cx,
                    &request,
                    &op,
                    &graph,
                    registry,
                    &mut counts,
                )
                .await?;
            }
        }
    }

    // A key whose owner the job saw but which no item claims any longer (its identity
    // value changed, it became a duplicate or invalid, or it was merged away) loses the
    // mirror's state (05 §4.2). An item the job did not see is left to the sweep.
    for (upstream_id, key) in &owners_before {
        if upstream_seen.contains(upstream_id)
            && owner_now.get(upstream_id) != Some(key)
            && !claimed.contains(key)
        {
            seen.insert(key.clone());
            if let Some(op) = clearing(
                store,
                &mut cx,
                tenant,
                partition,
                &graph_name,
                key,
                job_version.clone(),
            )
            .await?
            {
                write_counted(
                    store,
                    pipeline,
                    &mut cx,
                    &request,
                    &op,
                    &graph,
                    registry,
                    &mut counts,
                )
                .await?;
            }
        }
    }

    // The snapshot sweep (0002 §8.4): after the job completes, remove what it did not
    // see, unless a truncated input would wipe the mirror. Keyed entities, and every
    // entity of a `deletion = clear` provider, are cleared rather than tombstoned, and
    // one already cleared is left alone.
    let mut swept = 0usize;
    let mut extra = serde_json::Map::new();
    if mode == Mode::Snapshot {
        let present = store
            .entities_of_graph(&mut cx, tenant, &graph_name)
            .await?;
        let mut sweep = Vec::new();
        for id in present.into_iter().filter(|id| !seen.contains(id)) {
            if clears(provider, &id) {
                if let Some(op) = clearing(
                    store,
                    &mut cx,
                    tenant,
                    partition,
                    &graph_name,
                    &id,
                    job_version.clone(),
                )
                .await?
                {
                    sweep.push(op);
                }
            } else {
                sweep.push(Operation::Tombstone {
                    id,
                    upstream: job_version.clone(),
                });
            }
        }
        if sweep.len() > threshold {
            let err = IngestError::SweepThreshold {
                count: sweep.len(),
                threshold,
            };
            job.fail(store, pipeline, &mut cx, &err.to_string(), now)
                .await?;
            store.commit(cx).await?;
            return Err(err);
        }
        for op in sweep {
            write_counted(
                store,
                pipeline,
                &mut cx,
                &request,
                &op,
                &graph,
                registry,
                &mut counts,
            )
            .await?;
            swept += 1;
        }
        extra.insert(
            "sweep".into(),
            serde_json::json!({"count": swept, "threshold": threshold}),
        );
    }
    job.finish(store, pipeline, &mut cx, &counts, extra, now)
        .await?;
    store.commit(cx).await?;
    Ok(SyncOutcome {
        job_id: job.id,
        counts,
        swept,
    })
}

/// Writes one operation and counts it: a new subject is `created`, a clearing `put` is
/// `cleared`, another `put` is `merged`.
#[allow(clippy::too_many_arguments)]
async fn write_counted<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    request: &Request,
    op: &Operation,
    graph: &Graph,
    registry: &Registry,
    counts: &mut Counts,
) -> Result<(), IngestError> {
    write_to(store, pipeline, cx, request, op, graph, registry).await?;
    count(op, counts);
    Ok(())
}

/// Counts a written operation: a new subject is `created`, a clearing `put` is `cleared`,
/// another `put` is `merged`.
fn count(op: &Operation, counts: &mut Counts) {
    match op {
        Operation::Put {
            prev_upstream: None,
            ..
        } => counts.created += 1,
        Operation::Put {
            entity,
            upstream_id: None,
            ..
        } if is_cleared(entity) => counts.cleared += 1,
        Operation::Put { .. } => counts.merged += 1,
        Operation::Tombstone { .. } => counts.tombstoned += 1,
        _ => counts.other += 1,
    }
}
