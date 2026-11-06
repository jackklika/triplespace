//! What a job needs of the store, beyond the projection [`Backend`] it extends: appending
//! inside the same unit of work the projections run in (0013 §7), the per-tenant
//! sequences and their floors (0013 §6; 0035 §4), the version cursor (0002 §8.4), match
//! keys (0002 §8.5), surrogates (0009 §7) and the property type map (0002 §8.6).
//!
//! The Postgres implementation lives with the surfaces; [`crate::memory::MemoryIngest`]
//! is the in-memory one the tests use.

use std::collections::BTreeMap;
use std::future::Future;

use scatter_log::hash::Hash;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::{Appended, Draft};
use scatter_projection::Backend;
use scatter_wikibase_changeset::MatchKey;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::DataType;

use crate::IngestError;

/// A per-tenant sequence (0013 §6). The instance's own sequences are under the tenant `""`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sequence {
    /// Header field 7 of a `local` or `pages` record.
    Revision,
    /// Header field 8 of a record that projects as a log event.
    Log,
    /// Header field 9, the first time a key is written.
    Page,
    /// A minted entity type's counter, by type name (`item`, `property`).
    Entity(String),
    /// A keyed type's surrogate counter (0009 §7), instance-wide.
    Surrogate(String),
    /// Job IDs (0011 §6.3), minted by the instance.
    Job,
    /// Local user IDs (0007 §3), per tenant.
    User,
    /// The tenant's surrogates, `actor:{n}` (0007 §5, A27).
    ActorSurrogate,
}

/// A graph's current record for an entity: the version cursor of 0002 §8.4.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    /// The record's offset in the graph's partition.
    pub offset: u64,
    /// The upstream version the record carries, if any.
    pub version: Option<String>,
    /// `H(0x03 ‖ content)` of the record.
    pub content_hash: [u8; 32],
    /// The record's `appended_at`, µs since the epoch.
    pub synced_at: u64,
    /// The size the record declared, or its content length.
    pub size: u64,
}

/// What the store knows beyond the pipeline's backend.
pub trait IngestStore: Backend {
    /// Appends a draft to a partition inside the unit of work.
    fn append(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        draft: Draft,
    ) -> impl Future<Output = Result<Appended, IngestError>> + Send;

    /// [`Self::append`], returning the record the draft became: sealing hashes every part
    /// of its body, and the write path goes on to project the record. The default seals
    /// it again; a store that sealed it to append it returns that.
    fn append_sealed(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        draft: Draft,
    ) -> impl Future<Output = Result<(Appended, Record), IngestError>> + Send {
        async move {
            let appended = self.append(cx, partition, draft.clone()).await?;
            let record = draft.seal(partition, appended.offset);
            Ok((appended, record))
        }
    }

    /// Hands the projections an operation the write path has just encoded into `body`,
    /// before the record is appended: a projection that would otherwise decode the
    /// record's content can keep what it was encoded from (0013 A74). The default does
    /// nothing.
    fn encoded(&self, body: &scatter_log::body::Body, op: scatter_wikibase_changeset::Operation) {
        let _ = (body, op);
    }

    /// Appends drafts at consecutive offsets of a partition inside the unit of work, each
    /// with its body's commitment, which the caller computed: bootstrap mode's block write
    /// (0013 §9). Returns the records in order. The default appends one at a time; a store
    /// with a bulk path (binary `COPY` in Postgres) overrides it.
    fn append_many(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        drafts: Vec<(Draft, Hash)>,
    ) -> impl Future<Output = Result<Vec<Record>, IngestError>> + Send {
        async move {
            let mut out = Vec::with_capacity(drafts.len());
            for (draft, _) in drafts {
                let appended = self.append(cx, partition, draft.clone()).await?;
                out.push(draft.seal(partition, appended.offset));
            }
            Ok(out)
        }
    }

