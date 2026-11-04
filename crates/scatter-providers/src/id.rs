//! Foreign entity IDs and the rewriting between prefixed and upstream forms
//! (ADR 0002 §4, 0017 §1–2).
//!
//! A foreign ID is a two-letter provider code, a one-letter type code and the rest in the
//! type's grammar: `WDQ42`, `OAW123`, `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d`. Input is
//! case-insensitive; the canonical form uppercases the prefix and applies the grammar's
//! canonical form to the rest. Local IDs (`Q5`) and keyed IDs (`domain:example.org`) are not
//! foreign IDs and are rejected here; the full three-form parser lives in
//! `scatter-wikibase-model`, which calls this for the foreign form.

use std::fmt;

use oxrdf::NamedNode;

use crate::grammar::IdGrammar;
use crate::registry::{EntityType, Provider, Registry};

/// A parsed, canonical foreign entity ID, borrowing its provider and type from the registry.
#[derive(Debug, Clone, PartialEq)]
pub struct ForeignId<'r> {
    provider: &'r Provider,
    entity_type: &'r EntityType,
    rest: String,
}

impl<'r> ForeignId<'r> {
    /// The provider that minted the ID.
    #[must_use]
    pub fn provider(&self) -> &'r Provider {
        self.provider
    }

    /// The entity type within the provider.
    #[must_use]
    pub fn entity_type(&self) -> &'r EntityType {
        self.entity_type
    }

    /// The part after the three-letter prefix, in canonical form.
    #[must_use]
    pub fn rest(&self) -> &str {
        &self.rest
    }

    /// The three-letter prefix, `WDQ`; for a source, `{name}:{upstream prefix}`.
    #[must_use]
    pub fn prefix(&self) -> String {
        if self.provider.source {
            format!(
                "{}:{}",
                self.provider.slug, self.entity_type.upstream_prefix
            )
        } else {
            format!("{}{}", self.provider.code, self.entity_type.code)
        }
    }

    /// The provider's own form of the ID: `WDQ42` → `Q42`, `OAK…` → `keywords/…`,
    /// `OSN123` → `123` (0017 §2: `upstream_prefix` is honoured).
    #[must_use]
    pub fn upstream_id(&self) -> String {
        format!("{}{}", self.entity_type.upstream_prefix, self.rest)
    }

    /// The canonical concept IRI, from the type's template (0002 §4).
    ///
    /// # Panics
    ///
    /// If the registry's template does not produce an IRI, which registry validation
    /// prevents for the templates it can check.
    #[must_use]
    pub fn concept_iri(&self) -> NamedNode {
        NamedNode::new(self.entity_type.iri_for(&self.upstream_id()))
            .expect("registry IRI templates produce IRIs")
    }
}

impl fmt::Display for ForeignId<'_> {
    /// The canonical prefixed form, `WDQ42`; for a source, `mhc:Q1` (0078 §2).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix(), self.rest)
    }
}

/// Why a string is not a foreign ID of a registered provider.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdError {
    /// Not the shape of a foreign ID: shorter than four characters, a colon (a keyed ID),
    /// or a prefix that is not three ASCII letters. A local ID such as `Q5` lands here.
    #[error("`{0}` is not a foreign entity ID")]
    NotForeign(String),
    /// The two-letter code names no registered provider.
    #[error("`{0}` names no registered provider")]
    UnknownProvider(String),
    /// The provider mints no type with that one-letter code.
    #[error("provider `{provider}` mints no type `{code}`")]
    UnknownType {
        /// The provider code.
        provider: String,
        /// The type code.
        code: char,
    },
    /// The type is key-mapped (0009 §9): its entities are written under a keyed type's
    /// keys, and no ID with this prefix is ever minted.
    #[error("`{prefix}` is a key-mapped type; its entities have keyed IDs, not prefixed ones")]
    KeyMapped {
        /// The three-letter prefix.
        prefix: String,
    },
    /// The rest of the ID is not valid under the type's grammar.
    #[error("`{rest}` is not a valid `{grammar}` ID for `{prefix}`")]
    Grammar {
        /// The three-letter prefix.
        prefix: String,
        /// The type's grammar.
        grammar: IdGrammar,
        /// The offending rest.
        rest: String,
    },
    /// An upstream ID matches no type of the provider, or, with an empty upstream prefix
    /// on several types, cannot be told apart without the type.
    #[error("`{upstream_id}` matches {matches} types of provider `{provider}`; name the type")]
    Ambiguous {
        /// The provider code.
        provider: String,
        /// The provider's own ID.
        upstream_id: String,
        /// How many types matched (0 or more than 1).
        matches: usize,
    },
}

