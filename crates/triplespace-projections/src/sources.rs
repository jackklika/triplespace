//! The source-graph projections: `view.keyed_surrogate` (step 1; 0009 §7),
//! `view.entity_source`, the version cursor of 0002 §8.4 per graph (step 2; 0013 §5.1),
//! and `view.keyed_map`, the upstream ID ↔ key index of a key-mapped provider (step 2;
//! 0009 §9).

use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::{Operation, Upstream};
use scatter_wikibase_model::id::{EntityId, IdForm, Subject};

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_CHANGESET, PAYLOAD_KEYED_SURROGATE, PartitionInfo, attested, content, content_hash,
    offset_db, operation, parse_surrogate_key, partition_info, quote, time_of,
};
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

fn rows(n: u64) -> Applied {
    Applied::rows(usize::try_from(n).unwrap_or(0))
}

/// The entity an operation is about, when it is one (a page's statements are not).
#[must_use]
pub fn entity_subject(op: &Operation) -> Option<EntityId> {
    match op.subject()? {
        Subject::Entity(id) => Some(id),
        Subject::Page(_) => None,
    }
}

/// A version as `entity_source.upstream_version` stores it: the revision ID in decimal,
/// or the provider's version string.
#[must_use]
pub fn version_text(upstream: &Upstream) -> Option<String> {
    upstream
        .revid
        .map(|r| r.to_string())
        .or_else(|| upstream.version.clone())
}

// --- keyed surrogates --------------------------------------------------------------------

/// `view.keyed_surrogate` from `keyed-surrogate` mapping records in the instance `log`,
/// keyed `{type}#{n}` with content `{keyed_type, surrogate, key}`. An erased mapping
/// leaves the surrogate with a `NULL` key (0009 §7: only an opaque number remains).
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyedSurrogateProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for KeyedSurrogateProjection {
    fn name(&self) -> &'static str {
        "keyed_surrogate"
    }

    fn step(&self) -> Step {
        Step::Registry
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_KEYED_SURROGATE
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = partition_info(cx, h.partition).await?.tenant;
            let key = h
                .key
                .as_deref()
                .ok_or("a keyed-surrogate record has a key")?;
            let (keyed_type, surrogate) = parse_surrogate_key(key)
                .ok_or_else(|| format!("`{key}` is not a surrogate key"))?;
            let mapped: Option<String> = match content(record)? {
                None => None,
                Some(v) => {
                    let t = v
                        .get("keyed_type")
                        .and_then(Value::as_text)
                        .ok_or("`keyed_type`")?;
                    let n = v
                        .get("surrogate")
                        .and_then(Value::as_u64)
                        .ok_or("`surrogate`")?;
                    if t != keyed_type || i64::try_from(n).ok() != Some(surrogate) {
                        return Err(format!("mapping content names {t}#{n}, the key is {key}"));
                    }
                    Some(
                        v.get("key")
                            .and_then(Value::as_text)
                            .ok_or("`key`")?
                            .to_string(),
                    )
                }
            };
            let n = cx.execute(
                    "INSERT INTO view.keyed_surrogate (tenant, keyed_type, surrogate, key) VALUES ($1, $2, $3, $4)
                     ON CONFLICT (tenant, keyed_type, surrogate) DO UPDATE SET key = EXCLUDED.key",
                    &[&tenant, &keyed_type, &surrogate, &mapped],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(rows(n))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let tenant = partition_info(cx, partition).await?.tenant;
            cx.batch_execute(&format!(
                "DELETE FROM view.keyed_surrogate WHERE tenant = {}",
                quote(&tenant)
            ))
            .await
            .map_err(|e| sql(&e))
        })
    }
}

// --- entity sources ----------------------------------------------------------------------

/// `view.entity_source`: per entity and graph, the graph's current record for it. A `put`
/// or any local operation sets the row; a `tombstone`, or an upstream `redirect`, removes
/// the mirror's row, since the mirror no longer contributes.
#[derive(Debug, Default, Clone, Copy)]
pub struct EntitySourceProjection;

/// The version a record sets the cursor to.
struct Cursor {
    version: Option<String>,
    prev: Option<String>,
    size: i32,
}

