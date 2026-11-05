//! Entity and statement IDs (ADR 0017 §1–3).
//!
//! Every entity ID has one of three forms, told apart by its first character and by the
//! presence of a colon:
//!
//! | Form | Shape | Examples |
//! |---|---|---|
//! | Local | One uppercase letter and digits | `Q5`, `P12`, `L3` |
//! | Foreign | Two-letter provider code, one-letter type code, and the rest in the type's grammar | `WDQ42`, `MBAb10bbbfc-…` |
//! | Keyed | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org` |
//!
//! On `L` only, a local or foreign ID may end in a lexeme part suffix, `-F` or `-S` and
//! digits, naming a form or a sense of the lexeme (0066 §4): `L3-F1`, `WDL3-S2`, and in the
//! input form `LLL3-F1`. The part letter is canonical uppercase. A part is an entity value
//! and a statement subject, never an entity of its own ([`EntityId::lexeme_part`]).
//!
//! No minted ID contains a colon and every keyed ID does. This module classifies and
//! canonicalizes IDs by their *shape*; whether a foreign prefix or a keyed prefix is
//! registered, and whether the rest is valid under its grammar, is checked against the
//! registries by [`EntityId::validate`], which the adapters and the write path call.
//! The model itself stays registry-free so that it can hold IDs from any source in the
//! form that source wrote them.
//!
//! One more shape is accepted on input only: the **tenant-relative** form of 0044 §1, a
//! type letter written three times and then digits (`QQQ5`). It is the local ID `Q5` of
//! the tenant reading it, canonicalized here so that it is never stored or returned.
//! Local `M` IDs are derived, not minted: `M{page ID}` names the MediaInfo of a File page
//! (0041 §7), and a page's own statements carry the page ID in decimal as their subject
//! (0038 §1), which [`StatementId`] tells apart from an entity ID by its leading digit.

use std::fmt;

use scatter_normalize::KeyedRegistry;
use scatter_providers::Registry as ProviderRegistry;
use serde::{Deserialize, Serialize};

/// Which of the three forms an ID has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdForm {
    /// Minted by this instance (`Q5`).
    Local,
    /// Minted by a provider (`WDQ42`).
    Foreign,
    /// The thing itself (`domain:example.org`).
    Keyed,
}

/// An entity ID in canonical form.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(String);

/// Why a string is not an entity ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IdParseError {
    /// Empty, or not one of the three shapes.
    #[error("`{0}` is not an entity ID")]
    Shape(String),
    /// A local ID with something other than digits after its letter.
    #[error("`{0}` is not a local entity ID: one uppercase letter then digits")]
    Local(String),
    /// A foreign ID the provider registry rejects.
    #[error("{0}")]
    Foreign(#[from] scatter_providers::IdError),
    /// A keyed ID the keyed-type registry rejects.
    #[error("{0}")]
    Keyed(#[from] scatter_normalize::KeyError),
    /// A statement ID without `$`, or whose entity part is not an ID, or whose GUID part
    /// is not 36 hex-and-hyphen characters.
    #[error("`{0}` is not a statement ID (`<EntityId>$<UUID>`)")]
    Statement(String),
}

/// Which part of a lexeme a lexeme part ID names (0066 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LexemePart {
    /// A form, `L1-F1`.
    Form,
    /// A sense, `L1-S1`.
    Sense,
}

impl LexemePart {
    /// The Wikibase `entity-type` of the part's ID in an entity value: `form` or `sense`.
    #[must_use]
    pub const fn entity_type(self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::Sense => "sense",
        }
    }
}

/// The canonical rest of a local ID after its type letter: digits, and on `L` only a
/// lexeme part suffix, `-F` or `-S` and digits, with the part letter uppercased (0066 §4).
/// `None` if `rest` is neither.
fn local_rest(letter: u8, rest: &str) -> Option<String> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let Some((number, part)) = rest.split_once('-') else {
        return digits(rest).then(|| rest.to_string());
    };
    if letter != b'L' || !digits(number) {
        return None;
    }
    let kind = part.as_bytes().first()?.to_ascii_uppercase();
    ((kind == b'F' || kind == b'S') && digits(&part[1..]))
        .then(|| format!("{number}-{}{}", kind as char, &part[1..]))
}

