//! An in-memory [`IngestStore`] with transactional units of work, for tests: a copy of the
//! state is taken on `begin` and swapped in on `commit`. It keeps the indexes a job reads
//! (cursors, surrogates, match keys, adoption sources) up to date from the records it
//! appends, as the projections would.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log::store::{Appended, Draft};
use scatter_projection::memory::WorkQueue;
use scatter_projection::{Backend, ProjectionError, QueueLag, Queued, Work};
use scatter_wikibase_changeset::{MatchKey, Operation};
use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::statement::SnakKind;
use scatter_wikibase_model::value::{DataType, DataValue};

use crate::IngestError;
use crate::job::PAYLOAD_LOGEVENT;
use crate::store::{Cursor, IngestStore, Sequence};
use crate::write::{PAYLOAD_CHANGESET, PAYLOAD_KEYED_SURROGATE};

/// A partition.
#[derive(Debug, Clone, Default)]
pub struct Partition {
    /// The tenant, `""` for an instance partition.
    pub tenant: String,
    /// The graph name.
    pub graph: String,
    /// The records, by offset.
    pub records: Vec<Record>,
}

/// The committed state.
#[derive(Debug, Clone, Default)]
pub struct State {
    /// Projection positions.
    pub positions: BTreeMap<(String, u64), u64>,
    /// Queued fan-out.
    pub work: WorkQueue,
    /// Partitions by ID.
    pub partitions: BTreeMap<u64, Partition>,
    /// Sequences' last values.
    pub sequences: BTreeMap<(String, Sequence), u64>,
    /// `(tenant, entity, graph)` → cursor.
    pub cursors: BTreeMap<(String, String, String), Cursor>,
    /// `(keyed type, key)` → surrogate.
    pub surrogates: BTreeMap<(String, String), u64>,
    /// `(mirror graph, upstream item ID)` → the keyed entity it owns (0009 §9).
    pub keyed_owners: BTreeMap<(String, String), EntityId>,
    /// `(tenant, property, value)` → entity, for identifier match keys.
    pub identifiers: BTreeMap<(String, String, String), EntityId>,
    /// `(tenant, property)` → data type.
    pub datatypes: BTreeMap<(String, String), DataType>,
    /// `(tenant, source)` of adoption jobs started.
    pub adoptions: Vec<(String, String)>,
    /// Rows test projections write: `(table, key)` → value.
    pub rows: BTreeMap<(String, String), String>,
}

/// A unit of work.
#[derive(Debug)]
pub struct MemoryCx {
    /// The state as this unit sees it.
    pub state: State,
}

impl MemoryCx {
    /// Writes a row, for a test projection.
    pub fn put(&mut self, table: &str, key: &str, value: &str) {
        self.state
            .rows
            .insert((table.into(), key.into()), value.into());
    }
}

/// The store.
#[derive(Debug, Clone, Default)]
pub struct MemoryIngest {
    state: Arc<Mutex<State>>,
    /// What the write path handed the projections ([`IngestStore::encoded`]): each body's
    /// content bytes and the operation it was encoded from, outside any unit of work.
    encoded: Arc<Mutex<Vec<Encoded>>>,
}

/// A body's content bytes and the operation the write path encoded into them.
type Encoded = (Vec<u8>, Operation);

impl MemoryIngest {
    /// Empty.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The content bytes and operations the write path handed over
    /// ([`IngestStore::encoded`]), in order.
    #[must_use]
    pub fn encoded_ops(&self) -> Vec<(Vec<u8>, Operation)> {
        self.encoded.lock().expect("not poisoned").clone()
    }

