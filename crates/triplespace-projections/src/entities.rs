//! The resolution projection (step 4; 0013 §5.1–5.2, §7): `view.entity`, `view.term` and
//! `view.identifier` for one subject, recomputed whenever a record about it lands in any
//! source graph.
//!
//! For a subject the projection gathers its contributions from `view.entity_source`: the
//! tenant's local graph, folded from the subject's local records
//! ([`LocalState`]), and each mirror's current
//! state, read from the record the cursor names; resolves them in graph order
//! ([`scatter_wikibase_resolve::resolve`]); and writes the rows. The resolved JSON is
//! stored only where it differs from a single whole-state source (0013 §5.1).
//!
//! Tenant overlays (0018 §6; 0013 §7): a record in a tenant partition recomputes that
//! tenant's `(tenant, id)` row; a record in a shared mirror partition recomputes the shared
//! `('', id)` row and the row of every tenant that holds one for the key.
//!
//! `term` and `identifier` are tables of this projection rather than projections of their
//! own: they are derived from the same resolution pass, and splitting them would resolve
//! each subject three times. Clusters are not handled yet: every ID is its own canonical
//! ID, and there is no fan-out to referrers.

use std::collections::BTreeMap;
use std::sync::Arc;

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::Slot;
use scatter_log_postgres::PgClient;
use scatter_log_postgres::log;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_providers::Registry;
use scatter_wikibase_changeset::apply::{Correction as LocalCorrection, LocalState};
use scatter_wikibase_changeset::{Operation, Retention};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::{EntityId, IdForm, StatementId};
use scatter_wikibase_model::key::{Identity, value_key};
use scatter_wikibase_model::statement::SnakKind;
use scatter_wikibase_model::value::{DataType, DataValue};
use scatter_wikibase_resolve::{Contribution, Correction, Resolved, resolve};

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_CHANGESET, Partitions, header_key_for, operation, parse_surrogate_key, partition_info,
    partition_of, time_of,
};
use crate::sources::entity_subject;

/// The instance's term languages, the languages in which a mirrored entity's terms are
/// shared rows of `view.term` (05 §5.8; 0070 §6; 0082 §6), as a CTE named
/// `shared_term_languages(lang)`. The instance's `entities.term_languages` is the ceiling
/// it sets for every tenant, the `tenancy:entities.term_languages` switch (23 §3.2,
/// §3.4); without one, the default: the instance's content languages, `mul` and `en`.
/// The content languages are every tenant's `site.language` and the instance's default
/// for it (`template:site:site.language`), so each tenant's own language is always among
/// them (0082 A2: what "the instance's content languages" are).
pub const SHARED_TERM_LANGUAGES: &str = "\
    term_ceiling AS (
        SELECT config->'value' AS v FROM view.registry
        WHERE tenant = '' AND kind = 'tenancy' AND code = 'entities.term_languages'
          AND jsonb_typeof(config->'value') = 'array'
    ),
    shared_term_languages(lang) AS (
        SELECT jsonb_array_elements_text(v) FROM term_ceiling
        UNION
        SELECT d.lang FROM (
            SELECT 'mul' AS lang
            UNION SELECT 'en'
            UNION SELECT config->>'value' FROM view.registry
                WHERE tenant <> '' AND kind = 'site' AND code = 'site.language'
            UNION SELECT coalesce(config->'record'->>'value', config->>'value') FROM view.registry
                WHERE tenant = '' AND kind = 'template' AND code = 'site:site.language'
        ) d
        WHERE d.lang IS NOT NULL AND NOT EXISTS (SELECT 1 FROM term_ceiling)
    )";

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// `view.entity`, `view.term`, `view.identifier`.
#[derive(Debug, Clone)]
pub struct EntityProjection {
    /// Provider slugs in the instance's order (0004 §4), for the order of mirror graphs.
    order: Vec<String>,
}

impl Default for EntityProjection {
    /// The default order of 0004 §4: Wikidata, OpenAlex, then the registry's order.
    fn default() -> Self {
        Self::new(&["wikidata", "openalex"], Registry::default_registry())
    }
}

