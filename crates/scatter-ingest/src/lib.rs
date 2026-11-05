//! Ingest (0002 §8; 0011 §6.3; 0035): the write path that turns operations into records,
//! and the jobs that run it in bulk.
//!
//! - [`mod@write`]: one operation becomes one record in the right partition, with its IDs
//!   taken from the tenant's sequences, its key (an entity ID or a keyed entity's
//!   surrogate), its attestation, and the base-offset check of 0006 §8; then the
//!   synchronous projections run on it in the same unit of work (0013 §7).
//! - [`job`]: a job's records in the log — `start`, `finish`, `fail` — and its counts.
//! - [`batch`]: a local bulk job over an NDJSON batch: temporary handles, match keys,
//!   `create-or-add` resolution, bulk `retain`, atomic or streamed, with rejects.
//! - [`sync`]: a mirror sync in `upsert` or `snapshot` mode: the version cursor, the hash
//!   guard of 0006 §2, `prev_upstream`, `first_seen`, sizes and the `changes` summary of
//!   0012 §2.2, and the tombstone sweep with its threshold.
//! - [`follow`]: following a provider by its changes (0092): batches from a `ChangeSource`
//!   the surfaces implement, each written as an `upsert` sync, the position saved after.
//! - [`adopt`]: the adoption job of 0035: preconditions, sequence floors, idempotent
//!   resume, adopted accounts.
//!
//! Everything runs against an [`IngestStore`], the projection backend extended with
//! appending, sequences and the lookups a job needs; [`memory::MemoryIngest`] is the
//! in-memory one. The crate knows no database (0005 §2).

#![forbid(unsafe_code)]

pub mod adopt;
pub mod batch;
pub mod bootstrap;
pub mod follow;
pub mod job;
pub mod memory;
pub mod store;
pub mod sync;
pub mod write;

pub use adopt::{Adoption, AdoptionOutcome, Floors};
pub use batch::{BatchOutcome, run_batch};
pub use bootstrap::{Prepare, Prepared, run_bootstrap};
pub use follow::{ChangeBatch, ChangeSource, FollowOutcome, run_follow};
pub use job::{Counts, Job, Reject};
pub use store::{Cursor, IngestStore, Sequence};
pub use sync::{SyncItem, SyncOutcome, run_sync};
pub use write::{
    Attestation, MintedValue, RawRecord, Request, Written, write_operation, write_raw,
};

use scatter_projection::ProjectionError;

/// Why ingest stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IngestError {
    /// The store refused.
    #[error("store: {0}")]
    Store(String),
    /// The log refused.
    #[error(transparent)]
    Log(#[from] scatter_log::store::StoreError),
    /// A projection failed.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
    /// The batch could not be read.
    #[error(transparent)]
    Wire(#[from] scatter_wikibase_changeset::WireError),
    /// An operation failed validation; the problems, in order.
    #[error("invalid operation: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<scatter_wikibase_changeset::Problem>),
    /// The key has a newer record than the request's base offset (0006 §8).
    #[error("`{key}` changed since offset {base}: its newest record is at {latest}")]
    Conflict {
        /// The key.
        key: String,
        /// The base offset the request named.
        base: u64,
        /// The newest record's offset.
        latest: u64,
    },
    /// A graph has no partition.
    #[error("tenant `{tenant}` has no `{graph}` partition")]
    NoPartition {
        /// The tenant, `""` for the instance.
        tenant: String,
        /// The graph.
        graph: String,
    },
    /// An operation names a provider the registry does not know.
    #[error("no provider for `{0}`")]
    UnknownProvider(String),
    /// An entity type this instance cannot mint IDs for.
    #[error("cannot mint an ID for a {0}")]
    NoIdFor(String),
    /// An adoption precondition failed (0035 §2).
    #[error("adoption refused: {0}")]
    AdoptionRefused(String),
    /// The `snapshot` sweep would tombstone more than the threshold allows.
    #[error("snapshot sweep would tombstone {count} entities, above the threshold of {threshold}")]
    SweepThreshold {
        /// Entities the job did not see.
        count: usize,
        /// The configured limit.
        threshold: usize,
    },
    /// An adapter's output could not be used.
    #[error("adapter: {0}")]
    Adapter(String),
}

impl From<String> for IngestError {
    fn from(s: String) -> Self {
        Self::Store(s)
    }
}
