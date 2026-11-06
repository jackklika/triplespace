//! `LogStore` over `log.partition` and `log.record` (0013 §2–3).
//!
//! [`PgLog`] wraps either an owned [`Client`], in which case every mutating call runs in
//! a transaction of its own, or a borrowed [`Transaction`], in which case it joins the
//! caller's: that is how the write path appends a record and applies its projections
//! atomically (0013 §7). Reads run on whichever it holds.
//!
//! An append locks the partition's row (`FOR UPDATE`), takes `next_offset`, inserts the
//! record and its Merkle nodes, and advances the offset; a rollback releases it, so
//! offsets stay gapless (0006 §3). The tree nodes an append completes, and the new root,
//! come from the partition's [`Frontier`], which a writer carries in a [`Frontiers`]
//! across the appends of one unit of work: loaded once per partition from the stored
//! nodes, checked against `next_offset` under the lock on every append, so the append
//! itself reads nothing back from `log.merkle_node`. Erasure rewrites the body in place and marks the
//! `erased` bitmask (bit *i* for part *i* below 15; bit 15 for any part beyond).
//! Compaction deletes the row; the leaf stays in `log.merkle_node` (0006 A14).

use scatter_log::body::Body;
use scatter_log::hash::Hash;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::{Appended, Draft, Head, LogStore, Slot, StoreError, check_restorable};
use scatter_log::tree::{Frontier, Segments};
use tokio_postgres::{Client, Transaction};

use crate::client::PgClient;

use crate::ids::{from_db, partition_from_db, partition_to_db, record_table, to_db};
use crate::merkle;
use crate::partition::{PartitionInfo, export_str, history_str, integrity_str};
use crate::storage_error;

/// The log over a client or a transaction.
#[derive(Debug)]
pub struct PgLog<C> {
    client: C,
    frontiers: Frontiers,
}

/// The frontiers of the partitions a writer has appended to, by partition. One per unit
/// of work: a frontier is valid for as long as the writer holds the partition's row
/// lock, and [`Frontiers::load`] reloads one whose size disagrees with `next_offset`,
/// which is how an append after another writer's, or after a rollback, recovers.
#[derive(Debug, Default)]
pub struct Frontiers {
    by_partition: std::collections::HashMap<u64, Frontier>,
}

impl Frontiers {
    /// No frontiers yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The frontier of a partition at `size` leaves: the cached one when its size agrees,
    /// else read from the stored nodes.
    pub async fn load<C: PgClient>(
        &mut self,
        client: &C,
        partition: u64,
        size: u64,
    ) -> Result<&mut Frontier, StoreError> {
        let stale = self
            .by_partition
            .get(&partition)
            .is_none_or(|f| f.size() != size);
        if stale {
            let f = merkle::frontier(client, partition, size).await?;
            self.by_partition.insert(partition, f);
        }
        Ok(self
            .by_partition
            .get_mut(&partition)
            .expect("just inserted"))
    }

    /// Forgets a partition's frontier, as after a failed append.
    pub fn forget(&mut self, partition: u64) {
        self.by_partition.remove(&partition);
    }
}

impl<C> PgLog<C> {
    /// Wraps a client (every mutating call in its own transaction) or a borrowed
    /// transaction (joining it).
    pub fn new(client: C) -> Self {
        Self {
            client,
            frontiers: Frontiers::new(),
        }
    }

    /// The client or transaction.
    pub fn client(&self) -> &C {
        &self.client
    }

    /// Takes the client back.
    pub fn into_inner(self) -> C {
        self.client
    }
}

/// The bitmask of erased parts: bit *i* for part *i* below 15, bit 15 for any beyond.
#[must_use]
pub fn erased_mask(parts: impl IntoIterator<Item = usize>) -> i16 {
    let mut mask: u16 = 0;
    for p in parts {
        mask |= 1 << p.min(15);
    }
    #[allow(clippy::cast_possible_wrap)]
    {
        mask as i16
    }
}

