//! The database layer of a Triplespace instance (0013 §4–7; 0005 §2): the `view`,
//! `private` and `ops` schemas and their migrations, the runner that applies every
//! crate's migrations in build order and records them in `ops.migration`, the roles and
//! grants that enforce `private`'s privacy, connection pools, and replica routing by
//! WAL position.
//!
//! - [`migrate`]: [`Migrator`], which applies `scatter-log-postgres`'s migrations and then
//!   this crate's ([`MIGRATIONS`]), checksummed, idempotent, recorded;
//! - [`private`]: the portability class of every `private` table (0027 §2), checked
//!   against the live schema;
//! - [`grants`]: per-partition grants, including the accounts partition's exclusion;
//! - [`pool`]: `deadpool-postgres` pools from a URL;
//! - [`routing`]: read-your-writes across replicas by LSN (0013 §7);
//! - [`projection_state`]: `ops.projection_state`, how far each projection has replayed.
//!
//! Tests run against a real database named by `TRIPLESPACE_TEST_DATABASE_URL`.

pub mod grants;
pub mod migrate;
pub mod pool;
pub mod private;
pub mod projection_state;
pub mod routing;

pub use migrate::{Applied, MigrateError, Migrator};
pub use scatter_log_postgres::Migration;

/// This crate's migrations, in order, after `scatter-log-postgres`'s.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        name: "0001_ops",
        sql: include_str!("../migrations/0001_ops.sql"),
    },
    Migration {
        name: "0002_view",
        sql: include_str!("../migrations/0002_view.sql"),
    },
    Migration {
        name: "0003_private",
        sql: include_str!("../migrations/0003_private.sql"),
    },
    Migration {
        name: "0004_roles",
        sql: include_str!("../migrations/0004_roles.sql"),
    },
    Migration {
        name: "0005_resolved_compression",
        sql: include_str!("../migrations/0005_resolved_compression.sql"),
    },
    Migration {
        name: "0006_private_session",
        sql: include_str!("../migrations/0006_private_session.sql"),
    },
    Migration {
        name: "0007_private_forwarder",
        sql: include_str!("../migrations/0007_private_forwarder.sql"),
    },
    Migration {
        name: "0008_view_lookup_indexes",
        sql: include_str!("../migrations/0008_view_lookup_indexes.sql"),
    },
    Migration {
        name: "0009_keyed_map_aliases",
        sql: include_str!("../migrations/0009_keyed_map_aliases.sql"),
    },
    Migration {
        name: "0010_revision_state",
        sql: include_str!("../migrations/0010_revision_state.sql"),
    },
    Migration {
        name: "0011_settings_and_preferences",
        sql: include_str!("../migrations/0011_settings_and_preferences.sql"),
    },
    Migration {
        name: "0012_registration",
        sql: include_str!("../migrations/0012_registration.sql"),
    },
    Migration {
        name: "0013_external_identity",
        sql: include_str!("../migrations/0013_external_identity.sql"),
    },
    Migration {
        name: "0014_pages",
        sql: include_str!("../migrations/0014_pages.sql"),
    },
    Migration {
        name: "0015_threads",
        sql: include_str!("../migrations/0015_threads.sql"),
    },
    Migration {
        name: "0016_access_controls",
        sql: include_str!("../migrations/0016_access_controls.sql"),
    },
    Migration {
        name: "0017_set_members",
        sql: include_str!("../migrations/0017_set_members.sql"),
    },
    Migration {
        name: "0018_inbox",
        sql: include_str!("../migrations/0018_inbox.sql"),
    },
    Migration {
        name: "0019_purges",
        sql: include_str!("../migrations/0019_purges.sql"),
    },
    Migration {
        name: "0020_page_title_bytes",
        sql: include_str!("../migrations/0020_page_title_bytes.sql"),
    },
    Migration {
        name: "0021_watch",
        sql: include_str!("../migrations/0021_watch.sql"),
    },
    Migration {
        name: "0022_followers",
        sql: include_str!("../migrations/0022_followers.sql"),
    },
    Migration {
        name: "0023_reports",
        sql: include_str!("../migrations/0023_reports.sql"),
    },
    Migration {
        name: "0024_work_attempts",
        sql: include_str!("../migrations/0024_work_attempts.sql"),
    },
    Migration {
        name: "0025_page_random",
        sql: include_str!("../migrations/0025_page_random.sql"),
    },
];

/// The crate name migrations are recorded under.
pub const CRATE: &str = "triplespace-db";

/// A migrator loaded with every schema in 0005 §7's build order: the log, then this
/// crate. Crates that own later tables add theirs.
#[must_use]
pub fn migrator() -> Migrator {
    Migrator::new()
        .with("scatter-log-postgres", scatter_log_postgres::MIGRATIONS)
        .with(CRATE, MIGRATIONS)
}
