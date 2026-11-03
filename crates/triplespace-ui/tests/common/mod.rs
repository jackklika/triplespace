//! Shared test support: the fixture API (`fixture_api`).

#![allow(dead_code)]

pub mod fixture_api;

/// The site's router over a client.
pub fn site_router(client: triplespace_client::Client) -> axum::Router {
    triplespace_ui::router_for(triplespace_ui::Site::new(client))
}
