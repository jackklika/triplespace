//! Parsing and validating `providers.toml` (ADR 0015 §5).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use serde::Deserialize;

use crate::DEFAULT_TOML;
use crate::grammar::{IdGrammar, UnknownGrammar};

/// How a provider that publishes a transparency-log checkpoint is read (0022 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Trust {
    /// Every mirrored batch is checked against the provider's checkpoint and key chain.
    /// The default for a Triplespace provider.
    #[default]
    Verified,
    /// Mirrored without proofs, like any other source.
    Stream,
}

/// One entity type a provider mints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityType {
    /// The one-letter type code; the entity ID prefix is the provider code followed by it.
    pub code: char,
    /// The Wikibase entity type of the mirrored entity (`item`, `property`, `lexeme`, …).
    pub entity_type: String,
    /// What the provider's own IDs start with, stripped and re-added when rewriting.
    pub upstream_prefix: String,
    /// The canonical concept-IRI template; `{upstream_id}` is the provider's own ID.
    pub iri: String,
    /// The Triplespace namespace whose pages host the type, if one is registered.
    pub namespace: Option<String>,
    /// A display label for the type (`Work`, `Author`, …).
    pub label: Option<String>,
    /// The type's ID grammar: its own, or the provider's, or `digits`.
    pub id_grammar: IdGrammar,
    /// Upstream entities of this type are written under a keyed type's keys (0009 §9),
    /// never under this prefix. The code stays reserved.
    pub key_mapped: bool,
}

impl EntityType {
    /// The concept IRI for an upstream ID of this type, as a string.
    #[must_use]
    pub fn iri_for(&self, upstream_id: &str) -> String {
        self.iri.replace("{upstream_id}", upstream_id)
    }

    /// The canonical spelling of an ID's rest (after the prefix) under this type, or
    /// `None` if it is not valid: the type's grammar, and for a type whose entities are
    /// lexemes, optionally a part suffix after it, `-F` or `-S` and digits, naming a form
    /// or a sense, with the part letter uppercased (0066 §4). The suffix belongs to the
    /// lexeme entity type, whatever the grammar of its base; no other type takes one.
    #[must_use]
    pub fn canonical_rest(&self, rest: &str) -> Option<String> {
        if self.entity_type == "lexeme"
            && let Some((base, part)) = rest.rsplit_once('-')
            && let Some(kind) = part.as_bytes().first().map(u8::to_ascii_uppercase)
            && (kind == b'F' || kind == b'S')
            && part.len() > 1
            && part.bytes().skip(1).all(|b| b.is_ascii_digit())
            && let Some(base) = self.id_grammar.canonicalize(base)
        {
            return Some(format!("{base}-{}{}", kind as char, &part[1..]));
        }
        self.id_grammar.canonicalize(rest)
    }
}

/// A provider registry entry.
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// The two-letter code that prefixes entity IDs minted by the provider.
    pub code: String,
    /// The lower-case name used in graph names and the API.
    pub slug: String,
    /// The display name.
    pub name: String,
    /// The provider number for the revision-ID range (0015 §2). Never 0.
    pub number: u32,
    /// The issuer that the provider's revisions attribute actors to (0007 §1, §6).
    pub issuer: String,
    /// Whether the provider publishes revision IDs.
    pub revision_ids: bool,
    /// The article path of the provider's wiki, where it has one.
    pub article_path: Option<String>,
    /// The provider's API endpoint.
    pub api: Option<String>,
    /// The provider's `Special:EntityData/` base, where it has one.
    pub entity_data: Option<String>,
    /// The crate that implements the provider's adapter (0002 §8.4), one per provider.
    pub adapter: Option<String>,
    /// Whether the provider's revisions name individual actors (0007 §6, 0037 §1; 0002 A28).
    pub actor_model: ActorModel,
    /// Whether a sync stores the per-field delta beside its summary (0012 §2.2, 0012 A58).
    pub sync_deltas: SyncDeltas,
    /// What an upstream deletion does to the mirror (0002 §5 as amended by 0002 A29).
    pub deletion: Deletion,
    /// The provider as a `prov:Organization` (0007 §6).
    pub agent_iri: Option<String>,
    /// The provider-level ID grammar, applied to types that name none.
    pub id_grammar: IdGrammar,
    /// Trust mode for a provider that publishes a checkpoint (0022 §2).
    pub trust: Option<Trust>,
    /// The provider's key-chain URL, required when `trust` is `verified`.
    pub keys: Option<String>,
    /// A retired entry: its code and number are never reused, and it mints nothing new.
    pub retired: bool,
    /// Adapter configuration, opaque to this crate (0009 §9: the identity property of a
    /// key-mapped provider, for example). Never a credential: the registry is a committed,
    /// public file, and an adapter's secrets come from the instance configuration, from
    /// files or the environment (0033 §12; 0056 §10, line 11). [`Registry::parse`] refuses
    /// a key that looks like one.
    pub adapter_config: Option<toml::Table>,
    /// The entity types the provider mints, in registry order.
    pub types: Vec<EntityType>,
    /// The colours of the provider's chip in the site (0010 §2), where the registry gives
    /// them.
    pub chip: Option<Chip>,
    /// An entity source a tenant declared (0078 §1; [`crate::EntitySource`]): no code, IDs
    /// in the colon form `{slug}:{upstream ID}`, records in the tenant partition
    /// `source/{slug}`.
    pub source: bool,
}

