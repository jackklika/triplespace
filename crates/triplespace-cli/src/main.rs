//! The `triplespace` binary.

#![forbid(unsafe_code)]

// The process allocator is mimalloc (0033 §2, A24): glibc's `malloc` and `free` were a
// third of a Wikidata sync's instructions (docs/testing/performance.md §8). Built without the
// default `mimalloc` feature, the binary uses the system allocator, for heap tools.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use triplespace_cli::{
    accounts, adopt, challenge, env_file, follow, forwarder, instance, oauth_client, rights,
    settings, source, status, sync, tenant,
};

/// Triplespace: a Wikibase as an append-only log.
#[derive(Debug, Parser)]
#[command(name = "triplespace", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// The instance: create it; its forwarder keys.
    Instance {
        #[command(subcommand)]
        command: InstanceCommand,
    },
    /// Tenants: create one on an existing instance, list them (0018; 0079).
    Tenant {
        #[command(subcommand)]
        command: tenant::TenantCommand,
    },
    /// Adopt a frozen Wikibase from its XML dump into a tenant (0035).
    Adopt(adopt::Adopt),
    /// Sync a provider's dump into its mirror (0002 §8.4).
    Sync(sync::Sync),
    /// Follow a provider's wiki by its recent changes (0092 §1).
    Follow(follow::Follow),
    /// The bot password a provider's follower logs in with (0092 §10).
    UpstreamLogin {
        #[command(subcommand)]
        command: follow::UpstreamLogin,
    },
    /// A tenant's entity sources (0078 §1).
    Source {
        #[command(subcommand)]
        command: source::SourceCommand,
    },
    /// Passwords of primary accounts (0007 §3).
    Password {
        #[command(subcommand)]
        command: PasswordCommand,
    },
    /// Subsidiary accounts and their API keys (0024).
    Subsidiary {
        #[command(subcommand)]
        command: accounts::Subsidiary,
    },
    /// Registration and login challenges (0087 §9).
    Challenge {
        #[command(subcommand)]
        command: ChallengeCommand,
    },
    /// Client credentials for logging in through external issuers (0088 §3).
    OauthClient {
        #[command(subcommand)]
        command: oauth_client::OAuthClient,
    },
    /// Settings: a tenant's, the instance's, its defaults and locks (0085 §3).
    Settings {
        #[command(subcommand)]
        command: settings::Settings,
    },
    /// Memberships, as instance acts: appointing owners among them (09 §3.6).
    Member {
        #[command(subcommand)]
        command: rights::Member,
    },
    /// Group definitions (09 §3.2).
    Group {
        #[command(subcommand)]
        command: rights::GroupCommand,
    },
    /// ACLs on pages, namespaces, sets and the tenant (09 §4).
    Acl {
        #[command(subcommand)]
        command: rights::AclCommand,
    },
    /// Partitions, heads and projection lags, and any migration not yet applied.
    Status(status::Target),
    /// Apply the migrations this binary carries that the database has not applied.
    Migrate(status::Database),
    /// Replay every projection from the log.
    Rebuild(status::Target),
}

#[derive(Debug, Subcommand)]
enum PasswordCommand {
    /// Set or replace an account's password from a file or `TRIPLESPACE_PASSWORD`.
    Set(accounts::PasswordSet),
}

#[derive(Debug, Subcommand)]
enum ChallengeCommand {
    /// Print a solution of a challenge.
    Solve(challenge::Solve),
}

#[derive(Debug, Subcommand)]
#[allow(clippy::large_enum_variant)] // parsed once from the command line
enum InstanceCommand {
    /// Create the instance and its primary tenant.
    Create(instance::Create),
    /// Forwarder keys, which let a proxy vouch for the client address (0057 §10).
    Forwarder {
        #[command(subcommand)]
        command: forwarder::Forwarder,
    },
}

fn main() -> Result<()> {
    // Before the runtime starts: the `.env` file is read into the environment (23 §5.1).
    env_file::load()?;
    run()
}

#[tokio::main]
async fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Instance {
            command: InstanceCommand::Create(args),
        } => instance::run(args).await,
        Command::Instance {
            command: InstanceCommand::Forwarder { command },
        } => forwarder::run(command).await,
        Command::Tenant { command } => tenant::run(command).await,
        Command::Adopt(args) => adopt::run(args).await,
        Command::Sync(args) => sync::run(args).await,
        Command::Follow(args) => follow::run(args).await,
        Command::UpstreamLogin { command } => follow::run_login(command).await,
        Command::Source { command } => source::run(command).await,
        Command::Password {
            command: PasswordCommand::Set(args),
        } => accounts::run_password_set(args).await,
        Command::Subsidiary { command } => accounts::run_subsidiary(command).await,
        Command::Challenge {
            command: ChallengeCommand::Solve(args),
        } => challenge::run(&args),
        Command::OauthClient { command } => oauth_client::run(command).await,
        Command::Settings { command } => settings::run(command).await,
        Command::Member { command } => rights::run_member(command).await,
        Command::Group { command } => rights::run_group(command).await,
        Command::Acl { command } => rights::run_acl(command).await,
        Command::Status(args) => status::status(args).await,
        Command::Migrate(args) => status::migrate(args).await,
        Command::Rebuild(args) => status::rebuild(args).await,
    }
}