    /// Refreshes the query planner's statistics on the tables a load writes. A bootstrap
    /// load (0013 §9) grows them from nothing faster than a database's own statistics
    /// keep up with, and a plan chosen for an empty table (an index that narrows only by
    /// tenant) costs a scan of every row written so far, on every block. The default does
    /// nothing: a store without a planner has nothing to refresh.
    fn refresh_statistics(
        &self,
        cx: &mut Self::Cx,
    ) -> impl Future<Output = Result<(), IngestError>> + Send {
        let _ = cx;
        async { Ok(()) }
    }

    /// Whether the current unit of work's commit waits until it is on disk. A bootstrap
    /// load's blocks do not (0013 §9, A72): a crash loses the last blocks committed, never
    /// consistency, and the load's last unit, which waits, makes everything before it
    /// durable. Only for the unit of work in `cx`; the default does nothing, for a store
    /// whose commit has no such choice.
    fn commit_waits_for_disk(
        &self,
        cx: &mut Self::Cx,
        wait: bool,
    ) -> impl Future<Output = Result<(), IngestError>> + Send {
        let _ = (cx, wait);
        async { Ok(()) }
    }

    /// How many records a partition holds: its next offset. Zero is the empty partition a
    /// bootstrap load requires (0013 §9).
    fn partition_size(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
    ) -> impl Future<Output = Result<u64, IngestError>> + Send;

    /// The record at an offset; `None` when compacted away.
    fn read(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        offset: u64,
    ) -> impl Future<Output = Result<Option<Record>, IngestError>> + Send;