impl EntityId {
    /// Parses an ID by shape, canonicalizing case: the prefix of a minted ID is uppercased
    /// and a keyed ID's type name is lowercased. The rest is kept as given, because its
    /// canonical form depends on the registries ([`EntityId::validate`]).
    ///
    /// The tenant-relative form `QQQ5` (0044 §1) parses as the local ID `Q5`. It has the
    /// shape of a foreign ID under the reserved doubled code `QQ`, so no registered
    /// provider can collide with it; its rest must be digits, as every local grammar is.
    ///
    /// ```
    /// use scatter_wikibase_model::{EntityId, IdForm};
    /// assert_eq!(EntityId::parse("q5").unwrap().as_str(), "Q5");
    /// assert_eq!(EntityId::parse("wdq42").unwrap().form(), IdForm::Foreign);
    /// assert_eq!(EntityId::parse("Domain:example.org").unwrap().as_str(), "domain:example.org");
    /// assert_eq!(EntityId::parse("qqq5").unwrap().as_str(), "Q5");
    /// assert!(EntityId::parse("5").is_err());
    /// assert!(EntityId::parse("Q").is_err());
    /// ```
    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        if let Some((prefix, key)) = s.split_once(':') {
            if prefix.is_empty()
                || key.is_empty()
                || !prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            {
                return Err(IdParseError::Shape(s.to_string()));
            }
            return Ok(Self(format!("{}:{key}", prefix.to_ascii_lowercase())));
        }
        let b = s.as_bytes();
        if b.len() < 2 || !b[0].is_ascii_alphabetic() {
            return Err(IdParseError::Shape(s.to_string()));
        }
        if b.len() >= 4 && b[1].is_ascii_alphabetic() && b[2].is_ascii_alphabetic() {
            let letter = b[0].to_ascii_uppercase();
            if b[1].to_ascii_uppercase() == letter && b[2].to_ascii_uppercase() == letter {
                // Tenant-relative (0044 §1): `QQQ5` is this tenant's `Q5`. The rest must be
                // a local ID's rest (0044 §2): `QQQb10…` is not an ID, `LLL3-F1` is
                // `L3-F1` (0066 §4).
                let rest = local_rest(letter, &s[3..])
                    .ok_or_else(|| IdParseError::Local(s.to_string()))?;
                let mut out = String::with_capacity(rest.len() + 1);
                out.push(letter as char);
                out.push_str(&rest);
                return Ok(Self(out));
            }
            // Foreign: three letters, then the rest in the type's grammar.
            let mut out = s[..3].to_ascii_uppercase();
            out.push_str(&s[3..]);
            return Ok(Self(out));
        }
        let letter = b[0].to_ascii_uppercase();
        let rest = local_rest(letter, &s[1..]).ok_or_else(|| IdParseError::Local(s.to_string()))?;
        let mut out = String::with_capacity(s.len());
        out.push(letter as char);
        out.push_str(&rest);
        Ok(Self(out))
    }

    /// Checks the ID against the registries and returns it in full canonical form: a
    /// foreign ID's rest canonicalized by its type's grammar, a keyed ID's key normalized.
    /// A local ID has nothing to check beyond its shape.
    pub fn validate(
        &self,
        providers: &ProviderRegistry,
        keyed: &KeyedRegistry,
    ) -> Result<Self, IdParseError> {
        match self.form() {
            IdForm::Local => Ok(self.clone()),
            IdForm::Foreign => Ok(Self(providers.parse_foreign_id(&self.0)?.to_string())),
            IdForm::Keyed => Ok(Self(keyed.parse_id(&self.0)?.to_string())),
        }
    }

    /// The ID as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Which form the ID has.
    #[must_use]
    pub fn form(&self) -> IdForm {
        if self.0.contains(':') {
            IdForm::Keyed
        } else if self.0.len() >= 4 && self.0.as_bytes()[1].is_ascii_alphabetic() {
            IdForm::Foreign
        } else {
            IdForm::Local
        }
    }

    /// The numeric part, for the `numeric-id` Wikibase emits beside `id`: the digits of a
    /// local ID, or of a foreign ID whose rest is digits. `None` for keyed IDs and for
    /// foreign IDs under another grammar (0009 §3, 0017 §2).
    #[must_use]
    pub fn numeric_id(&self) -> Option<u64> {
        let rest = match self.form() {
            IdForm::Local => &self.0[1..],
            IdForm::Foreign => &self.0[3..],
            IdForm::Keyed => return None,
        };
        rest.parse().ok()
    }

    /// For a local ID, its type letter (`Q`, `P`, `L`). For a foreign ID, its type code.
    #[must_use]
    pub fn type_code(&self) -> Option<char> {
        match self.form() {
            IdForm::Local => self.0.chars().next(),
            IdForm::Foreign => self.0.chars().nth(2),
            IdForm::Keyed => None,
        }
    }

    /// For a keyed ID, its type name and key.
    #[must_use]
    pub fn keyed_parts(&self) -> Option<(&str, &str)> {
        self.0.split_once(':')
    }

    /// The Wikibase `entity-type` this ID's own shape implies, where it can: `item` for a
    /// local or foreign `Q`, `property` for `P`, `lexeme` for `L`, `mediainfo` for a local
    /// `M` (0041 §7), the type name for a keyed ID. Other foreign type codes need the
    /// provider registry: a foreign `M` is whatever that provider's type table says.
    #[must_use]
    pub fn implied_entity_type(&self) -> Option<&str> {
        if let Some((name, _)) = self.keyed_parts() {
            return Some(name);
        }
        match (self.form(), self.type_code()?) {
            (_, 'Q') => Some("item"),
            (_, 'P') => Some("property"),
            (_, 'L') => Some(
                self.lexeme_part()
                    .map_or("lexeme", |(_, part)| part.entity_type()),
            ),
            (IdForm::Local, 'M') => Some("mediainfo"),
            _ => None,
        }
    }

    /// For the ID of a lexeme's form or sense (`L1-F1`, `WDL1-S2`; 0066 §4), the lexeme's
    /// ID and which part it is. Parts are not entities of their own: they belong to the
    /// lexeme's record stream, and `L1-F1` and `L1` are different IDs of it.
    ///
    /// ```
    /// use scatter_wikibase_model::{EntityId, LexemePart};
    /// let f = EntityId::parse("l1-f2").unwrap();
    /// assert_eq!(f.as_str(), "L1-F2");
    /// let (lexeme, part) = f.lexeme_part().unwrap();
    /// assert_eq!((lexeme.as_str(), part), ("L1", LexemePart::Form));
    /// assert_eq!(f.implied_entity_type(), Some("form"));
    /// assert_eq!(f.numeric_id(), None);
    /// assert!(EntityId::parse("L1").unwrap().lexeme_part().is_none());
    /// assert!(EntityId::parse("Q1-F1").is_err());
    /// ```
    #[must_use]
    pub fn lexeme_part(&self) -> Option<(Self, LexemePart)> {
        if self.form() == IdForm::Keyed || self.type_code() != Some('L') {
            return None;
        }
        let (lexeme, part) = self.0.rsplit_once('-')?;
        let kind = match part.as_bytes().first()?.to_ascii_uppercase() {
            b'F' => LexemePart::Form,
            b'S' => LexemePart::Sense,
            _ => return None,
        };
        Some((Self(lexeme.to_string()), kind))
    }

    /// Whether this is a derived local ID, one not minted from a sequence of its own but
    /// named after something else: today only `M{page ID}`, the MediaInfo ID of a File page
    /// (0041 §7, 0017 §1).
    #[must_use]
    pub fn is_derived(&self) -> bool {
        self.form() == IdForm::Local && self.type_code() == Some('M')
    }

    /// For a derived `M` ID, the page ID it is derived from.
    #[must_use]
    pub fn derived_page_id(&self) -> Option<u64> {
        if self.is_derived() {
            self.numeric_id()
        } else {
            None
        }
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EntityId({})", self.0)
    }
}

