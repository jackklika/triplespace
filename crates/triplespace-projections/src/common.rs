//! What every projection needs: the tenant and name of a partition, the record's parts
//! decoded, the operation a change-set record carries, and the header key a subject's
//! records are filed under.

use std::future::Future;

use scatter_log::cbor::{self, Value};
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_log_postgres::ids::partition_to_db;
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_model::id::{EntityId, IdForm};

/// The change-set payload type.
pub const PAYLOAD_CHANGESET: &str = "scatter:v0/changeset";
/// The log-event payload type.
pub const PAYLOAD_LOGEVENT: &str = "scatter:v0/logevent";
/// A reclaimed account (0088 §8; 0018 A17), projected as `reclaim/reclaim`.
pub const PAYLOAD_RECLAIM: &str = "scatter:v0/reclaim";
/// The keyed-surrogate mapping payload type (0009 §7).
pub const PAYLOAD_KEYED_SURROGATE: &str = "scatter:v0/keyed-surrogate";

/// `view.activity.target_kind` codes.
pub mod target_kind {
    /// An entity, by ID.
    pub const ENTITY: i16 = 1;
    /// A page, by page ID.
    pub const PAGE: i16 = 2;
    /// An actor, by key.
    pub const ACTOR: i16 = 3;
    /// A job, by ID.
    pub const JOB: i16 = 4;
    /// A record, as `{partition}:{offset}`.
    pub const RECORD: i16 = 5;
    /// A title, as `{ns}:{title}` with the namespace by number: a `view.page_link` row's
    /// target page, existing or not, so that a link follows the title as MediaWiki's
    /// `pagelinks` does.
    pub const TITLE: i16 = 6;
}

/// A partition's tenant and graph name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionInfo {
    /// The tenant slug, `""` for an instance partition.
    pub tenant: String,
    /// The graph name: `local`, `mirror/wikidata`, `pages`, `log`, …
    pub name: String,
}

impl PartitionInfo {
    /// Whether the partition holds mirrored entities: a provider's mirror, or a tenant's
    /// entity source (0078 §4).
    #[must_use]
    pub fn is_mirror(&self) -> bool {
        self.name.starts_with("mirror/") || self.name.starts_with("source/")
    }

    /// Whether the partition is a tenant's entity source.
    #[must_use]
    pub fn is_source(&self) -> bool {
        self.name.starts_with("source/")
    }

    /// The provider slug of a mirror partition.
    #[must_use]
    pub fn provider_slug(&self) -> Option<&str> {
        self.name
            .strip_prefix("mirror/")
            .or_else(|| self.name.strip_prefix("source/"))
    }
}

/// A connection that answers for `log.partition`: which tenant and graph a partition is,
/// and which partition a tenant's graph is. Rows there are fixed at creation, so a unit
/// of work ([`crate::PgCx`]) answers from its cache after the first lookup; a plain
/// client asks the table each time.
pub trait Partitions: PgClient {
    /// The tenant and name of a partition.
    fn partition_info(
        &self,
        partition: u64,
    ) -> impl Future<Output = Result<PartitionInfo, String>> + Send;

    /// The partition of a graph: the tenant's own for `local` and `pages`, the
    /// instance's (`""`) for a mirror (0018 §2). `None` when no such partition exists.
    fn partition_of(
        &self,
        tenant: &str,
        name: &str,
    ) -> impl Future<Output = Result<Option<u64>, String>> + Send;
}

/// `log.partition`'s row for a partition, asked of the table.
async fn lookup_info<C: PgClient>(client: &C, partition: u64) -> Result<PartitionInfo, String> {
    let row = client
        .query_opt(
            "SELECT tenant, name FROM log.partition WHERE partition = $1",
            &[&partition_to_db(partition)],
        )
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("partition {partition} is not registered"))?;
    Ok(PartitionInfo {
        tenant: row.get::<_, Option<String>>(0).unwrap_or_default(),
        name: row.get(1),
    })
}

/// The partition of a tenant's graph, asked of the table.
async fn lookup_partition<C: PgClient>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<Option<u64>, String> {
    let tenant = (!tenant.is_empty()).then_some(tenant);
    let row = client
        .query_opt(
            "SELECT partition FROM log.partition WHERE tenant IS NOT DISTINCT FROM $1 AND name = $2",
            &[&tenant, &name],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| scatter_log_postgres::ids::partition_from_db(r.get(0))))
}

