//! What every command needs: a connection, a pool-backed store, the pipeline, the clock.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use scatter_actors::issuer::IssuerRegistry;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::store::Draft;
use scatter_projection::Pipeline;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
use triplespace_db::pool::{tls, wants_tls};
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

/// The farm a database serves (0046 §6; 0079 §4): the instance `config`'s `farm` record,
/// which `instance create` writes and nothing changes.
pub async fn farm_of(client: &Client) -> Result<Farm> {
    triplespace_projections::tenants::farm(client)
        .await?
        .context("this database holds no instance (no `farm` record); run `instance create` first")
}

/// The farm of the database at `database`, over a connection of its own.
pub async fn farm_at(database: &str) -> Result<Farm> {
    farm_of(&connect(database).await?).await
}

/// A tenant's issuer code, by slug, over a connection of its own.
pub async fn issuer_at(database: &str, slug: &str) -> Result<String> {
    issuer_of(&connect(database).await?, slug).await
}

/// A tenant's issuer code, the code its actor keys carry (0079 §1), by slug.
pub async fn issuer_of(client: &Client, slug: &str) -> Result<String> {
    triplespace_projections::tenants::issuer_of(client, slug)
        .await?
        .with_context(|| format!("tenant `{slug}` is not registered"))
}

/// Microseconds since the epoch, now.
pub fn now() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_micros()),
    )
    .unwrap_or(u64::MAX)
}

/// A random 64-bit partition ID (0018 §2), never 0.
pub fn random_partition() -> Result<u64> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).context("random partition ID")?;
    Ok(u64::from_le_bytes(bytes).max(1))
}

/// A plain connection, over TLS when the URL says `sslmode=require` (0033 A24).
pub async fn connect(database: &str) -> Result<Client> {
    let config: Config = database.parse().context("database URL")?;
    if wants_tls(&config) {
        let (client, connection) = config
            .connect(tls().context("TLS")?)
            .await
            .context("connect")?;
        drive(connection);
        Ok(client)
    } else {
        let (client, connection) = config.connect(NoTls).await.context("connect")?;
        drive(connection);
        Ok(client)
    }
}

/// Runs a connection's I/O on its own task, reporting how it ended.
fn drive<F>(connection: F)
where
    F: Future<Output = Result<(), tokio_postgres::Error>> + Send + 'static,
{
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection: {e}");
        }
    });
}

/// The store and its pipeline.
pub fn store(database: &str, farm: &Farm) -> Result<(PgIngest, Pipeline<PgIngest>)> {
    let pool = triplespace_db::pool::pool(database, 8).context("pool")?;
    let store = PgIngest::new(PgBackend::new(pool));
    let pipeline = milestone_pipeline(IssuerRegistry::default_registry().clone(), farm.clone())
        .and_then(triplespace_notify::with_addressing)
        .context("pipeline")?;
    Ok((store, pipeline))
}

/// A draft with a JSON content, an optional comment and the attestation `attest` makes
/// from the content and comment values, for attestations that sign them (0040 §3).
pub fn draft_attested(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    comment: Option<&str>,
    now: u64,
    attest: impl FnOnce(&Value, &Value) -> Result<Value>,
) -> Result<Draft> {
    let content = Value::from_json(content);
    let comment = comment.map_or(Value::Null, Value::text);
    let attestation = attest(&content, &comment)?;
    Ok(Draft {
        appended_at: now,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(&content, &comment, &attestation)
            .map_err(|e| anyhow::anyhow!("body: {e}"))?,
    })
}

/// A draft with a JSON content, an optional comment and an actor's attestation.
pub fn draft(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    actor: &str,
    comment: Option<&str>,
    now: u64,
) -> Result<Draft> {
    Ok(Draft {
        appended_at: now,
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(content),
            &comment.map_or(Value::Null, Value::text),
            &Value::map(vec![(Value::text("actor"), Value::text(actor))]),
        )
        .map_err(|e| anyhow::anyhow!("body: {e}"))?,
    })
}

/// Reads or creates the instance signing key: 32 seed bytes in hex.
pub fn signing_key(path: &std::path::Path) -> Result<ed25519_dalek::SigningKey> {
    if path.exists() {
        let hex = std::fs::read_to_string(path).context("read key file")?;
        let bytes = decode_hex(hex.trim()).context("key file is 64 hex digits")?;
        return Ok(ed25519_dalek::SigningKey::from_bytes(&bytes));
    }
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).context("random key")?;
    let key = ed25519_dalek::SigningKey::from_bytes(&seed);
    let hex = scatter_log::hash::hex(&seed);
    std::fs::write(path, format!("{hex}\n")).context("write key file")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .context("key file mode")?;
    }
    Ok(key)
}

fn decode_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(chunk).ok()?, 16).ok()?;
    }
    Some(out)
}