    /// The committed state.
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("not poisoned")
    }

    /// Registers a partition.
    pub fn create_partition(&self, partition: u64, tenant: &str, graph: &str) {
        self.state().partitions.insert(
            partition,
            Partition {
                tenant: tenant.into(),
                graph: graph.into(),
                records: Vec::new(),
            },
        );
    }

    /// Sets a property's data type for a tenant.
    pub fn set_datatype(&self, tenant: &str, property: &str, datatype: DataType) {
        self.state()
            .datatypes
            .insert((tenant.into(), property.into()), datatype);
    }

    /// The records of a partition.
    #[must_use]
    pub fn records(&self, partition: u64) -> Vec<Record> {
        self.state()
            .partitions
            .get(&partition)
            .map(|p| p.records.clone())
            .unwrap_or_default()
    }

    /// The operations of a partition's change-set records, in order.
    #[must_use]
    pub fn operations(&self, partition: u64) -> Vec<Operation> {
        self.records(partition)
            .iter()
            .filter(|r| r.header().payload_type == PAYLOAD_CHANGESET)
            .filter_map(|r| r.body().content().value().ok().flatten())
            .filter_map(|v| scatter_log::cbor::from_value(v).ok())
            .collect()
    }
}

fn store(e: impl std::fmt::Display) -> IngestError {
    IngestError::Store(e.to_string())
}

/// The key-mapped owners, as `view.keyed_map` keeps them: the newest `put` under a key
/// decides who owns it (0009 A13).
fn index_keyed_owner(state: &mut State, graph: &str, id: &EntityId, op: &Operation) {
    let Operation::Put {
        upstream_id,
        upstream_aliases,
        ..
    } = op
    else {
        return;
    };
    if id.form() != IdForm::Keyed {
        return;
    }
    state.keyed_owners.retain(|(g, u), owned| {
        g != graph
            || (owned != id
                && upstream_id.as_deref() != Some(u.as_str())
                && !upstream_aliases.contains(u))
    });
    if let Some(u) = upstream_id {
        state
            .keyed_owners
            .insert((graph.to_string(), u.clone()), id.clone());
    }
}

/// Keeps the indexes in step with an appended record.
fn index(state: &mut State, partition: &Partition, record: &Record) -> Result<(), IngestError> {
    let h = record.header();
    let Some(content) = record.body().content().value().map_err(store)? else {
        return Ok(());
    };
    match h.payload_type.as_str() {
        PAYLOAD_CHANGESET => {
            let op: Operation = scatter_log::cbor::from_value(content).map_err(store)?;
            let Some(id) = op.subject().and_then(|s| s.entity_id().cloned()) else {
                return Ok(());
            };
            let key = (
                partition.tenant.clone(),
                id.to_string(),
                partition.graph.clone(),
            );
            match &op {
                Operation::Tombstone { .. }
                | Operation::Redirect {
                    upstream: Some(_), ..
                } => {
                    state.cursors.remove(&key);
                }
                _ => {
                    let version = match &op {
                        Operation::Put { upstream, .. } => upstream
                            .revid
                            .map(|r| r.to_string())
                            .or_else(|| upstream.version.clone()),
                        Operation::Adopt { source_revid, .. } => Some(source_revid.to_string()),
                        _ => None,
                    };
                    let size = match &op {
                        Operation::Put { size: Some(s), .. } => *s,
                        _ => record
                            .body()
                            .content()
                            .bytes()
                            .map_or(0, |b| b.len() as u64),
                    };
                    state.cursors.insert(
                        key,
                        Cursor {
                            offset: h.offset,
                            version,
                            content_hash: record
                                .body()
                                .content_hash()
                                .ok_or_else(|| store("content present"))?,
                            synced_at: h.appended_at,
                            size,
                        },
                    );
                }
            }
            index_keyed_owner(state, &partition.graph, &id, &op);
            // Identifier values, for match keys: every external-id statement the
            // operation carries.
            for s in op.statements() {
                if let (Some(DataType::ExternalId), SnakKind::Value(DataValue::String(v))) =
                    (s.mainsnak.datatype.as_ref(), &s.mainsnak.kind)
                {
                    state.identifiers.insert(
                        (
                            partition.tenant.clone(),
                            s.mainsnak.property.to_string(),
                            v.clone(),
                        ),
                        id.clone(),
                    );
                }
            }
        }
        PAYLOAD_KEYED_SURROGATE => {
            let t = content
                .get("keyed_type")
                .and_then(Value::as_text)
                .ok_or_else(|| store("keyed_type"))?;
            let k = content
                .get("key")
                .and_then(Value::as_text)
                .ok_or_else(|| store("key"))?;
            let n = content
                .get("surrogate")
                .and_then(Value::as_u64)
                .ok_or_else(|| store("surrogate"))?;
            state.surrogates.insert((t.to_string(), k.to_string()), n);
        }
        PAYLOAD_LOGEVENT => {
            if content.get("type").is_some_and(|t| t.text_eq("job"))
                && content.get("action").is_some_and(|a| a.text_eq("start"))
                && let Some(params) = content.get("params")
                && params.get("mode").is_some_and(|m| m.text_eq("adopt"))
                && let Some(source) = params.get("source").and_then(Value::as_text)
            {
                state
                    .adoptions
                    .push((partition.tenant.clone(), source.to_string()));
            }
        }
        _ => {}
    }
    Ok(())
}

