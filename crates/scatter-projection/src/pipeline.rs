//! The pipeline: projections in step order, applied inline or catching up.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use scatter_log::record::Record;
use scatter_log::store::{LogStore, Slot};

use crate::backend::{Backend, Lag, Work};
use crate::projection::{Applied, BoxFuture, Projection, ProjectionError, Step, Target};
use crate::worker::Consumer;

/// The synchronous budget (0013 §7): fan-out is applied inline until either is spent,
/// then queued. `projections.sync_budget` and `projections.sync_time` in `site`
/// configuration; the defaults are 1,000 rows and 250 ms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// Rows the inline fan-out may write.
    pub rows: usize,
    /// Time it may take.
    pub time: Duration,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            rows: 1_000,
            time: Duration::from_millis(250),
        }
    }
}

impl Budget {
    /// No fan-out inline at all: everything is queued.
    pub const NONE: Self = Self {
        rows: 0,
        time: Duration::ZERO,
    };
    /// No limit: everything inline, as a bulk job's batch would.
    pub const UNLIMITED: Self = Self {
        rows: usize::MAX,
        time: Duration::MAX,
    };
}

/// What an inline application did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inline {
    /// Rows written for the record's own subjects, across projections.
    pub rows: usize,
    /// Fan-out targets re-projected inline.
    pub fanned_out: usize,
    /// Fan-out targets queued for the worker.
    pub queued: usize,
}

/// What a catch-up did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Progress {
    /// Records read.
    pub records: u64,
    /// Rows written.
    pub rows: usize,
    /// Fan-out targets queued.
    pub queued: usize,
    /// The head the partition was caught up to.
    pub head: u64,
}

/// Projections in step order.
pub struct Pipeline<B: Backend> {
    projections: Vec<Arc<dyn Projection<B>>>,
    /// Run over a block before any projection applies it ([`Self::before_block`]).
    prelude: Option<fn(&[Record])>,
}

impl<B: Backend> Default for Pipeline<B> {
    fn default() -> Self {
        Self {
            projections: Vec::new(),
            prelude: None,
        }
    }
}

impl<B: Backend> Pipeline<B> {
    /// No projections yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Runs `prelude` over each block [`Self::apply_inline_many`] applies, before the first
    /// projection: work every projection over the block would otherwise repeat, such as
    /// decoding the records' content, done once and on as many threads as it likes.
    #[must_use]
    pub fn before_block(mut self, prelude: fn(&[Record])) -> Self {
        self.prelude = Some(prelude);
        self
    }

    /// Adds a projection, keeping step order; within a step, the order of addition.
    pub fn with(
        mut self,
        projection: impl Projection<B> + 'static,
    ) -> Result<Self, ProjectionError> {
        if self
            .projections
            .iter()
            .any(|p| p.name() == projection.name())
        {
            return Err(ProjectionError::DuplicateName(projection.name().into()));
        }
        let step = projection.step();
        let at = self
            .projections
            .iter()
            .position(|p| p.step() > step)
            .unwrap_or(self.projections.len());
        self.projections.insert(at, Arc::new(projection));
        Ok(self)
    }

    /// The projections, in order.
    #[must_use]
    pub fn projections(&self) -> &[Arc<dyn Projection<B>>] {
        &self.projections
    }

    /// The projections of the steps the write path applies synchronously.
    fn synchronous(&self) -> impl Iterator<Item = &dyn Projection<B>> {
        self.projections
            .iter()
            .filter(|p| p.step().is_synchronous())
            .map(AsRef::as_ref)
    }

    fn failed(p: &dyn Projection<B>, record: &Record, message: String) -> ProjectionError {
        ProjectionError::Failed {
            projection: p.name().into(),
            partition: record.header().partition,
            offset: record.header().offset,
            message,
        }
    }

