//! Bootstrap mode (0013 §9; 05 §3.7–3.8): the initial load of an empty mirror partition.
//!
//! An initial load has no previous state to read, so every record it writes is known from
//! the dump alone: a new entity's `put` (prev_upstream, prev_size and delta absent,
//! `first_seen` now, the changes against nothing) as [`crate::sync::run_sync_in`] writes for
//! a subject with no cursor. That is what lets the work be split. [`Prepare::item`] turns
//! one sync item into a draft and its body's commitment, touching no store: the hash check,
//! the operation, its canonical encoding and the part hashes, which a caller runs on worker
//! threads. [`run_bootstrap`] takes the prepared records in dump order and, a block at a
//! time, gives them page IDs from one reservation, appends them with
//! [`IngestStore::append_many`] (binary `COPY` in Postgres) and commits.
//!
//! The projections run during the load, over each block at once
//! ([`scatter_projection::Pipeline::apply_inline_many`]; 0013 A71): set-based where a
//! projection has a form for it, record by record otherwise, so the record table keeps its
//! key index for that path. A load runs in units of `batch`
//! records, each committed: a load that fails part way leaves a partition without holes,
//! which a rerun (no longer empty) continues through the ordinary sync, whose version
//! cursor skips what is already there. The coordinator table of 05 §3.8, for writers in
//! several processes resuming a block, is not needed by one in-process coordinator and is
//! not built.
//!
//! A block's commit does not wait for the disk ([`IngestStore::commit_waits_for_disk`];
//! 0013 A72): a crash loses at most the last blocks committed, which the rerun loads
//! again; the last unit, which finishes the job, waits, and so makes the whole load
//! durable once it returns.
//!
//! The tables a load writes grow from nothing faster than the database's own statistics
//! follow, so the load refreshes them ([`IngestStore::refresh_statistics`]) each time the
//! number of blocks written doubles (0013 A73): a plan chosen for an empty table scans every row
//! written so far on every block, which makes a load quadratic.
//!
//! A [`Prepare`] may be given the projections' per-record work ([`Warm`]; 0013 A74), which
//! then runs on the threads that prepare the records, ahead of the coordinator, so that
//! applying a block is little more than its statements.
//!
//! Only an instance mirror whose subjects are their own keys is bootstrapped: a keyed
//! subject's surrogate is minted on the write path (0009 §7), so a key-mapped provider
//! loads through the ordinary sync.

use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::hash::Hash;
use scatter_log::record::{Record, global_id};
use scatter_log::store::Draft;
use scatter_projection::{Budget, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{Changes, Graph, JobHeader, Mode, Operation};
use scatter_wikibase_model::hash::{HashMismatches, Hasher};
use scatter_wikibase_model::id::{EntityId, IdForm};

use crate::IngestError;
use crate::job::{Counts, Job, Reject};
use crate::store::{IngestStore, Sequence};
use crate::sync::{SyncItem, SyncOutcome};
use crate::write::{Attestation, PAYLOAD_CHANGESET};

/// What preparing a record needs, fixed for the whole load: shared by the worker threads.
#[derive(Clone)]
pub struct Prepare {
    hasher: Hasher,
    attestation: Value,
    now: u64,
    provider_number: u64,
    graph: String,
    warm: Option<Warm>,
}

/// Work the projections do per record, run where the record is prepared (0013 A74): given
/// the graph, the record's body and the operation it carries, a projection can compute
/// ahead, off the coordinator, what it would otherwise compute from the record when its
/// block is applied. The records are appended as prepared, so the content bytes it sees
/// are the record's.
pub type Warm = std::sync::Arc<dyn Fn(&str, &Body, Operation) + Send + Sync>;

impl std::fmt::Debug for Prepare {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Prepare")
            .field("hasher", &self.hasher)
            .field("now", &self.now)
            .field("provider_number", &self.provider_number)
            .field("graph", &self.graph)
            .field("warm", &self.warm.is_some())
            .finish_non_exhaustive()
    }
}

/// One sync item, prepared.
#[derive(Debug)]
pub enum Prepared {
    /// A record to append, its body's commitment computed. The draft has no page ID yet:
    /// [`run_bootstrap`] assigns them in order.
    Record {
        /// The draft.
        draft: Draft,
        /// `draft.body.commitment()`.
        commitment: Hash,
        /// What the hash check found, for the job's counts.
        mismatches: HashMismatches,
        /// Whether it is a new subject's `put` (`created`), else a redirect (`other`).
        created: bool,
    },
    /// The adapter's reject, for the job's rejects (its state, if any, follows separately).
    Rejected {
        /// The upstream ID.
        upstream_id: String,
        /// Why.
        reason: String,
    },
    /// Nothing to write: an upstream deletion of an entity the partition never held.
    Unchanged,
}