impl Backend for MemoryIngest {
    type Cx = MemoryCx;

    async fn begin(&self) -> Result<MemoryCx, ProjectionError> {
        Ok(MemoryCx {
            state: self.state().clone(),
        })
    }

    async fn commit(&self, cx: MemoryCx) -> Result<(), ProjectionError> {
        *self.state() = cx.state;
        Ok(())
    }

    async fn rollback(&self, _cx: MemoryCx) -> Result<(), ProjectionError> {
        Ok(())
    }

    async fn applied(
        &self,
        cx: &mut MemoryCx,
        projection: &str,
        partition: u64,
    ) -> Result<u64, ProjectionError> {
        Ok(cx
            .state
            .positions
            .get(&(projection.to_string(), partition))
            .copied()
            .unwrap_or(0))
    }

    async fn set_applied(
        &self,
        cx: &mut MemoryCx,
        projection: &str,
        partition: u64,
        applied: u64,
    ) -> Result<(), ProjectionError> {
        cx.state
            .positions
            .insert((projection.to_string(), partition), applied);
        Ok(())
    }

    async fn queue(&self, cx: &mut MemoryCx, work: &[Work]) -> Result<(), ProjectionError> {
        cx.state.work.queue(work);
        Ok(())
    }

    async fn tenants_waiting(
        &self,
        cx: &mut MemoryCx,
        consumer: &str,
        max_attempts: u32,
    ) -> Result<Vec<String>, ProjectionError> {
        Ok(cx.state.work.tenants_waiting(consumer, max_attempts))
    }

    async fn lock_tenant(
        &self,
        _cx: &mut MemoryCx,
        _consumer: &str,
        _tenant: &str,
    ) -> Result<bool, ProjectionError> {
        Ok(true)
    }

    async fn claim(
        &self,
        cx: &mut MemoryCx,
        consumer: &str,
        tenant: &str,
        limit: usize,
        max_attempts: u32,
    ) -> Result<Vec<Queued>, ProjectionError> {
        Ok(cx.state.work.claim(consumer, tenant, limit, max_attempts))
    }

    async fn finish(&self, cx: &mut MemoryCx, ids: &[u64]) -> Result<(), ProjectionError> {
        cx.state.work.finish(ids);
        Ok(())
    }

    async fn fail(
        &self,
        cx: &mut MemoryCx,
        ids: &[u64],
        _error: &str,
    ) -> Result<(), ProjectionError> {
        cx.state.work.fail(ids);
        Ok(())
    }

    async fn queue_lag(
        &self,
        cx: &mut MemoryCx,
        max_attempts: u32,
    ) -> Result<Vec<QueueLag>, ProjectionError> {
        Ok(cx.state.work.lag(max_attempts))
    }
}

