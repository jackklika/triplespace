//! Shared test support: the fixture API (`fixture_api`).

#![allow(dead_code)]

pub mod fixture_api;

/// The site's router over a client, with the renderer `SKIN_PARITY_RENDERER` names
/// (`builtin`, `skin` or `compare`; the built-in one by default), so that every suite
/// runs through either renderer while the skin migration runs (19 §5.10).
pub fn site_router(client: triplespace_client::Client) -> axum::Router {
    let renderer = std::env::var("SKIN_PARITY_RENDERER")
        .ok()
        .and_then(|r| triplespace_ui::Renderer::parse(&r))
        .unwrap_or_default();
    triplespace_ui::router_for(triplespace_ui::Site::new(client).with_renderer(renderer))
}
