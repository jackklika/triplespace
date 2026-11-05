//! `triplespace-server`: the Action API and the `triplespace/v0` REST API over axum
//! (0033 §3; 0056 §10), and the site embedded unless `--ui off` (0057 §1.4).

#![forbid(unsafe_code)]

// The process allocator is mimalloc (0033 §2, A24): glibc's `malloc` and `free` were a
// third of a Wikidata sync's instructions (docs/testing/performance.md §8). Built without the
// default `mimalloc` feature, the binary uses the system allocator, for heap tools.
#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use triplespace_api_action::{App, Config, Mode, router_with};
use triplespace_client::{Client, ServiceTransport};

/// The Triplespace server.
#[derive(Debug, Parser)]
#[command(name = "triplespace-server", version, about)]
struct Args {
    /// The Postgres URL.
    #[arg(long, env = "TRIPLESPACE_DATABASE_URL")]
    database: String,
    /// Where to listen.
    #[arg(long, env = "TRIPLESPACE_LISTEN", default_value = "127.0.0.1:8080")]
    listen: String,
    /// A separate listener for health and debug routes (`server.admin_listen`).
    #[arg(long, env = "TRIPLESPACE_ADMIN_LISTEN")]
    admin_listen: Option<String>,
    /// `production` or `development` (`server.mode`).
    #[arg(long, env = "TRIPLESPACE_MODE", default_value = "production")]
    mode: String,
    /// Proxies whose X-Forwarded-* headers are trusted by address: addresses and CIDR
    /// ranges (`server.trusted_proxies`, 0057 §10). Forwarder keys need no setting here.
    #[arg(
        long = "trusted-proxy",
        env = "TRIPLESPACE_TRUSTED_PROXIES",
        value_delimiter = ','
    )]
    trusted_proxies: Vec<String>,
    /// In development mode, the tenant an unregistered host (localhost) is served as.
    #[arg(long, env = "TRIPLESPACE_DEV_TENANT")]
    dev_tenant: Option<String>,
    /// The instance key file; sessions' tokens derive from it.
    #[arg(long, env = "TRIPLESPACE_KEY_FILE", default_value = "triplespace.key")]
    key_file: PathBuf,
    /// Connections in the pool.
    #[arg(long, default_value_t = 16)]
    pool: usize,
    /// Worker tasks over the work queue (03 §6.1): fan-out past the synchronous budget
    /// and the tier-3 consumers (reports). `0` runs none, for a process that only serves
    /// while another runs the workers.
    #[arg(long, env = "TRIPLESPACE_WORKERS", default_value_t = 2)]
    workers: usize,
    /// The auth host, `server.auth_base` (0089 §1): where a person authenticates to the
    /// instance; required once the instance has a second tenant.
    #[arg(long, env = "TRIPLESPACE_AUTH_BASE")]
    auth_base: Option<String>,
    /// `embedded` to serve the site from this process, over the API's own router;
    /// `off` when a separate web tier serves it (`server.ui`, 0057 §1.4).
    #[arg(long, env = "TRIPLESPACE_UI", default_value = "embedded")]
    ui: String,
    /// A directory of installed skins beside the shipped ones (`server.skins`, 0097 §8).
    #[arg(long, env = "TRIPLESPACE_SKINS")]
    skins: Option<std::path::PathBuf>,
}

/// How long an idle worker waits before looking again.
const WORKER_IDLE: std::time::Duration = std::time::Duration::from_secs(1);

/// The background work of a running server: the services' states for Special:Version,
/// the asynchronous projection steps, and the workers.
async fn background(app: &triplespace_api_action::App, workers: usize) {
    // The services' states and versions for Special:Version and siteinfo: read now and
    // every minute, never while answering a request (0077 §4).
    triplespace_api_action::version::refresh(app).await;
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(PROBE_EVERY);
            every.tick().await;
            loop {
                every.tick().await;
                triplespace_api_action::version::refresh(&app).await;
            }
        });
    }
    // The position-based asynchronous step (addressing), behind every write by about this
    // much (0013 §7).
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(WORKER_IDLE);
            loop {
                every.tick().await;
                if let Err(e) = app.catch_up_async().await {
                    eprintln!("asynchronous projections: {e}");
                }
            }
        });
    }
    // The workers over the work queue (03 §6.1). Each runs a pass, then goes again at once
    // while there is work and waits when there is none; the queue's per-tenant locks share
    // the tenants between them. A consumer's failure is logged once per batch and recorded
    // on the items, which are retried a bounded number of times.
    for n in 0..workers {
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                match app.work_once().await {
                    Ok(ran) => {
                        for f in &ran.failures {
                            eprintln!(
                                "worker {n}: {} for tenant {:?} failed on {} item(s): {}",
                                f.consumer,
                                f.tenant,
                                f.ids.len(),
                                f.error
                            );
                        }
                        if ran.idle() {
                            tokio::time::sleep(WORKER_IDLE).await;
                        }
                    }
                    Err(e) => {
                        eprintln!("worker {n}: {e}");
                        tokio::time::sleep(WORKER_IDLE).await;
                    }
                }
            }
        });
    }
}

fn main() -> Result<()> {
    // Before the runtime starts: the `.env` file is read into the environment (23 §5.1).
    if let Some(path) = triplespace_cli::env_file::load()? {
        eprintln!("configuration from {}", path.display());
    }
    run()
}