/// Whether a provider's revisions name individual actors (0007 §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActorModel {
    /// Revisions name actors, attributed through the provider's issuer.
    #[default]
    Individual,
    /// No per-change attribution: every change is attributed to the provider's own actor record.
    ProviderOnly,
}

/// Whether a sync stores the per-field delta of an observed change set (0012 §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncDeltas {
    /// The summary only (the default).
    #[default]
    Summary,
    /// The summary and the per-field delta.
    Full,
}

/// What an upstream deletion does to the mirror (0002 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Deletion {
    /// A tombstone that applies the retention policy (the default).
    #[default]
    Tombstone,
    /// A `put` of the empty state, for a provider whose IDs are never reused (0036 §2).
    Clear,
}

/// A provider chip's colours (0010 §2): text on a background, both `#RRGGBB`. The site
/// checks them for 4.5:1 contrast before using them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// The text colour.
    pub color: String,
    /// The background colour.
    pub background: String,
}

/// Whether a string is a `#RRGGBB` colour.
fn is_hex_colour(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

impl Provider {
    /// The page of one of the provider's revisions, for a MediaWiki provider that
    /// publishes revision IDs: `index.php?oldid={revision}` beside its `api.php`. `None`
    /// for any other provider, or a revision that is not a revision ID.
    #[must_use]
    pub fn revision_url(&self, revision: &str) -> Option<String> {
        if !self.revision_ids
            || revision.is_empty()
            || !revision.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let base = self.api.as_deref()?.strip_suffix("api.php")?;
        Some(format!("{base}index.php?oldid={revision}"))
    }

    /// The entity type with the given one-letter code.
    #[must_use]
    pub fn entity_type(&self, code: char) -> Option<&EntityType> {
        let code = code.to_ascii_uppercase();
        self.types.iter().find(|t| t.code == code)
    }

    /// The graph name of the provider's mirror partition, `mirror/{slug}` (0015 §5), or
    /// `source/{slug}` for an entity source (0078 §4).
    #[must_use]
    pub fn mirror_graph(&self) -> String {
        if self.source {
            format!("source/{}", self.slug)
        } else {
            format!("mirror/{}", self.slug)
        }
    }

    /// The entity type whose upstream prefix `upstream_id` starts with, the longest prefix
    /// winning; `None` when none does.
    #[must_use]
    pub fn type_of_upstream(&self, upstream_id: &str) -> Option<&EntityType> {
        self.types
            .iter()
            .filter(|t| {
                upstream_id.len() >= t.upstream_prefix.len()
                    && upstream_id[..t.upstream_prefix.len()]
                        .eq_ignore_ascii_case(&t.upstream_prefix)
            })
            .max_by_key(|t| t.upstream_prefix.len())
    }
}

/// The parsed and validated provider registry.
#[derive(Debug, Clone, PartialEq)]
pub struct Registry {
    version: u32,
    reserved_codes: BTreeSet<String>,
    providers: Vec<Provider>,
    by_code: BTreeMap<String, usize>,
    by_slug: BTreeMap<String, usize>,
    by_number: BTreeMap<u32, usize>,
}

/// Why a registry file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RegistryError {
    /// The TOML did not parse or did not have the expected shape.
    #[error("providers.toml: {0}")]
    Toml(String),
    /// A provider entry is missing a field an active entry needs.
    #[error("provider `{slug}` is missing `{field}`")]
    Missing {
        /// The provider's slug.
        slug: String,
        /// The missing field.
        field: &'static str,
    },
    /// A field holds a value it does not take.
    #[error("provider `{slug}`: `{field}` is `{value}`, which is not a value it takes")]
    BadField {
        /// The provider's slug.
        slug: String,
        /// The field.
        field: &'static str,
        /// The value given.
        value: String,
    },
    /// A provider code is not two uppercase ASCII letters.
    #[error("provider code `{0}` is not two uppercase ASCII letters")]
    BadCode(String),
    /// A provider takes a code listed in `reserved_codes` (0044 §2).
    #[error("provider `{slug}` takes the reserved code `{code}`")]
    ReservedCode {
        /// The provider's slug.
        slug: String,
        /// The reserved code.
        code: String,
    },
    /// A slug is not lower-case ASCII letters, digits and hyphens.
    #[error("provider slug `{0}` is not lower-case ASCII letters, digits and hyphens")]
    BadSlug(String),
    /// Provider number 0 is the instance and is never assigned (0015 §2).
    #[error("provider `{0}` has number 0, which is the instance itself")]
    NumberZero(String),
    /// Two entries share a code, slug or number.
    #[error("providers `{a}` and `{b}` share the {what} `{value}`")]
    Duplicate {
        /// The first provider's slug.
        a: String,
        /// The second provider's slug.
        b: String,
        /// `code`, `slug` or `number`.
        what: &'static str,
        /// The shared value.
        value: String,
    },
    /// An `adapter_config` key looks like a credential (0033 §12; 0056 §10, line 11).
    #[error(
        "provider `{slug}`: adapter_config key `{key}` looks like a credential; the registry is public, secrets come from the instance configuration"
    )]
    SecretInRegistry {
        /// The provider's slug.
        slug: String,
        /// The offending key.
        key: String,
    },
    /// A type code is not one uppercase ASCII letter.
    #[error("provider `{slug}`: type code `{code}` is not one uppercase ASCII letter")]
    BadTypeCode {
        /// The provider's slug.
        slug: String,
        /// The offending code.
        code: String,
    },
    /// Two types of one provider share a code.
    #[error("provider `{slug}` declares type code `{code}` twice")]
    DuplicateType {
        /// The provider's slug.
        slug: String,
        /// The shared code.
        code: char,
    },
    /// A type's IRI template does not contain `{upstream_id}`.
    #[error("provider `{slug}`, type `{code}`: IRI template `{iri}` has no `{{upstream_id}}`")]
    BadIriTemplate {
        /// The provider's slug.
        slug: String,
        /// The type code.
        code: char,
        /// The template.
        iri: String,
    },
    /// An unknown ID grammar name.
    #[error("provider `{slug}`: {source}")]
    Grammar {
        /// The provider's slug.
        slug: String,
        /// The unknown name.
        #[source]
        source: UnknownGrammar,
    },
    /// `trust = "verified"` without a `keys` URL.
    #[error("provider `{0}` is `trust = \"verified\"` but has no `keys` URL")]
    VerifiedWithoutKeys(String),
    /// The file's `version` is one this crate does not read.
    #[error("providers.toml version {0} is not supported (this crate reads version 1)")]
    Version(u32),
}