impl Registry {
    /// Parses a foreign ID in prefixed form, case-insensitively, into its canonical form.
    ///
    /// ```
    /// use scatter_providers::Registry;
    /// let r = Registry::default_registry();
    /// let id = r.parse_foreign_id("wdq42").unwrap();
    /// assert_eq!(id.to_string(), "WDQ42");
    /// assert_eq!(id.upstream_id(), "Q42");
    /// assert_eq!(id.concept_iri().as_str(), "http://www.wikidata.org/entity/Q42");
    /// assert!(r.parse_foreign_id("Q5").is_err());
    /// assert!(r.parse_foreign_id("domain:example.org").is_err());
    /// ```
    pub fn parse_foreign_id<'r>(&'r self, id: &str) -> Result<ForeignId<'r>, IdError> {
        if let Some((name, rest)) = id.split_once(':') {
            return self.parse_source_id(name, rest, id);
        }
        let b = id.as_bytes();
        if b.len() < 4 || !b[..3].iter().all(u8::is_ascii_alphabetic) {
            return Err(IdError::NotForeign(id.to_string()));
        }
        let code = id[..2].to_ascii_uppercase();
        let type_code = b[2].to_ascii_uppercase() as char;
        let provider = self
            .by_code(&code)
            .ok_or_else(|| IdError::UnknownProvider(code.clone()))?;
        let entity_type = provider
            .entity_type(type_code)
            .ok_or_else(|| IdError::UnknownType {
                provider: code.clone(),
                code: type_code,
            })?;
        let prefix = format!("{code}{type_code}");
        if entity_type.key_mapped {
            return Err(IdError::KeyMapped { prefix });
        }
        let rest = &id[3..];
        let rest = entity_type
            .canonical_rest(rest)
            .ok_or_else(|| IdError::Grammar {
                prefix,
                grammar: entity_type.id_grammar,
                rest: rest.to_string(),
            })?;
        Ok(ForeignId {
            provider,
            entity_type,
            rest,
        })
    }

    /// A source ID, `{name}:{upstream ID}` (0078 §2): the name must be an entity source
    /// of this table, and the rest the upstream prefix of one of its types followed by a
    /// string in that type's grammar. Any other colon form is not a foreign ID here.
    fn parse_source_id<'r>(
        &'r self,
        name: &str,
        rest: &str,
        id: &str,
    ) -> Result<ForeignId<'r>, IdError> {
        let name = name.to_ascii_lowercase();
        let Some(provider) = self.by_slug(&name).filter(|p| p.source) else {
            return Err(IdError::NotForeign(id.to_string()));
        };
        let entity_type = provider
            .type_of_upstream(rest)
            .ok_or_else(|| IdError::Ambiguous {
                provider: name.clone(),
                upstream_id: rest.to_string(),
                matches: 0,
            })?;
        let prefix = format!("{name}:{}", entity_type.upstream_prefix);
        let tail = &rest[entity_type.upstream_prefix.len()..];
        let tail = entity_type
            .canonical_rest(tail)
            .ok_or_else(|| IdError::Grammar {
                prefix,
                grammar: entity_type.id_grammar,
                rest: tail.to_string(),
            })?;
        Ok(ForeignId {
            provider,
            entity_type,
            rest: tail,
        })
    }

    /// Rewrites a provider's own ID to the prefixed form, given the type: `Q42` of
    /// Wikidata's `Q` type → `WDQ42`. This is the adapter's direction (0002 §8.4).
    ///
    /// ```
    /// use scatter_providers::Registry;
    /// let r = Registry::default_registry();
    /// let lb = r.by_slug("librarybase").unwrap();
    /// assert_eq!(r.from_upstream_typed(lb, 'P', "P12").unwrap().to_string(), "LBP12");
    /// ```
    pub fn from_upstream_typed<'r>(
        &'r self,
        provider: &'r Provider,
        type_code: char,
        upstream_id: &str,
    ) -> Result<ForeignId<'r>, IdError> {
        let entity_type = provider
            .entity_type(type_code)
            .ok_or_else(|| IdError::UnknownType {
                provider: provider.code.clone(),
                code: type_code.to_ascii_uppercase(),
            })?;
        let prefix = format!("{}{}", provider.code, entity_type.code);
        if entity_type.key_mapped {
            return Err(IdError::KeyMapped { prefix });
        }
        let rest = upstream_id
            .strip_prefix(entity_type.upstream_prefix.as_str())
            .and_then(|rest| entity_type.canonical_rest(rest))
            .ok_or_else(|| IdError::Grammar {
                prefix,
                grammar: entity_type.id_grammar,
                rest: upstream_id.to_string(),
            })?;
        Ok(ForeignId {
            provider,
            entity_type,
            rest,
        })
    }

    /// Rewrites a provider's own ID to the prefixed form, choosing the type by its upstream
    /// prefix and grammar. Works where the provider's prefixes tell its types apart
    /// (Wikidata's `Q`/`P`/`L`, OpenAlex's `W`/`A`/…); where they do not (MusicBrainz and
    /// OpenStreetMap, whose IDs are bare), it fails with [`IdError::Ambiguous`] and the
    /// caller uses [`Registry::from_upstream_typed`].
    ///
    /// ```
    /// use scatter_providers::Registry;
    /// let r = Registry::default_registry();
    /// let wd = r.by_code("WD").unwrap();
    /// assert_eq!(r.from_upstream(wd, "P31").unwrap().to_string(), "WDP31");
    /// ```
    pub fn from_upstream<'r>(
        &'r self,
        provider: &'r Provider,
        upstream_id: &str,
    ) -> Result<ForeignId<'r>, IdError> {
        let mut found: Vec<ForeignId<'r>> = Vec::new();
        for t in provider.types.iter().filter(|t| !t.key_mapped) {
            if let Ok(id) = self.from_upstream_typed(provider, t.code, upstream_id) {
                found.push(id);
            }
        }
        // Prefer the longest upstream prefix, so `keywords/…` never falls to a bare type.
        found.sort_by_key(|id| std::cmp::Reverse(id.entity_type.upstream_prefix.len()));
        match found.len() {
            1 => Ok(found.pop().expect("one")),
            _ if found.len() > 1
                && found[0].entity_type.upstream_prefix.len()
                    > found[1].entity_type.upstream_prefix.len() =>
            {
                Ok(found.swap_remove(0))
            }
            n => Err(IdError::Ambiguous {
                provider: provider.code.clone(),
                upstream_id: upstream_id.to_string(),
                matches: n,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg() -> &'static Registry {
        Registry::default_registry()
    }

    /// A form or sense of a lexeme (0066 §4): the suffix after the lexeme type's grammar,
    /// in both directions of the rewrite, and on no other type.
    #[test]
    fn lexeme_parts() {
        let wd = reg().by_code("WD").unwrap();
        let f = reg().from_upstream(wd, "L252247-F2").unwrap();
        assert_eq!(f.to_string(), "WDL252247-F2");
        assert_eq!(f.upstream_id(), "L252247-F2");
        assert_eq!(f.entity_type().entity_type, "lexeme");
        assert_eq!(
            f.concept_iri().as_str(),
            "http://www.wikidata.org/entity/L252247-F2"
        );
        let s = reg().parse_foreign_id("wdl470-s1").unwrap();
        assert_eq!((s.to_string(), s.rest()), ("WDL470-S1".into(), "470-S1"));
        assert_eq!(
            reg()
                .from_upstream_typed(wd, 'L', "L1-S3")
                .unwrap()
                .to_string(),
            "WDL1-S3"
        );
        for bad in [
            "WDQ1-F1",
            "WDP1-S1",
            "WDL1-X1",
            "WDL1-F",
            "WDL-F1",
            "WDL1-F1-S1",
        ] {
            assert!(reg().parse_foreign_id(bad).is_err(), "{bad}");
        }
        assert!(reg().from_upstream(wd, "Q1-F1").is_err());
    }

    #[test]
    fn canonical_forms() {
        let id = reg().parse_foreign_id("wDq42").unwrap();
        assert_eq!(id.to_string(), "WDQ42");
        assert_eq!(id.prefix(), "WDQ");
        assert_eq!(id.rest(), "42");
        assert_eq!(id.provider().slug, "wikidata");
        assert_eq!(id.entity_type().entity_type, "item");

        let mb = reg()
            .parse_foreign_id("mbab10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D")
            .unwrap();
        assert_eq!(mb.to_string(), "MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d");
        assert_eq!(mb.upstream_id(), "b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d");
        assert_eq!(
            mb.concept_iri().as_str(),
            "https://musicbrainz.org/artist/b10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
        );

        let gd = reg().parse_foreign_id("gdd20150218230000-t12").unwrap();
        assert_eq!(gd.to_string(), "GDD20150218230000-T12");
        let cameo = reg().parse_foreign_id("GDC0311").unwrap();
        assert_eq!(
            cameo.to_string(),
            "GDC0311",
            "leading zeros are significant"
        );
        assert_eq!(
            cameo.concept_iri().as_str(),
            "https://scatter.red/gdelt/cameo/0311"
        );
        let theme = reg().parse_foreign_id("GDTtax_fncact_mayor").unwrap();
        assert_eq!(theme.to_string(), "GDTTAX_FNCACT_MAYOR");
        assert_eq!(
            theme.concept_iri().as_str(),
            "https://scatter.red/gdelt/theme/TAX_FNCACT_MAYOR"
        );
    }

    #[test]
    fn xd_and_lb() {
        let xd = reg().parse_foreign_id("XDQ7").unwrap();
        assert_eq!(xd.upstream_id(), "Q7");
        assert_eq!(
            xd.concept_iri().as_str(),
            "https://internetdomains.wiki/entity/Q7"
        );
        let lbp = reg().parse_foreign_id("LBP12").unwrap();
        assert_eq!(
            lbp.concept_iri().as_str(),
            "https://librarybase.org/entity/P12"
        );
    }

    #[test]
    fn rejections() {
        let r = reg();
        assert!(matches!(
            r.parse_foreign_id("Q5"),
            Err(IdError::NotForeign(_))
        ));
        assert!(matches!(
            r.parse_foreign_id("P31"),
            Err(IdError::NotForeign(_))
        ));
        assert!(matches!(
            r.parse_foreign_id("domain:en.wikipedia.org"),
            Err(IdError::NotForeign(_))
        ));
        assert!(matches!(
            r.parse_foreign_id("WDQ"),
            Err(IdError::NotForeign(_))
        ));
        assert!(matches!(
            r.parse_foreign_id("ZZQ1"),
            Err(IdError::UnknownProvider(c)) if c == "ZZ"
        ));
        assert!(matches!(
            r.parse_foreign_id("WDX1"),
            Err(IdError::UnknownType { code: 'X', .. })
        ));
        assert!(matches!(
            r.parse_foreign_id("OAKmachine-learning"),
            Err(IdError::KeyMapped { prefix }) if prefix == "OAK"
        ));
        assert!(matches!(
            r.parse_foreign_id("WDQ4a"),
            Err(IdError::Grammar {
                grammar: IdGrammar::Digits,
                ..
            })
        ));
        assert!(matches!(
            r.parse_foreign_id("MBA42"),
            Err(IdError::Grammar {
                grammar: IdGrammar::Uuid,
                ..
            })
        ));
    }

    #[test]
    fn upstream_rewriting() {
        let r = reg();
        let wd = r.by_code("WD").unwrap();
        assert_eq!(r.from_upstream(wd, "Q42").unwrap().to_string(), "WDQ42");
        assert_eq!(r.from_upstream(wd, "L3").unwrap().to_string(), "WDL3");
        assert!(matches!(
            r.from_upstream(wd, "X42"),
            Err(IdError::Ambiguous { matches: 0, .. })
        ));

        let oa = r.by_code("OA").unwrap();
        assert_eq!(r.from_upstream(oa, "W123").unwrap().to_string(), "OAW123");
        assert!(matches!(
            r.from_upstream(oa, "keywords/machine-learning"),
            Err(IdError::Ambiguous { matches: 0, .. })
        ));
        assert!(matches!(
            r.from_upstream_typed(oa, 'K', "keywords/x"),
            Err(IdError::KeyMapped { .. })
        ));

        // Bare IDs need the type.
        let os = r.by_code("OS").unwrap();
        assert!(matches!(
            r.from_upstream(os, "123"),
            Err(IdError::Ambiguous { matches: 3, .. })
        ));
        let node = r.from_upstream_typed(os, 'n', "123").unwrap();
        assert_eq!(node.to_string(), "OSN123");
        assert_eq!(node.upstream_id(), "123");

        let mb = r.by_code("MB").unwrap();
        let rec = r
            .from_upstream_typed(mb, 'C', "B10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D")
            .unwrap();
        assert_eq!(rec.to_string(), "MBCb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d");
    }

    #[test]
    fn round_trip_through_upstream_form() {
        let r = reg();
        for s in ["WDQ42", "LBP12", "OAW123", "XDQ7", "GDE1234567890", "OWQ3"] {
            let id = r.parse_foreign_id(s).unwrap();
            let back = r
                .from_upstream_typed(id.provider(), id.entity_type().code, &id.upstream_id())
                .unwrap();
            assert_eq!(back, id);
            assert_eq!(back.to_string(), s);
        }
    }
}
