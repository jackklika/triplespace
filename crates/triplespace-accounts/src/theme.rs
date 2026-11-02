//! Palettes and type sets (0086 §1–4): the catalogue a tenant offers and the theme
//! settings that choose from it, as `siprop=triplespace` reports them.
//!
//! The shipped palettes and type sets are `docs/registry/themes.toml`'s. An instance's and
//! a tenant's own, written as `palette` and `typeset` config records, join the catalogue
//! when their routes are built; until then a tenant offers the shipped ones. A setting or
//! preference that names something the tenant does not offer, or does not let readers
//! choose, resolves as unset (0086 §4).

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

const REGISTRY: &str = include_str!("../../../docs/registry/themes.toml");

/// The default palette and type set, which every wiki offers (0086 §1).
pub const DEFAULT_PALETTE: &str = "parchment";
/// The default type set.
pub const DEFAULT_TYPESET: &str = "default26";

/// The variants a palette may have, in the order their rules are written (0086 §2).
pub const VARIANTS: [&str; 4] = ["light", "dark", "light-more", "dark-more"];

/// A palette: the colour tokens of each variant, and the shape tokens of every variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Palette {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The palette it lays each variant over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends: Option<String>,
    /// The palette whose accessibility alternates it takes where it keys none of its
    /// own and extends none (0094 §2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternates_from: Option<String>,
    /// Its own accessibility alternates, by key (0094 §1).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub alternate: BTreeMap<String, Alternate>,
    /// `light`, required.
    pub light: BTreeMap<String, String>,
    /// `dark`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<BTreeMap<String, String>>,
    /// `light-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_more: Option<BTreeMap<String, String>>,
    /// `dark-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark_more: Option<BTreeMap<String, String>>,
    /// The shape tokens: radii and spacing.
    #[serde(flatten)]
    pub shape: BTreeMap<String, String>,
}

/// An accessibility alternate of a palette (0094 §1): up to four variants, each laid
/// over the palette's own as served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Alternate {
    /// `light`.
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    /// `dark`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark: Option<BTreeMap<String, String>>,
    /// `light-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light_more: Option<BTreeMap<String, String>>,
    /// `dark-more`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark_more: Option<BTreeMap<String, String>>,
}

/// A typeface a type set serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Font {
    /// The family.
    pub family: String,
    /// The weights served.
    pub weights: Vec<u16>,
    /// The licence.
    pub licence: String,
}

/// A type set: the three typeface roles, stacks per language, and the typefaces served.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Typeset {
    /// The name it is chosen by.
    #[serde(default)]
    pub name: String,
    /// What a reader sees.
    pub label: String,
    /// Whether a reader may always choose it.
    #[serde(default)]
    pub accessibility: bool,
    /// The typefaces it serves.
    #[serde(default, rename = "font")]
    pub fonts: Vec<Font>,
    /// A stack per language tag, set on the elements of that language (0086 §3).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub languages: BTreeMap<String, String>,
    /// The fallback layer whose typefaces back its own up, by name (0093 §1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
    /// `font-family-base`, `font-family-heading-main` and `font-family-monospace`.
    #[serde(flatten)]
    pub roles: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct File {
    palette: BTreeMap<String, toml::Value>,
    typeset: BTreeMap<String, Typeset>,
    #[serde(default)]
    alternate: BTreeMap<String, toml::Value>,
}

/// Whether a registry entry is retired, kept only so its name is never reused.
fn retired(v: &toml::Value) -> bool {
    v.get("retired").and_then(toml::Value::as_bool) == Some(true)
}

/// A retired palette's name and what it reads as (0094 §4): the palette that replaced
/// it, and the alternate a reader who chose it now wears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Retired {
    /// The retired name.
    pub name: String,
    /// The palette it reads as.
    pub palette: String,
    /// The alternate it reads as, if any.
    pub alternate: Option<String>,
}

static FILE: LazyLock<File> =
    LazyLock::new(|| toml::from_str(REGISTRY).expect("docs/registry/themes.toml parses"));

/// The accessibility alternates' keys, which the registry alone defines (0094 §1).
#[must_use]
pub fn alternate_keys() -> Vec<String> {
    FILE.alternate.keys().cloned().collect()
}

/// The retired palettes, with what each reads as (0094 §4).
#[must_use]
pub fn retired_palettes() -> Vec<Retired> {
    FILE.palette
        .iter()
        .filter(|(_, v)| retired(v))
        .map(|(name, v)| {
            let by = |prefix: &str| {
                v.get("replaced_by")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .find_map(|r| r.strip_prefix(prefix))
                    .map(str::to_string)
            };
            Retired {
                name: name.clone(),
                palette: by("palette.").unwrap_or_else(|| DEFAULT_PALETTE.into()),
                alternate: by("alternate."),
            }
        })
        .collect()
}

/// What a palette's name reads as: itself, or for a retired one the palette that
/// replaced it, with the alternate it carries.
#[must_use]
pub fn resolve_palette(name: &str) -> (String, Option<String>) {
    retired_palettes()
        .into_iter()
        .find(|r| r.name == name)
        .map_or_else(|| (name.to_string(), None), |r| (r.palette, r.alternate))
}

