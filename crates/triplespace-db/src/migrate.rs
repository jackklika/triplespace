//! The migration runner (0005 rule 9; 0033 §4): each crate embeds its SQL files, this
//! applies them in build order and records each in `ops.migration` with a checksum. A
//! migration is applied once; a recorded migration whose SQL has changed is an error,
//! since a change to a schema is a new migration, never an edit.

use sha2::{Digest as _, Sha256};
use tokio_postgres::GenericClient;

use scatter_log_postgres::Migration;

/// Why migrating stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrateError {
    /// The database refused.
    #[error("{crate_name}/{name}: {source}")]
    Sql {
        /// The crate.
        crate_name: String,
        /// The migration.
        name: String,
        /// What Postgres said.
        #[source]
        source: tokio_postgres::Error,
    },
    /// The bookkeeping table could not be reached.
    #[error("ops.migration: {0}")]
    Bookkeeping(#[from] tokio_postgres::Error),
    /// A recorded migration's SQL has changed since it was applied.
    #[error(
        "{crate_name}/{name} was applied with checksum {recorded}, but its SQL now hashes to {current}"
    )]
    Changed {
        /// The crate.
        crate_name: String,
        /// The migration.
        name: String,
        /// The checksum in `ops.migration`.
        recorded: String,
        /// The checksum of the embedded SQL.
        current: String,
    },
}

/// What a run did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// `(crate, name)` of each migration applied this run.
    pub applied: Vec<(String, String)>,
    /// `(crate, name)` of each already recorded.
    pub skipped: Vec<(String, String)>,
}

/// The status of one migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    /// The crate.
    pub crate_name: String,
    /// The migration.
    pub name: String,
    /// The checksum of the embedded SQL.
    pub checksum: String,
    /// Whether and when it was applied.
    pub applied_at: Option<std::time::SystemTime>,
}

/// Migrations from several crates, in order.
#[derive(Debug, Clone, Default)]
pub struct Migrator {
    sets: Vec<(&'static str, &'static [Migration])>,
}

/// SHA-256 of the SQL, lower-case hex.
#[must_use]
pub fn checksum(sql: &str) -> String {
    use std::fmt::Write as _;
    Sha256::digest(sql.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

const BOOKKEEPING: &str = "
CREATE SCHEMA IF NOT EXISTS ops;
CREATE TABLE IF NOT EXISTS ops.migration (
  crate_name text NOT NULL,
  name       text NOT NULL,
  checksum   text NOT NULL,
  applied_at timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (crate_name, name)
);";

impl Migrator {
    /// No migrations yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a crate's migrations after the ones so far.
    #[must_use]
    pub fn with(mut self, crate_name: &'static str, migrations: &'static [Migration]) -> Self {
        self.sets.push((crate_name, migrations));
        self
    }

    /// Every migration in order.
    pub fn migrations(&self) -> impl Iterator<Item = (&'static str, &'static Migration)> + '_ {
        self.sets
            .iter()
            .flat_map(|(c, ms)| ms.iter().map(move |m| (*c, m)))
    }

    /// Creates the bookkeeping table if needed.
    pub async fn prepare<C: GenericClient>(&self, client: &C) -> Result<(), MigrateError> {
        client.batch_execute(BOOKKEEPING).await?;
        Ok(())
    }

    /// Applies every migration not yet recorded, in order, each in its own transaction
    /// with its bookkeeping row; stops at the first failure.
    pub async fn apply(
        &self,
        client: &mut tokio_postgres::Client,
    ) -> Result<Applied, MigrateError> {
        self.prepare(client).await?;
        let mut report = Applied::default();
        for (crate_name, m) in self.migrations() {
            let current = checksum(m.sql);
            let recorded = client
                .query_opt(
                    "SELECT checksum FROM ops.migration WHERE crate_name = $1 AND name = $2",
                    &[&crate_name, &m.name],
                )
                .await?;
            if let Some(row) = recorded {
                let recorded: String = row.get(0);
                if recorded != current {
                    return Err(MigrateError::Changed {
                        crate_name: crate_name.into(),
                        name: m.name.into(),
                        recorded,
                        current,
                    });
                }
                report.skipped.push((crate_name.into(), m.name.into()));
                continue;
            }
            let tx = client.transaction().await?;
            tx.batch_execute(m.sql)
                .await
                .map_err(|source| MigrateError::Sql {
                    crate_name: crate_name.into(),
                    name: m.name.into(),
                    source,
                })?;
            tx.execute(
                "INSERT INTO ops.migration (crate_name, name, checksum) VALUES ($1, $2, $3)",
                &[&crate_name, &m.name, &current],
            )
            .await?;
            tx.commit().await?;
            report.applied.push((crate_name.into(), m.name.into()));
        }
        Ok(report)
    }

    /// The `(crate, name)` of every migration the database has not applied, in order:
    /// what `triplespace migrate` would apply, and what a server refuses to start over.
    pub async fn pending<C: GenericClient>(
        &self,
        client: &C,
    ) -> Result<Vec<(String, String)>, MigrateError> {
        Ok(self
            .status(client)
            .await?
            .into_iter()
            .filter(|s| s.applied_at.is_none())
            .map(|s| (s.crate_name, s.name))
            .collect())
    }

    /// The status of every migration against the database.
    pub async fn status<C: GenericClient>(&self, client: &C) -> Result<Vec<Status>, MigrateError> {
        self.prepare(client).await?;
        let rows = client
            .query(
                "SELECT crate_name, name, applied_at FROM ops.migration",
                &[],
            )
            .await?;
        let mut out = Vec::new();
        for (crate_name, m) in self.migrations() {
            let applied_at = rows
                .iter()
                .find(|r| r.get::<_, String>(0) == crate_name && r.get::<_, String>(1) == m.name)
                .map(|r| r.get::<_, std::time::SystemTime>(2));
            out.push(Status {
                crate_name: crate_name.into(),
                name: m.name.into(),
                checksum: checksum(m.sql),
                applied_at,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_checksums() {
        let m = crate::migrator();
        let names: Vec<_> = m
            .migrations()
            .map(|(c, m)| format!("{c}/{}", m.name))
            .collect();
        assert_eq!(names[0], "scatter-log-postgres/0001_log");
        assert_eq!(names[1], "triplespace-db/0001_ops");
        assert!(names.last().unwrap().ends_with("0025_page_random"));
        assert_eq!(checksum("").len(), 64);
        assert_ne!(checksum("a"), checksum("b"));
    }
}