/// Registers a partition with its registry row and creates its record table.
pub async fn create_partition<C: PgClient>(
    client: &C,
    partition: u64,
    segments: Segments,
    info: &PartitionInfo,
) -> Result<(), StoreError> {
    let p = partition_to_db(partition);
    let exists = client
        .query_opt("SELECT 1 FROM log.partition WHERE partition = $1", &[&p])
        .await
        .map_err(|e| storage_error(&e))?;
    if exists.is_some() {
        return Err(StoreError::PartitionExists(partition));
    }
    client
        .execute(
            "INSERT INTO log.partition (partition, tenant, name, graph_iri, history, integrity, export, segment_k, hash)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            &[
                &p,
                &info.tenant,
                &info.name,
                &info.graph_iri,
                &history_str(info.history),
                &integrity_str(info.integrity),
                &export_str(info.export),
                &i16::from(segments.exponent()),
                &info.hash,
            ],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    let table = record_table(partition);
    client
        .batch_execute(&format!(
            "CREATE TABLE {table} PARTITION OF log.record FOR VALUES IN ({p});
             CREATE INDEX ON {table} (key, \"offset\" DESC);"
        ))
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

/// Every partition, ascending by ID.
pub async fn partitions<C: PgClient>(client: &C) -> Result<Vec<u64>, StoreError> {
    let rows = client
        .query("SELECT partition FROM log.partition", &[])
        .await
        .map_err(|e| storage_error(&e))?;
    let mut out: Vec<u64> = rows
        .iter()
        .map(|r| partition_from_db(r.get::<_, i64>(0)))
        .collect();
    out.sort_unstable();
    Ok(out)
}

struct PartitionRow {
    size: u64,
    segments: Segments,
}

async fn partition_row<C: PgClient>(
    client: &C,
    partition: u64,
    lock: bool,
) -> Result<PartitionRow, StoreError> {
    let sql = if lock {
        "SELECT next_offset, segment_k FROM log.partition WHERE partition = $1 FOR UPDATE"
    } else {
        "SELECT next_offset, segment_k FROM log.partition WHERE partition = $1"
    };
    let row = client
        .query_opt(sql, &[&partition_to_db(partition)])
        .await
        .map_err(|e| storage_error(&e))?
        .ok_or(StoreError::NoPartition(partition))?;
    let k: i16 = row.get(1);
    let segments = u8::try_from(k)
        .ok()
        .and_then(Segments::new)
        .ok_or_else(|| StoreError::Corrupt(format!("segment_k {k}")))?;
    Ok(PartitionRow {
        size: from_db(row.get(0))?,
        segments,
    })
}

/// The partition's size, root and layout.
pub async fn head<C: PgClient>(client: &C, partition: u64) -> Result<Head, StoreError> {
    let row = partition_row(client, partition, false).await?;
    Ok(Head {
        size: row.size,
        root: merkle::root(client, partition, row.size).await?,
        segments: row.segments,
    })
}

async fn insert_record<C: PgClient>(
    client: &C,
    partition: u64,
    record: &Record,
    erased: i16,
) -> Result<(), StoreError> {
    let h = record.header();
    let header = record.header().encode();
    let leaf = record.leaf();
    client
        .execute(
            "INSERT INTO log.record (partition, \"offset\", appended_at, payload_type, key, commitment,
                                     revid, logid, page_id, header, leaf, body, erased)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
            &[
                &partition_to_db(partition),
                &to_db(h.offset)?,
                &to_db(h.appended_at)?,
                &h.payload_type,
                &h.key,
                &h.commitment.as_slice(),
                &h.revid.map(to_db).transpose()?,
                &h.logid.map(to_db).transpose()?,
                &h.page_id.map(to_db).transpose()?,
                &header.as_slice(),
                &leaf.as_slice(),
                &record.body().encode().as_slice(),
                &erased,
            ],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

/// Writes the tree nodes the leaf at `offset` completes and moves `next_offset` past it.
/// The frontier is the partition's at `offset` leaves; it is advanced here, and dropped
/// from the cache if the write fails, so a retry reloads it.
async fn advance<C: PgClient>(
    client: &C,
    partition: u64,
    offset: u64,
    leaf: [u8; 32],
    frontiers: &mut Frontiers,
) -> Result<Appended, StoreError> {
    let frontier = frontiers.load(client, partition, offset).await?;
    let nodes = frontier.push_leaf_completing(leaf);
    let root = frontier.root();
    let written = async {
        merkle::put_nodes(client, partition, &nodes).await?;
        client
            .execute(
                "UPDATE log.partition SET next_offset = $2 WHERE partition = $1",
                &[&partition_to_db(partition), &to_db(offset + 1)?],
            )
            .await
            .map_err(|e| storage_error(&e))?;
        Ok::<(), StoreError>(())
    }
    .await;
    if let Err(e) = written {
        frontiers.forget(partition);
        return Err(e);
    }
    Ok(Appended { offset, leaf, root })
}

/// Appends a draft at the next offset, carrying the partition's frontier in `frontiers`
/// (0013 §2). Must run inside a transaction.
pub async fn append_with<C: PgClient>(
    client: &C,
    partition: u64,
    draft: Draft,
    frontiers: &mut Frontiers,
) -> Result<Appended, StoreError> {
    Ok(append_sealed_with(client, partition, draft, frontiers)
        .await?
        .0)
}

/// [`append_with`], returning the record the draft became as well: sealing it hashes
/// every part of its body, which a caller that goes on to use the record need not do a
/// second time.
pub async fn append_sealed_with<C: PgClient>(
    client: &C,
    partition: u64,
    draft: Draft,
    frontiers: &mut Frontiers,
) -> Result<(Appended, Record), StoreError> {
    let row = partition_row(client, partition, true).await?;
    let record = draft.seal(partition, row.size);
    insert_record(client, partition, &record, 0).await?;
    let appended = advance(client, partition, row.size, record.leaf(), frontiers).await?;
    Ok((appended, record))
}

/// Appends a draft at the next offset, with no frontier carried between calls: the
/// frontier is read once from the stored nodes. Must run inside a transaction.
pub async fn append<C: PgClient>(
    client: &C,
    partition: u64,
    draft: Draft,
) -> Result<Appended, StoreError> {
    append_with(client, partition, draft, &mut Frontiers::new()).await
}

/// Restores a slot at the next offset. Must run inside a transaction.
pub async fn append_slot_with<C: PgClient>(
    client: &C,
    partition: u64,
    slot: Slot,
    frontiers: &mut Frontiers,
) -> Result<Appended, StoreError> {
    let row = partition_row(client, partition, true).await?;
    check_restorable(&slot, partition, row.size)?;
    let leaf = slot.leaf();
    if let Slot::Record(record) = &slot {
        let erased = erased_mask(
            record
                .body()
                .parts()
                .iter()
                .enumerate()
                .filter(|(_, p)| p.is_erased())
                .map(|(i, _)| i),
        );
        insert_record(client, partition, record, erased).await?;
    }
    advance(client, partition, row.size, leaf, frontiers).await
}

/// Restores a slot at the next offset, with no frontier carried between calls.
pub async fn append_slot<C: PgClient>(
    client: &C,
    partition: u64,
    slot: Slot,
) -> Result<Appended, StoreError> {
    append_slot_with(client, partition, slot, &mut Frontiers::new()).await
}

/// Appends drafts at consecutive offsets, a block at a time, by binary `COPY`: bootstrap
/// mode's writer (0013 §9; 05 §3.8). One row lock, one `COPY` of the records, one `COPY`
/// of the tree nodes their leaves complete (the frontier is carried in `frontiers`, as
/// [`append_with`] carries it), and `next_offset` moved once. Each draft comes with its
/// body's commitment, which the caller computed on worker threads. Returns the records in
/// order. Must run inside a transaction; a failure forgets the frontier, as an append's does.
pub async fn append_many<C: PgClient>(
    client: &C,
    partition: u64,
    drafts: Vec<(Draft, Hash)>,
    frontiers: &mut Frontiers,
) -> Result<Vec<Record>, StoreError> {
    if drafts.is_empty() {
        return Ok(Vec::new());
    }
    let row = partition_row(client, partition, true).await?;
    let records: Vec<Record> = drafts
        .into_iter()
        .zip(row.size..)
        .map(|((draft, commitment), offset)| draft.seal_committed(partition, offset, commitment))
        .collect();
    let end = row.size + records.len() as u64;
    let written = async {
        copy_records(client, partition, &records).await?;
        let frontier = frontiers.load(client, partition, row.size).await?;
        let mut nodes = Vec::with_capacity(records.len() * 2);
        for r in &records {
            nodes.extend(frontier.push_leaf_completing(r.leaf()));
        }
        copy_nodes(client, partition, &nodes).await?;
        client
            .execute(
                "UPDATE log.partition SET next_offset = $2 WHERE partition = $1",
                &[&partition_to_db(partition), &to_db(end)?],
            )
            .await
            .map_err(|e| storage_error(&e))?;
        Ok::<(), StoreError>(())
    }
    .await;
    if let Err(e) = written {
        frontiers.forget(partition);
        return Err(e);
    }
    Ok(records)
}

/// The records of [`append_many`], by binary `COPY` into `log.record`, with the columns
/// [`insert_record`] writes for an unerased record.
async fn copy_records<C: PgClient>(
    client: &C,
    partition: u64,
    records: &[Record],
) -> Result<(), StoreError> {
    use tokio_postgres::binary_copy::BinaryCopyInWriter;
    use tokio_postgres::types::Type;
    let sink = client
        .raw()
        .copy_in(
            "COPY log.record (partition, \"offset\", appended_at, payload_type, key, commitment,
                              revid, logid, page_id, header, leaf, body, erased)
             FROM STDIN BINARY",
        )
        .await
        .map_err(|e| storage_error(&e))?;
    let writer = BinaryCopyInWriter::new(
        sink,
        &[
            Type::INT8,
            Type::INT8,
            Type::INT8,
            Type::TEXT,
            Type::TEXT,
            Type::BYTEA,
            Type::INT8,
            Type::INT8,
            Type::INT8,
            Type::BYTEA,
            Type::BYTEA,
            Type::BYTEA,
            Type::INT2,
        ],
    );
    let mut writer = std::pin::pin!(writer);
    let p = partition_to_db(partition);
    for record in records {
        let h = record.header();
        let header = h.encode();
        let leaf = record.leaf();
        let body = record.body().encode();
        writer
            .as_mut()
            .write(&[
                &p,
                &to_db(h.offset)?,
                &to_db(h.appended_at)?,
                &h.payload_type,
                &h.key,
                &h.commitment.as_slice(),
                &h.revid.map(to_db).transpose()?,
                &h.logid.map(to_db).transpose()?,
                &h.page_id.map(to_db).transpose()?,
                &header.as_slice(),
                &leaf.as_slice(),
                &body.as_slice(),
                &0i16,
            ])
            .await
            .map_err(|e| storage_error(&e))?;
    }
    writer.finish().await.map_err(|e| storage_error(&e))?;
    Ok(())
}

/// The tree nodes of [`append_many`], by binary `COPY` into `log.merkle_node`, as
/// [`merkle::put_nodes`] stores them.
async fn copy_nodes<C: PgClient>(
    client: &C,
    partition: u64,
    nodes: &[(u32, u64, Hash)],
) -> Result<(), StoreError> {
    use tokio_postgres::binary_copy::BinaryCopyInWriter;
    use tokio_postgres::types::Type;
    let sink = client
        .raw()
        .copy_in("COPY log.merkle_node (partition, level, index, hash) FROM STDIN BINARY")
        .await
        .map_err(|e| storage_error(&e))?;
    let writer = BinaryCopyInWriter::new(sink, &[Type::INT8, Type::INT2, Type::INT8, Type::BYTEA]);
    let mut writer = std::pin::pin!(writer);
    let p = partition_to_db(partition);
    for (level, index, hash) in nodes {
        let level = i16::try_from(*level).map_err(|_| StoreError::Corrupt("level".into()))?;
        writer
            .as_mut()
            .write(&[&p, &level, &to_db(*index)?, &hash.as_slice()])
            .await
            .map_err(|e| storage_error(&e))?;
    }
    writer.finish().await.map_err(|e| storage_error(&e))?;
    Ok(())
}

fn record_from(header: &[u8], body: &[u8]) -> Result<Record, StoreError> {
    Ok(Record::new(Header::decode(header)?, Body::decode(body)?)?)
}

/// The slot at an offset.
pub async fn read<C: PgClient>(
    client: &C,
    partition: u64,
    offset: u64,
) -> Result<Slot, StoreError> {
    let row = partition_row(client, partition, false).await?;
    if offset >= row.size {
        return Err(StoreError::NoOffset { partition, offset });
    }
    let found = client
        .query_opt(
            "SELECT header, body FROM log.record WHERE partition = $1 AND \"offset\" = $2",
            &[&partition_to_db(partition), &to_db(offset)?],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    if let Some(r) = found {
        return Ok(Slot::Record(record_from(r.get(0), r.get(1))?));
    }
    let leaf = merkle::node(client, partition, 0, offset)
        .await?
        .ok_or_else(|| {
            StoreError::Corrupt(format!("offset {offset} has neither a row nor a leaf"))
        })?;
    Ok(Slot::Compacted { leaf })
}

/// The records at several offsets of a partition, in offset order: what a sync's unit
/// reads at its subjects' cursors in one statement (0013 A76). A compacted offset has no
/// row and is not returned, nor is an offset past the end.
pub async fn read_many<C: PgClient>(
    client: &C,
    partition: u64,
    offsets: &[u64],
) -> Result<Vec<(u64, Record)>, StoreError> {
    if offsets.is_empty() {
        return Ok(Vec::new());
    }
    let offsets: Vec<i64> = offsets
        .iter()
        .map(|&o| to_db(o))
        .collect::<Result<_, _>>()?;
    let rows = client
        .query(
            "SELECT \"offset\", header, body FROM log.record
             WHERE partition = $1 AND \"offset\" = ANY($2) ORDER BY \"offset\"",
            &[&partition_to_db(partition), &offsets],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    rows.iter()
        .map(|r| Ok((from_db(r.get(0))?, record_from(r.get(1), r.get(2))?)))
        .collect()
}

/// Every record under a header key, in offset order: the replay a projection does to
/// fold one subject's history (0013 §2: the `(key, "offset")` index). Compacted offsets
/// have no row and are not returned; nor is anything for a partition that does not
/// exist, since its caller has the partition from the registry already.
pub async fn read_by_key<C: PgClient>(
    client: &C,
    partition: u64,
    key: &str,
) -> Result<Vec<(u64, Record)>, StoreError> {
    let rows = client
        .query(
            "SELECT \"offset\", header, body FROM log.record
             WHERE partition = $1 AND key = $2 ORDER BY \"offset\"",
            &[&partition_to_db(partition), &key],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    rows.iter()
        .map(|r| Ok((from_db(r.get(0))?, record_from(r.get(1), r.get(2))?)))
        .collect()
}

/// Up to `limit` records from `from`.
pub async fn scan<C: PgClient>(
    client: &C,
    partition: u64,
    from: u64,
    limit: usize,
) -> Result<Vec<(u64, Record)>, StoreError> {
    partition_row(client, partition, false).await?;
    if limit == 0 {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT \"offset\", header, body FROM log.record
             WHERE partition = $1 AND \"offset\" >= $2 ORDER BY \"offset\" LIMIT $3",
            &[
                &partition_to_db(partition),
                &to_db(from)?,
                &i64::try_from(limit).unwrap_or(i64::MAX),
            ],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    rows.iter()
        .map(|r| Ok((from_db(r.get(0))?, record_from(r.get(1), r.get(2))?)))
        .collect()
}

/// Erases parts of a record, recording `erased_by`, the offset of the `erase` record
/// (0013 §3), when the caller has one. Must run inside a transaction.
pub async fn erase_parts<C: PgClient>(
    client: &C,
    partition: u64,
    offset: u64,
    parts: &[usize],
    erased_by: Option<u64>,
) -> Result<(), StoreError> {
    let row = partition_row(client, partition, true).await?;
    if offset >= row.size {
        return Err(StoreError::NoOffset { partition, offset });
    }
    let found = client
        .query_opt(
            "SELECT header, body, erased FROM log.record WHERE partition = $1 AND \"offset\" = $2 FOR UPDATE",
            &[&partition_to_db(partition), &to_db(offset)?],
        )
        .await
        .map_err(|e| storage_error(&e))?
        .ok_or(StoreError::Compacted { partition, offset })?;
    let mut record = record_from(found.get(0), found.get(1))?;
    for &p in parts {
        record.body_mut().erase(p)?;
    }
    let was: i16 = found.get(2);
    let mask = was | erased_mask(parts.iter().copied());
    client
        .execute(
            "UPDATE log.record SET body = $3, erased = $4, erased_by = COALESCE($5, erased_by)
             WHERE partition = $1 AND \"offset\" = $2",
            &[
                &partition_to_db(partition),
                &to_db(offset)?,
                &record.body().encode().as_slice(),
                &mask,
                &erased_by.map(to_db).transpose()?,
            ],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

/// Deletes records, leaving their leaves. Must run inside a transaction.
pub async fn compact<C: PgClient>(
    client: &C,
    partition: u64,
    offsets: &[u64],
) -> Result<(), StoreError> {
    let row = partition_row(client, partition, true).await?;
    let mut db: Vec<i64> = Vec::with_capacity(offsets.len());
    for &o in offsets {
        if o >= row.size {
            return Err(StoreError::NoOffset {
                partition,
                offset: o,
            });
        }
        db.push(to_db(o)?);
    }
    client
        .execute(
            "DELETE FROM log.record WHERE partition = $1 AND \"offset\" = ANY($2)",
            &[&partition_to_db(partition), &db],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

/// Compacts a `latest` partition by key: every record for each key but the newest
/// (0013 §3). Returns the offsets removed. Must run inside a transaction.
pub async fn compact_keys<C: PgClient>(
    client: &C,
    partition: u64,
    keys: &[String],
) -> Result<Vec<u64>, StoreError> {
    partition_row(client, partition, true).await?;
    let rows = client
        .query(
            "DELETE FROM log.record r
             USING (SELECT key, max(\"offset\") AS keep FROM log.record
                    WHERE partition = $1 AND key = ANY($2) GROUP BY key) k
             WHERE r.partition = $1 AND r.key = k.key AND r.\"offset\" < k.keep
             RETURNING r.\"offset\"",
            &[&partition_to_db(partition), &keys],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    let mut out: Vec<u64> = rows
        .iter()
        .map(|r| from_db(r.get(0)))
        .collect::<Result<_, _>>()?;
    out.sort_unstable();
    Ok(out)
}

impl LogStore for PgLog<Client> {
    async fn create_partition(
        &mut self,
        partition: u64,
        segments: Segments,
    ) -> Result<(), StoreError> {
        let tx = self
            .client
            .transaction()
            .await
            .map_err(|e| storage_error(&e))?;
        create_partition(&tx, partition, segments, &PartitionInfo::unnamed(partition)).await?;
        tx.commit().await.map_err(|e| storage_error(&e))
    }

    async fn partitions(&self) -> Result<Vec<u64>, StoreError> {
        partitions(&self.client).await
    }

    async fn head(&self, partition: u64) -> Result<Head, StoreError> {
        head(&self.client, partition).await
    }

    async fn append(&mut self, partition: u64, draft: Draft) -> Result<Appended, StoreError> {
        let tx = self
            .client
            .transaction()
            .await
            .map_err(|e| storage_error(&e))?;
        let a = append_with(&tx, partition, draft, &mut self.frontiers).await?;
        tx.commit().await.map_err(|e| storage_error(&e))?;
        Ok(a)
    }

    async fn append_slot(&mut self, partition: u64, slot: Slot) -> Result<Appended, StoreError> {
        let tx = self
            .client
            .transaction()
            .await
            .map_err(|e| storage_error(&e))?;
        let a = append_slot_with(&tx, partition, slot, &mut self.frontiers).await?;
        tx.commit().await.map_err(|e| storage_error(&e))?;
        Ok(a)
    }

    async fn read(&self, partition: u64, offset: u64) -> Result<Slot, StoreError> {
        read(&self.client, partition, offset).await
    }

    async fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Record)>, StoreError> {
        scan(&self.client, partition, from, limit).await
    }

    async fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> Result<(), StoreError> {
        let tx = self
            .client
            .transaction()
            .await
            .map_err(|e| storage_error(&e))?;
        erase_parts(&tx, partition, offset, parts, None).await?;
        tx.commit().await.map_err(|e| storage_error(&e))
    }

    async fn compact(&mut self, partition: u64, offsets: &[u64]) -> Result<(), StoreError> {
        let tx = self
            .client
            .transaction()
            .await
            .map_err(|e| storage_error(&e))?;
        compact(&tx, partition, offsets).await?;
        tx.commit().await.map_err(|e| storage_error(&e))
    }
}

impl LogStore for PgLog<&Transaction<'_>> {
    async fn create_partition(
        &mut self,
        partition: u64,
        segments: Segments,
    ) -> Result<(), StoreError> {
        create_partition(
            self.client,
            partition,
            segments,
            &PartitionInfo::unnamed(partition),
        )
        .await
    }

    async fn partitions(&self) -> Result<Vec<u64>, StoreError> {
        partitions(self.client).await
    }

    async fn head(&self, partition: u64) -> Result<Head, StoreError> {
        head(self.client, partition).await
    }

    async fn append(&mut self, partition: u64, draft: Draft) -> Result<Appended, StoreError> {
        append_with(self.client, partition, draft, &mut self.frontiers).await
    }

    async fn append_slot(&mut self, partition: u64, slot: Slot) -> Result<Appended, StoreError> {
        append_slot_with(self.client, partition, slot, &mut self.frontiers).await
    }

    async fn read(&self, partition: u64, offset: u64) -> Result<Slot, StoreError> {
        read(self.client, partition, offset).await
    }

    async fn scan(
        &self,
        partition: u64,
        from: u64,
        limit: usize,
    ) -> Result<Vec<(u64, Record)>, StoreError> {
        scan(self.client, partition, from, limit).await
    }

    async fn erase_parts(
        &mut self,
        partition: u64,
        offset: u64,
        parts: &[usize],
    ) -> Result<(), StoreError> {
        erase_parts(self.client, partition, offset, parts, None).await
    }

    async fn compact(&mut self, partition: u64, offsets: &[u64]) -> Result<(), StoreError> {
        compact(self.client, partition, offsets).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erased_masks() {
        assert_eq!(erased_mask([]), 0);
        assert_eq!(erased_mask([0]), 1);
        assert_eq!(erased_mask([1, 2]), 0b110);
        assert_eq!(erased_mask([15]), i16::MIN);
        assert_eq!(erased_mask([40]), i16::MIN);
        assert_eq!(erased_mask([0, 200]), i16::MIN | 1);
    }
}