/// One source graph's cursor row.
pub(crate) struct Source {
    pub(crate) tenant: String,
    pub(crate) graph: String,
    pub(crate) offset: u64,
    pub(crate) synced_at: std::time::SystemTime,
}

/// What the local graph contributed, beyond its state.
#[derive(Default)]
pub(crate) struct Local {
    pub(crate) state: LocalState,
    pub(crate) latest: Option<Header>,
    /// Whether the local records are exactly one whole-state record.
    pub(crate) single_whole: bool,
}

impl EntityProjection {
    /// With the given provider slugs first, then the registry's other providers.
    #[must_use]
    pub fn new(slugs: &[&str], registry: &Registry) -> Self {
        let mut order: Vec<String> = slugs.iter().map(|s| (*s).to_string()).collect();
        for p in registry.providers() {
            if !order.contains(&p.slug) {
                order.push(p.slug.clone());
            }
        }
        Self { order }
    }

    pub(crate) fn mirror_rank(&self, graph: &str) -> usize {
        graph
            .strip_prefix("mirror/")
            .and_then(|slug| self.order.iter().position(|s| s == slug))
            .unwrap_or(usize::MAX)
    }

    pub(crate) async fn sources<C: Partitions>(
        client: &C,
        tenant: &str,
        id: &EntityId,
    ) -> Result<Vec<Source>, String> {
        let rows = client
            .query(
                "SELECT tenant, graph, \"offset\", synced_at FROM view.entity_source
                 WHERE entity_id = $1 AND (tenant = $2 OR tenant = '')",
                &[&id.as_str(), &tenant],
            )
            .await
            .map_err(|e| sql(&e))?;
        rows.iter()
            .map(|r| {
                Ok(Source {
                    tenant: r.get(0),
                    graph: r.get(1),
                    offset: scatter_log_postgres::ids::from_db(r.get(2))
                        .map_err(|e| e.to_string())?,
                    synced_at: r.get(3),
                })
            })
            .collect()
    }

    /// Folds the tenant's local records for the subject.
    pub(crate) async fn local<C: Partitions>(
        client: &C,
        tenant: &str,
        id: &EntityId,
    ) -> Result<Local, String> {
        let Some(partition) = partition_of(client, tenant, "local").await? else {
            return Ok(Local::default());
        };
        // A keyed subject with local records has a surrogate, written before them; its
        // absence means the instance `log` has not been projected yet, which a rebuild
        // that orders partitions wrongly would do. Fail loudly rather than drop the local
        // contribution.
        let Some(key) = header_key_for(client, id).await? else {
            return Err(format!(
                "`{id}` has local records but no surrogate in view.keyed_surrogate; project the instance log first"
            ));
        };
        // The tier-1 state (0083 §4): one row, and the one record it names, where the
        // table has the key; the fold of every record otherwise (0083 A1).
        if let Some((state, offset, _)) =
            crate::revision_state::graph_state(client, tenant, id.as_str(), "local").await?
            && let Ok(scatter_log::store::Slot::Record(record)) =
                log::read(client, partition, offset).await
            && let Some(op) = operation(&record)?
        {
            let whole = matches!(*op, Operation::Create { .. } | Operation::Adopt { .. });
            return Ok(Local {
                single_whole: state.offsets.len() == 1 && whole,
                latest: Some(record.header().clone()),
                state,
            });
        }
        let records = log::read_by_key(client, partition, &key)
            .await
            .map_err(|e| e.to_string())?;
        let mut local = Local::default();
        let mut whole = 0usize;
        let mut applied = 0usize;
        for (offset, record) in &records {
            if record.header().payload_type != PAYLOAD_CHANGESET {
                continue;
            }
            let Some(op) = operation(record)? else {
                continue;
            };
            if entity_subject(&op).as_ref() != Some(id) {
                // A surrogate key is shared by nothing else; an entity key names only
                // itself. This guards a stale index, not a design case.
                continue;
            }
            if matches!(*op, Operation::Create { .. } | Operation::Adopt { .. }) {
                whole += 1;
            }
            applied += 1;
            local.state.apply(&op, Some(*offset));
            local.latest = Some(record.header().clone());
        }
        local.single_whole = applied == 1 && whole == 1;
        Ok(local)
    }