impl Serialize for EntityId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EntityId {
    /// Parses by shape, so an ID is canonical the moment it is in the model.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for EntityId {
    type Err = IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// What a statement is about: an entity, or a page by its page ID (0038 §1).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Subject {
    /// An entity, by its ID.
    Entity(EntityId),
    /// A page, by its page ID. Written in decimal where a subject has to be a string.
    Page(u64),
}

impl Subject {
    /// Parses a subject as it is written in a statement ID: digits alone are a page ID,
    /// anything else must be an entity ID. The two cannot be confused because every
    /// entity ID starts with a letter (0017 §1).
    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
            return s
                .parse()
                .map(Self::Page)
                .map_err(|_| IdParseError::Shape(s.to_string()));
        }
        EntityId::parse(s).map(Self::Entity)
    }

    /// The entity, if the subject is one.
    #[must_use]
    pub fn entity_id(&self) -> Option<&EntityId> {
        match self {
            Self::Entity(e) => Some(e),
            Self::Page(_) => None,
        }
    }

    /// The page ID, if the subject is a page.
    #[must_use]
    pub fn page_id(&self) -> Option<u64> {
        match self {
            Self::Entity(_) => None,
            Self::Page(p) => Some(*p),
        }
    }
}

impl fmt::Display for Subject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entity(e) => f.write_str(e.as_str()),
            Self::Page(p) => write!(f, "{p}"),
        }
    }
}