/// The palettes and type sets on offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Catalogue {
    /// The palettes, by name.
    pub palettes: Vec<Palette>,
    /// The type sets, by name.
    pub typesets: Vec<Typeset>,
}

static SHIPPED: LazyLock<Catalogue> = LazyLock::new(|| Catalogue {
    palettes: FILE
        .palette
        .iter()
        .filter(|(_, v)| !retired(v))
        .map(|(name, v)| Palette {
            name: name.clone(),
            ..v.clone()
                .try_into()
                .expect("docs/registry/themes.toml's palettes parse")
        })
        .collect(),
    typesets: FILE
        .typeset
        .iter()
        .map(|(name, t)| Typeset {
            name: name.clone(),
            ..t.clone()
        })
        .collect(),
});

impl Catalogue {
    /// The shipped palettes and type sets.
    #[must_use]
    pub fn shipped() -> &'static Self {
        &SHIPPED
    }

    /// A palette by name.
    #[must_use]
    pub fn palette(&self, name: &str) -> Option<&Palette> {
        self.palettes.iter().find(|p| p.name == name)
    }

    /// A type set by name.
    #[must_use]
    pub fn typeset(&self, name: &str) -> Option<&Typeset> {
        self.typesets.iter().find(|t| t.name == name)
    }

    /// The palettes a palette depends on (0094 §3): the one it extends and the one it
    /// takes its alternates from.
    #[must_use]
    pub fn prerequisites(p: &Palette) -> Vec<&str> {
        let mut out: Vec<&str> = [p.extends.as_deref(), p.alternates_from.as_deref()]
            .into_iter()
            .flatten()
            .filter(|n| *n != p.name)
            .collect();
        out.dedup();
        out
    }

    /// The prerequisites of a palette this catalogue does not offer: a palette that
    /// names one cannot be written here (0094 §3).
    #[must_use]
    pub fn missing_prerequisites<'a>(&self, p: &'a Palette) -> Vec<&'a str> {
        Self::prerequisites(p)
            .into_iter()
            .filter(|n| self.palette(n).is_none())
            .collect()
    }

    /// The palettes that depend on one, which stops its deletion (0086 §2; 0094 §3).
    #[must_use]
    pub fn dependents(&self, name: &str) -> Vec<&str> {
        self.palettes
            .iter()
            .filter(|p| p.name != name && Self::prerequisites(p).contains(&name))
            .map(|p| p.name.as_str())
            .collect()
    }

    /// The alternate keys a palette keys itself that the registry does not define;
    /// a palette that keys one cannot be written (0094 §1).
    #[must_use]
    pub fn unknown_alternates(p: &Palette) -> Vec<&str> {
        let known = alternate_keys();
        p.alternate
            .keys()
            .filter(|k| !known.contains(k))
            .map(String::as_str)
            .collect()
    }
}

/// What readers may choose (`theme.choice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    /// Any palette and type set offered.
    Any,
    /// Only the accessibility ones, beside the wiki's default.
    Accessibility,
}

impl Choice {
    /// The setting's value; anything else is `any`, its default.
    #[must_use]
    pub fn parse(s: Option<&str>) -> Self {
        if s == Some("accessibility") {
            Self::Accessibility
        } else {
            Self::Any
        }
    }
}

/// The theme of a wiki: the catalogue, the defaults and what readers may choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Theme {
    /// The palettes offered.
    pub palettes: Vec<Palette>,
    /// The type sets offered.
    pub typesets: Vec<Typeset>,
    /// `theme.palette`, resolved: a palette offered, or the default.
    pub palette: String,
    /// `theme.typeset`, resolved.
    pub typeset: String,
    /// `theme.choice`.
    pub choice: Choice,
}

