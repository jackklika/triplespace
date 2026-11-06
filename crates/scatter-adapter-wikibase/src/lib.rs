//! The Wikibase adapter (0002 §8.4; 0035 §3): reading a Wikibase's dumps and turning them
//! into what the ingester writes.
//!
//! - [`json_dump`]: the entity JSON dump `dumpJson.php` writes (one entity per line in a
//!   JSON array; Wikidata's `latest-all.json.gz`), streamed, gzip or bzip2 compressed.
//! - [`xml_dump`]: a MediaWiki XML dump (`dumpBackup.php`, Special:Export), streamed page
//!   by page with each page's revisions: the complete input for an adoption, since it
//!   carries the page ID, the revision ID and timestamp, the contributor and the entity
//!   JSON together.
//! - [`adoption`]: a dump's pages as adopted entities in home form (0035 §3), with the
//!   [`adoption::Survey`] that derives the sequence floors and the accounts from a pass
//!   over the dump.
//! - [`mirror`]: a dump's entities as mirror states under the provider's prefixed IDs.
//! - [`entities`]: either dump as one stream of entities with their revision metadata,
//!   for a mirror sync.
//! - [`typing`]: the snak data types an XML dump leaves out, from the properties the
//!   dump defines, for a mirror sync.
//! - [`follow`]: the follower's reading of the API: entity namespaces from `siteinfo`, a
//!   recent-changes title as an entity ID, and `wbgetentities` as states, redirects and
//!   misses (0092 §1–§2).
//! - [`key_map`]: the adapter in mirror mode, with the provider's key-map rules, which map
//!   upstream items onto keyed types (0009 §9; 0091 §2–§3).
//!
//! One crate reads every MediaWiki+Wikibase provider, parameterised by its registry entry
//! (0091 §1).
//!
//! The crate reads files and parses; it knows no log and no database (0005 §2).

#![forbid(unsafe_code)]

pub mod adoption;
pub mod entities;
pub mod follow;
pub mod json_dump;
pub mod key_map;
pub mod mirror;
pub mod typing;
pub mod xml_dump;

pub use adoption::{Account, AdoptedRevision, Survey, adopt_page};
pub use entities::{DumpEntry, DumpRedirect, Entities, Entries};
pub use follow::{EntityNamespaces, Fetched, FollowError, entity_id_of, read_entities};
pub use json_dump::{JsonDump, parse_line};
pub use key_map::{ConfigError, Index, KeyMappedAdapter, KeyRule, KeyRules, MapError, Outcome};
pub use typing::DumpPropertyTypes;
pub use xml_dump::{DumpPage, Revision, XmlDump};

/// The crate's name, as a provider's `adapter` field names it (0091 §1).
pub const NAME: &str = env!("CARGO_PKG_NAME");
/// The crate's version, recorded with [`NAME`] in a job's `adapter_version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use std::io::{BufRead, BufReader};
use std::path::Path;

/// Why a dump could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DumpError {
    /// Reading failed.
    #[error("read: {0}")]
    Io(#[from] std::io::Error),
    /// The XML is not a MediaWiki dump.
    #[error("xml: {0}")]
    Xml(String),
    /// A line is not an entity.
    #[error("line {line}: {message}")]
    Entity {
        /// 1-based.
        line: usize,
        /// What went wrong.
        message: String,
    },
    /// An ID could not be rewritten.
    #[error(transparent)]
    Adapter(#[from] scatter_wikibase_changeset::AdapterError),
}

/// Opens a dump for reading, decompressing by extension: `.gz` and `.bz2`, else plain.
pub fn open(path: &Path) -> Result<Box<dyn BufRead + Send>, DumpError> {
    let file = std::fs::File::open(path)?;
    let name = path.to_string_lossy();
    Ok(if name.ends_with(".gz") {
        Box::new(BufReader::new(flate2::read::MultiGzDecoder::new(file)))
    } else if name.ends_with(".bz2") {
        Box::new(BufReader::new(bzip2::read::MultiBzDecoder::new(file)))
    } else {
        Box::new(BufReader::new(file))
    })
}