impl IngestStore for MemoryIngest {
    fn encoded(&self, body: &scatter_log::body::Body, op: Operation) {
        if let Some(bytes) = body.content().bytes() {
            self.encoded
                .lock()
                .expect("not poisoned")
                .push((bytes.to_vec(), op));
        }
    }

    async fn partition_size(&self, cx: &mut MemoryCx, partition: u64) -> Result<u64, IngestError> {
        cx.state
            .partitions
            .get(&partition)
            .map(|p| p.records.len() as u64)
            .ok_or_else(|| store(format!("no partition {partition:#x}")))
    }

    async fn append(
        &self,
        cx: &mut MemoryCx,
        partition: u64,
        draft: Draft,
    ) -> Result<Appended, IngestError> {
        let p = cx
            .state
            .partitions
            .get(&partition)
            .cloned()
            .ok_or_else(|| store(format!("no partition {partition:#x}")))?;
        let offset = p.records.len() as u64;
        let record = draft.seal(partition, offset);
        let leaf = record.leaf();
        index(&mut cx.state, &p, &record)?;
        cx.state
            .partitions
            .get_mut(&partition)
            .expect("present")
            .records
            .push(record);
        Ok(Appended {
            offset,
            leaf,
            root: leaf,
        })
    }

    async fn read(
        &self,
        cx: &mut MemoryCx,
        partition: u64,
        offset: u64,
    ) -> Result<Option<Record>, IngestError> {
        let p = cx
            .state
            .partitions
            .get(&partition)
            .ok_or_else(|| store(format!("no partition {partition:#x}")))?;
        Ok(p.records
            .get(usize::try_from(offset).map_err(store)?)
            .cloned())
    }

    async fn highest_revid(
        &self,
        cx: &mut MemoryCx,
        partition: u64,
        key: &str,
    ) -> Result<Option<u64>, IngestError> {
        let p = cx
            .state
            .partitions
            .get(&partition)
            .ok_or_else(|| store(format!("no partition {partition:#x}")))?;
        Ok(p.records
            .iter()
            .filter(|r| r.header().key.as_deref() == Some(key))
            .filter_map(|r| r.header().revid)
            .max())
    }

    async fn latest_for_key(
        &self,
        cx: &mut MemoryCx,
        partition: u64,
        key: &str,
    ) -> Result<Option<(u64, Header)>, IngestError> {
        let p = cx
            .state
            .partitions
            .get(&partition)
            .ok_or_else(|| store(format!("no partition {partition:#x}")))?;
        Ok(p.records
            .iter()
            .rev()
            .find(|r| r.header().key.as_deref() == Some(key))
            .map(|r| (r.header().offset, r.header().clone())))
    }

    async fn page_id_of(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        key: &str,
    ) -> Result<Option<u64>, IngestError> {
        Ok(cx
            .state
            .partitions
            .values()
            .filter(|p| p.tenant == tenant || p.tenant.is_empty())
            .flat_map(|p| p.records.iter())
            .find(|r| r.header().key.as_deref() == Some(key))
            .and_then(|r| r.header().page_id))
    }

    async fn partition(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        graph: &str,
    ) -> Result<Option<u64>, IngestError> {
        Ok(cx
            .state
            .partitions
            .iter()
            .find(|(_, p)| p.tenant == tenant && p.graph == graph)
            .map(|(id, _)| *id))
    }

    async fn next_id(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        sequence: &Sequence,
    ) -> Result<u64, IngestError> {
        let n = cx
            .state
            .sequences
            .entry((tenant.to_string(), sequence.clone()))
            .or_insert(0);
        *n += 1;
        Ok(*n)
    }

    async fn floor(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        sequence: &Sequence,
        consumed: u64,
    ) -> Result<(), IngestError> {
        let n = cx
            .state
            .sequences
            .entry((tenant.to_string(), sequence.clone()))
            .or_insert(0);
        *n = (*n).max(consumed);
        Ok(())
    }