impl Theme {
    /// The theme from the catalogue and the settings' values, each of which falls back to
    /// the default where it names nothing offered.
    #[must_use]
    pub fn new(
        catalogue: &Catalogue,
        palette: Option<&str>,
        typeset: Option<&str>,
        choice: Option<&str>,
    ) -> Self {
        // A retired name reads as the palette that replaced it (0094 §4).
        let resolved = palette.map(|n| resolve_palette(n).0);
        let palette = resolved
            .as_deref()
            .filter(|n| catalogue.palette(n).is_some())
            .unwrap_or(DEFAULT_PALETTE);
        let typeset = typeset
            .filter(|n| catalogue.typeset(n).is_some())
            .unwrap_or(DEFAULT_TYPESET);
        Self {
            palettes: catalogue.palettes.clone(),
            typesets: catalogue.typesets.clone(),
            palette: palette.into(),
            typeset: typeset.into(),
            choice: Choice::parse(choice),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_catalogue() {
        let c = Catalogue::shipped();
        let names: Vec<&str> = c.palettes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "catppuccin",
                "cyanotype",
                "encyclopedia",
                "greenbar",
                "gruvbox",
                "nord",
                "parchment"
            ]
        );
        let p = c.palette("parchment").unwrap();
        assert!(!p.accessibility && p.extends.is_none());
        assert_eq!(
            p.shape.get("border-radius-base").map(String::as_str),
            Some("6px")
        );
        assert!(p.dark.is_some() && p.light_more.is_some() && p.dark_more.is_some());
        assert_eq!(
            p.alternate.keys().collect::<Vec<_>>(),
            ["by", "rg"],
            "parchment keys both alternates"
        );
        let rg = &p.alternate["rg"];
        assert_eq!(rg.light["color-error"], "#A84300");
        assert!(rg.dark.is_some() && rg.light_more.is_some() && rg.dark_more.is_some());
        assert!(!p.shape.contains_key("alternate"));
        let t = c.typeset("default26").unwrap();
        assert_eq!(t.fonts.len(), 3);
        assert_eq!(t.roles.len(), 3);
        assert_eq!(t.fallback.as_deref(), Some("noto"));
        assert!(c.typeset("opendyslexic").unwrap().accessibility);
        let v = c.palette("encyclopedia").unwrap();
        assert!(!v.accessibility && v.extends.is_none());
        assert!(v.dark.is_some() && v.light_more.is_some() && v.dark_more.is_some());
        let t = c.typeset("basic").unwrap();
        assert!(!t.accessibility && t.fonts.is_empty() && t.fallback.is_none());
        assert_eq!(t.roles["font-family-base"], "sans-serif");
        for (palette, typeset) in [
            ("gruvbox", "retro"),
            ("nord", "nordic"),
            ("catppuccin", "cozy"),
            ("greenbar", "mainframe"),
            ("cyanotype", "drafting"),
        ] {
            let p = c.palette(palette).unwrap();
            assert!(p.dark.is_some() && p.light_more.is_some() && p.dark_more.is_some());
            assert_eq!(
                p.alternate.keys().collect::<Vec<_>>(),
                ["by", "rg"],
                "{palette}"
            );
            let t = c.typeset(typeset).unwrap();
            assert!(!t.accessibility && t.fonts.len() == 3, "{typeset}");
            assert_eq!(t.fallback.as_deref(), Some("noto"));
        }
    }

    #[test]
    fn alternates_and_retired_names() {
        assert_eq!(alternate_keys(), ["by", "rg"]);
        assert_eq!(
            resolve_palette("parchment-rg"),
            ("parchment".to_string(), Some("rg".to_string()))
        );
        assert_eq!(
            resolve_palette("parchment-by"),
            ("parchment".to_string(), Some("by".to_string()))
        );
        assert_eq!(
            resolve_palette("encyclopedia"),
            ("encyclopedia".into(), None)
        );
        // A setting naming a retired palette reads as its replacement.
        let t = Theme::new(Catalogue::shipped(), Some("parchment-rg"), None, None);
        assert_eq!(t.palette, "parchment");
        let j = serde_json::to_value(&t).unwrap();
        let parchment = j["palettes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "parchment")
            .unwrap();
        assert_eq!(
            parchment["alternate"]["by"]["light"]["color-error"],
            "#B3263E"
        );
    }

    #[test]
    fn prerequisites() {
        let c = Catalogue::shipped();
        let mut p = c.palette("encyclopedia").unwrap().clone();
        p.name = "brand".into();
        assert!(c.missing_prerequisites(&p).is_empty());
        p.extends = Some("parchment".into());
        p.alternates_from = Some("house".into());
        assert_eq!(Catalogue::prerequisites(&p), ["parchment", "house"]);
        assert_eq!(c.missing_prerequisites(&p), ["house"], "not offered here");
        let mut with = c.clone();
        with.palettes.push(p.clone());
        assert_eq!(with.dependents("parchment"), ["brand"]);
        assert!(with.dependents("encyclopedia").is_empty());
        p.alternate.insert(
            "xy".into(),
            Alternate {
                light: BTreeMap::new(),
                dark: None,
                light_more: None,
                dark_more: None,
            },
        );
        assert_eq!(Catalogue::unknown_alternates(&p), ["xy"]);
    }

    #[test]
    fn unknown_names_fall_back_to_the_defaults() {
        let t = Theme::new(
            Catalogue::shipped(),
            Some("nope"),
            Some("hyperlegible"),
            Some("accessibility"),
        );
        assert_eq!(t.palette, "parchment");
        assert_eq!(t.typeset, "hyperlegible");
        assert_eq!(t.choice, Choice::Accessibility);
        let j = serde_json::to_value(&t).unwrap();
        assert_eq!(j["choice"], "accessibility");
        let named = |list: &str, name: &str| {
            j[list]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .cloned()
                .unwrap()
        };
        let parchment = named("palettes", "parchment");
        assert_eq!(parchment["light"]["color-base"], "#1D1C1A");
        assert!(parchment["light-more"]["color-base"].is_string());
        assert_eq!(
            named("typesets", "default26")["font"][0]["family"],
            "Source Serif 4"
        );
        assert_eq!(named("typesets", "default26")["fallback"], "noto");
        assert!(named("typesets", "basic").get("fallback").is_none());
    }
}
