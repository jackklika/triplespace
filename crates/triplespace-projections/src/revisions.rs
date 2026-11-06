//! Past states of the local graph (18 §1.3, §5.1; 0083 §4): what `oldid=`,
//! `prop=revisions` with content, `GET /entity/{id}/changes/{partition}/{offset}` and
//! `GET /entity/{id}/compare` read.
//!
//! **One row, not a replay.** 0083 §4 has every local revision's state in
//! `view.entity_revision`, written by the appending transaction
//! ([`crate::revision_state`]); [`local_state_at`] and [`local_change_at`] read that row
//! and the record it belongs to. For a row the table does not have — a database migrated
//! to the table but not yet rebuilt — they fall back to the fold of the subject's local
//! records (0083 A1), the same fold [`crate::read::current`] performs for the latest
//! state, so an instance answers either way and a rebuild makes the fallback moot.
//!
//! Mirror states are not reconstructable here: under the `latest` policy only the
//! current `put` exists, and under `full` each observed state is its own record, which
//! [`crate::read`] serves by offset.

use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::log;
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_changeset::apply::LocalState;
use scatter_wikibase_model::id::EntityId;

use scatter_log::store::Slot;

use crate::common::{
    PAYLOAD_CHANGESET, Partitions, attested, comment, header_key_for, operation, partition_of,
};
use crate::revision_state;
use crate::sources::entity_subject;

/// One local revision of a subject: the record and what it did.
#[derive(Debug, Clone)]
pub struct LocalRevision {
    /// The record's header: offset, revision ID, time, page ID.
    pub header: Header,
    /// The operation.
    pub operation: Operation,
    /// The actor key, from the attestation.
    pub actor: Option<String>,
    /// The user's comment.
    pub comment: Option<String>,
    /// Change tags.
    pub tags: Vec<String>,
}

impl LocalRevision {
    /// The offset.
    #[must_use]
    pub fn offset(&self) -> u64 {
        self.header.offset
    }

    /// The revision ID, which every local change-set record has (01 §2.5).
    #[must_use]
    pub fn revid(&self) -> Option<u64> {
        self.header.revid
    }
}

fn revision_of(record: &Record) -> Result<Option<Operation>, String> {
    if record.header().payload_type != PAYLOAD_CHANGESET {
        return Ok(None);
    }
    Ok(operation(record)?.map(std::sync::Arc::unwrap_or_clone))
}

/// Every local change-set record of a subject on a tenant, in offset order; empty when
/// the local graph has none. An erased record (its content part gone) is left out, as
/// it no longer says what it did.
pub async fn local_revisions<C: Partitions>(
    client: &C,
    tenant: &str,
    id: &EntityId,
) -> Result<Vec<LocalRevision>, String> {
    let Some(partition) = partition_of(client, tenant, "local").await? else {
        return Ok(Vec::new());
    };
    let Some(key) = header_key_for(client, id).await? else {
        return Ok(Vec::new());
    };
    let records = log::read_by_key(client, partition, &key)
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (_, record) in &records {
        let Some(op) = revision_of(record)? else {
            continue;
        };
        if entity_subject(&op).as_ref() != Some(id) {
            continue;
        }
        let a = attested(record);
        out.push(LocalRevision {
            header: record.header().clone(),
            operation: op,
            actor: a.actor,
            comment: comment(record),
            tags: a.tags,
        });
    }
    Ok(out)
}