    /// The highest revision ID any record under a key in a partition carries (header
    /// field 7), whatever the record is: the revisions a mirror has claimed for an entity,
    /// a tombstone or a redirect in between notwithstanding (0092 §2: an undeleted entity
    /// comes back at a revision already recorded).
    fn highest_revid(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// The header of the newest record under a key in a partition, with its offset.
    fn latest_for_key(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        key: &str,
    ) -> impl Future<Output = Result<Option<(u64, Header)>, IngestError>> + Send;

    /// The page ID a key already has, in any partition the tenant reads (its own and the
    /// shared ones), since every record for a key repeats it (0015 §2).
    fn page_id_of(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// The partition of a graph: a tenant's `local`, `pages`, `log` or `actors`, or with
    /// the tenant `""` the instance's `log` or a `mirror/{slug}` (0018 §2).
    fn partition(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        graph: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// The next value of a sequence.
    fn next_id(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        sequence: &Sequence,
    ) -> impl Future<Output = Result<u64, IngestError>> + Send;

    /// The next `n` values of a sequence, in order: a block for a bootstrap load
    /// (0013 §9). The default takes them one at a time.
    fn next_ids(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        sequence: &Sequence,
        n: usize,
    ) -> impl Future<Output = Result<Vec<u64>, IngestError>> + Send {
        async move {
            let mut out = Vec::with_capacity(n);
            for _ in 0..n {
                out.push(self.next_id(cx, tenant, sequence).await?);
            }
            Ok(out)
        }
    }

    /// Raises a sequence past `consumed`, never lowering it (0035 §4).
    fn floor(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        sequence: &Sequence,
        consumed: u64,
    ) -> impl Future<Output = Result<(), IngestError>> + Send;

    /// A graph's cursor for an entity (`view.entity_source`).
    fn cursor(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        entity_id: &EntityId,
        graph: &str,
    ) -> impl Future<Output = Result<Option<Cursor>, IngestError>> + Send;

    /// [`Self::cursor`] for several entities at once: a sync's unit read set-based
    /// (0013 A76). Those with no cursor are absent. The default asks one at a time.
    fn cursors(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        entity_ids: &[EntityId],
        graph: &str,
    ) -> impl Future<Output = Result<BTreeMap<EntityId, Cursor>, IngestError>> + Send {
        async move {
            let mut out = BTreeMap::new();
            for id in entity_ids {
                if let Some(c) = self.cursor(cx, tenant, id, graph).await? {
                    out.insert(id.clone(), c);
                }
            }
            Ok(out)
        }
    }

    /// [`Self::read`] at several offsets of a partition at once, by offset; a compacted
    /// slot is absent. The default reads one at a time.
    fn read_many(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        offsets: &[u64],
    ) -> impl Future<Output = Result<BTreeMap<u64, Record>, IngestError>> + Send {
        async move {
            let mut out = BTreeMap::new();
            for &offset in offsets {
                if let Some(r) = self.read(cx, partition, offset).await? {
                    out.insert(offset, r);
                }
            }
            Ok(out)
        }
    }

    /// [`Self::highest_revid`] for several keys of a partition at once; keys with no
    /// revision are absent. The default asks one at a time.
    fn highest_revids(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
        keys: &[String],
    ) -> impl Future<Output = Result<BTreeMap<String, u64>, IngestError>> + Send {
        async move {
            let mut out = BTreeMap::new();
            for key in keys {
                if let Some(r) = self.highest_revid(cx, partition, key).await? {
                    out.insert(key.clone(), r);
                }
            }
            Ok(out)
        }
    }

    /// [`Self::page_id_of`] for several keys at once; keys with no page ID yet are absent.
    /// The default asks one at a time.
    fn page_ids_of(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        keys: &[String],
    ) -> impl Future<Output = Result<BTreeMap<String, u64>, IngestError>> + Send {
        async move {
            let mut out = BTreeMap::new();
            for key in keys {
                if let Some(p) = self.page_id_of(cx, tenant, key).await? {
                    out.insert(key.clone(), p);
                }
            }
            Ok(out)
        }
    }

    /// Every entity a graph currently contributes to, for the `snapshot` sweep.
    fn entities_of_graph(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        graph: &str,
    ) -> impl Future<Output = Result<Vec<EntityId>, IngestError>> + Send;

    /// The keyed entity each upstream item of a key-mapped provider owns
    /// (`view.keyed_map`, merge aliases excluded), as the mirror graph `graph` last
    /// recorded it: upstream item ID → keyed ID (0009 §9).
    fn keyed_owners(
        &self,
        cx: &mut Self::Cx,
        graph: &str,
    ) -> impl Future<Output = Result<BTreeMap<String, EntityId>, IngestError>> + Send;

    /// The entity a match key finds (`view.identifier` for an identifier key; the entity
    /// itself, if present, for a foreign ID).
    fn find_match(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        key: &MatchKey,
    ) -> impl Future<Output = Result<Option<EntityId>, IngestError>> + Send;

    /// The surrogate of a keyed entity, if one was minted (`view.keyed_surrogate`).
    fn surrogate(
        &self,
        cx: &mut Self::Cx,
        keyed_type: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>, IngestError>> + Send;

    /// A property's data type, from the resolved view.
    fn datatype(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        property: &EntityId,
    ) -> impl Future<Output = Result<Option<DataType>, IngestError>> + Send;

    /// Whether every change-set record in a partition is an `adopt` (0035 §2,
    /// precondition 3): true for an empty partition.
    fn only_adoptions(
        &self,
        cx: &mut Self::Cx,
        partition: u64,
    ) -> impl Future<Output = Result<bool, IngestError>> + Send;

    /// The surrogate actor key, `actor:{n}`, a tenant already minted for a raw value of a
    /// surrogate kind (`anonymous`, `imported`), from its actor records (0007 §5, A27).
    fn surrogate_actor(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
        kind: &str,
        raw: &str,
    ) -> impl Future<Output = Result<Option<String>, IngestError>> + Send;

    /// The sources of the tenant's earlier adoption jobs (`view.job` with mode `adopt`).
    fn adoption_sources(
        &self,
        cx: &mut Self::Cx,
        tenant: &str,
    ) -> impl Future<Output = Result<Vec<String>, IngestError>> + Send;
}
