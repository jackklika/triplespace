//! `triplespace-web`: the stateless web tier (0057 §1–2).
//!
//! It serves the site ([`triplespace_ui`]) from the instance's public API, which it
//! reaches over HTTP ([`triplespace_client::HttpTransport`]). It keeps nothing between
//! requests, so any number of replicas can stand behind the edge, which sends the API's
//! paths to the API and the rest here (`triplespace-web routes`, 0057 §3).
//!
//! ```text
//! triplespace-web --api http://api.svc:8080
//! triplespace-web routes --format caddy
//! triplespace-web record --api … --host wiki.example --out set.json /wiki/Item:Q6 …
//! ```
//!
//! `record` renders each path given through the site, as a browser at `--host` would ask
//! for it, and writes every API exchange the pages made to `--out`: a recording the
//! parity harness of the skin migration replays (`triplespace_client::replay`;
//! 0097 §16). `--cookie` records as a signed-in viewer.

#![forbid(unsafe_code)]

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use clap::{Parser, Subcommand};
use triplespace_client::replay::Record;
use triplespace_client::{Client, HttpTransport, Incoming};
use triplespace_ui::routes::{Format, print};

/// The Triplespace web tier.
#[derive(Debug, Parser)]
#[command(
    name = "triplespace-web",
    version,
    about,
    args_conflicts_with_subcommands = true
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    serve: Serve,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Prints the edge proxy's routing: which paths go to the API, which to the web tier.
    Routes {
        /// `caddy`, `nginx`, `haproxy`, or `json` for tools.
        #[arg(long, default_value = "caddy")]
        format: String,
    },
    /// Renders pages from a real instance and records the API exchanges they made, for
    /// the parity harness (`cargo xtask skin-parity`).
    Record {
        /// The API's base URL, as `--api`.
        #[arg(long, env = "TRIPLESPACE_WEB_API")]
        api: String,
        /// The wiki's host, as the browser would send it.
        #[arg(long)]
        host: String,
        /// Where to write the recording, a JSON list of exchanges.
        #[arg(long)]
        out: PathBuf,
        /// A `Cookie` header value, to record as a signed-in viewer.
        #[arg(long)]
        cookie: Option<String>,
        /// A file holding the forwarder key, as `--forwarder-key-file`.
        #[arg(long)]
        forwarder_key_file: Option<PathBuf>,
        /// The paths to render, `/wiki/Item:Q6`, `/w/index.php?title=…`.
        #[arg(required = true)]
        paths: Vec<String>,
    },
}

#[derive(Debug, clap::Args)]
struct Serve {
    /// The API's base URL (`web.api`): `https://…`, or plain `http://…` to a host that
    /// resolves only to internal addresses (0057 §4).
    #[arg(long, env = "TRIPLESPACE_WEB_API")]
    api: Option<String>,
    /// Where to listen (`web.listen`).
    #[arg(long, env = "TRIPLESPACE_WEB_LISTEN", default_value = "127.0.0.1:8081")]
    listen: String,
    /// A separate listener for `/healthz` and `/readyz` (`web.admin_listen`).
    #[arg(long, env = "TRIPLESPACE_WEB_ADMIN_LISTEN")]
    admin_listen: Option<String>,
    /// A file holding this tier's forwarder key, sent to the API in
    /// `Triplespace-Forwarder` (0057 §10). Read from a file so that it never appears in
    /// a process listing.
    #[arg(long, env = "TRIPLESPACE_WEB_FORWARDER_KEY_FILE")]
    forwarder_key_file: Option<PathBuf>,
    /// A directory of installed skins beside the shipped ones (`web.skins`, 0097 §8).
    #[arg(long, env = "TRIPLESPACE_WEB_SKINS")]
    skins: Option<PathBuf>,
    /// Which renderer draws pages until the skin migration is done (`web.renderer`,
    /// 19 §5.10): `builtin`, `skin`, or `compare`, which renders both, serves the
    /// built-in result and logs each route that differs.
    #[arg(long, env = "TRIPLESPACE_WEB_RENDERER", default_value = "builtin")]
    renderer: String,
}

fn main() -> Result<()> {
    // Before the runtime starts: the `.env` file is read into the environment (23 §5.1).
    if let Some(path) = load_env_file()? {
        eprintln!("configuration from {}", path.display());
    }
    run()
}

/// Reads the `.env` file, by the rules of `triplespace_cli::env_file`, which this crate
/// may not depend on (22 §1.2 rule 6): the file `TRIPLESPACE_ENV_FILE` names, which must
/// exist, else `./.env` if there is one; never overriding a variable already set.
fn load_env_file() -> Result<Option<PathBuf>> {
    let (path, required) = match std::env::var_os("TRIPLESPACE_ENV_FILE") {
        Some(p) if !p.is_empty() => (PathBuf::from(p), true),
        _ => (PathBuf::from(".env"), false),
    };
    match dotenvy::from_path(&path) {
        Ok(()) => Ok(Some(path)),
        Err(e) if e.not_found() && !required => Ok(None),
        Err(e) if e.not_found() => Err(e).with_context(|| {
            format!(
                "TRIPLESPACE_ENV_FILE names {}, which does not exist",
                path.display()
            )
        }),
        Err(e) => Err(e).with_context(|| format!("read {}", path.display())),
    }
}