/// A statement ID, `<Subject>$<UUID>` (wikibase-compat §2; 0017 §1; 0018 §7; 0038 §1).
///
/// The subject is an entity ID, or for a page's own statements the page ID in decimal.
/// The subject is canonicalized as [`Subject::parse`] does: the tenant-relative `QQQ8`
/// becomes `Q8` (0044 §3) and a page ID loses its leading zeros. Where the written
/// subject differs from the canonical one only in letter case, the ID is also kept as
/// written: Wikidata holds statements whose GUIDs start `q731$` (Q731, manganese), and a
/// stored value is never changed (0004 §7; 0006 §2), so the storage and Wikibase forms
/// keep that spelling. [`Self::as_str`] gives the ID as kept; comparison, ordering and
/// hashing use the canonical form, so `q731$…` and `Q731$…` are one statement, as
/// Wikibase's own GUID parser has it.
///
/// The UUID part is kept as written. Wikibase compares it case-insensitively and writes
/// it in uppercase; the resolved view rewrites the entity part to the canonical member's
/// ID and keeps the UUID (0018 §7).
#[derive(Clone)]
pub struct StatementId {
    /// The ID with its subject canonical.
    canonical: String,
    /// The ID as written, where it differs from `canonical` (only in case).
    written: Option<Box<str>>,
}

impl PartialEq for StatementId {
    fn eq(&self, other: &Self) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for StatementId {}

impl std::hash::Hash for StatementId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl PartialOrd for StatementId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StatementId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.canonical.cmp(&other.canonical)
    }
}

impl StatementId {
    fn canonical_of(canonical: String) -> Self {
        Self {
            canonical,
            written: None,
        }
    }

    /// Parses and checks the shape: the subject canonicalized, the spelling kept where it
    /// differs only in case.
    pub fn parse(s: &str) -> Result<Self, IdParseError> {
        let Some((subject, uuid)) = s.rsplit_once('$') else {
            return Err(IdParseError::Statement(s.to_string()));
        };
        let canonical_subject =
            Subject::parse(subject).map_err(|_| IdParseError::Statement(s.to_string()))?;
        if !is_guid(uuid) {
            return Err(IdParseError::Statement(s.to_string()));
        }
        let canonical = format!("{canonical_subject}${uuid}");
        let written = (canonical != s && canonical.eq_ignore_ascii_case(s))
            .then(|| s.to_string().into_boxed_str());
        Ok(Self { canonical, written })
    }