#[tokio::main]
async fn run() -> Result<()> {
    let args = Args::parse();
    let embedded = match args.ui.as_str() {
        "embedded" => true,
        "off" => false,
        other => bail!("--ui is embedded or off, not `{other}`"),
    };
    let mode = Mode::parse(&args.mode)
        .with_context(|| format!("--mode is production or development, not `{}`", args.mode))?;
    if mode == Mode::Production && args.dev_tenant.is_some() {
        bail!("--dev-tenant is a development-mode setting (0056 §10)");
    }
    let key = triplespace_cli::common::signing_key(&args.key_file)?;
    let secret = triplespace_accounts_secret(&key);
    let client = triplespace_cli::common::connect(&args.database).await?;
    // A database behind this binary's migrations would answer every read that touches a
    // missing table with an internal error; refuse up front and name the fix.
    let pending = triplespace_db::migrator()
        .pending(&client)
        .await
        .context("migrations")?;
    if !pending.is_empty() {
        bail!(
            "the database has not applied {}; run `triplespace migrate` first",
            pending
                .iter()
                .map(|(c, n)| format!("{c}/{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let farm = triplespace_cli::common::farm_of(&client).await?;
    let auth_base = auth_base(&client, &farm, args.auth_base.as_deref()).await?;
    drop(client);
    let pool = triplespace_db::pool::pool(&args.database, args.pool).context("pool")?;
    let app = App::new(
        pool,
        secret,
        Config {
            mode,
            trusted_proxies: args.trusted_proxies.clone(),
            dev_tenant: args.dev_tenant.clone(),
            farm,
            auth_base,
            // Clients parse a MediaWiki version out of this (Pywikibot refuses anything
            // under 1.31); the reference install this API is measured against is 1.43.9
            // (mediawiki-compat.md), so that is the version claimed.
            generator: format!(
                "MediaWiki 1.43.9 (Triplespace {})",
                env!("CARGO_PKG_VERSION")
            ),
            version: Some(VERSION_JSON),
        },
    )
    .map_err(|e| anyhow::anyhow!(e))?;
    background(&app, args.workers).await;
    if let Some(admin) = &args.admin_listen {
        let admin_router =
            axum::Router::new().route("/healthz", axum::routing::get(|| async { "ok" }));
        let listener = tokio::net::TcpListener::bind(admin)
            .await
            .with_context(|| format!("bind {admin}"))?;
        eprintln!("admin on {admin}");
        tokio::spawn(async move {
            let _ = axum::serve(listener, admin_router).await;
        });
    }
    let listener = tokio::net::TcpListener::bind(&args.listen)
        .await
        .with_context(|| format!("bind {}", args.listen))?;
    eprintln!(
        "triplespace-server {} on {} ({}, site {})",
        env!("CARGO_PKG_VERSION"),
        args.listen,
        args.mode,
        args.ui
    );
    let api = router_with(app, triplespace_api_rest::routes());
    let router = if embedded {
        with_site(api, args.skins.as_deref())
    } else {
        api
    };
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .context("serve")?;
    Ok(())
}

/// The API's router with the site as its fallback, calling the API in process (0057 §1.4,
/// §2): whatever the API does not route, the site serves, except the API's own paths,
/// which the site refuses.
fn with_site(api: axum::Router, skins: Option<&std::path::Path>) -> axum::Router {
    let client = Client::new(ServiceTransport::new(api.clone()));
    let site = triplespace_ui::Site::new(client).with_skins_dir(skins);
    api.fallback_service(triplespace_ui::router_for(site))
}

/// How often the services are probed for Special:Version (0077 §4), in seconds.
const PROBE_SECS: u64 = 60;
const PROBE_EVERY: std::time::Duration = std::time::Duration::from_secs(PROBE_SECS);

/// This binary's build and its part of the component manifest (0077 §15), written by
/// `build.rs`.
const VERSION_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/version.json"));

fn triplespace_accounts_secret(key: &ed25519_dalek::SigningKey) -> triplespace_accounts::Secret {
    triplespace_accounts::Secret::derive(&key.to_bytes())
}

/// The auth base (0089 §1): required once there is a second tenant, and neither a tenant
/// base nor the farm base.
async fn auth_base(
    client: &tokio_postgres::Client,
    farm: &triplespace_projections::Farm,
    given: Option<&str>,
) -> Result<Option<String>> {
    let tenants = triplespace_api_action::tenant::registered(client).await?;
    let auth_base = given.filter(|a| !a.is_empty()).map(str::to_owned);
    match &auth_base {
        None if tenants.len() > 1 => bail!(
            "ts-auth-base-required: this instance has {} tenants and needs an auth host; set --auth-base (server.auth_base, 0089 §1)",
            tenants.len()
        ),
        Some(a) => {
            let host = triplespace_api_action::tenant::host_of(a)
                .with_context(|| format!("--auth-base `{a}` is not a URL"))?;
            let clash = |base: &str| {
                triplespace_api_action::tenant::host_of(base)
                    .is_some_and(|h| h.eq_ignore_ascii_case(&host))
            };
            if clash(&farm.base) || tenants.iter().any(|t| clash(&t.base)) {
                bail!("--auth-base `{a}` is the farm base or a tenant's base (0089 §1)");
            }
        }
        None => {}
    }
    Ok(auth_base)
}