/// A plain client, and a pooled connection outside a unit of work, ask the table.
macro_rules! uncached_partitions {
    ($($t:ty),*) => {$(
        impl Partitions for $t {
            async fn partition_info(&self, partition: u64) -> Result<PartitionInfo, String> {
                lookup_info(self, partition).await
            }

            async fn partition_of(&self, tenant: &str, name: &str) -> Result<Option<u64>, String> {
                lookup_partition(self, tenant, name).await
            }
        }
    )*};
}
uncached_partitions!(
    tokio_postgres::Client,
    tokio_postgres::Transaction<'_>,
    deadpool_postgres::ClientWrapper,
    deadpool_postgres::Object
);

/// The tenant and name of a partition.
pub async fn partition_info<C: Partitions>(
    client: &C,
    partition: u64,
) -> Result<PartitionInfo, String> {
    client.partition_info(partition).await
}

/// The partition of a graph (see [`Partitions::partition_of`]).
pub async fn partition_of<C: Partitions>(
    client: &C,
    tenant: &str,
    name: &str,
) -> Result<Option<u64>, String> {
    client.partition_of(tenant, name).await
}

/// The record's comment part as text; `None` when erased, `null` or not text.
pub fn comment(record: &Record) -> Option<String> {
    record
        .body()
        .comment()
        .value()
        .ok()
        .flatten()
        .and_then(|v| v.as_text().map(str::to_owned))
}

/// The record's attestation part; `None` when erased.
pub fn attestation(record: &Record) -> Option<Value> {
    record.body().attestation().value().ok().flatten()
}

/// The attestation's `actor`, `job` and `tags`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Attested {
    /// The actor key, absent when erased or hidden.
    pub actor: Option<String>,
    /// The job ID, for a record a job wrote.
    pub job: Option<u64>,
    /// Change tags.
    pub tags: Vec<String>,
}

/// Reads the attestation's common fields.
#[must_use]
pub fn attested(record: &Record) -> Attested {
    let Some(a) = attestation(record) else {
        return Attested::default();
    };
    Attested {
        actor: a.get("actor").and_then(Value::as_text).map(str::to_owned),
        job: a.get("job").and_then(Value::as_u64),
        tags: match a.get("tags") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_text)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        },
    }
}

/// The operation a change-set record carries; `None` when its content is erased.
pub fn operation(record: &Record) -> Result<Option<Operation>, String> {
    let Some(value) = content(record)? else {
        return Ok(None);
    };
    cbor::from_value(value)
        .map(Some)
        .map_err(|e| format!("not a change-set operation: {e}"))
}

/// The header key a subject's records are filed under: the entity ID itself, or for a
/// keyed entity its surrogate as `{type}#{n}` (0009 §7), looked up in
/// `view.keyed_surrogate`. `None` for a keyed entity that has no surrogate yet. An
/// entity source's ID is in the colon shape too (0078 §2) but its prefix is no keyed
/// type (0078 §3 rule 1 keeps the names apart), and its key is the ID itself.
pub async fn header_key_for<C: PgClient>(
    client: &C,
    id: &EntityId,
) -> Result<Option<String>, String> {
    if id.form() != IdForm::Keyed {
        return Ok(Some(id.as_str().to_string()));
    }
    let (keyed_type, key) = id.keyed_parts().ok_or("a keyed ID has a type and a key")?;
    if scatter_normalize::KeyedRegistry::default_registry()
        .by_name(keyed_type)
        .is_none()
    {
        return Ok(Some(id.as_str().to_string()));
    }
    let row = client
        .query_opt(
            "SELECT surrogate FROM view.keyed_surrogate WHERE tenant = '' AND keyed_type = $1 AND key = $2",
            &[&keyed_type, &key],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| surrogate_key(keyed_type, r.get::<_, i64>(0))))
}

/// A Postgres error as a projection reports it: the driver's one-word summary ("db
/// error") with the server's message, code and detail, so that a refused write says
/// which constraint refused it.
#[must_use]
pub fn describe(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(d) => {
            let mut s = format!("{e}: {} [{}]", d.message(), d.code().code());
            if let Some(detail) = d.detail() {
                s.push_str(": ");
                s.push_str(detail);
            }
            s
        }
        None => e.to_string(),
    }
}

/// The header key of a surrogate, `domain#17`.
#[must_use]
pub fn surrogate_key(keyed_type: &str, surrogate: i64) -> String {
    format!("{keyed_type}#{surrogate}")
}

