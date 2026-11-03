//! `triplespace status`, `triplespace migrate` and `triplespace rebuild`: the partitions,
//! their heads and every projection's lag, with the migrations the database has not
//! applied; the migrations applied; and a replay of the projections from offset 0 in
//! dependency order (0013 §7, A28).

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use clap::Args;
use scatter_log_postgres::PgLog;

use crate::common::{connect, farm_of, store};

/// `status` / `rebuild`.
#[derive(Debug, Args)]
pub struct Target {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// A partition as `log.partition` describes it.
struct Partition {
    id: u64,
    tenant: Option<String>,
    name: String,
    size: u64,
}

async fn partitions(client: &tokio_postgres::Client) -> Result<Vec<Partition>> {
    let rows = client
        .query(
            "SELECT partition, tenant, name, next_offset FROM log.partition ORDER BY tenant NULLS FIRST, name",
            &[],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| Partition {
            id: scatter_log_postgres::ids::partition_from_db(r.get(0)),
            tenant: r.get(1),
            name: r.get(2),
            size: u64::try_from(r.get::<_, i64>(3)).unwrap_or(0),
        })
        .collect())
}

/// The replay order of a rebuild (03 §6.1): the instance `config`, the instance `log`,
/// mirrors and the instance's other partitions, each on its own; then each tenant's
/// partitions as one group, replayed interleaved in `(appended_at, partition, offset)`
/// order, so that a record changing a row another of the tenant's partitions made (a
/// deletion in its `log` of a page in its `pages`) meets that row as it did when written.
fn grouped(partitions: &[Partition]) -> Vec<Vec<u64>> {
    let rank = |name: &str| match name {
        "config" => 0,
        "log" => 1,
        n if n.starts_with("mirror/") => 2,
        _ => 3,
    };
    let mut instance: Vec<&Partition> = partitions.iter().filter(|p| p.tenant.is_none()).collect();
    instance.sort_by_key(|p| (rank(&p.name), p.id));
    let mut groups: Vec<Vec<u64>> = instance.into_iter().map(|p| vec![p.id]).collect();
    let mut tenants: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for p in partitions {
        if let Some(t) = &p.tenant {
            tenants.entry(t.as_str()).or_default().push(p.id);
        }
    }
    for (_, mut ids) in tenants {
        ids.sort_unstable();
        groups.push(ids);
    }
    groups
}

/// A database URL alone.
#[derive(Debug, Args)]
pub struct Database {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    pub database: String,
}

/// Applies every migration the database has not applied, in build order (0033 §4), and
/// prints each; a database an older binary created gains the tables a newer one reads.
pub async fn migrate(args: Database) -> Result<()> {
    let mut client = connect(&args.database).await?;
    let applied = triplespace_db::migrator()
        .apply(&mut client)
        .await
        .context("migrate")?;
    for (c, name) in &applied.applied {
        println!("applied {c}/{name}");
    }
    println!(
        "{} applied, {} already recorded",
        applied.applied.len(),
        applied.skipped.len()
    );
    if applied
        .applied
        .iter()
        .any(|(_, name)| name.starts_with("0010_"))
    {
        println!(
            "note: view.graph_state and view.entity_revision are filled by writes from now on; \
             `triplespace rebuild` fills them for earlier revisions (0083 A2)"
        );
    }
    Ok(())
}

/// Prints every partition with its head and each projection's lag, and the migrations the
/// database has not applied.
pub async fn status(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let pending = triplespace_db::migrator()
        .pending(&client)
        .await
        .context("migrations")?;
    if !pending.is_empty() {
        println!(
            "pending migrations (run `triplespace migrate`): {}",
            pending
                .iter()
                .map(|(c, n)| format!("{c}/{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let parts = partitions(&client).await?;
    let log_store = PgLog::new(client);
    println!(
        "{:<18} {:<14} {:<24} {:>10}  lags",
        "partition", "tenant", "graph", "head"
    );
    for p in &parts {
        let lags = pipeline.lags(&store, &log_store, p.id).await?;
        let behind: Vec<String> = lags
            .iter()
            .filter(|l| l.behind() > 0)
            .map(|l| format!("{} -{}", l.projection, l.behind()))
            .collect();
        println!(
            "{:#018x} {:<14} {:<24} {:>10}  {}",
            p.id,
            p.tenant.as_deref().unwrap_or("(instance)"),
            p.name,
            p.size,
            if behind.is_empty() {
                "current".to_string()
            } else {
                behind.join(", ")
            }
        );
    }
    Ok(())
}

/// Replays every projection from offset 0, in replay order (03 §6.1).
pub async fn rebuild(args: Target) -> Result<()> {
    let client = connect(&args.database).await?;
    let farm = farm_of(&client).await?;
    let (store, pipeline) = store(&args.database, &farm)?;
    let parts = partitions(&client).await?;
    let groups = grouped(&parts);
    let log_store = PgLog::new(client);
    let progress = pipeline
        .rebuild_grouped(&store, &log_store, &groups, 500)
        .await
        .context("rebuild")?;
    for (id, p) in progress {
        println!("{id:#018x}: {} records, {} rows", p.records, p.rows);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Partition, grouped};

    fn part(id: u64, tenant: Option<&str>, name: &str) -> Partition {
        Partition {
            id,
            tenant: tenant.map(str::to_string),
            name: name.into(),
            size: 0,
        }
    }

    // 03 §6.1: instance partitions first, in dependency order, then one group per tenant.
    #[test]
    fn a_tenant_replays_as_one_group_after_the_instance() {
        let parts = vec![
            part(9, Some("lb"), "pages"),
            part(5, None, "mirror/wikidata"),
            part(7, Some("lb"), "log"),
            part(2, None, "log"),
            part(1, None, "config"),
            part(8, Some("lb"), "config"),
            part(3, Some("cu"), "local"),
        ];
        assert_eq!(
            grouped(&parts),
            vec![vec![1], vec![2], vec![5], vec![3], vec![7, 8, 9]]
        );
    }
}