impl Prepare {
    /// The same, with `warm` run on each record prepared ([`Warm`]).
    #[must_use]
    pub fn warmed_by(mut self, warm: Warm) -> Self {
        self.warm = Some(warm);
        self
    }

    /// The record a sync item becomes in an empty partition. Touches no store.
    pub fn item(&self, item: SyncItem) -> Result<Prepared, IngestError> {
        match item {
            SyncItem::State {
                entity,
                upstream,
                upstream_id,
                mut upstream_aliases,
            } => {
                let mut entity = *entity;
                let mismatches = self.hasher.reconcile(&mut entity);
                upstream_aliases.sort();
                upstream_aliases.dedup();
                let size = entity.to_canonical_json().len() as u64;
                let changes = Changes::between(None, &entity);
                let id = entity.id.clone();
                let revid = upstream.revid;
                let op = Operation::Put {
                    id: id.clone(),
                    entity,
                    upstream,
                    prev_upstream: None,
                    first_seen: Some(self.now),
                    size: Some(size),
                    prev_size: None,
                    changes: Some(changes),
                    delta: None,
                    upstream_id,
                    upstream_aliases,
                };
                let (draft, commitment) = self.draft(&id, revid, op)?;
                Ok(Prepared::Record {
                    draft,
                    commitment,
                    mismatches,
                    created: true,
                })
            }
            SyncItem::Redirected { from, to, upstream } => {
                let revid = upstream.revid;
                let op = Operation::Redirect {
                    from: from.clone(),
                    to,
                    upstream: Some(upstream),
                };
                let (draft, commitment) = self.draft(&from, revid, op)?;
                Ok(Prepared::Record {
                    draft,
                    commitment,
                    mismatches: HashMismatches::default(),
                    created: false,
                })
            }
            SyncItem::Deleted { .. } => Ok(Prepared::Unchanged),
            SyncItem::Rejected {
                upstream_id,
                reason,
            } => Ok(Prepared::Rejected {
                upstream_id,
                reason,
            }),
        }
    }

    /// The draft of a mirror operation on `subject`, as [`crate::write::write_to`] builds it
    /// (the subject its own key, the provider-ranged revision ID, no comment), and its
    /// body's commitment, the operation then handed to [`Warm`] if there is one. Mirror
    /// operations carry their adapter's statement IDs, so none are filled in.
    fn draft(
        &self,
        subject: &EntityId,
        upstream_revid: Option<u64>,
        op: Operation,
    ) -> Result<(Draft, Hash), IngestError> {
        if subject.form() == IdForm::Keyed {
            return Err(IngestError::Adapter(format!(
                "`{subject}` is keyed: its surrogate is minted on the write path, so its provider loads through the ordinary sync"
            )));
        }
        let revid = upstream_revid
            .map(|n| {
                global_id(self.provider_number, n).ok_or_else(|| {
                    IngestError::Adapter(format!("upstream revision {n} is out of range"))
                })
            })
            .transpose()?;
        let content = serde_json::to_value(&op).map_err(|e| e.to_string())?;
        let body = Body::core(&Value::from_json(&content), &Value::Null, &self.attestation)
            .map_err(|e| e.to_string())?;
        drop(content);
        let commitment = body.commitment();
        if let Some(warm) = &self.warm {
            warm(&self.graph, &body, op);
        }
        Ok((
            Draft {
                appended_at: self.now,
                payload_type: PAYLOAD_CHANGESET.into(),
                key: Some(subject.as_str().to_string()),
                revid,
                logid: None,
                page_id: None,
                body,
            },
            commitment,
        ))
    }
}