    /// Applies one just-appended record inside the appending transaction (0013 §7):
    /// every synchronous projection, then the fan-out within `budget`, the rest queued.
    /// Advances each synchronous projection's position past the record.
    pub async fn apply_inline(
        &self,
        backend: &B,
        cx: &mut B::Cx,
        record: &Record,
        budget: Budget,
    ) -> Result<Inline, ProjectionError> {
        let started = Instant::now();
        let mut out = Inline::default();
        let mut pending: Vec<(&dyn Projection<B>, Target)> = Vec::new();
        let h = record.header();
        for p in self.synchronous() {
            if p.accepts(h) {
                let applied = p
                    .apply(cx, record)
                    .await
                    .map_err(|m| Self::failed(p, record, m))?;
                out.rows += applied.rows;
                pending.extend(applied.fanout.into_iter().map(|t| (p, t)));
            }
            backend
                .set_applied(cx, p.name(), h.partition, h.offset + 1)
                .await?;
        }
        let mut spent = 0usize;
        let mut queue = Vec::new();
        for (p, target) in pending {
            if spent < budget.rows && started.elapsed() < budget.time {
                let rows = p
                    .fanout(cx, &target.tenant, &target.key)
                    .await
                    .map_err(|m| Self::failed(p, record, m))?;
                spent += rows;
                out.fanned_out += 1;
            } else {
                queue.push(Work {
                    projection: p.name().into(),
                    tenant: target.tenant,
                    partition: h.partition,
                    offset: h.offset,
                    key: target.key,
                });
            }
        }
        if !queue.is_empty() {
            out.queued = queue.len();
            backend.queue(cx, &queue).await?;
        }
        Ok(out)
    }

    /// [`Self::apply_inline`] for consecutive records of one partition, a block at a time:
    /// each synchronous projection takes the records it accepts with
    /// [`Projection::apply_many`] and its position moves once, past the last. Projections
    /// run in dependency order, each over the whole block, which is safe because a
    /// projection reads only the tables of earlier steps. Fan-out the budget leaves is
    /// queued at the block's last offset.
    ///
    /// # Panics
    ///
    /// When the records are not consecutive offsets of one partition.
    pub async fn apply_inline_many(
        &self,
        backend: &B,
        cx: &mut B::Cx,
        records: &[Record],
        budget: Budget,
    ) -> Result<Inline, ProjectionError> {
        let (Some(first), Some(last)) = (records.first(), records.last()) else {
            return Ok(Inline::default());
        };
        let partition = first.header().partition;
        assert!(
            records
                .iter()
                .zip(first.header().offset..)
                .all(|(r, o)| r.header().partition == partition && r.header().offset == o),
            "apply_inline_many takes consecutive records of one partition"
        );
        let started = Instant::now();
        if let Some(prelude) = self.prelude {
            prelude(records);
        }
        let mut out = Inline::default();
        let mut pending: Vec<(&dyn Projection<B>, Target)> = Vec::new();
        for p in self.synchronous() {
            // Most projections accept every record of a block; only a subset is copied.
            let subset: Vec<Record>;
            let accepted: &[Record] = if records.iter().all(|r| p.accepts(r.header())) {
                records
            } else {
                subset = records
                    .iter()
                    .filter(|r| p.accepts(r.header()))
                    .cloned()
                    .collect();
                &subset
            };
            if !accepted.is_empty() {
                let applied = p
                    .apply_many(cx, accepted)
                    .await
                    .map_err(|m| Self::failed(p, last, m))?;
                out.rows += applied.rows;
                pending.extend(applied.fanout.into_iter().map(|t| (p, t)));
            }
            backend
                .set_applied(cx, p.name(), partition, last.header().offset + 1)
                .await?;
        }
        let mut spent = 0usize;
        let mut queue = Vec::new();
        for (p, target) in pending {
            if spent < budget.rows && started.elapsed() < budget.time {
                let rows = p
                    .fanout(cx, &target.tenant, &target.key)
                    .await
                    .map_err(|m| Self::failed(p, last, m))?;
                spent += rows;
                out.fanned_out += 1;
            } else {
                queue.push(Work {
                    projection: p.name().into(),
                    tenant: target.tenant,
                    partition,
                    offset: last.header().offset,
                    key: target.key,
                });
            }
        }
        if !queue.is_empty() {
            out.queued = queue.len();
            backend.queue(cx, &queue).await?;
        }
        Ok(out)
    }

    /// Applies one record for every projection whose position is at or below it.
    /// Fan-out is queued, never applied inline: a catch-up is bulk work.
    async fn apply_behind(
        &self,
        backend: &B,
        cx: &mut B::Cx,
        record: &Record,
        positions: &mut BTreeMap<&'static str, u64>,
        progress: &mut Progress,
    ) -> Result<(), ProjectionError> {
        let h = record.header();
        let mut queue = Vec::new();
        for p in &self.projections {
            let pos = positions.entry(p.name()).or_insert(0);
            if *pos > h.offset {
                continue;
            }
            if p.accepts(h) {
                let Applied { rows, fanout } = p
                    .apply(cx, record)
                    .await
                    .map_err(|m| Self::failed(p.as_ref(), record, m))?;
                progress.rows += rows;
                queue.extend(fanout.into_iter().map(|t| Work {
                    projection: p.name().into(),
                    tenant: t.tenant,
                    partition: h.partition,
                    offset: h.offset,
                    key: t.key,
                }));
            }
            *pos = h.offset + 1;
        }
        if !queue.is_empty() {
            progress.queued += queue.len();
            backend.queue(cx, &queue).await?;
        }
        Ok(())
    }