#[tokio::main]
async fn run() -> Result<()> {
    let args = Args::parse();
    match args.command {
        Some(Command::Routes { format }) => {
            let Some(f) = Format::parse(&format) else {
                bail!("--format is caddy, nginx, haproxy or json, not `{format}`");
            };
            print!("{}", print(f));
            Ok(())
        }
        Some(Command::Record {
            api,
            host,
            out,
            cookie,
            forwarder_key_file,
            paths,
        }) => {
            record(
                &api,
                &host,
                &out,
                cookie.as_deref(),
                forwarder_key_file.as_deref(),
                &paths,
            )
            .await
        }
        None => serve(args.serve).await,
    }
}

/// The forwarder key a file holds.
fn read_key(path: Option<&std::path::Path>) -> Result<Option<String>> {
    match path {
        Some(path) => {
            let k = std::fs::read_to_string(path)
                .with_context(|| format!("read {}", path.display()))?
                .trim()
                .to_string();
            if k.is_empty() {
                bail!("{} is empty", path.display());
            }
            Ok(Some(k))
        }
        None => Ok(None),
    }
}

/// Renders each path through the site over a recording transport and writes the
/// exchanges.
async fn record(
    api: &str,
    host: &str,
    out: &std::path::Path,
    cookie: Option<&str>,
    key_file: Option<&std::path::Path>,
    paths: &[String],
) -> Result<()> {
    use axum::body::Body;
    use axum::http::{Request, header};
    use tower::ServiceExt as _;

    let transport = HttpTransport::new(api, read_key(key_file)?)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let recorder = std::sync::Arc::new(Record::new(transport));
    let client = Client::new(Shared(recorder.clone()));
    for path in paths {
        let mut req = Request::get(path).header(header::HOST, host);
        if let Some(c) = cookie {
            req = req.header(header::COOKIE, c);
        }
        let r = triplespace_ui::router_with_build(client.clone(), VERSION_JSON)
            .oneshot(req.body(Body::empty()).context("request")?)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        eprintln!("{} {path}", r.status());
    }
    // One entry per distinct request: the frame asks for `siteinfo` on every page.
    let exchanges: Vec<_> = triplespace_client::replay::Replay::new(recorder.exchanges())
        .exchanges()
        .cloned()
        .collect();
    let text = serde_json::to_string_pretty(&exchanges).context("encode the recording")?;
    std::fs::write(out, text).with_context(|| format!("write {}", out.display()))?;
    eprintln!("{} exchanges written to {}", exchanges.len(), out.display());
    Ok(())
}

/// A transport shared with the recorder, so that the exchanges can be read after the
/// client is done with it.
struct Shared<T>(std::sync::Arc<Record<T>>);

impl<T: triplespace_client::Transport> triplespace_client::Transport for Shared<T> {
    fn send(
        &self,
        request: triplespace_client::ApiRequest,
    ) -> triplespace_client::transport::BoxFuture<
        '_,
        Result<triplespace_client::ApiResponse, triplespace_client::ClientError>,
    > {
        self.0.send(request)
    }

    fn embedded(&self) -> bool {
        self.0.embedded()
    }

    fn forwarder_key(&self) -> Option<&str> {
        self.0.forwarder_key()
    }
}

async fn serve(args: Serve) -> Result<()> {
    let Some(api) = args.api else {
        bail!("--api (or TRIPLESPACE_WEB_API) names the API's base URL");
    };
    let Some(renderer) = triplespace_ui::Renderer::parse(&args.renderer) else {
        bail!(
            "--renderer is builtin, skin or compare, not `{}`",
            args.renderer
        );
    };
    let key = read_key(args.forwarder_key_file.as_deref())?;
    let transport = HttpTransport::new(&api, key)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let client = Client::new(transport);
    if let Some(admin) = &args.admin_listen {
        let client = client.clone();
        let admin_router = axum::Router::new()
            .route("/healthz", axum::routing::get(|| async { "ok" }))
            .route(
                "/readyz",
                axum::routing::get(move || {
                    let client = client.clone();
                    async move { ready(&client).await }
                }),
            );
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
        "triplespace-web {} on {}, API at {api}",
        env!("CARGO_PKG_VERSION"),
        args.listen
    );
    let site = triplespace_ui::Site::new(client)
        .with_build(VERSION_JSON)
        .with_skins_dir(args.skins.as_deref())
        .with_renderer(renderer);
    axum::serve(
        listener,
        triplespace_ui::router_for(site).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await
    .context("serve")?;
    Ok(())
}

/// Ready when the API answers its health check. The API's version is checked on every
/// page instead, since `siteinfo` needs a tenant's host (0057 §9).
async fn ready(client: &Client) -> axum::response::Response {
    match client.get(&Incoming::default(), "/healthz").await {
        Ok(r) if r.status == StatusCode::OK => "ok".into_response(),
        Ok(r) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("the API answered {}\n", r.status),
        )
            .into_response(),
        Err(e) => (StatusCode::SERVICE_UNAVAILABLE, format!("{e}\n")).into_response(),
    }
}

/// This binary's build and its part of the component manifest (0077 §15), written by
/// `build.rs`: Special:Version shows the web tier's build beside the API server's.
const VERSION_JSON: &str = include_str!(concat!(env!("OUT_DIR"), "/version.json"));