/// Loads an empty instance mirror partition from prepared records (see the module).
/// `header` names the `mirror/{slug}` graph and the mode, as for [`crate::sync::run_sync`];
/// `items` is given the [`Prepare`] for this job (its attestation names the job) and returns
/// the prepared records in dump order, typically from worker threads. A non-empty partition
/// is refused: an incremental load is the ordinary sync's.
///
/// # Errors
///
/// When the graph is not an instance mirror, the partition holds records, an item cannot
/// be prepared or the store fails.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub async fn run_bootstrap<S, F, I>(
    store: &S,
    pipeline: &Pipeline<S>,
    header: JobHeader,
    attestation: Attestation,
    registry: &Registry,
    now: u64,
    batch: usize,
    items: F,
) -> Result<SyncOutcome, IngestError>
where
    S: IngestStore,
    F: FnOnce(Prepare) -> I,
    I: IntoIterator<Item = Result<Prepared, IngestError>>,
{
    let graph = header.graph();
    let Some(Graph::Mirror(slug)) = &graph else {
        return Err(IngestError::Adapter(format!(
            "bootstrap mode loads an instance mirror, not `{}`",
            header.graph
        )));
    };
    if header.mode.unwrap_or(Mode::Upsert) == Mode::Adopt {
        return Err(IngestError::Adapter(
            "a sync is `upsert` or `snapshot`".into(),
        ));
    }
    let provider = registry
        .by_slug(slug)
        .ok_or_else(|| IngestError::UnknownProvider(slug.clone()))?;
    let graph_name = format!("mirror/{slug}");

    let mut cx = store.begin().await?;
    let partition = store
        .partition(&mut cx, "", &graph_name)
        .await?
        .ok_or_else(|| IngestError::NoPartition {
            tenant: String::new(),
            graph: graph_name.clone(),
        })?;
    let size = store.partition_size(&mut cx, partition).await?;
    if size != 0 {
        return Err(IngestError::Adapter(format!(
            "bootstrap mode needs an empty partition; `{graph_name}` holds {size} records"
        )));
    }
    let mut job = Job::start(store, pipeline, &mut cx, "", header, attestation, now).await?;
    let prepare = Prepare {
        hasher: Hasher::mirrored_from(&provider.code),
        attestation: job.record_attestation().to_value(),
        now,
        provider_number: u64::from(provider.number),
        graph: graph_name.clone(),
        warm: None,
    };
    let batch = batch.max(1);
    let mut counts = Counts::default();
    let mut rejected_line = 1usize;
    let mut block: Vec<(Draft, Hash)> = Vec::with_capacity(batch);
    let mut blocks = 0u64;
    for prepared in items(prepare) {
        match prepared? {
            Prepared::Record {
                draft,
                commitment,
                mismatches,
                created,
            } => {
                for (kind, n) in mismatches.by_kind() {
                    job.mismatch(kind, *n);
                }
                if created {
                    counts.created += 1;
                } else {
                    counts.other += 1;
                }
                block.push((draft, commitment));
                if block.len() >= batch {
                    write_block(
                        store,
                        pipeline,
                        &mut cx,
                        partition,
                        std::mem::take(&mut block),
                    )
                    .await?;
                    store.commit(cx).await?;
                    cx = store.begin().await?;
                    store.commit_waits_for_disk(&mut cx, false).await?;
                    blocks += 1;
                    // The tables have doubled since the last refresh (0013 A73): about 20 for a
                    // full Wikidata load.
                    if blocks.is_power_of_two() {
                        store.refresh_statistics(&mut cx).await?;
                        store.commit(cx).await?;
                        cx = store.begin().await?;
                        store.commit_waits_for_disk(&mut cx, false).await?;
                    }
                }
            }
            Prepared::Rejected {
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
            Prepared::Unchanged => counts.unchanged += 1,
        }
    }
    // The last unit waits for the disk, and with it every block committed before it.
    store.commit_waits_for_disk(&mut cx, true).await?;
    write_block(store, pipeline, &mut cx, partition, block).await?;
    job.finish(
        store,
        pipeline,
        &mut cx,
        &counts,
        serde_json::Map::new(),
        now,
    )
    .await?;
    store.commit(cx).await?;
    Ok(SyncOutcome {
        job_id: job.id,
        counts,
        swept: 0,
    })
}

/// Gives a block its page IDs from one reservation, appends it, and runs the synchronous
/// projections over the block ([`Pipeline::apply_inline_many`]): set-based where a
/// projection has a form for it, record by record otherwise.
async fn write_block<S: IngestStore>(
    store: &S,
    pipeline: &Pipeline<S>,
    cx: &mut S::Cx,
    partition: u64,
    mut block: Vec<(Draft, Hash)>,
) -> Result<(), IngestError> {
    if block.is_empty() {
        return Ok(());
    }
    let pages = store.next_ids(cx, "", &Sequence::Page, block.len()).await?;
    for ((draft, _), page) in block.iter_mut().zip(pages) {
        draft.page_id = Some(page);
    }
    let records: Vec<Record> = store.append_many(cx, partition, block).await?;
    pipeline
        .apply_inline_many(store, cx, &records, Budget::NONE)
        .await?;
    Ok(())
}