    /// The state a mirror's cursor names.
    pub(crate) async fn mirror<C: Partitions>(
        client: &C,
        source: &Source,
    ) -> Result<Option<(Entity, Header)>, String> {
        let Some(partition) = partition_of(client, &source.tenant, &source.graph).await? else {
            return Err(format!("graph `{}` has no partition", source.graph));
        };
        let slot = log::read(client, partition, source.offset)
            .await
            .map_err(|e| e.to_string())?;
        let Slot::Record(record) = slot else {
            return Ok(None);
        };
        match operation(&record)?.as_deref() {
            Some(Operation::Put { entity, .. }) => {
                Ok(Some((entity.clone(), record.header().clone())))
            }
            _ => Ok(None),
        }
    }

    async fn delete_rows(cx: &PgCx, tenant: &str, id: &EntityId) -> Result<u64, String> {
        let mut n = 0;
        for table in ["view.term", "view.identifier"] {
            n += cx
                .execute(
                    &format!("DELETE FROM {table} WHERE tenant = $1 AND entity_id = $2"),
                    &[&tenant, &id.as_str()],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        n += cx
            .execute(
                "DELETE FROM view.entity WHERE tenant = $1 AND id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        Ok(n)
    }

    /// Recomputes one `(tenant, id)` row and its terms and identifiers.
    #[allow(clippy::too_many_lines)]
    async fn resolve_subject(&self, cx: &PgCx, tenant: &str, id: &EntityId) -> Result<u64, String> {
        let mut sources = Self::sources(cx, tenant, id).await?;
        sources.sort_by_key(|s| {
            (
                s.graph != "local",
                self.mirror_rank(&s.graph),
                s.graph.clone(),
            )
        });
        let has_local = !tenant.is_empty()
            && sources
                .iter()
                .any(|s| s.graph == "local" && s.tenant == tenant);
        let local = if has_local {
            Self::local(cx, tenant, id).await?
        } else {
            Local::default()
        };
        let mut contributions = Vec::new();
        let mut headers: Vec<Header> = local.latest.iter().cloned().collect();
        let mut first_seen = sources.iter().map(|s| s.synced_at).min();
        if let Some(e) = &local.state.entity {
            contributions.push(Contribution {
                graph: "local".to_string(),
                entity: e.clone(),
            });
        }
        let mut mirror_states = 0usize;
        for s in sources
            .iter()
            .filter(|s| s.graph.starts_with("mirror/") || s.graph.starts_with("source/"))
        {
            if let Some((entity, header)) = Self::mirror(cx, s).await? {
                headers.push(header);
                mirror_states += 1;
                contributions.push(Contribution {
                    graph: s.graph.clone(),
                    entity,
                });
            }
        }
        let corrections: BTreeMap<StatementId, Correction> = local
            .state
            .corrections
            .iter()
            .map(|(k, c)| {
                (
                    k.clone(),
                    match c {
                        LocalCorrection::Rank(r) => Correction::Rank(*r),
                        LocalCorrection::Suppress => Correction::Suppress,
                    },
                )
            })
            .collect();
        let Some(resolved) = resolve(&contributions, &corrections) else {
            return Self::delete_rows(cx, tenant, id).await;
        };
        let page_id = headers
            .iter()
            .filter_map(|h| h.page_id)
            .max()
            .ok_or_else(|| format!("no record for `{id}` carries a page ID (0013 §6)"))?;
        let page_id = i64::try_from(page_id).map_err(|e| e.to_string())?;
        // The resolved JSON is needed either way (the stored copy, or the size), so it is
        // built once and the comparison reuses it; the cheap conditions go first.
        let json = resolved.canonical_json();
        let single_source = contributions.len() == 1
            && (mirror_states == 1 || local.single_whole)
            && resolved.equals_given(&json, &contributions[0].entity);
        let (kind, blob): (&str, Option<Vec<u8>>) = if single_source {
            ("source", None)
        } else {
            ("materialized", Some(json.as_bytes().to_vec()))
        };
        let size = i32::try_from(json.len()).unwrap_or(i32::MAX);
        let surrogate = if id.form() == IdForm::Keyed {
            header_key_for(cx, id)
                .await?
                .and_then(|k| parse_surrogate_key(&k).map(|(_, n)| n))
        } else {
            None
        };
        let local_offset = local
            .latest
            .as_ref()
            .map(|h| i64::try_from(h.offset))
            .transpose()
            .map_err(|e| e.to_string())?;
        let local_revid = local
            .latest
            .as_ref()
            .and_then(|h| h.revid)
            .map(i64::try_from)
            .transpose()
            .map_err(|e| e.to_string())?;
        let retention = local.state.retention.map(|r| match r {
            Retention::Cascade => "cascade",
            Retention::Orphan => "orphan",
            Retention::Retain => "retain",
        });
        let first_seen = first_seen
            .get_or_insert_with(|| {
                time_of(headers.iter().map(|h| h.appended_at).min().unwrap_or(0))
            })
            .to_owned();
        // The three writes are independent once resolution is done; issued together they
        // go down the connection in one flush (tokio-postgres pipelines concurrently
        // polled queries), and the order within each pair (delete, then insert) holds.
        let id_text = id.as_str();
        let type_name = resolved.entity.entity_type.name();
        let entity_row = async {
            cx.execute(
                "INSERT INTO view.entity (tenant, id, type, page_id, surrogate, cluster_id, canonical_id, retention,
                                          first_seen, local_offset, local_revid, resolved_version, generation,
                                          resolved_kind, resolved, resolved_size, deleted)
                 VALUES ($1, $2, $3, $4, $5, NULL, $2, $6, $7, $8, $9, 1, 0, $10, $11, $12, false)
                 ON CONFLICT (tenant, id) DO UPDATE SET
                   type = EXCLUDED.type, page_id = EXCLUDED.page_id, surrogate = EXCLUDED.surrogate,
                   retention = EXCLUDED.retention, first_seen = LEAST(view.entity.first_seen, EXCLUDED.first_seen),
                   local_offset = EXCLUDED.local_offset, local_revid = EXCLUDED.local_revid,
                   resolved_version = view.entity.resolved_version + 1,
                   resolved_kind = EXCLUDED.resolved_kind, resolved = EXCLUDED.resolved, resolved_size = EXCLUDED.resolved_size",
                &[
                    &tenant,
                    &id_text,
                    &type_name,
                    &page_id,
                    &surrogate,
                    &retention,
                    &first_seen,
                    &local_offset,
                    &local_revid,
                    &kind,
                    &blob,
                    &size,
                ],
            )
            .await
            .map_err(|e| sql(&e))
        };
        let (n, terms, identifiers) = tokio::try_join!(
            entity_row,
            Self::write_terms(cx, tenant, id, &resolved),
            Self::write_identifiers(cx, tenant, id, &resolved),
        )?;
        Ok(n + terms + identifiers)
    }

    /// Replaces the subject's `view.term` rows: one delete, one multi-row insert.
    ///
    /// A tenant's row (a local entity, an overlay) holds every language. A shared row
    /// (`tenant = ''`, a mirrored entity) holds only the instance's term languages (05 §5.8;
    /// 0070 §6; 0082 §6): the other languages stay in the record, which `wbgetentities`
    /// reads. The set is [`SHARED_TERM_LANGUAGES`], applied inside the insert, so the rows
    /// follow the configuration in the store with no extra round trip; a change to it
    /// reaches rows already written when the entity projection is rebuilt.
    async fn write_terms(
        cx: &PgCx,
        tenant: &str,
        id: &EntityId,
        resolved: &Resolved,
    ) -> Result<u64, String> {
        let mut n = cx
            .execute(
                "DELETE FROM view.term WHERE tenant = $1 AND entity_id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        let mut kinds: Vec<i16> = Vec::new();
        let mut langs: Vec<&str> = Vec::new();
        let mut ordinals: Vec<i16> = Vec::new();
        let mut texts: Vec<&str> = Vec::new();
        for (kind, lang, ordinal, text) in term_rows(&resolved.entity) {
            kinds.push(kind);
            langs.push(lang);
            ordinals.push(ordinal);
            texts.push(text);
        }
        if !kinds.is_empty() {
            n += cx
                .execute(
                    &format!(
                        "WITH {SHARED_TERM_LANGUAGES}
                         INSERT INTO view.term (tenant, entity_id, kind, lang, ordinal, text)
                         SELECT $1, $2, t.kind, t.lang, t.ordinal, t.text
                         FROM unnest($3::smallint[], $4::text[], $5::smallint[], $6::text[]) AS t(kind, lang, ordinal, text)
                         WHERE $1 <> '' OR t.lang IN (SELECT lang FROM shared_term_languages)"
                    ),
                    &[&tenant, &id.as_str(), &kinds, &langs, &ordinals, &texts],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        Ok(n)
    }

    /// Replaces the subject's `view.identifier` rows: one delete, one multi-row insert.
    /// The same `(property, value, graph)` asserted twice keeps the later statement's
    /// ID, as the row-at-a-time upsert did.
    async fn write_identifiers(
        cx: &PgCx,
        tenant: &str,
        id: &EntityId,
        resolved: &Resolved,
    ) -> Result<u64, String> {
        let mut n = cx
            .execute(
                "DELETE FROM view.identifier WHERE tenant = $1 AND entity_id = $2",
                &[&tenant, &id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        let rows = identifier_rows(resolved);
        if rows.is_empty() {
            return Ok(n);
        }
        let mut properties: Vec<&str> = Vec::with_capacity(rows.len());
        let mut keys: Vec<&str> = Vec::with_capacity(rows.len());
        let mut graphs: Vec<&str> = Vec::with_capacity(rows.len());
        let mut sids: Vec<&str> = Vec::with_capacity(rows.len());
        for ((property, key, graph), sid) in &rows {
            properties.push(property);
            keys.push(key.as_str());
            graphs.push(graph);
            sids.push(sid);
        }
        n += cx
            .execute(
                "INSERT INTO view.identifier (tenant, property, value_key, entity_id, graph, statement_id)
                 SELECT $1, t.property, t.value_key, $2, t.graph, t.statement_id
                 FROM unnest($3::text[], $4::text[], $5::text[], $6::text[]) AS t(property, value_key, graph, statement_id)",
                &[&tenant, &id.as_str(), &properties, &keys, &graphs, &sids],
            )
            .await
            .map_err(|e| sql(&e))?;
        Ok(n)
    }

    /// [`Projection::apply_many`] for a block of one partition (0013 §9, A71). A mirror's `put`
    /// whose subject is in no other graph and no tenant's overlay resolves to its own state,
    /// so its rows come from the record alone: they are computed on worker threads and
    /// written with one statement per table for the block. Every other record (another
    /// operation, a keyed subject, a subject named twice in the block, or one another graph
    /// or an overlay also holds) is applied as [`Projection::apply`] applies it.
    async fn apply_block(&self, cx: &PgCx, records: &[Record]) -> Result<Applied, String> {
        let info = partition_info(cx, records[0].header().partition).await?;
        let graph = info.name.clone();
        let mut fast: Vec<(&Record, Arc<Operation>)> = Vec::new();
        let mut slow: Vec<&Record> = Vec::new();
        if !info.tenant.is_empty() || !graph.starts_with("mirror/") {
            slow.extend(records);
        } else {
            for record in records {
                match operation(record)? {
                    Some(op) if matches!(&*op, Operation::Put { id, .. } if id.form() != IdForm::Keyed) =>
                    {
                        fast.push((record, op));
                    }
                    _ => slow.push(record),
                }
            }
        }
        // A subject named twice in the block, or held elsewhere, goes the per-record way.
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for (_, op) in &fast {
            *seen.entry(put(op).0.as_str().to_string()).or_default() += 1;
        }
        let ids: Vec<&str> = seen.keys().map(String::as_str).collect();
        let held: std::collections::BTreeSet<String> = if ids.is_empty() {
            std::collections::BTreeSet::new()
        } else {
            cx.query(
                "SELECT entity_id FROM view.entity_source
                 WHERE entity_id = ANY($1) AND NOT (tenant = '' AND graph = $2)
                 UNION SELECT id FROM view.entity WHERE id = ANY($1) AND tenant <> ''",
                &[&ids, &graph],
            )
            .await
            .map_err(|e| sql(&e))?
            .iter()
            .map(|r| r.get(0))
            .collect()
        };
        let (fast, moved): (Vec<_>, Vec<_>) = fast.into_iter().partition(|(_, op)| {
            let id = put(op).0.as_str();
            seen[id] == 1 && !held.contains(id)
        });
        slow.extend(moved.into_iter().map(|(r, _)| r));
        slow.sort_by_key(|r| r.header().offset);

        let rows = shared_rows(&graph, fast)?;
        let mut n = write_shared_rows(cx, &rows).await?;
        for record in slow {
            n += self.apply_one(cx, record).await?;
        }
        Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
    }

    /// What [`Projection::apply`] does for one record: the subject's row, and on a shared
    /// partition every tenant's overlay of it.
    async fn apply_one(&self, cx: &PgCx, record: &Record) -> Result<u64, String> {
        let info = partition_info(cx, record.header().partition).await?;
        let Some(op) = operation(record)? else {
            return Ok(0);
        };
        let mut ids: Vec<EntityId> = entity_subject(&op).into_iter().collect();
        if let Operation::Convert { local, .. } = &*op {
            ids.push(local.clone());
        }
        let mut n = 0;
        for id in &ids {
            if info.tenant.is_empty() {
                n += self.resolve_subject(cx, "", id).await?;
                for tenant in Self::overlay_tenants(cx, id).await? {
                    n += self.resolve_subject(cx, &tenant, id).await?;
                }
            } else {
                n += self.resolve_subject(cx, &info.tenant, id).await?;
            }
        }
        Ok(n)
    }

    /// The tenants whose rows a shared-partition record about `id` touches: every tenant
    /// with an overlay row or a local cursor for the key.
    async fn overlay_tenants(cx: &PgCx, id: &EntityId) -> Result<Vec<String>, String> {
        let rows = cx
            .query(
                "SELECT tenant FROM view.entity WHERE id = $1 AND tenant <> ''
                 UNION SELECT tenant FROM view.entity_source WHERE entity_id = $1 AND tenant <> ''",
                &[&id.as_str()],
            )
            .await
            .map_err(|e| sql(&e))?;
        Ok(rows.iter().map(|r| r.get(0)).collect())
    }
}

/// One mirrored entity's shared rows, computed from its own state alone
/// ([`EntityProjection::apply_block`]).
struct SharedRows {
    id: String,
    type_name: String,
    page_id: i64,
    first_seen: std::time::SystemTime,
    kind: &'static str,
    blob: Option<Vec<u8>>,
    size: i32,
    /// `(kind, lang, ordinal, text)`.
    terms: Vec<(i16, String, i16, String)>,
    /// `(property, value key, graph, statement ID)`.
    identifiers: Vec<(String, String, String, String)>,
}

/// The shared rows of entities each held by one mirror record and nothing else, resolved
/// as [`EntityProjection::resolve_subject`] resolves such an entity: the record's state its
/// one contribution, no corrections, the page ID its header's, `first_seen` its append time
/// (the entity source's `synced_at`). Spread over worker threads for a large block.
fn shared_rows(
    graph: &str,
    fast: Vec<(&Record, Arc<Operation>)>,
) -> Result<Vec<SharedRows>, String> {
    let one = |(record, op): (&Record, Arc<Operation>)| -> Result<SharedRows, String> {
        let h = record.header();
        let (id, entity) = put(&op);
        let contributions = [Contribution {
            graph: graph.to_string(),
            entity: entity.clone(),
        }];
        let resolved =
            resolve(&contributions, &BTreeMap::new()).ok_or("one contribution resolves")?;
        let page_id = h
            .page_id
            .ok_or_else(|| format!("no record for `{id}` carries a page ID (0013 §6)"))?;
        let json = resolved.canonical_json();
        let (kind, blob) = if resolved.equals_given(&json, &contributions[0].entity) {
            ("source", None)
        } else {
            ("materialized", Some(json.as_bytes().to_vec()))
        };
        Ok(SharedRows {
            id: id.as_str().to_string(),
            type_name: resolved.entity.entity_type.name().to_string(),
            page_id: i64::try_from(page_id).map_err(|e| e.to_string())?,
            first_seen: time_of(h.appended_at),
            kind,
            blob,
            size: i32::try_from(json.len()).unwrap_or(i32::MAX),
            terms: term_rows(&resolved.entity)
                .into_iter()
                .map(|(k, l, o, x)| (k, l.to_string(), o, x.to_string()))
                .collect(),
            identifiers: identifier_rows(&resolved)
                .into_iter()
                .map(|((p, key, g), sid)| (p.to_string(), key, g.to_string(), sid.to_string()))
                .collect(),
        })
    };
    if fast.len() < 64 {
        return fast.into_iter().map(one).collect();
    }
    crate::common::par_map(fast, one).into_iter().collect()
}

/// The subject and state of a `put`, which [`EntityProjection::apply_block`] has
/// checked every operation it passes on is.
fn put(op: &Operation) -> (&EntityId, &Entity) {
    match op {
        Operation::Put { id, entity, .. } => (id, entity),
        _ => unreachable!("apply_block passes on only puts"),
    }
}

/// Writes [`SharedRows`] for a block: one upsert of `view.entity`, and the terms (in the
/// instance's term languages, as [`EntityProjection::write_terms`] writes shared rows) and
/// identifiers replaced with one delete and one insert each.
async fn write_shared_rows(cx: &PgCx, rows: &[SharedRows]) -> Result<u64, String> {
    if rows.is_empty() {
        return Ok(0);
    }
    let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    let types: Vec<&str> = rows.iter().map(|r| r.type_name.as_str()).collect();
    let pages: Vec<i64> = rows.iter().map(|r| r.page_id).collect();
    let first: Vec<std::time::SystemTime> = rows.iter().map(|r| r.first_seen).collect();
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind).collect();
    let blobs: Vec<Option<&[u8]>> = rows.iter().map(|r| r.blob.as_deref()).collect();
    let sizes: Vec<i32> = rows.iter().map(|r| r.size).collect();
    let mut n = cx
        .execute(
            "INSERT INTO view.entity (tenant, id, type, page_id, surrogate, cluster_id, canonical_id, retention,
                                      first_seen, local_offset, local_revid, resolved_version, generation,
                                      resolved_kind, resolved, resolved_size, deleted)
             SELECT '', t.id, t.type, t.page_id, NULL, NULL, t.id, NULL, t.first_seen, NULL, NULL, 1, 0,
                    t.kind, t.resolved, t.size, false
             FROM unnest($1::text[], $2::text[], $3::bigint[], $4::timestamptz[], $5::text[], $6::bytea[], $7::int[])
                  AS t(id, type, page_id, first_seen, kind, resolved, size)
             ON CONFLICT (tenant, id) DO UPDATE SET
               type = EXCLUDED.type, page_id = EXCLUDED.page_id, surrogate = EXCLUDED.surrogate,
               retention = EXCLUDED.retention, first_seen = LEAST(view.entity.first_seen, EXCLUDED.first_seen),
               local_offset = EXCLUDED.local_offset, local_revid = EXCLUDED.local_revid,
               resolved_version = view.entity.resolved_version + 1,
               resolved_kind = EXCLUDED.resolved_kind, resolved = EXCLUDED.resolved, resolved_size = EXCLUDED.resolved_size",
            &[&ids, &types, &pages, &first, &kinds, &blobs, &sizes],
        )
        .await
        .map_err(|e| sql(&e))?;
    for table in ["view.term", "view.identifier"] {
        n += cx
            .execute(
                &format!("DELETE FROM {table} WHERE tenant = '' AND entity_id = ANY($1)"),
                &[&ids],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    let (mut te, mut tk, mut tl, mut to, mut tx) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let (mut ie, mut ip, mut iv, mut ig, mut is) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for r in rows {
        for (kind, lang, ordinal, text) in &r.terms {
            te.push(r.id.as_str());
            tk.push(*kind);
            tl.push(lang.as_str());
            to.push(*ordinal);
            tx.push(text.as_str());
        }
        for (property, key, graph, sid) in &r.identifiers {
            ie.push(r.id.as_str());
            ip.push(property.as_str());
            iv.push(key.as_str());
            ig.push(graph.as_str());
            is.push(sid.as_str());
        }
    }
    if !te.is_empty() {
        n += cx
            .execute(
                &format!(
                    "WITH {SHARED_TERM_LANGUAGES}
                     INSERT INTO view.term (tenant, entity_id, kind, lang, ordinal, text)
                     SELECT '', t.entity_id, t.kind, t.lang, t.ordinal, t.text
                     FROM unnest($1::text[], $2::smallint[], $3::text[], $4::smallint[], $5::text[])
                          AS t(entity_id, kind, lang, ordinal, text)
                     WHERE t.lang IN (SELECT lang FROM shared_term_languages)"
                ),
                &[&te, &tk, &tl, &to, &tx],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    if !ie.is_empty() {
        n += cx
            .execute(
                "INSERT INTO view.identifier (tenant, property, value_key, entity_id, graph, statement_id)
                 SELECT '', t.property, t.value_key, t.entity_id, t.graph, t.statement_id
                 FROM unnest($1::text[], $2::text[], $3::text[], $4::text[], $5::text[])
                      AS t(entity_id, property, value_key, graph, statement_id)",
                &[&ie, &ip, &iv, &ig, &is],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(n)
}

/// An entity's `view.term` rows before the language test of [`SHARED_TERM_LANGUAGES`]:
/// `(kind, lang, ordinal, text)`, kind 1 a label, 2 a description, 3 an alias.
fn term_rows(e: &Entity) -> Vec<(i16, &str, i16, &str)> {
    let mut out = Vec::new();
    for (lang, text) in &e.labels {
        out.push((1, lang.as_str(), 0, text.as_str()));
    }
    for (lang, text) in &e.descriptions {
        out.push((2, lang.as_str(), 0, text.as_str()));
    }
    for (lang, values) in &e.aliases {
        for (i, v) in values.iter().enumerate() {
            out.push((
                3,
                lang.as_str(),
                i16::try_from(i).unwrap_or(i16::MAX),
                v.as_str(),
            ));
        }
    }
    out
}

/// A resolved state's `view.identifier` rows: `(property, value key, graph)` to the
/// statement ID. The same `(property, value, graph)` asserted twice keeps the later
/// statement's ID.
fn identifier_rows(resolved: &Resolved) -> BTreeMap<(&str, String, &str), &str> {
    let mut rows = BTreeMap::new();
    for (s, prov) in resolved.entity.all_statements().zip(&resolved.statements) {
        let (Some(DataType::ExternalId), SnakKind::Value(v @ DataValue::String(_)), Some(sid)) =
            (s.mainsnak.datatype.as_ref(), &s.mainsnak.kind, &s.id)
        else {
            continue;
        };
        let key = value_key(
            v,
            Some(&DataType::ExternalId),
            &s.mainsnak.property,
            &Identity,
        );
        for graph in &prov.graphs {
            rows.insert(
                (
                    s.mainsnak.property.as_str(),
                    key.as_str().to_string(),
                    graph.as_str(),
                ),
                sid.as_str(),
            );
        }
    }
    rows
}

impl<B: Backend<Cx = PgCx>> Projection<B> for EntityProjection {
    fn name(&self) -> &'static str {
        "entity"
    }

    fn step(&self) -> Step {
        Step::Resolution
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
            let n = self.apply_one(cx, record).await?;
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn apply_many<'a>(
        &'a self,
        cx: &'a mut PgCx,
        records: &'a [Record],
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move { self.apply_block(cx, records).await })
    }

    fn fanout<'a>(
        &'a self,
        cx: &'a mut PgCx,
        tenant: &'a str,
        key: &'a str,
    ) -> BoxFuture<'a, Result<usize, String>> {
        Box::pin(async move {
            let id = EntityId::parse(key).map_err(|e| e.to_string())?;
            let n = self.resolve_subject(cx, tenant, &id).await?;
            Ok(usize::try_from(n).unwrap_or(0))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            // A tenant partition resets the tenant's rows; a shared one the shared rows.
            for table in ["view.term", "view.identifier"] {
                cx.execute(
                    &format!("DELETE FROM {table} WHERE tenant = $1"),
                    &[&info.tenant],
                )
                .await
                .map_err(|e| sql(&e))?;
            }
            cx.execute("DELETE FROM view.entity WHERE tenant = $1", &[&info.tenant])
                .await
                .map(|_| ())
                .map_err(|e| sql(&e))
        })
    }
}