    /// Catches every projection up to the partition's head, `batch` records per unit of
    /// work. A failing record rolls its batch back and leaves positions where they were.
    pub async fn catch_up<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partition: u64,
        batch: usize,
    ) -> Result<Progress, ProjectionError> {
        let mut progress = Progress::default();
        let head = store.head(partition).await?.size;
        progress.head = head;
        let batch = batch.max(1);
        loop {
            let mut cx = backend.begin().await?;
            let mut positions: BTreeMap<&'static str, u64> = BTreeMap::new();
            for p in &self.projections {
                positions.insert(
                    p.name(),
                    backend.applied(&mut cx, p.name(), partition).await?,
                );
            }
            let from = positions.values().copied().min().unwrap_or(head);
            if from >= head {
                backend.rollback(cx).await?;
                break;
            }
            let to = from.saturating_add(batch as u64).min(head);
            let outcome: Result<(), ProjectionError> = async {
                for offset in from..to {
                    match store.read(partition, offset).await? {
                        Slot::Record(record) => {
                            self.apply_behind(
                                backend,
                                &mut cx,
                                &record,
                                &mut positions,
                                &mut progress,
                            )
                            .await?;
                        }
                        Slot::Compacted { .. } => {
                            for pos in positions.values_mut() {
                                if *pos <= offset {
                                    *pos = offset + 1;
                                }
                            }
                        }
                    }
                    progress.records += 1;
                }
                for (name, pos) in &positions {
                    backend.set_applied(&mut cx, name, partition, *pos).await?;
                }
                Ok(())
            }
            .await;
            match outcome {
                Ok(()) => backend.commit(cx).await?,
                Err(e) => {
                    backend.rollback(cx).await?;
                    return Err(e);
                }
            }
        }
        Ok(progress)
    }

    /// Resets every projection on `partitions` and replays each from offset 0, one
    /// partition after another.
    pub async fn rebuild<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partitions: &[u64],
        batch: usize,
    ) -> Result<Vec<Progress>, ProjectionError> {
        let groups: Vec<Vec<u64>> = partitions.iter().map(|p| vec![*p]).collect();
        Ok(self
            .rebuild_grouped(backend, store, &groups, batch)
            .await?
            .into_iter()
            .map(|(_, p)| p)
            .collect())
    }

    /// Resets every projection on every partition of `groups` and replays them from
    /// offset 0: the groups one after another, and the partitions of one group
    /// interleaved in `(appended_at, partition, offset)` order, the order the write path
    /// applied them in (03 §6.1, replay order; 0013 §7, A53). A projection that changes a row another
    /// partition's records create (a deletion ACL in a tenant's `log` on its `pages` row)
    /// then sees the same state on a rebuild as it did when the record was written.
    /// Returns each partition's progress, in the order replayed.
    pub async fn rebuild_grouped<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        groups: &[Vec<u64>],
        batch: usize,
    ) -> Result<Vec<(u64, Progress)>, ProjectionError> {
        let mut cx = backend.begin().await?;
        for &partition in groups.iter().flatten() {
            for p in &self.projections {
                p.reset(&mut cx, partition)
                    .await
                    .map_err(|message| ProjectionError::Failed {
                        projection: p.name().into(),
                        partition,
                        offset: 0,
                        message,
                    })?;
                backend.set_applied(&mut cx, p.name(), partition, 0).await?;
            }
        }
        backend.commit(cx).await?;
        let mut out = Vec::new();
        for group in groups {
            if let [partition] = group.as_slice() {
                out.push((
                    *partition,
                    self.catch_up(backend, store, *partition, batch).await?,
                ));
            } else {
                out.extend(self.replay_merged(backend, store, group, batch).await?);
            }
        }
        Ok(out)
    }

    /// Replays freshly reset partitions together, always applying next the pending record
    /// with the lowest `(appended_at, partition, offset)`, `batch` records per unit of
    /// work.
    async fn replay_merged<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        group: &[u64],
        batch: usize,
    ) -> Result<Vec<(u64, Progress)>, ProjectionError> {
        struct Cursor {
            partition: u64,
            next: u64,
            pending: Option<Record>,
            positions: BTreeMap<&'static str, u64>,
            progress: Progress,
        }
        let mut cursors = Vec::with_capacity(group.len());
        for &partition in group {
            let head = store.head(partition).await?.size;
            cursors.push(Cursor {
                partition,
                next: 0,
                pending: None,
                positions: self.projections.iter().map(|p| (p.name(), 0)).collect(),
                progress: Progress {
                    head,
                    ..Progress::default()
                },
            });
        }
        let batch = batch.max(1);
        loop {
            let mut cx = backend.begin().await?;
            let mut applied = 0usize;
            let outcome: Result<(), ProjectionError> = async {
                while applied < batch {
                    // Each cursor's next record, past any compacted slots.
                    for c in &mut cursors {
                        while c.pending.is_none() && c.next < c.progress.head {
                            match store.read(c.partition, c.next).await? {
                                Slot::Record(record) => c.pending = Some(record),
                                Slot::Compacted { .. } => {
                                    for pos in c.positions.values_mut() {
                                        if *pos <= c.next {
                                            *pos = c.next + 1;
                                        }
                                    }
                                    c.progress.records += 1;
                                    c.next += 1;
                                }
                            }
                        }
                    }
                    let Some(i) = cursors
                        .iter()
                        .enumerate()
                        .filter_map(|(i, c)| {
                            c.pending.as_ref().map(|r| {
                                let h = r.header();
                                ((h.appended_at, c.partition, h.offset), i)
                            })
                        })
                        .min()
                        .map(|(_, i)| i)
                    else {
                        break;
                    };
                    let c = &mut cursors[i];
                    let record = c.pending.take().expect("pending");
                    self.apply_behind(backend, &mut cx, &record, &mut c.positions, &mut c.progress)
                        .await?;
                    c.progress.records += 1;
                    c.next += 1;
                    applied += 1;
                }
                for c in &cursors {
                    for (name, pos) in &c.positions {
                        backend
                            .set_applied(&mut cx, name, c.partition, *pos)
                            .await?;
                    }
                }
                Ok(())
            }
            .await;
            match outcome {
                Ok(()) => backend.commit(cx).await?,
                Err(e) => {
                    backend.rollback(cx).await?;
                    return Err(e);
                }
            }
            if applied == 0 {
                break;
            }
        }
        Ok(cursors
            .into_iter()
            .map(|c| (c.partition, c.progress))
            .collect())
    }

    /// Each projection's lag on a partition.
    pub async fn lags<S: LogStore>(
        &self,
        backend: &B,
        store: &S,
        partition: u64,
    ) -> Result<Vec<Lag>, ProjectionError> {
        let head = store.head(partition).await?.size;
        let mut cx = backend.begin().await?;
        let mut out = Vec::new();
        for p in &self.projections {
            out.push(Lag {
                projection: p.name().into(),
                partition,
                applied: backend.applied(&mut cx, p.name(), partition).await?,
                head,
            });
        }
        backend.rollback(cx).await?;
        Ok(out)
    }

    /// The steps present, in order.
    #[must_use]
    pub fn steps(&self) -> Vec<Step> {
        let mut steps: Vec<Step> = self.projections.iter().map(|p| p.step()).collect();
        steps.dedup();
        steps
    }
}

impl<B: Backend + 'static> Pipeline<B> {
    /// Each projection as a [`Consumer`] of its own queued fan-out, for the worker.
    #[must_use]
    pub fn consumers(&self) -> Vec<Arc<dyn Consumer<B>>> {
        self.projections
            .iter()
            .map(|p| Arc::new(Fanout(Arc::clone(p))) as Arc<dyn Consumer<B>>)
            .collect()
    }
}

/// A projection's queued fan-out, applied key by key.
struct Fanout<B: Backend>(Arc<dyn Projection<B>>);

impl<B: Backend> Consumer<B> for Fanout<B> {
    fn name(&self) -> &'static str {
        self.0.name()
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut B::Cx,
        tenant: &'a str,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let mut rows = 0;
            for key in keys {
                rows += self.0.fanout(cx, tenant, key).await?;
            }
            Ok(rows)
        })
    }
}