/// The local revision at an offset of the tenant's local partition: the record, read
/// alone. `None` when there is no change-set record about `id` there.
async fn revision_at<C: Partitions>(
    client: &C,
    partition: u64,
    id: &EntityId,
    offset: u64,
) -> Result<Option<LocalRevision>, String> {
    let slot = match log::read(client, partition, offset).await {
        Ok(s) => s,
        Err(scatter_log::store::StoreError::NoOffset { .. }) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let Slot::Record(record) = slot else {
        return Ok(None);
    };
    let Some(op) = revision_of(&record)? else {
        return Ok(None);
    };
    if entity_subject(&op).as_ref() != Some(id) {
        return Ok(None);
    }
    let a = attested(&record);
    Ok(Some(LocalRevision {
        header: record.header().clone(),
        operation: op,
        actor: a.actor,
        comment: comment(&record),
        tags: a.tags,
    }))
}

/// The local graph's state after the revision at `offset`: the `view.entity_revision`
/// row (0083 §4), or the fold of every local record up to and including it where the
/// table has no row. `None` when no local record of the subject is at that offset.
pub async fn local_state_at<C: Partitions>(
    client: &C,
    tenant: &str,
    id: &EntityId,
    offset: u64,
) -> Result<Option<(LocalRevision, LocalState)>, String> {
    if let Some(partition) = partition_of(client, tenant, "local").await?
        && let Some((state, _)) = revision_state::state_at_record(client, partition, offset).await?
        && let Some(revision) = revision_at(client, partition, id, offset).await?
    {
        return Ok(Some((revision, state)));
    }
    let revisions = local_revisions(client, tenant, id).await?;
    Ok(fold_to(&revisions, offset))
}

/// The state just before and just after one revision, with the revision itself: two
/// `view.entity_revision` rows and two records, or the fold where either row is missing.
///
/// The row before is the one at the previous offset the stored state lists in its
/// `offsets`, not the table's nearest earlier row: on a database migrated but not rebuilt
/// the nearest row may be older than the previous revision, or absent, and the fold is
/// then the right answer.
pub async fn local_change_at<C: Partitions>(
    client: &C,
    tenant: &str,
    id: &EntityId,
    offset: u64,
) -> Result<Option<Change>, String> {
    if let Some(partition) = partition_of(client, tenant, "local").await?
        && let Some((after, _)) = revision_state::state_at_record(client, partition, offset).await?
        && let Some(revision) = revision_at(client, partition, id, offset).await?
    {
        let prev_offset = after.offsets.iter().rev().nth(1).copied();
        let before = match prev_offset {
            None => Some((LocalState::new(), None)),
            Some(po) => match revision_state::state_at_record(client, partition, po).await? {
                Some((state, _)) => Some((state, revision_at(client, partition, id, po).await?)),
                None => None,
            },
        };
        if let Some((before, previous)) = before {
            return Ok(Some(Change {
                revision,
                before,
                after,
                previous,
            }));
        }
    }
    let revisions = local_revisions(client, tenant, id).await?;
    Ok(change_at(&revisions, offset))
}

/// A revision with the states on either side of it.
#[derive(Debug, Clone)]
pub struct Change {
    /// The revision.
    pub revision: LocalRevision,
    /// The state before it: empty for the subject's first local record.
    pub before: LocalState,
    /// The state after it.
    pub after: LocalState,
    /// The previous local revision, when there is one.
    pub previous: Option<LocalRevision>,
}

/// Folds the revisions up to `offset`, inclusive.
#[must_use]
pub fn fold_to(revisions: &[LocalRevision], offset: u64) -> Option<(LocalRevision, LocalState)> {
    let mut state = LocalState::new();
    for r in revisions {
        if r.offset() > offset {
            break;
        }
        state.apply(&r.operation, Some(r.offset()));
        if r.offset() == offset {
            return Some((r.clone(), state));
        }
    }
    None
}

/// The change at `offset`: the fold before it and after it.
#[must_use]
pub fn change_at(revisions: &[LocalRevision], offset: u64) -> Option<Change> {
    let mut state = LocalState::new();
    let mut previous: Option<LocalRevision> = None;
    for r in revisions {
        if r.offset() > offset {
            break;
        }
        let before = state.clone();
        state.apply(&r.operation, Some(r.offset()));
        if r.offset() == offset {
            return Some(Change {
                revision: r.clone(),
                before,
                after: state,
                previous,
            });
        }
        previous = Some(r.clone());
    }
    None
}

/// The local offset a revision ID names, when it is a local revision of the tenant.
pub async fn offset_of_revid<C: Partitions>(
    client: &C,
    tenant: &str,
    revid: u64,
) -> Result<Option<u64>, String> {
    let row = client
        .query_opt(
            "SELECT a.\"offset\" FROM view.activity a
             WHERE a.tenant = $1 AND a.revid = $2 AND a.source = 'local'",
            &[&tenant, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| u64::try_from(r.get::<_, i64>(0)).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rev(offset: u64, op: serde_json::Value) -> LocalRevision {
        let header = Header {
            version: 0,
            partition: 0x2001,
            offset,
            appended_at: 1_700_000_000_000_000 + offset,
            payload_type: PAYLOAD_CHANGESET.to_string(),
            key: Some("Q6".into()),
            commitment: [0; 32],
            revid: Some(900 + offset),
            logid: None,
            page_id: Some(1),
        };
        LocalRevision {
            header,
            operation: serde_json::from_value(op).unwrap(),
            actor: Some("local:1".into()),
            comment: None,
            tags: Vec::new(),
        }
    }

    fn revisions() -> Vec<LocalRevision> {
        vec![
            rev(
                1,
                json!({"op": "create", "id": "Q6", "entity": {"type": "item", "id": "Q6",
                    "labels": {"en": {"language": "en", "value": "Six"}}}}),
            ),
            rev(3, json!({"op": "add", "id": "Q6", "labels": {"fr": "Six"}})),
            rev(5, json!({"op": "remove", "id": "Q6", "labels": ["en"]})),
        ]
    }

    #[test]
    fn folds_stop_at_the_offset() {
        let r = revisions();
        let (at, state) = fold_to(&r, 3).unwrap();
        assert_eq!(at.revid(), Some(903));
        let e = state.entity.unwrap();
        assert_eq!(e.labels.len(), 2, "en and fr");
        let (_, state) = fold_to(&r, 5).unwrap();
        assert_eq!(state.entity.unwrap().labels.len(), 1, "en removed");
        assert!(fold_to(&r, 4).is_none(), "no record at 4");
        assert!(fold_to(&r, 9).is_none());
    }

    #[test]
    fn a_change_has_both_sides() {
        let r = revisions();
        let c = change_at(&r, 5).unwrap();
        assert_eq!(c.before.entity.as_ref().unwrap().labels.len(), 2);
        assert_eq!(c.after.entity.as_ref().unwrap().labels.len(), 1);
        assert_eq!(c.previous.unwrap().offset(), 3);
        let first = change_at(&r, 1).unwrap();
        assert!(
            first.before.entity.is_none(),
            "nothing before the first record"
        );
        assert!(first.previous.is_none());
    }
}
