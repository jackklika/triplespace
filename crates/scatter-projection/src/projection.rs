//! What a projection is.

use std::future::Future;
use std::pin::Pin;

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::StoreError;

use crate::backend::Backend;

/// A boxed `Send` future, so that projections of different types can share a pipeline.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The dependency order of 0013 §7. A projection reads only tables of lower steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Step {
    /// Registry, surrogates, actors, groups, memberships, ACLs, blocks, filters: what
    /// the write path reads.
    Registry = 1,
    /// Entity sources, keyed maps, pages, jobs, upstream revisions, categories, files.
    Sources = 2,
    /// Clusters and links.
    Clusters = 3,
    /// Resolution: entities, terms, sitelinks, identifiers, references, assertions,
    /// corrections, constraints, page statements.
    Resolution = 4,
    /// Activity, patrol, filter hits, page links, record statements, threads, reports.
    Activity = 5,
    /// Addressing, which fills inboxes: the one step whose target is `private`.
    Addressing = 6,
    /// RDF and search.
    Rdf = 7,
}

impl Step {
    /// Every step, in order.
    pub const ALL: [Self; 7] = [
        Self::Registry,
        Self::Sources,
        Self::Clusters,
        Self::Resolution,
        Self::Activity,
        Self::Addressing,
        Self::Rdf,
    ];

    /// The steps the write path applies synchronously (0013 §7: projections 1–5).
    #[must_use]
    pub fn is_synchronous(self) -> bool {
        self <= Self::Activity
    }
}

/// A key in a tenant whose derived rows a record affects beyond its own subject.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    /// The tenant, `""` for the instance.
    pub tenant: String,
    /// The entity or page key.
    pub key: String,
}

/// What applying one record did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Rows written for the record's own subject.
    pub rows: usize,
    /// Keys whose derived rows the record also affects (referrers, cluster members),
    /// to be re-projected with [`Projection::fanout`] inside the budget, or queued.
    pub fanout: Vec<Target>,
}

impl Applied {
    /// `rows` rows, no fan-out.
    #[must_use]
    pub fn rows(rows: usize) -> Self {
        Self {
            rows,
            fanout: Vec::new(),
        }
    }
}

/// Why a projection run stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProjectionError {
    /// The log refused.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The backend refused.
    #[error("backend: {0}")]
    Backend(String),
    /// A projection failed on a record; its batch is rolled back and its position stays.
    #[error("projection `{projection}` at partition {partition} offset {offset}: {message}")]
    Failed {
        /// The projection.
        projection: String,
        /// The partition.
        partition: u64,
        /// The offset.
        offset: u64,
        /// What went wrong.
        message: String,
    },
    /// Two projections share a name.
    #[error("two projections are named `{0}`")]
    DuplicateName(String),
}

/// A consumer of records that keeps derived tables.
pub trait Projection<B: Backend>: Send + Sync {
    /// The name, which keys the position in `projection_state`.
    fn name(&self) -> &'static str;

    /// Where it sits in the dependency order.
    fn step(&self) -> Step;

    /// Whether this record concerns the projection at all, from its header. Records it
    /// does not accept still advance its position.
    fn accepts(&self, header: &Header) -> bool {
        let _ = header;
        true
    }

    /// Applies one record inside the unit of work.
    fn apply<'a>(
        &'a self,
        cx: &'a mut B::Cx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>>;

    /// Applies consecutive records of one partition, all accepted, inside the unit of work:
    /// the set-based form a bulk load uses (0013 §9, A71). The default applies them one at a
    /// time; a projection whose rows can be written for a block at once overrides it, and
    /// must leave the same rows as applying the records in order.
    fn apply_many<'a>(
        &'a self,
        cx: &'a mut B::Cx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let mut out = Applied::default();
            for record in records {
                let applied = self.apply(cx, record).await?;
                out.rows += applied.rows;
                out.fanout.extend(applied.fanout);
            }
            Ok(out)
        })
    }

    /// Re-projects one key the record's effects reached. Returns the rows written.
    fn fanout<'a>(
        &'a self,
        cx: &'a mut B::Cx,
        tenant: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, Result<usize, String>> {
        let _ = (cx, tenant, key);
        Box::pin(async { Ok(0) })
    }

    /// Drops every row derived from a partition, before a replay from offset 0.
    fn reset<'a>(&'a self, cx: &'a mut B::Cx, partition: u64) -> BoxFuture<'a, Result<(), String>>;
}