impl EntitySourceProjection {
    async fn upsert(
        cx: &PgCx,
        tenant: &str,
        id: &EntityId,
        graph: &str,
        record: &Record,
        cursor: Cursor,
    ) -> Result<u64, String> {
        let Cursor {
            version,
            prev,
            size,
        } = cursor;
        let hash = content_hash(record).ok_or("the content part is present")?;
        let job = attested(record)
            .job
            .map(i64::try_from)
            .transpose()
            .map_err(|e| e.to_string())?;
        cx.execute(
                "INSERT INTO view.entity_source
                   (tenant, entity_id, graph, \"offset\", upstream_version, prev_upstream, content_hash, size, synced_at, job_id)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                 ON CONFLICT (tenant, entity_id, graph) DO UPDATE SET
                   \"offset\" = EXCLUDED.\"offset\", upstream_version = EXCLUDED.upstream_version,
                   prev_upstream = EXCLUDED.prev_upstream, content_hash = EXCLUDED.content_hash,
                   size = EXCLUDED.size, synced_at = EXCLUDED.synced_at, job_id = EXCLUDED.job_id",
                &[
                    &tenant,
                    &id.as_str(),
                    &graph,
                    &offset_db(record)?,
                    &version,
                    &prev,
                    &hash.as_slice(),
                    &size,
                    &time_of(record.header().appended_at),
                    &job,
                ],
            )
            .await
            .map_err(|e| sql(&e))
    }

    async fn delete(cx: &PgCx, tenant: &str, id: &EntityId, graph: &str) -> Result<u64, String> {
        cx.execute(
            "DELETE FROM view.entity_source WHERE tenant = $1 AND entity_id = $2 AND graph = $3",
            &[&tenant, &id.as_str(), &graph],
        )
        .await
        .map_err(|e| sql(&e))
    }
}

/// The size a record contributes: the operation's own `size` for a `put`, else the
/// content part's length.
fn content_size(record: &Record, declared: Option<u64>) -> i32 {
    let n = declared.unwrap_or_else(|| {
        record
            .body()
            .content()
            .bytes()
            .map_or(0, |b| b.len() as u64)
    });
    i32::try_from(n).unwrap_or(i32::MAX)
}

impl<B: Backend<Cx = PgCx>> Projection<B> for EntitySourceProjection {
    fn name(&self) -> &'static str {
        "entity_source"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let info = partition_info(cx, record.header().partition).await?;
            let Some(op) = operation(record)? else {
                return Ok(Applied::default());
            };
            let Some(id) = entity_subject(&op) else {
                return Ok(Applied::default());
            };
            let graph = info.name.as_str();
            let t = info.tenant.as_str();
            let n = match &op {
                Operation::Put {
                    upstream,
                    prev_upstream,
                    size,
                    ..
                } => {
                    Self::upsert(
                        cx,
                        t,
                        &id,
                        graph,
                        record,
                        Cursor {
                            version: version_text(upstream),
                            prev: prev_upstream.as_ref().and_then(version_text),
                            size: content_size(record, *size),
                        },
                    )
                    .await?
                }
                Operation::Tombstone { .. }
                | Operation::Redirect {
                    upstream: Some(_), ..
                } => Self::delete(cx, t, &id, graph).await?,
                Operation::Adopt { source_revid, .. } => {
                    Self::upsert(
                        cx,
                        t,
                        &id,
                        graph,
                        record,
                        Cursor {
                            version: Some(source_revid.to_string()),
                            prev: None,
                            size: content_size(record, None),
                        },
                    )
                    .await?
                }
                _ => {
                    Self::upsert(
                        cx,
                        t,
                        &id,
                        graph,
                        record,
                        Cursor {
                            version: None,
                            prev: None,
                            size: content_size(record, None),
                        },
                    )
                    .await?
                }
            };
            Ok(rows(n))
        })
    }

    /// A block of `put`s, each naming its subject once, is one multi-row upsert of the rows
    /// [`Projection::apply`] writes one at a time (0013 §9); any other block is applied
    /// record by record.
    fn apply_many<'a>(
        &'a self,
        cx: &'a mut PgCx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let info = partition_info(cx, records[0].header().partition).await?;
            let mut subjects = std::collections::BTreeSet::new();
            let mut puts: Vec<(&Record, EntityId, Cursor)> = Vec::with_capacity(records.len());
            for record in records {
                match operation(record)? {
                    Some(Operation::Put {
                        id,
                        upstream,
                        prev_upstream,
                        size,
                        ..
                    }) if subjects.insert(id.as_str().to_string()) => {
                        let cursor = Cursor {
                            version: version_text(&upstream),
                            prev: prev_upstream.as_ref().and_then(version_text),
                            size: content_size(record, size),
                        };
                        puts.push((record, id, cursor));
                    }
                    _ => {
                        let mut out = Applied::default();
                        for record in records {
                            let a = Projection::<B>::apply(self, cx, record).await?;
                            out.rows += a.rows;
                            out.fanout.extend(a.fanout);
                        }
                        return Ok(out);
                    }
                }
            }
            let mut ids = Vec::with_capacity(puts.len());
            let mut offsets = Vec::with_capacity(puts.len());
            let mut versions = Vec::with_capacity(puts.len());
            let mut prevs = Vec::with_capacity(puts.len());
            let mut hashes: Vec<Vec<u8>> = Vec::with_capacity(puts.len());
            let mut sizes = Vec::with_capacity(puts.len());
            let mut synced = Vec::with_capacity(puts.len());
            let mut jobs = Vec::with_capacity(puts.len());
            for (record, id, cursor) in puts {
                ids.push(id.as_str().to_string());
                offsets.push(offset_db(record)?);
                versions.push(cursor.version);
                prevs.push(cursor.prev);
                hashes.push(
                    content_hash(record)
                        .ok_or("the content part is present")?
                        .to_vec(),
                );
                sizes.push(cursor.size);
                synced.push(time_of(record.header().appended_at));
                jobs.push(
                    attested(record)
                        .job
                        .map(i64::try_from)
                        .transpose()
                        .map_err(|e| e.to_string())?,
                );
            }
            let n = cx
                .execute(
                    "INSERT INTO view.entity_source
                       (tenant, entity_id, graph, \"offset\", upstream_version, prev_upstream, content_hash, size, synced_at, job_id)
                     SELECT $1, t.entity_id, $2, t.\"offset\", t.upstream_version, t.prev_upstream, t.content_hash, t.size, t.synced_at, t.job_id
                     FROM unnest($3::text[], $4::bigint[], $5::text[], $6::text[], $7::bytea[], $8::int[], $9::timestamptz[], $10::bigint[])
                          AS t(entity_id, \"offset\", upstream_version, prev_upstream, content_hash, size, synced_at, job_id)
                     ON CONFLICT (tenant, entity_id, graph) DO UPDATE SET
                       \"offset\" = EXCLUDED.\"offset\", upstream_version = EXCLUDED.upstream_version,
                       prev_upstream = EXCLUDED.prev_upstream, content_hash = EXCLUDED.content_hash,
                       size = EXCLUDED.size, synced_at = EXCLUDED.synced_at, job_id = EXCLUDED.job_id",
                    &[
                        &info.tenant,
                        &info.name,
                        &ids,
                        &offsets,
                        &versions,
                        &prevs,
                        &hashes,
                        &sizes,
                        &synced,
                        &jobs,
                    ],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(rows(n))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            cx.execute(
                "DELETE FROM view.entity_source WHERE tenant = $1 AND graph = $2",
                &[&info.tenant, &info.name],
            )
            .await
            .map(|_| ())
            .map_err(|e| sql(&e))
        })
    }
}

