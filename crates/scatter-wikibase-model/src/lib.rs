//! The Wikibase data model (ADR 0005 §2; [wikibase-compat.md](../../../docs/api/wikibase-compat.md)
//! §2–3), with the three entity ID forms of 0017 §1 and the keyed entity types of 0009 §3 and
//! 0017 §5.
//!
//! The model mirrors Wikibase's canonical JSON faithfully. 0006 §2 makes the stored form
//! "the JSON's structure and nothing else", and 0004 §7 says stored values are never
//! changed, so this crate holds what a source asserted, in the shape it asserted it:
//! sitelinks stay keyed by site with their title and URL, values keep the strings
//! Wikibase gives them, and property order inside qualifiers and references is data.
//! Normalization (URLs, value keys, IDs) is a function over the model, in
//! `scatter-normalize` and the projections, not a transformation at parse time.
//!
//! Two derivable fields are dropped at ingest and recomputed on output (0006 §2, as
//! amended): the `hash` on snaks and references, and `numeric-id` beside `id` on entity
//! values. [`json::Form::Storage`] is the form without them, which the log stores;
//! [`json::Form::Wikibase`] is what the API serves. [`hash::Hasher`] reproduces Wikibase's
//! own hash computation, PHP serializations and all, so that a recomputed hash equals the
//! one Wikidata serves for mirrored data; [`hash::Hasher::reconcile`] is the ingest guard
//! that checks it does, and keeps upstream's hash in place where it does not.
//!
//! Two functions over the model serve the resolved view and the index: [`key::statement_key`],
//! the key statements fuse by (0004 §8), with the canonical IDs and resolvers passed in
//! through [`key::Canon`]; and [`search::entity_document`], one document of the `entities`
//! index (0014 §7), which carries its target's visibility set as `read_groups` (0056 §8).
//!
//! Canonical JSON ([`json::canonical`]) sorts object keys the way the core deterministic
//! CBOR encoding does (by encoded bytes: shorter keys first, then bytewise), so that
//! JSON → CBOR → JSON reproduces the bytes.
//!
//! This crate is pure (0005 §3, rule 2) and, through `scatter-wikibase-shape`, has to build
//! for `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]

pub mod entity;
pub mod hash;
pub mod id;
pub mod json;
pub mod key;
mod php;
pub mod search;
pub mod sites;
pub mod statement;
pub mod value;

pub use entity::{Entity, EntityType, PageInfo, ParsedEntity, Sitelink, Term};
pub use hash::{HashMismatches, Hasher};
pub use id::{EntityId, IdForm, IdParseError, LexemePart, StatementId, Subject};
pub use json::{Form, JsonError};
pub use key::{Canon, Identity, snak_key, statement_key, value_key};
pub use search::{DocumentContext, EntityDocument, entity_document};
pub use sites::{Site, SiteRegistry, SiteRegistryError};
pub use statement::{Rank, Reference, Snak, SnakKind, Statement};
pub use value::{
    DataType, DataValue, EntityIdValue, GlobeCoordinate, MonolingualText, Quantity, Time, ValueType,
};

/// The default site-alias registry, `docs/registry/sites.toml`, embedded at build time.
pub const SITES_TOML: &str = include_str!("../../../docs/registry/sites.toml");