    async fn cursor(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        entity_id: &EntityId,
        graph: &str,
    ) -> Result<Option<Cursor>, IngestError> {
        Ok(cx
            .state
            .cursors
            .get(&(tenant.to_string(), entity_id.to_string(), graph.to_string()))
            .cloned())
    }

    async fn entities_of_graph(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        graph: &str,
    ) -> Result<Vec<EntityId>, IngestError> {
        cx.state
            .cursors
            .keys()
            .filter(|(t, _, g)| t == tenant && g == graph)
            .map(|(_, id, _)| EntityId::parse(id).map_err(store))
            .collect()
    }

    async fn keyed_owners(
        &self,
        cx: &mut MemoryCx,
        graph: &str,
    ) -> Result<BTreeMap<String, EntityId>, IngestError> {
        Ok(cx
            .state
            .keyed_owners
            .iter()
            .filter(|((g, _), _)| g == graph)
            .map(|((_, u), id)| (u.clone(), id.clone()))
            .collect())
    }

    async fn find_match(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        key: &MatchKey,
    ) -> Result<Option<EntityId>, IngestError> {
        match key {
            MatchKey::Identifier(m) => Ok(m.iter().find_map(|(p, v)| {
                cx.state
                    .identifiers
                    .get(&(tenant.to_string(), p.to_string(), v.clone()))
                    .cloned()
            })),
            MatchKey::Entity(id) => {
                let present = cx
                    .state
                    .cursors
                    .keys()
                    .any(|(t, e, _)| (t == tenant || t.is_empty()) && e == id.as_str());
                Ok(present.then(|| id.clone()))
            }
        }
    }

    async fn surrogate(
        &self,
        cx: &mut MemoryCx,
        keyed_type: &str,
        key: &str,
    ) -> Result<Option<u64>, IngestError> {
        Ok(cx
            .state
            .surrogates
            .get(&(keyed_type.to_string(), key.to_string()))
            .copied())
    }

    async fn datatype(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        property: &EntityId,
    ) -> Result<Option<DataType>, IngestError> {
        Ok(cx
            .state
            .datatypes
            .get(&(tenant.to_string(), property.to_string()))
            .cloned())
    }

    async fn surrogate_actor(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
        kind: &str,
        raw: &str,
    ) -> Result<Option<String>, IngestError> {
        let Some((_, p)) = cx
            .state
            .partitions
            .iter()
            .find(|(_, p)| p.tenant == tenant && p.graph == "actors")
        else {
            return Ok(None);
        };
        for r in &p.records {
            let Some(key) = r.header().key.as_deref() else {
                continue;
            };
            if !key.starts_with("actor:") {
                continue;
            }
            let Some(v) = r.body().content().value().map_err(store)? else {
                continue;
            };
            if v.get("kind").is_some_and(|k| k.text_eq(kind))
                && v.get("raw").is_some_and(|k| k.text_eq(raw))
            {
                return Ok(Some(key.to_string()));
            }
        }
        Ok(None)
    }

    async fn only_adoptions(&self, cx: &mut MemoryCx, partition: u64) -> Result<bool, IngestError> {
        let p = cx
            .state
            .partitions
            .get(&partition)
            .ok_or_else(|| store(format!("no partition {partition:#x}")))?;
        for r in p
            .records
            .iter()
            .filter(|r| r.header().payload_type == PAYLOAD_CHANGESET)
        {
            let Some(v) = r.body().content().value().map_err(store)? else {
                return Ok(false);
            };
            if !v.get("op").is_some_and(|o| o.text_eq("adopt")) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn adoption_sources(
        &self,
        cx: &mut MemoryCx,
        tenant: &str,
    ) -> Result<Vec<String>, IngestError> {
        Ok(cx
            .state
            .adoptions
            .iter()
            .filter(|(t, _)| t == tenant)
            .map(|(_, s)| s.clone())
            .collect())
    }
}