// --- keyed maps --------------------------------------------------------------------------

/// `view.keyed_map` from a mirror's `put` records whose subject is a keyed ID (0009 §9,
/// A13). The newest `put` under a key decides all of the key's rows: its `upstream_id` is
/// the owner, its `upstream_aliases` are the items upstream merged into the owner, and
/// rows those items held under other keys are dropped (an identity value that moved). A
/// `put` of the empty state, with no `upstream_id`, clears the key's rows.
#[derive(Debug, Clone, Default)]
pub struct KeyedMapProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for KeyedMapProjection {
    fn name(&self) -> &'static str {
        "keyed_map"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_CHANGESET
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let info: PartitionInfo = partition_info(cx, record.header().partition).await?;
            let Some(slug) = info.provider_slug() else {
                return Ok(Applied::default());
            };
            // A source's entities are in the colon shape but are not keyed (0078 §2).
            if info.is_source() {
                return Ok(Applied::default());
            }
            let Some(op) = operation(record)? else {
                return Ok(Applied::default());
            };
            let Operation::Put {
                id,
                upstream_id,
                upstream_aliases,
                ..
            } = &op
            else {
                return Ok(Applied::default());
            };
            if id.form() != IdForm::Keyed {
                return Ok(Applied::default());
            }
            let (keyed_type, key) = id.keyed_parts().ok_or("a keyed ID has a type and a key")?;
            let mut n = cx
                .execute(
                    "DELETE FROM view.keyed_map
                     WHERE tenant = $1 AND provider = $2 AND keyed_type = $3 AND key = $4",
                    &[&info.tenant, &slug, &keyed_type, &key],
                )
                .await
                .map_err(|e| sql(&e))?;
            let named: Vec<(&str, bool)> = upstream_id
                .iter()
                .map(|u| (u.as_str(), false))
                .chain(
                    upstream_aliases
                        .iter()
                        .filter(|a| upstream_id.as_ref() != Some(*a))
                        .map(|a| (a.as_str(), true)),
                )
                .collect();
            for (upstream, alias) in named {
                n += cx
                    .execute(
                        "INSERT INTO view.keyed_map (tenant, provider, upstream_id, keyed_type, key, alias)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (tenant, provider, upstream_id) DO UPDATE
                         SET keyed_type = EXCLUDED.keyed_type, key = EXCLUDED.key, alias = EXCLUDED.alias",
                        &[&info.tenant, &slug, &upstream, &keyed_type, &key, &alias],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }
            Ok(rows(n))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            let Some(slug) = info.provider_slug() else {
                return Ok(());
            };
            cx.execute(
                "DELETE FROM view.keyed_map WHERE tenant = $1 AND provider = $2",
                &[&info.tenant, &slug],
            )
            .await
            .map(|_| ())
            .map_err(|e| sql(&e))
        })
    }
}