    /// The ID as kept: as written where only its case differed, else canonical.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.written.as_deref().unwrap_or(&self.canonical)
    }

    /// The ID with its subject canonical (`q731$…` → `Q731$…`).
    #[must_use]
    pub fn canonical_str(&self) -> &str {
        &self.canonical
    }

    /// The same ID with its subject canonical.
    #[must_use]
    pub fn canonical(&self) -> Self {
        Self::canonical_of(self.canonical.clone())
    }

    /// The subject part, canonical.
    ///
    /// # Panics
    ///
    /// Never: the constructor checked the shape.
    #[must_use]
    pub fn subject(&self) -> Subject {
        let s = self.canonical.rsplit_once('$').expect("checked at parse").0;
        if s.as_bytes()[0].is_ascii_digit() {
            Subject::Page(s.parse().expect("checked at parse"))
        } else {
            Subject::Entity(EntityId(s.to_string()))
        }
    }

    /// The entity part, when the subject is an entity.
    #[must_use]
    pub fn entity_id(&self) -> Option<EntityId> {
        match self.subject() {
            Subject::Entity(e) => Some(e),
            Subject::Page(_) => None,
        }
    }

    /// The UUID part, as written.
    ///
    /// # Panics
    ///
    /// Never: the constructor checked the shape.
    #[must_use]
    pub fn uuid(&self) -> &str {
        self.canonical.rsplit_once('$').expect("checked at parse").1
    }

    /// The same statement under another subject.
    #[must_use]
    pub fn with_subject(&self, subject: &Subject) -> Self {
        Self::canonical_of(format!("{subject}${}", self.uuid()))
    }

    /// The same statement under another entity ID, for the canonical-ID rewrite of
    /// 0018 §7.
    #[must_use]
    pub fn with_entity(&self, entity: &EntityId) -> Self {
        Self::canonical_of(format!("{entity}${}", self.uuid()))
    }
}

fn is_guid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => *c == b'-',
            _ => c.is_ascii_hexdigit(),
        })
}

impl fmt::Display for StatementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for StatementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "StatementId({})", self.as_str())
    }
}

impl Serialize for StatementId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StatementId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = std::borrow::Cow::<str>::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl std::str::FromStr for StatementId {
    type Err = IdParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_forms_by_shape() {
        let q = EntityId::parse("q5").unwrap();
        assert_eq!(
            (q.as_str(), q.form(), q.numeric_id()),
            ("Q5", IdForm::Local, Some(5))
        );
        assert_eq!(q.implied_entity_type(), Some("item"));
        let wd = EntityId::parse("wdp31").unwrap();
        assert_eq!(
            (wd.as_str(), wd.form(), wd.numeric_id()),
            ("WDP31", IdForm::Foreign, Some(31))
        );
        assert_eq!(wd.implied_entity_type(), Some("property"));
        let mb = EntityId::parse("MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d").unwrap();
        assert_eq!(mb.form(), IdForm::Foreign);
        assert_eq!(mb.numeric_id(), None);
        assert_eq!(mb.implied_entity_type(), None);
        let d = EntityId::parse("Domain:en.wikipedia.org").unwrap();
        assert_eq!(
            (d.as_str(), d.form(), d.numeric_id()),
            ("domain:en.wikipedia.org", IdForm::Keyed, None)
        );
        assert_eq!(d.implied_entity_type(), Some("domain"));
        // CAMEO codes keep their zeros (0017 §2, as amended).
        assert_eq!(EntityId::parse("GDC0311").unwrap().numeric_id(), Some(311));
    }

    #[test]
    fn rejections_by_shape() {
        for s in ["", "5", "Q", "Q5a", "QQ", "QQQ", ":x", "domain:", "a b:c"] {
            assert!(EntityId::parse(s).is_err(), "{s:?}");
        }
    }

