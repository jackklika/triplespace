//! The `LogStore` contract (0005 §4.2; 0013 §1): `append`, `read`, `scan`,
//! `erase_parts`, `compact` and `head`, over partitions of gapless offsets.
//!
//! The store owns the header fields it alone can fill — the partition, the offset and
//! the body commitment — and takes the rest from a [`Draft`]: the time, the payload
//! type, the key and the global IDs, which the appending layer assigns in the same
//! transaction (0015 §2). Offsets are never reused: an erased record keeps its header,
//! and a compacted one leaves a hole that keeps only its Merkle leaf, which `read`
//! reports as [`Slot::Compacted`], so the tree over every offset still folds.
//!
//! The two backends are [`MemoryStore`](crate::memory::MemoryStore) and the
//! [`SegmentStore`](crate::segments::SegmentStore) file backend here, and
//! `scatter-log-postgres`. All of them pass [`conformance`](crate::conformance).

use std::future::Future;

use crate::body::{Body, BodyError};
use crate::hash::Hash;
use crate::header::{FORMAT_VERSION, Header, HeaderError};
use crate::record::{Record, RecordError};
use crate::tree::Segments;

/// What a store is asked to append: everything in a header but what the store fills in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    /// Header field 3: microseconds since the epoch.
    pub appended_at: u64,
    /// Header field 4.
    pub payload_type: String,
    /// Header field 5.
    pub key: Option<String>,
    /// Header field 7.
    pub revid: Option<u64>,
    /// Header field 8.
    pub logid: Option<u64>,
    /// Header field 9.
    pub page_id: Option<u64>,
    /// The body, whose commitment becomes header field 6.
    pub body: Body,
}

impl Draft {
    /// The record this draft becomes at `offset` of `partition`.
    #[must_use]
    pub fn seal(self, partition: u64, offset: u64) -> Record {
        let header = Header {
            version: FORMAT_VERSION,
            partition,
            offset,
            appended_at: self.appended_at,
            payload_type: self.payload_type,
            key: self.key,
            commitment: [0; 32],
            revid: self.revid,
            logid: self.logid,
            page_id: self.page_id,
        };
        Record::seal(header, self.body)
    }

    /// [`Self::seal`] with the body's commitment computed beforehand: a bootstrap writer
    /// hashes bodies on worker threads and assigns offsets in order afterwards, so the
    /// coordinator does not hash each body a second time (0013 §9). `commitment` must be
    /// `self.body.commitment()`, which debug builds check.
    #[must_use]
    pub fn seal_committed(self, partition: u64, offset: u64, commitment: Hash) -> Record {
        debug_assert_eq!(commitment, self.body.commitment(), "a stale commitment");
        let header = Header {
            version: FORMAT_VERSION,
            partition,
            offset,
            appended_at: self.appended_at,
            payload_type: self.payload_type,
            key: self.key,
            commitment,
            revid: self.revid,
            logid: self.logid,
            page_id: self.page_id,
        };
        Record::committed(header, self.body)
    }
}

/// What `append` returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appended {
    /// The offset the record took.
    pub offset: u64,
    /// The record's Merkle leaf.
    pub leaf: Hash,
    /// The tree head after the append.
    pub root: Hash,
}

/// The state of a partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The number of offsets assigned: the next offset.
    pub size: u64,
    /// The RFC 6962 tree head over the headers.
    pub root: Hash,
    /// The segment layout.
    pub segments: Segments,
}

/// Checks that a slot may be restored at `offset` of `partition`.
pub fn check_restorable(slot: &Slot, partition: u64, offset: u64) -> Result<(), StoreError> {
    if let Slot::Record(r) = slot {
        let h = r.header();
        if h.partition != partition || h.offset != offset {
            return Err(StoreError::NotNext {
                partition,
                offset,
                found_partition: h.partition,
                found_offset: h.offset,
            });
        }
        r.body().verify(&h.commitment)?;
    }
    Ok(())
}

/// What is at an offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Slot {
    /// A record, possibly with erased parts.
    Record(Record),
    /// A record compaction removed; the offset is never reused and its leaf remains.
    Compacted {
        /// The Merkle leaf the record had.
        leaf: Hash,
    },
}

