//! The site (0034; 0057): pages rendered on the server from the public API, in Codex's
//! CSS-only markup, readable without JavaScript.
//!
//! One router serves the site wherever it runs: from `triplespace-web` over an HTTP
//! [`triplespace_client::HttpTransport`], or from `triplespace-server` (`server.ui =
//! embedded`) over an in-process [`triplespace_client::ServiceTransport`] into the API's
//! own router (0057 §1.4, §2). The site holds no state, reaches no store and depends on
//! no API crate (0005 §3 rule 10).
//!
//! - [`routes`]: which paths the site serves and which the API does, the one list the
//!   edge proxy's configuration and the embedded mode both come from (0057 §3).
//! - [`pages`]: the handlers, and the caching a page composes from the API responses it
//!   used (0057 §6).
//! - [`entity`]: entity pages and their `action=render` regions (0010 §2; 0057 §8).
//! - [`special`]: the special pages served so far (0047), starting with `Special:Search`.
//! - [`document`]: document pages, read, edited and their histories (0008; 19 §3).
//! - [`discussion`]: talk pages and threads, read and posted to (0019 §8; 19 §6.1).
//! - [`home`]: the main page, `Project:Home`, generated until a page of that title exists.
//! - [`features`]: what the site offers, from what it has built and what the API serves;
//!   nothing links to a feature that is not offered.
//! - [`appearance`]: the Appearance menu and the reader's choices on `<html>` (0085 §6).
//! - [`frame`]: the page frame of 0010 §2, with the [`codex`] builder's components.
//! - [`skin`]: the skins (0097 §8), Plain and Worksheet embedded and the operator's
//!   installed, and the renderer that draws a page through one; [`Renderer`] says which
//!   renderer a deployment runs until the migration of 19 §5.10 is done.
//! - [`theme`]: a stylesheet per palette, with its light, dark and high-contrast
//!   variants, and one per type set, with its typefaces and text sizes (0086 §5).
//! - [`assets`]: the built frontend (`ui/dist`), embedded, and its manifest.
//! - [`i18n`]: interface messages from `i18n/` (0034 §9).
//! - [`html`]: escaping, for the pieces too small for a template.
//! - [`parity`]: two renderings of a page compared as a browser reads them, for the
//!   skin migration (0097 §16).

#![forbid(unsafe_code)]

pub mod appearance;
pub mod assets;
pub mod codex;
pub mod discussion;
pub mod document;
pub mod entity;
pub mod features;
pub mod frame;
pub mod home;
pub mod html;
pub mod i18n;
pub mod moderation;
pub mod pages;
pub mod parity;
pub mod routes;
pub mod skin;
pub mod special;
pub mod theme;
pub mod watch;

use axum::Router;
use axum::routing::get;
use triplespace_client::Client;

/// Lower-case hex of some bytes.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

/// The lowest `api_version` this build of the site works with (0057 §9).
pub const MIN_API_VERSION: u32 = 1;

/// Which renderer draws a page (19 §5.10): the built-in rendering, the skin, or both
/// with the built-in result served and each route that differs logged. Deployment
/// configuration `server.renderer` and `web.renderer`, for the groups not yet switched;
/// removed with the last of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Renderer {
    /// The built-in rendering: `askama` templates and the string builders.
    #[default]
    Builtin,
    /// The skin.
    Skin,
    /// Both; the built-in result is served, and a route whose results differ is logged
    /// with the request that showed it.
    Compare,
}

impl Renderer {
    /// `builtin`, `skin` or `compare`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "builtin" => Some(Self::Builtin),
            "skin" => Some(Self::Skin),
            "compare" => Some(Self::Compare),
            _ => None,
        }
    }
}

/// The site's state: its client of the API, the skins it has and which renderer draws
/// pages, and, in the web tier, the tier's own build. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Site {
    client: Client,
    build: Option<std::sync::Arc<triplespace_client::version::Build>>,
    skins: std::sync::Arc<skin::Skins>,
    renderer: Renderer,
}

impl Site {
    /// The site over a client, with the shipped skins and the built-in renderer.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            build: None,
            skins: std::sync::Arc::new(skin::Skins::load(None)),
            renderer: Renderer::default(),
        }
    }

    /// The site with the skins installed under a directory beside the shipped ones
    /// (`server.skins`, `web.skins`), read now.
    #[must_use]
    pub fn with_skins_dir(mut self, dir: Option<&std::path::Path>) -> Self {
        self.skins = std::sync::Arc::new(skin::Skins::load(dir));
        for r in self.skins.refused() {
            eprintln!(
                "triplespace-ui: the skin `{}` ({}) is not loaded: {}",
                r.name, r.origin, r.reason
            );
        }
        self
    }

    /// The site with a renderer (`server.renderer`, `web.renderer`).
    #[must_use]
    pub fn with_renderer(mut self, renderer: Renderer) -> Self {
        self.renderer = renderer;
        self
    }

    /// The skins.
    #[must_use]
    pub fn skins(&self) -> &skin::Skins {
        &self.skins
    }

    /// The renderer.
    #[must_use]
    pub fn renderer(&self) -> Renderer {
        self.renderer
    }

    /// The site of a separate web tier, whose own build `Special:Version` shows beside
    /// the API server's (0077 §1, §4): `version_json` is the binary's `version.json`.
    #[must_use]
    pub fn with_build(mut self, version_json: &str) -> Self {
        self.build = serde_json::from_str::<serde_json::Value>(version_json)
            .ok()
            .and_then(|v| serde_json::from_value(v["build"].clone()).ok())
            .map(std::sync::Arc::new);
        self
    }

    /// The client.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// The web tier's own build, where the site runs in one.
    #[must_use]
    pub fn build(&self) -> Option<&triplespace_client::version::Build> {
        self.build.as_deref()
    }
}

/// The site's router: every path of [`routes::TABLE`] the site serves, with its own 404
/// page for everything else but the API's paths, which it refuses with a plain 404, so
/// that it can be the fallback of the API's router (0057 §3).
pub fn router(client: Client) -> Router {
    router_for(Site::new(client))
}

/// The router of a separate web tier: [`router`], with the tier's own build for
/// `Special:Version` (`version_json`, its binary's `version.json`).
pub fn router_with_build(client: Client, version_json: &str) -> Router {
    router_for(Site::new(client).with_build(version_json))
}

/// The router over a site configured by hand: the skins directory and the renderer.
pub fn router_for(site: Site) -> Router {
    Router::new()
        .route("/", get(pages::root))
        .route("/wiki/", get(pages::root))
        .route("/wiki/{*title}", get(pages::wiki).post(pages::wiki_post))
        .route(
            "/w/index.php",
            get(pages::index_php).post(pages::index_post),
        )
        .route("/index.php", get(pages::index_php).post(pages::index_post))
        .route("/ui/assets/{*path}", get(pages::asset))
        .route("/ui/skins/{skin}/{hash}/{*path}", get(pages::skin_asset))
        .route("/ui/theme/{file}", get(pages::theme_css))
        .fallback(pages::not_found)
        .with_state(site)
        .layer(axum::middleware::from_fn(pages::api_paths_refused))
        .layer(axum::middleware::from_fn(pages::security_headers))
}