    /// A form or sense of a lexeme (0066 §4): a suffix on `L` only, in the local, input and
    /// foreign forms, canonical with its part letter uppercase.
    #[test]
    fn lexeme_part_ids() {
        for (written, canonical, form, lexeme, kind) in [
            ("L1-F1", "L1-F1", IdForm::Local, "L1", "form"),
            ("l470-s1", "L470-S1", IdForm::Local, "L470", "sense"),
            ("LLL3-f2", "L3-F2", IdForm::Local, "L3", "form"),
            (
                "wdl252247-F2",
                "WDL252247-F2",
                IdForm::Foreign,
                "WDL252247",
                "form",
            ),
        ] {
            let id = EntityId::parse(written).unwrap();
            assert_eq!((id.as_str(), id.form()), (canonical, form), "{written}");
            let (base, part) = id.lexeme_part().unwrap();
            assert_eq!(
                (base.as_str(), part.entity_type()),
                (lexeme, kind),
                "{written}"
            );
            assert_eq!(id.implied_entity_type(), Some(kind));
            assert_eq!(
                id.numeric_id(),
                None,
                "no numeric-id, as Wikibase writes them"
            );
            assert_eq!(id.type_code(), Some('L'));
        }
        assert_eq!(
            EntityId::parse("L1").unwrap().implied_entity_type(),
            Some("lexeme")
        );
        assert!(EntityId::parse("L1").unwrap().lexeme_part().is_none());
        assert!(
            EntityId::parse("domain:l1-f1")
                .unwrap()
                .lexeme_part()
                .is_none()
        );
        // On `L` only, one suffix, `F` or `S` and digits.
        for s in [
            "Q1-F1", "P1-S1", "QQQ1-F1", "L1-", "L1-F", "L-F1", "L1-X1", "L1-F1-S1", "L1-Fa",
            "L1F1",
        ] {
            assert!(EntityId::parse(s).is_err(), "{s:?}");
        }
        // The provider's grammar takes the suffix for its lexeme type only.
        let p = ProviderRegistry::default_registry();
        let k = KeyedRegistry::default_registry();
        let v = |s: &str| EntityId::parse(s).unwrap().validate(p, k);
        assert_eq!(v("WDL1-f1").unwrap().as_str(), "WDL1-F1");
        assert_eq!(v("WDL1-S12").unwrap().as_str(), "WDL1-S12");
        assert!(v("WDQ1-F1").is_err());
        assert!(v("WDL1-X1").is_err());
        // A statement on a part, as Wikibase writes it (wikibase-compat §2).
        let sid = StatementId::parse("L1-F1$8a3c0c6b-0b7e-4f1e-9d2b-3f4e5a6b7c8d").unwrap();
        assert_eq!(
            sid.subject(),
            Subject::Entity(EntityId::parse("L1-F1").unwrap())
        );
    }