impl Slot {
    /// The record, if the slot holds one.
    #[must_use]
    pub fn record(&self) -> Option<&Record> {
        match self {
            Self::Record(r) => Some(r),
            Self::Compacted { .. } => None,
        }
    }

    /// The Merkle leaf, whether or not the record is still there.
    #[must_use]
    pub fn leaf(&self) -> Hash {
        match self {
            Self::Record(r) => r.leaf(),
            Self::Compacted { leaf } => *leaf,
        }
    }
}

/// Why a store call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The partition does not exist in this store.
    #[error("partition {0} does not exist")]
    NoPartition(u64),
    /// The partition already exists.
    #[error("partition {0} already exists")]
    PartitionExists(u64),
    /// The offset is past the head.
    #[error("partition {partition} has no offset {offset}")]
    NoOffset {
        /// The partition.
        partition: u64,
        /// The offset asked for.
        offset: u64,
    },
    /// A restored record's header names another partition or offset.
    #[error(
        "partition {partition}: a restored record is for partition {found_partition} offset {found_offset}, not offset {offset}"
    )]
    NotNext {
        /// The partition.
        partition: u64,
        /// The next offset.
        offset: u64,
        /// What the header said.
        found_partition: u64,
        /// What the header said.
        found_offset: u64,
    },
    /// The offset was compacted, so there is nothing to erase.
    #[error("partition {partition} offset {offset} was compacted")]
    Compacted {
        /// The partition.
        partition: u64,
        /// The offset asked for.
        offset: u64,
    },
    /// A body operation failed.
    #[error(transparent)]
    Body(#[from] BodyError),
    /// Stored bytes are not a record.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// Stored bytes are not a header.
    #[error(transparent)]
    Header(#[from] HeaderError),
    /// The backend's storage failed.
    #[error("storage: {0}")]
    Storage(String),
    /// Stored data is inconsistent: a gap, a wrong partition or offset in a header, a
    /// commitment that does not match.
    #[error("corrupt: {0}")]
    Corrupt(String),
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Storage(e.to_string())
    }
}

/// An append-only log of partitions.
///
/// The methods are asynchronous so that a backend can run them inside a transaction the
/// caller holds on an async driver (0013 §7: an interactive write appends and applies
/// projections in one transaction). The trait needs no runtime: a synchronous caller
/// drives a call to completion with any executor, and the file backend does plain
/// synchronous I/O inside its futures (0005 rule 2). Every future is `Send` and a store
/// is `Sync`, so one can be shared by the handlers of a multi-threaded server.
pub trait LogStore: Send + Sync {
    /// Creates an empty partition with the given segment layout.
    fn create_partition(
        &mut self,
        partition: u64,
        segments: Segments,
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// The partitions this store holds, ascending.
    fn partitions(&self) -> impl Future<Output = Result<Vec<u64>, StoreError>> + Send;

    /// The partition's size, root and layout.
    fn head(&self, partition: u64) -> impl Future<Output = Result<Head, StoreError>> + Send;

    /// Appends a record at the next offset.
    fn append(
        &mut self,
        partition: u64,
        draft: Draft,
    ) -> impl Future<Output = Result<Appended, StoreError>> + Send;

    /// Restores an existing slot at the next offset, as when loading an export bundle
    /// or replicating: a record keeps its own header, which must name this partition
    /// and the next offset; a compacted slot keeps its leaf.
    fn append_slot(
        &mut self,
        partition: u64,
        slot: Slot,
    ) -> impl Future<Output = Result<Appended, StoreError>> + Send;

    /// The slot at an offset.
    fn read(
        &self,
        partition: u64,
        offset: u64,
    ) -> impl Future<Output = Result<Slot, StoreError>> + Send;

    /// Up to `limit` records from `from`, with their offsets, in order. Compacted
    /// offsets are skipped; the scan ends at the head.
    fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<(u64, Record)>, StoreError>> + Send;

    /// Erases the named parts of the record at `offset`, keeping its leaves (0015 §1).
    /// Erasing an erased part is a no-op; an index past the body is an error.
    fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;

    /// Removes whole records, leaving holes that keep their leaves (0002 §2; 0013 §1;
    /// 0006 A14). Compacting a compacted offset is a no-op.
    fn compact(
        &mut self,
        partition: u64,
        offsets: &[u64],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}
