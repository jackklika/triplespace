//! The `view` projections over Postgres (0013 §5, §7; 0005 §2): the [`PgBackend`] that
//! drives `scatter-projection`'s pipeline on the `ops` tables, and the projections in
//! step order.
//!
//! - Step 1, what the write path reads: [`registry`], [`actors`] (actor, group,
//!   membership, block, acl), the keyed surrogates ([`sources`]), and the tier-1 state of
//!   0083 §4, `graph_state` and `entity_revision` ([`revision_state`]).
//! - Step 2, the source graphs: `entity_source` and `keyed_map` ([`sources`]), `job`
//!   ([`jobs`]).
//! - Step 4, resolution: `entity` with its `term` and `identifier` tables ([`entities`]).
//! - Step 5: `activity` ([`activity`]); the feeds that read it ([`feed`], 0020 §1).
//!
//! Document pages are step 2 ([`pages`]). Clusters (step 3), sitelinks, references, corrections, constraints, patrol,
//! filters, notifications and RDF follow as the crates that produce their records are
//! built. [`milestone_pipeline`] assembles the set built so far.

pub mod acl_read;
pub mod activity;
pub mod actors;
pub mod backend;
pub mod common;
pub mod entities;
pub mod entity_sources;
pub mod feed;
pub mod jobs;
pub mod moderation;
pub mod pages;
pub mod read;
pub mod registry;
pub mod reports;
pub mod revision_state;
pub mod revisions;
pub mod sources;
pub mod tenants;
pub mod threads;
pub mod watched;

pub use activity::ActivityProjection;
pub use actors::Farm;
pub use backend::{PgBackend, PgCx};
pub use entities::EntityProjection;
pub use jobs::JobProjection;
pub use pages::PageProjection;
pub use registry::RegistryProjection;
pub use revision_state::RevisionStateProjection;
pub use sources::{EntitySourceProjection, KeyedMapProjection, KeyedSurrogateProjection};

use scatter_actors::issuer::IssuerRegistry;
use scatter_projection::{Backend, Pipeline, ProjectionError};

/// The pipeline of every projection built so far, in step order, over any backend whose
/// unit of work is a [`PgCx`] (the projections' own [`PgBackend`], or a store wrapping it).
pub fn milestone_pipeline<B: Backend<Cx = PgCx>>(
    issuers: IssuerRegistry,
    farm: Farm,
) -> Result<Pipeline<B>, ProjectionError> {
    Pipeline::new()
        .before_block(common::decode_block)
        .with(RegistryProjection)?
        .with(KeyedSurrogateProjection)?
        .with(actors::ActorProjection::new(issuers, farm))?
        .with(actors::GroupProjection)?
        .with(actors::MembershipProjection)?
        .with(actors::BlockProjection)?
        .with(actors::AclProjection)?
        .with(RevisionStateProjection)?
        .with(EntitySourceProjection)?
        .with(KeyedMapProjection)?
        .with(JobProjection)?
        .with(PageProjection)?
        .with(threads::ThreadProjection)?
        .with(moderation::ModerationProjection)?
        .with(EntityProjection::default())?
        .with(ActivityProjection)
}