    #[test]
    fn tenant_relative_input_form_is_the_local_id() {
        // 0044 §1: a type letter three times, then digits, is the tenant's own local ID.
        for (written, local, kind) in [
            ("QQQ5", "Q5", "item"),
            ("ppp31", "P31", "property"),
            ("LLL3", "L3", "lexeme"),
            ("MMM1234", "M1234", "mediainfo"),
        ] {
            let id = EntityId::parse(written).unwrap();
            assert_eq!(id.as_str(), local, "{written}");
            assert_eq!(id.form(), IdForm::Local);
            assert_eq!(id.implied_entity_type(), Some(kind));
        }
        // 0044 §2: the rest follows the local grammar, which is digits.
        assert!(matches!(
            EntityId::parse("QQQb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"),
            Err(IdParseError::Local(_))
        ));
        // Only the exact triple is tenant-relative; `QQP5` is a (reserved, unregistered)
        // foreign shape and is left for the provider registry to reject.
        let qqp = EntityId::parse("QQP5").unwrap();
        assert_eq!((qqp.as_str(), qqp.form()), ("QQP5", IdForm::Foreign));
        assert!(matches!(
            qqp.validate(
                ProviderRegistry::default_registry(),
                KeyedRegistry::default_registry()
            ),
            Err(IdParseError::Foreign(_))
        ));
        // Round trip through serde canonicalizes too, so the form is never stored.
        let id: EntityId = serde_json::from_str(r#""QQQ5""#).unwrap();
        assert_eq!(serde_json::to_string(&id).unwrap(), r#""Q5""#);
    }

    #[test]
    fn derived_mediainfo_ids() {
        let m = EntityId::parse("m1234").unwrap();
        assert_eq!(m.as_str(), "M1234");
        assert!(m.is_derived());
        assert_eq!(m.derived_page_id(), Some(1234));
        assert_eq!(m.implied_entity_type(), Some("mediainfo"));
        // Minted IDs and foreign `M` IDs are not derived; a foreign type table decides
        // what a provider's `M` means.
        assert!(!EntityId::parse("Q5").unwrap().is_derived());
        let foreign = EntityId::parse("WDM1234").unwrap();
        assert!(!foreign.is_derived());
        assert_eq!(foreign.derived_page_id(), None);
        assert_eq!(foreign.implied_entity_type(), None);
    }

    #[test]
    fn page_subject_statement_ids() {
        // 0038 §1: a page's own statements have `{page ID}$<UUID>` IDs.
        let s = StatementId::parse("1234$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap();
        assert_eq!(s.subject(), Subject::Page(1234));
        assert_eq!(s.entity_id(), None);
        assert_eq!(s.subject().page_id(), Some(1234));
        let e = StatementId::parse("Q8$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap();
        assert_eq!(e.subject(), Subject::Entity(EntityId::parse("Q8").unwrap()));
        assert_eq!(e.subject().entity_id().unwrap().as_str(), "Q8");
        let moved = e.with_subject(&Subject::Page(7));
        assert_eq!(moved.as_str(), "7$C13E7A23-11E7-4C91-A799-3D1806B65444");
        // A leading zero would make the decimal form ambiguous; a page ID is written
        // as `u64` prints it, so the ID canonicalizes.
        assert_eq!(
            StatementId::parse("0012$C13E7A23-11E7-4C91-A799-3D1806B65444")
                .unwrap()
                .as_str(),
            "12$C13E7A23-11E7-4C91-A799-3D1806B65444"
        );
        assert!(StatementId::parse("$C13E7A23-11E7-4C91-A799-3D1806B65444").is_err());
        assert!(Subject::parse("").is_err());
        assert!(Subject::parse("99999999999999999999999").is_err());
    }

    #[test]
    fn validation_against_registries() {
        let p = ProviderRegistry::default_registry();
        let k = KeyedRegistry::default_registry();
        let v = |s: &str| EntityId::parse(s).unwrap().validate(p, k);
        assert_eq!(v("Q5").unwrap().as_str(), "Q5");
        assert_eq!(
            v("mbab10BBBFC-CF9E-42E0-BE17-E2C3E1D2600D")
                .unwrap()
                .as_str(),
            "MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d"
        );
        assert_eq!(
            v("domain:Bücher.example").unwrap().as_str(),
            "domain:xn--bcher-kva.example"
        );
        assert!(matches!(v("ZZQ1"), Err(IdParseError::Foreign(_))));
        assert!(matches!(v("isbn:123"), Err(IdParseError::Keyed(_))));
        assert!(matches!(
            v("OAKmachine-learning"),
            Err(IdParseError::Foreign(_))
        ));
    }

    #[test]
    fn statement_ids() {
        let s = StatementId::parse("q8$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap();
        // Kept as written (0006 §2), compared canonical.
        assert_eq!(s.as_str(), "q8$C13E7A23-11E7-4C91-A799-3D1806B65444");
        assert_eq!(s.canonical_str(), "Q8$C13E7A23-11E7-4C91-A799-3D1806B65444");
        assert_eq!(
            s,
            StatementId::parse("Q8$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap()
        );
        assert_eq!(
            s.canonical().as_str(),
            "Q8$C13E7A23-11E7-4C91-A799-3D1806B65444"
        );
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            "\"q8$C13E7A23-11E7-4C91-A799-3D1806B65444\""
        );
        assert_eq!(s.entity_id().unwrap().as_str(), "Q8");
        assert_eq!(s.uuid(), "C13E7A23-11E7-4C91-A799-3D1806B65444");
        let k = StatementId::parse("domain:en.wikipedia.org$c13e7a23-11e7-4c91-a799-3d1806b65444")
            .unwrap();
        assert_eq!(k.entity_id().unwrap().as_str(), "domain:en.wikipedia.org");
        // The tenant-relative form canonicalizes inside a statement ID too (0044 §3), and
        // is not kept: it differs from the canonical subject by more than case.
        let t = StatementId::parse("QQQ8$C13E7A23-11E7-4C91-A799-3D1806B65444").unwrap();
        assert_eq!(t, s);
        assert_eq!(t.as_str(), "Q8$C13E7A23-11E7-4C91-A799-3D1806B65444");
        let rewritten = s.with_entity(&EntityId::parse("LBQ8").unwrap());
        assert_eq!(
            rewritten.as_str(),
            "LBQ8$C13E7A23-11E7-4C91-A799-3D1806B65444"
        );
        for bad in [
            "Q8",
            "Q8$",
            "Q8$abc",
            "$C13E7A23-11E7-4C91-A799-3D1806B65444",
            "Q8$C13E7A23-11E7-4C91-A799-3D1806B6544G",
        ] {
            assert!(StatementId::parse(bad).is_err(), "{bad}");
        }
    }
}