// ---------------------------------------------------------------------------------------
// Raw TOML shape
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegistry {
    version: u32,
    #[serde(default)]
    reserved_codes: Vec<String>,
    #[serde(default, rename = "provider")]
    providers: Vec<RawProvider>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvider {
    code: Option<String>,
    slug: String,
    name: Option<String>,
    number: Option<u32>,
    issuer: Option<String>,
    #[serde(default)]
    revision_ids: bool,
    article_path: Option<String>,
    api: Option<String>,
    entity_data: Option<String>,
    adapter: Option<String>,
    actor_model: Option<String>,
    sync_deltas: Option<String>,
    deletion: Option<String>,
    agent_iri: Option<String>,
    id_grammar: Option<String>,
    trust: Option<String>,
    keys: Option<String>,
    #[serde(default)]
    pending: bool,
    #[serde(default)]
    retired: bool,
    adapter_config: Option<toml::Table>,
    chip_color: Option<String>,
    chip_background: Option<String>,
    #[serde(default, rename = "type")]
    types: Vec<RawType>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawType {
    code: String,
    entity_type: String,
    #[serde(default)]
    upstream_prefix: String,
    iri: String,
    namespace: Option<String>,
    label: Option<String>,
    id_grammar: Option<String>,
    #[serde(default)]
    key_mapped: bool,
}

/// The value `value` names in `table`, the first entry when absent (the default), or
/// [`RegistryError::BadField`].
fn enum_field<T: Copy>(
    slug: &str,
    field: &'static str,
    value: Option<&str>,
    table: &[(&str, T)],
) -> Result<T, RegistryError> {
    match value {
        None => Ok(table[0].1),
        Some(v) => table
            .iter()
            .find(|(name, _)| *name == v)
            .map(|(_, t)| *t)
            .ok_or_else(|| RegistryError::BadField {
                slug: slug.to_string(),
                field,
                value: v.to_string(),
            }),
    }
}

impl RawProvider {
    /// The three policy fields, each defaulting to its first value (0002 A28, A29; 0012 A58).
    fn policies(&self) -> Result<(ActorModel, SyncDeltas, Deletion), RegistryError> {
        let slug = &self.slug;
        Ok((
            enum_field(
                slug,
                "actor_model",
                self.actor_model.as_deref(),
                &[
                    ("individual", ActorModel::Individual),
                    ("provider-only", ActorModel::ProviderOnly),
                ],
            )?,
            enum_field(
                slug,
                "sync_deltas",
                self.sync_deltas.as_deref(),
                &[("summary", SyncDeltas::Summary), ("full", SyncDeltas::Full)],
            )?,
            enum_field(
                slug,
                "deletion",
                self.deletion.as_deref(),
                &[
                    ("tombstone", Deletion::Tombstone),
                    ("clear", Deletion::Clear),
                ],
            )?,
        ))
    }

    /// Validates one raw entry; `None` for a pending entry.
    fn validate(self) -> Result<Option<Provider>, RegistryError> {
        let p = self;
        if p.pending {
            return Ok(None);
        }
        let (actor_model, sync_deltas, deletion) = p.policies()?;
        let slug = p.slug;
        if !is_slug(&slug) {
            return Err(RegistryError::BadSlug(slug));
        }
        let missing = |field| RegistryError::Missing {
            slug: slug.clone(),
            field,
        };
        let code = p.code.ok_or_else(|| missing("code"))?;
        if !is_code(&code) {
            return Err(RegistryError::BadCode(code));
        }
        let number = p.number.ok_or_else(|| missing("number"))?;
        if number == 0 {
            return Err(RegistryError::NumberZero(slug));
        }
        let issuer = p.issuer.unwrap_or_else(|| slug.clone());
        let grammar_err = |source| RegistryError::Grammar {
            slug: slug.clone(),
            source,
        };
        let id_grammar = match p.id_grammar {
            Some(g) => g.parse().map_err(grammar_err)?,
            None => IdGrammar::Digits,
        };
        let trust = match p.trust.as_deref() {
            None => None,
            Some("verified") => Some(Trust::Verified),
            Some("stream") => Some(Trust::Stream),
            Some(other) => {
                return Err(RegistryError::Toml(format!(
                    "provider `{slug}`: trust must be \"verified\" or \"stream\", not \"{other}\""
                )));
            }
        };
        if trust == Some(Trust::Verified) && p.keys.is_none() {
            return Err(RegistryError::VerifiedWithoutKeys(slug));
        }
        if let Some(cfg) = &p.adapter_config
            && let Some(key) = secret_like_key(cfg)
        {
            return Err(RegistryError::SecretInRegistry {
                slug,
                key: key.to_string(),
            });
        }

        let types = validate_types(&slug, p.types, id_grammar)?;
        let chip = match (p.chip_color, p.chip_background) {
            (None, None) => None,
            (Some(color), Some(background))
                if is_hex_colour(&color) && is_hex_colour(&background) =>
            {
                Some(Chip { color, background })
            }
            _ => {
                return Err(RegistryError::Toml(format!(
                    "provider `{slug}`: chip_color and chip_background are both #RRGGBB, or neither is given"
                )));
            }
        };

        Ok(Some(Provider {
            code,
            name: p.name.unwrap_or_else(|| slug.clone()),
            slug,
            number,
            issuer,
            revision_ids: p.revision_ids,
            article_path: p.article_path,
            api: p.api,
            entity_data: p.entity_data,
            adapter: p.adapter,
            actor_model,
            sync_deltas,
            deletion,
            agent_iri: p.agent_iri,
            id_grammar,
            trust,
            keys: p.keys,
            retired: p.retired,
            adapter_config: p.adapter_config,
            types,
            chip,
            source: false,
        }))
    }
}

/// Validates a provider's `[[provider.type]]` entries.
fn validate_types(
    slug: &str,
    raw: Vec<RawType>,
    id_grammar: IdGrammar,
) -> Result<Vec<EntityType>, RegistryError> {
    let mut types = Vec::with_capacity(raw.len());
    for t in raw {
        let mut chars = t.code.chars();
        let code_char = match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_uppercase() => c,
            _ => {
                return Err(RegistryError::BadTypeCode {
                    slug: slug.to_string(),
                    code: t.code,
                });
            }
        };
        if types.iter().any(|e: &EntityType| e.code == code_char) {
            return Err(RegistryError::DuplicateType {
                slug: slug.to_string(),
                code: code_char,
            });
        }
        if !t.iri.contains("{upstream_id}") {
            return Err(RegistryError::BadIriTemplate {
                slug: slug.to_string(),
                code: code_char,
                iri: t.iri,
            });
        }
        let type_grammar = match t.id_grammar {
            Some(g) => g.parse().map_err(|source| RegistryError::Grammar {
                slug: slug.to_string(),
                source,
            })?,
            None => id_grammar,
        };
        types.push(EntityType {
            code: code_char,
            entity_type: t.entity_type,
            upstream_prefix: t.upstream_prefix,
            iri: t.iri,
            namespace: t.namespace,
            label: t.label,
            id_grammar: type_grammar,
            key_mapped: t.key_mapped,
        });
    }
    Ok(types)
}

/// The first key, at any depth of an `adapter_config` table, that names a credential.
fn secret_like_key(table: &toml::Table) -> Option<&str> {
    const MARKERS: [&str; 7] = [
        "secret",
        "password",
        "passwd",
        "token",
        "api_key",
        "apikey",
        "credential",
    ];
    for (k, v) in table {
        let lower = k.to_ascii_lowercase();
        if MARKERS.iter().any(|m| lower.contains(m)) {
            return Some(k);
        }
        if let toml::Value::Table(inner) = v
            && let Some(found) = secret_like_key(inner)
        {
            return Some(found);
        }
    }
    None
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !s.starts_with('-')
        && !s.ends_with('-')
}

fn is_code(s: &str) -> bool {
    s.len() == 2 && s.bytes().all(|b| b.is_ascii_uppercase())
}

impl Registry {
    /// Parses and validates a registry file in the format of `docs/registry/providers.toml`.
    ///
    /// Entries marked `pending = true` have no code or number yet and are left out; the
    /// registry README allocates them by commit. Entries marked `retired = true` are kept,
    /// so that their codes and numbers stay reserved and old data still parses.
    pub fn parse(text: &str) -> Result<Self, RegistryError> {
        let raw: RawRegistry =
            toml::from_str(text).map_err(|e| RegistryError::Toml(e.to_string()))?;
        if raw.version != 1 {
            return Err(RegistryError::Version(raw.version));
        }

        let mut reserved_codes = BTreeSet::new();
        for code in raw.reserved_codes {
            if !is_code(&code) {
                return Err(RegistryError::BadCode(code));
            }
            reserved_codes.insert(code);
        }

        let mut providers = Vec::with_capacity(raw.providers.len());
        for p in raw.providers {
            if let Some(provider) = p.validate()? {
                // Retired entries too: a reserved code was never allocated.
                if reserved_codes.contains(&provider.code) {
                    return Err(RegistryError::ReservedCode {
                        slug: provider.slug,
                        code: provider.code,
                    });
                }
                providers.push(provider);
            }
        }

        let mut by_code = BTreeMap::new();
        let mut by_slug = BTreeMap::new();
        let mut by_number = BTreeMap::new();
        for (i, p) in providers.iter().enumerate() {
            let dup = |what, value: String, j: usize| RegistryError::Duplicate {
                a: providers[j].slug.clone(),
                b: p.slug.clone(),
                what,
                value,
            };
            if let Some(j) = by_code.insert(p.code.clone(), i) {
                return Err(dup("code", p.code.clone(), j));
            }
            if let Some(j) = by_slug.insert(p.slug.clone(), i) {
                return Err(dup("slug", p.slug.clone(), j));
            }
            if let Some(j) = by_number.insert(p.number, i) {
                return Err(dup("number", p.number.to_string(), j));
            }
        }

        Ok(Self {
            version: raw.version,
            reserved_codes,
            providers,
            by_code,
            by_slug,
            by_number,
        })
    }

    /// The embedded default registry, `docs/registry/providers.toml`, parsed once.
    ///
    /// # Panics
    ///
    /// Never in a build that passed its tests: the default file is validated by a test.
    #[must_use]
    pub fn default_registry() -> &'static Self {
        static DEFAULT: LazyLock<Registry> = LazyLock::new(|| {
            Registry::parse(DEFAULT_TOML).expect("docs/registry/providers.toml is valid")
        });
        &DEFAULT
    }

    /// The file format version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The provider codes that are never allocated (0044 §2), in sorted order.
    pub fn reserved_codes(&self) -> impl Iterator<Item = &str> {
        self.reserved_codes.iter().map(String::as_str)
    }

    /// Whether this two-letter code is reserved. Case-insensitive.
    #[must_use]
    pub fn is_reserved(&self, code: &str) -> bool {
        self.reserved_codes.contains(&code.to_ascii_uppercase())
    }

    /// Every provider, in registry order, retired ones included.
    #[must_use]
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    /// The provider with this two-letter code. Case-insensitive.
    #[must_use]
    pub fn by_code(&self, code: &str) -> Option<&Provider> {
        let code = code.to_ascii_uppercase();
        self.by_code.get(&code).map(|&i| &self.providers[i])
    }

    /// The provider with this slug.
    #[must_use]
    pub fn by_slug(&self, slug: &str) -> Option<&Provider> {
        self.by_slug.get(slug).map(|&i| &self.providers[i])
    }

    /// The provider with this number. Number 0 is the instance and has no entry.
    #[must_use]
    pub fn by_number(&self, number: u32) -> Option<&Provider> {
        self.by_number.get(&number).map(|&i| &self.providers[i])
    }

    /// A tenant's table of names (0078 §3): this registry with the tenant's entity
    /// sources added as providers ([`crate::EntitySource::to_provider`]). A source whose
    /// name the registry has, or whose number another entry has, is refused (rule 1).
    ///
    /// # Errors
    ///
    /// [`RegistryError::Toml`] naming the clash.
    pub fn with_sources(&self, sources: Vec<Provider>) -> Result<Self, RegistryError> {
        let mut out = self.clone();
        for p in sources {
            if !p.source {
                return Err(RegistryError::Toml(format!(
                    "`{}` is not an entity source",
                    p.slug
                )));
            }
            if out.by_slug.contains_key(&p.slug) {
                return Err(RegistryError::Toml(format!(
                    "`{}` is a name the registry has (ts-source-name-taken)",
                    p.slug
                )));
            }
            if out.by_number.contains_key(&p.number) {
                return Err(RegistryError::Toml(format!(
                    "source `{}`: number {} is taken",
                    p.slug, p.number
                )));
            }
            let i = out.providers.len();
            out.by_slug.insert(p.slug.clone(), i);
            out.by_number.insert(p.number, i);
            out.providers.push(p);
        }
        Ok(out)
    }

    /// The entity sources in this table, in the order they were added.
    pub fn sources(&self) -> impl Iterator<Item = &Provider> {
        self.providers.iter().filter(|p| p.source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mediawiki_providers_revision_has_a_page() {
        let r = Registry::default_registry();
        let wd = r.by_slug("wikidata").unwrap();
        assert_eq!(
            wd.revision_url("2251512453").as_deref(),
            Some("https://www.wikidata.org/w/index.php?oldid=2251512453")
        );
        assert_eq!(wd.revision_url("x1"), None);
        assert_eq!(wd.revision_url(""), None);
        // OpenAlex publishes no revision IDs.
        assert_eq!(r.by_slug("openalex").unwrap().revision_url("5"), None);
    }

    #[test]
    fn every_provider_has_a_chip_and_chips_are_checked() {
        for p in Registry::default_registry().providers() {
            assert!(p.chip.is_some(), "{} has no chip colours", p.slug);
        }
        let one = |extra: &str| {
            format!("version = 1\n[[provider]]\ncode = \"ZQ\"\nslug = \"z\"\nnumber = 99\n{extra}")
        };
        assert!(Registry::parse(&one("chip_color = \"#000000\"\n")).is_err());
        assert!(
            Registry::parse(&one(
                "chip_color = \"red\"\nchip_background = \"#FFFFFF\"\n"
            ))
            .is_err()
        );
        let r = Registry::parse(&one(
            "chip_color = \"#000000\"\nchip_background = \"#FFFFFF\"\n",
        ))
        .unwrap();
        assert_eq!(
            r.providers()[0].chip.as_ref().unwrap().background,
            "#FFFFFF"
        );
    }

    #[test]
    fn default_registry_parses_and_is_consistent() {
        let r = Registry::default_registry();
        assert_eq!(r.version(), 1);
        // The allocations the ADRs record (0000 §3, 0015 §5, 0017 §6, 0036, 0037).
        for (code, slug, number) in [
            ("WD", "wikidata", 1),
            ("LB", "librarybase", 2),
            ("OA", "openalex", 3),
            ("MB", "musicbrainz", 4),
            ("XD", "internetdomains", 5),
            ("OS", "openstreetmap", 6),
            ("OW", "osmwiki", 7),
            ("GD", "gdelt", 8),
        ] {
            let p = r
                .by_code(code)
                .unwrap_or_else(|| panic!("{code} registered"));
            assert_eq!(p.slug, slug);
            assert_eq!(p.number, number);
            assert!(std::ptr::eq(r.by_slug(slug).unwrap(), p));
            assert!(std::ptr::eq(r.by_number(number).unwrap(), p));
            assert!(!p.types.is_empty(), "{code} mints something");
        }
        assert!(r.by_number(0).is_none());
    }

    #[test]
    fn default_registry_reserves_every_doubled_code() {
        // 0044 §2: `QQQ5` is the tenant-relative form, so no doubled letter is a provider code.
        let r = Registry::default_registry();
        let doubled: Vec<String> = ('A'..='Z').map(|c| format!("{c}{c}")).collect();
        assert_eq!(r.reserved_codes().collect::<Vec<_>>(), doubled);
        assert!(r.is_reserved("qq"));
        assert!(!r.is_reserved("WD"));
        assert!(r.providers().iter().all(|p| !r.is_reserved(&p.code)));
    }

    #[test]
    fn grammars_and_key_mapping_from_the_default_file() {
        let r = Registry::default_registry();
        let mb = r.by_code("MB").unwrap();
        assert_eq!(mb.id_grammar, IdGrammar::Uuid);
        assert!(mb.types.iter().all(|t| t.id_grammar == IdGrammar::Uuid));
        let gd = r.by_code("GD").unwrap();
        assert_eq!(gd.entity_type('E').unwrap().id_grammar, IdGrammar::Digits);
        assert_eq!(
            gd.entity_type('D').unwrap().id_grammar,
            IdGrammar::GdeltRecord
        );
        assert_eq!(gd.entity_type('T').unwrap().id_grammar, IdGrammar::Token);
        let oa = r.by_code("OA").unwrap();
        assert!(oa.entity_type('K').unwrap().key_mapped);
        assert!(!oa.entity_type('W').unwrap().key_mapped);
        let xd = r.by_code("XD").unwrap();
        let cfg = xd.adapter_config.as_ref().expect("XD has adapter_config");
        let rules = cfg
            .get("key_map")
            .and_then(|v| v.as_array())
            .expect("XD's key map is a rule list (0091 §2)");
        assert_eq!(rules.len(), 1);
        assert_eq!(
            rules[0].get("identity_property").and_then(|v| v.as_str()),
            Some("P1")
        );
        assert_eq!(xd.mirror_graph(), "mirror/internetdomains");
    }

    fn one(extra: &str) -> String {
        format!(
            "version = 1\n[[provider]]\ncode = \"ZZ\"\nslug = \"zed\"\nnumber = 99\n{extra}\n[[provider.type]]\ncode = \"Q\"\nentity_type = \"item\"\nupstream_prefix = \"Q\"\niri = \"https://zed.example/entity/{{upstream_id}}\"\n"
        )
    }

    #[test]
    fn rejects_bad_entries() {
        assert!(matches!(
            Registry::parse(&one("").replace("code = \"ZZ\"", "code = \"zz\"")),
            Err(RegistryError::BadCode(_))
        ));
        assert!(matches!(
            Registry::parse(&one("").replace("number = 99", "number = 0")),
            Err(RegistryError::NumberZero(_))
        ));
        assert!(matches!(
            Registry::parse(&one("").replace("{upstream_id}", "{id}")),
            Err(RegistryError::BadIriTemplate { .. })
        ));
        assert!(matches!(
            Registry::parse(&one("id_grammar = \"slug\"")),
            Err(RegistryError::Grammar { .. })
        ));
        assert!(matches!(
            Registry::parse(&one("trust = \"verified\"")),
            Err(RegistryError::VerifiedWithoutKeys(_))
        ));
        assert!(matches!(
            Registry::parse(&one("unknown_field = 1")),
            Err(RegistryError::Toml(_))
        ));
        assert!(matches!(
            Registry::parse("version = 2\n"),
            Err(RegistryError::Version(2))
        ));
    }

    #[test]
    fn rejects_credentials_in_adapter_config() {
        assert!(
            Registry::parse(&one(
                "[provider.adapter_config]\nidentity_property = \"P1\""
            ))
            .is_ok()
        );
        for bad in [
            "api_key = \"x\"",
            "ApiKey = \"x\"",
            "client_secret = \"x\"",
            "[provider.adapter_config.oauth]\naccess_token = \"x\"",
        ] {
            assert!(
                matches!(
                    Registry::parse(&one(&format!("[provider.adapter_config]\n{bad}"))),
                    Err(RegistryError::SecretInRegistry { .. })
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn rejects_reserved_codes() {
        let reserving = |codes: &str| {
            one("").replace(
                "version = 1\n",
                &format!("version = 1\nreserved_codes = [{codes}]\n"),
            )
        };
        assert!(Registry::parse(&reserving("\"QQ\"")).is_ok());
        assert!(matches!(
            Registry::parse(&reserving("\"QQ\", \"ZZ\"")),
            Err(RegistryError::ReservedCode { ref code, .. }) if code == "ZZ"
        ));
        assert!(matches!(
            Registry::parse(
                &reserving("\"ZZ\"").replace("number = 99", "number = 99\nretired = true")
            ),
            Err(RegistryError::ReservedCode { .. })
        ));
        assert!(matches!(
            Registry::parse(&reserving("\"qq\"")),
            Err(RegistryError::BadCode(_))
        ));
    }

    #[test]
    fn rejects_duplicates() {
        let two = one("")
            + &one("")
                .replace("version = 1\n", "")
                .replace("zed", "zed2")
                .replace("99", "98");
        assert!(matches!(
            Registry::parse(&two),
            Err(RegistryError::Duplicate { what: "code", .. })
        ));
    }

    #[test]
    fn pending_entries_are_skipped_and_defaults_apply() {
        let text = "version = 1\n[[provider]]\nslug = \"later\"\npending = true\n".to_string()
            + &one("").replace("version = 1\n", "");
        let r = Registry::parse(&text).unwrap();
        assert!(r.by_slug("later").is_none());
        let z = r.by_slug("zed").unwrap();
        assert_eq!(z.issuer, "zed", "issuer defaults to the slug");
        assert_eq!(z.name, "zed");
        assert_eq!(z.id_grammar, IdGrammar::Digits);
        assert!(!z.revision_ids);
    }

    #[test]
    fn actor_model_sync_deltas_and_deletion() {
        let r = Registry::default_registry();
        let osm = r.by_slug("openstreetmap").expect("OS");
        assert_eq!(osm.actor_model, ActorModel::Individual);
        assert_eq!(osm.deletion, Deletion::Clear);
        assert_eq!(osm.sync_deltas, SyncDeltas::Summary);
        let oa = r.by_slug("openalex").expect("OA");
        assert_eq!(oa.actor_model, ActorModel::ProviderOnly);
        assert_eq!(oa.deletion, Deletion::Tombstone);
        let err = Registry::parse(&one("deletion = \"purge\"")).expect_err("bad value");
        assert!(
            matches!(
                err,
                RegistryError::BadField {
                    field: "deletion",
                    ..
                }
            ),
            "{err}"
        );
    }
}