/// Splits a surrogate header key into its type and number.
#[must_use]
pub fn parse_surrogate_key(key: &str) -> Option<(&str, i64)> {
    let (t, n) = key.split_once('#')?;
    Some((t, n.parse().ok()?))
}

/// A page's `random` (`page_random`, 03 §4.5), as SQL over the statement's `$1` (the
/// tenant) and `$2` (the page ID): the first 52 bits of the MD5 of `{tenant}:{page_id}`,
/// as a fraction in [0, 1). It is a function of the page, not a draw, so that a rebuild
/// projects the same value the write path did (0047 A29). Migration
/// `0025_page_random.sql` computes the same over existing rows.
#[macro_export]
macro_rules! page_random_sql {
    () => {
        "(('x' || substr(md5($1::text || ':' || $2::bigint::text), 1, 13))::bit(52)::bigint)::double precision / 4503599627370496.0"
    };
}

/// Quotes a string for use inside SQL text.
#[must_use]
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// The tenant of a partition, `""` for an instance partition (0018 §2).
pub async fn tenant_of<C: Partitions>(client: &C, partition: u64) -> Result<String, String> {
    Ok(client.partition_info(partition).await?.tenant)
}

/// The layer a membership or block record is in: `tenant` for a tenant's own partition,
/// `instance` for an instance partition (0040 §5).
#[must_use]
pub fn layer_of(tenant: &str) -> &'static str {
    if tenant.is_empty() {
        "instance"
    } else {
        "tenant"
    }
}

/// A record's content part, decoded; `None` when erased or `null`.
pub fn content(record: &Record) -> Result<Option<Value>, String> {
    match record.body().content().value().map_err(|e| e.to_string())? {
        None | Some(Value::Null) => Ok(None),
        Some(v) => Ok(Some(v)),
    }
}

/// CBOR to JSON for a `jsonb` column: byte strings, which JSON lacks, become lower-case
/// hex text, so a `key:` record's public key is still inspectable.
#[must_use]
pub fn to_jsonb(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => i64::try_from(*i)
            .map(serde_json::Value::from)
            .or_else(|_| u64::try_from(*i).map(serde_json::Value::from))
            .unwrap_or_else(|_| serde_json::Value::String(i.to_string())),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Bytes(b) => serde_json::Value::String(scatter_log::hash::hex(b)),
        Value::Text(t) => serde_json::Value::String(t.clone()),
        Value::Array(a) => serde_json::Value::Array(a.iter().map(to_jsonb).collect()),
        Value::Map(m) => serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| {
                    let key = match k {
                        Value::Text(t) => t.clone(),
                        other => other.to_string(),
                    };
                    (key, to_jsonb(v))
                })
                .collect(),
        ),
    }
}

/// A header key split at its first colon: `kind:code`, `acl:page:7` → `("acl", "page:7")`.
#[must_use]
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

/// The time a header's µs-since-epoch becomes, for `timestamptz` columns.
#[must_use]
pub fn time_of(micros: u64) -> std::time::SystemTime {
    std::time::UNIX_EPOCH + std::time::Duration::from_micros(micros)
}

/// The offset as `bigint`.
pub fn offset_db(record: &Record) -> Result<i64, String> {
    scatter_log_postgres::ids::to_db(record.header().offset).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonb_conversion() {
        let v = Value::map(vec![
            (Value::text("k"), Value::Bytes(vec![0xab, 0xcd])),
            (Value::text("n"), Value::Int(5)),
            (Value::text("big"), Value::Int(i128::from(u64::MAX))),
            (
                Value::text("a"),
                Value::Array(vec![Value::Bool(true), Value::Null]),
            ),
        ]);
        let j = to_jsonb(&v);
        assert_eq!(j["k"], "abcd");
        assert_eq!(j["n"], 5);
        assert_eq!(j["big"], u64::MAX);
        assert_eq!(j["a"][0], true);
        assert_eq!(split_key("acl:page:7"), Some(("acl", "page:7")));
        assert_eq!(layer_of(""), "instance");
        assert_eq!(layer_of("librarybase"), "tenant");
        assert_eq!(surrogate_key("domain", 17), "domain#17");
        assert_eq!(parse_surrogate_key("domain#17"), Some(("domain", 17)));
        assert_eq!(parse_surrogate_key("Q17"), None);
        assert_eq!(quote("it's"), "'it''s'");
    }
}
