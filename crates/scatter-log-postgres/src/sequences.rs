//! The ID sequences of 0013 §6: per tenant, `revision_id`, `log_id`, `page_id`, `user_id`
//! (0007 §3), `actor_surrogate` (0007 A27) and one per minted entity type (`item_id`,
//! `property_id`, …); for the
//! instance, under the reserved tenant name `instance`, `job_id` (0011 §6.3), `log_id`
//! and `page_id` for the instance partitions, and one surrogate counter per keyed type
//! (`domain_surrogate`, 0009 §7). Each is a Postgres sequence named
//! `log."{tenant}.{kind}"`. They are taken in the appending transaction, like Wikibase's
//! `wb_id_counters`; an adoption sets a tenant's floors past what the source consumed
//! (0035 §4). The empty tenant `""` names the instance.

use crate::client::PgClient;
use scatter_log::store::StoreError;

use crate::ids::{from_db, to_db};
use crate::storage_error;

/// Which sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sequence {
    /// Header field 7 of a `local` or `pages` record.
    Revision,
    /// Header field 8 of a record that projects as a log event.
    Log,
    /// Header field 9: the first time a key is written.
    Page,
    /// A minted entity type's counter, by the type name (`item`, `property`).
    Entity(String),
    /// Local user IDs (0007 §3).
    User,
    /// The tenant's surrogates, `actor:{n}` (0007 §5, A27).
    ActorSurrogate,
    /// Job IDs (0011 §6.3); instance only.
    Job,
    /// A keyed type's surrogate counter (0009 §7); instance only.
    Surrogate(String),
}

/// The tenant name the instance's sequences live under. No tenant may take this slug.
pub const INSTANCE: &str = "instance";

impl Sequence {
    fn kind(&self) -> String {
        match self {
            Self::Revision => "revision_id".into(),
            Self::Log => "log_id".into(),
            Self::Page => "page_id".into(),
            Self::Entity(t) => format!("{t}_id"),
            Self::User => "user_id".into(),
            Self::ActorSurrogate => "actor_surrogate".into(),
            Self::Job => "job_id".into(),
            Self::Surrogate(t) => format!("{t}_surrogate"),
        }
    }
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The quoted sequence name for a tenant and a kind; `""` is the instance.
fn sequence_name(tenant: &str, seq: &Sequence) -> Result<String, StoreError> {
    let tenant = if tenant.is_empty() { INSTANCE } else { tenant };
    let kind = seq.kind();
    if !is_slug(tenant) || !is_slug(&kind) {
        return Err(StoreError::Storage(format!(
            "`{tenant}` / `{kind}` is not a sequence name"
        )));
    }
    Ok(format!("log.\"{tenant}.{kind}\""))
}

/// Creates the sequences for a tenant, if they do not exist. Entity types are given
/// by name.
pub async fn create<C: PgClient>(
    client: &C,
    tenant: &str,
    entity_types: &[&str],
) -> Result<(), StoreError> {
    if tenant.is_empty() {
        return Err(StoreError::Storage(
            "the instance's sequences are made by `create_instance`".into(),
        ));
    }
    let mut seqs = vec![
        Sequence::Revision,
        Sequence::Log,
        Sequence::Page,
        Sequence::User,
        Sequence::ActorSurrogate,
    ];
    seqs.extend(
        entity_types
            .iter()
            .map(|t| Sequence::Entity((*t).to_string())),
    );
    create_all(client, tenant, &seqs).await
}

/// Creates the instance's sequences, if they do not exist: job, log and page IDs, and a
/// surrogate counter per keyed type, given by name.
pub async fn create_instance<C: PgClient>(
    client: &C,
    keyed_types: &[&str],
) -> Result<(), StoreError> {
    let mut seqs = vec![Sequence::Job, Sequence::Log, Sequence::Page];
    seqs.extend(
        keyed_types
            .iter()
            .map(|t| Sequence::Surrogate((*t).to_string())),
    );
    create_all(client, INSTANCE, &seqs).await
}

async fn create_all<C: PgClient>(
    client: &C,
    tenant: &str,
    seqs: &[Sequence],
) -> Result<(), StoreError> {
    let mut sql = String::new();
    for s in seqs {
        use std::fmt::Write as _;
        let _ = writeln!(
            sql,
            "CREATE SEQUENCE IF NOT EXISTS {} AS bigint START 1;",
            sequence_name(tenant, s)?
        );
    }
    client
        .batch_execute(&sql)
        .await
        .map_err(|e| storage_error(&e))
}

/// Creates one sequence of a tenant if it does not exist: for a sequence added after the
/// tenant was created, such as `actor_surrogate` (0007 A27).
pub async fn ensure<C: PgClient>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
) -> Result<(), StoreError> {
    create_all(client, tenant, std::slice::from_ref(seq)).await
}

/// The next value of a sequence.
pub async fn next<C: PgClient>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
) -> Result<u64, StoreError> {
    let row = client
        .query_one(
            &format!("SELECT nextval('{}')", sequence_name(tenant, seq)?),
            &[],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    from_db(row.get(0))
}

/// The next `n` values of a sequence, in order, in one statement: a bootstrap load's
/// block of page IDs (0013 §9). Concurrent callers may interleave, so the values are
/// increasing but not necessarily contiguous.
pub async fn next_n<C: PgClient>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
    n: usize,
) -> Result<Vec<u64>, StoreError> {
    let rows = client
        .query(
            &format!(
                "SELECT nextval('{}') FROM generate_series(1, $1::bigint)",
                sequence_name(tenant, seq)?
            ),
            &[&i64::try_from(n).map_err(|_| StoreError::Corrupt("block size".into()))?],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    rows.iter().map(|r| from_db(r.get(0))).collect()
}

/// Raises the sequence so that its next value is above `consumed`, never lowering it:
/// an adoption's floor (0035 §4).
pub async fn floor<C: PgClient>(
    client: &C,
    tenant: &str,
    seq: &Sequence,
    consumed: u64,
) -> Result<(), StoreError> {
    let name = sequence_name(tenant, seq)?;
    client
        .execute(
            &format!(
                "SELECT setval('{name}', GREATEST($1::bigint, (SELECT last_value FROM {name})), true)"
            ),
            &[&to_db(consumed)?],
        )
        .await
        .map_err(|e| storage_error(&e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert_eq!(
            sequence_name("librarybase", &Sequence::Revision).unwrap(),
            "log.\"librarybase.revision_id\""
        );
        assert_eq!(
            sequence_name("librarybase", &Sequence::Entity("item".into())).unwrap(),
            "log.\"librarybase.item_id\""
        );
        assert_eq!(
            sequence_name("", &Sequence::Surrogate("domain".into())).unwrap(),
            "log.\"instance.domain_surrogate\""
        );
        assert_eq!(
            sequence_name("", &Sequence::Job).unwrap(),
            "log.\"instance.job_id\""
        );
        assert!(sequence_name("Bad Slug", &Sequence::Log).is_err());
        assert!(sequence_name("t", &Sequence::Entity("x\"; DROP".into())).is_err());
    }
}
