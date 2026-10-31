//! The special-page registry (0047 §1; 21 §1.1), embedding `docs/registry/special-pages.toml`:
//! every special page name the instance knows, with the heading `Special:SpecialPages`
//! lists it under, whether it is listed, and the permission without which it is not.
//!
//! This is the registry half; resolving a `Special:` title by alias and section alias, and
//! intersecting the served set with the handlers a binary compiled (21 §1.1, §1.2), join it
//! as their callers are built.

use std::sync::OnceLock;

use serde::Deserialize;

/// The registry file, as embedded.
pub const SPECIAL_PAGES_TOML: &str = include_str!("../../../docs/registry/special-pages.toml");

/// The groups of `Special:SpecialPages`, in the order the page lists them: MediaWiki's
/// group keys in the order Wikimedia wikis show them, then Wikibase's and Triplespace's,
/// with `other` last as MediaWiki keeps it. `unlisted` is never listed.
pub const GROUPS: [&str; 15] = [
    "maintenance",
    "pages",
    "login",
    "users",
    "changes",
    "media",
    "wiki",
    "redirects",
    "highuse",
    "pagetools",
    "wikibaserepo",
    "wikibaseclient",
    "identity",
    "instance",
    "other",
];

/// One special page name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SpecialPage {
    /// The canonical name: `RecentChanges`.
    pub name: String,
    /// MediaWiki's internal name, where it differs: `Recentchanges`.
    #[serde(default)]
    pub mediawiki_name: Option<String>,
    /// `served`, `deferred`, `declined` or `reserved`.
    pub status: String,
    /// The heading it is listed under: a key of [`GROUPS`], or `unlisted`.
    pub group: String,
    /// `false` for a page served but not listed.
    #[serde(default = "yes")]
    pub listed: bool,
    /// The permission without which it is not listed and refuses to act.
    #[serde(default)]
    pub restricted: Option<String>,
    /// A `site` setting that must be on for it to be listed and served (0042 §2).
    #[serde(default)]
    pub requires: Option<String>,
    /// Other names that reach it.
    #[serde(default)]
    pub aliases: Vec<String>,
}

fn yes() -> bool {
    true
}

impl SpecialPage {
    /// Whether `Special:SpecialPages` lists it at all: served, listed, under a group.
    #[must_use]
    pub fn is_listed(&self) -> bool {
        self.status == "served" && self.listed && self.group != "unlisted"
    }
}

#[derive(Deserialize)]
struct File {
    page: Vec<SpecialPage>,
}

/// The registry.
#[derive(Debug, Clone)]
pub struct SpecialPages {
    rows: Vec<SpecialPage>,
}

impl SpecialPages {
    /// The embedded registry.
    ///
    /// # Panics
    ///
    /// If the embedded file does not parse, which its test rules out.
    #[must_use]
    pub fn embedded() -> &'static SpecialPages {
        static R: OnceLock<SpecialPages> = OnceLock::new();
        R.get_or_init(|| Self::parse(SPECIAL_PAGES_TOML).expect("the embedded registry parses"))
    }

    /// Parses a registry file.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        let f: File = toml::from_str(text)?;
        Ok(Self { rows: f.page })
    }

    /// A page by its name, MediaWiki name or an alias, ignoring case, as MediaWiki
    /// resolves them.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&SpecialPage> {
        self.rows.iter().find(|p| {
            p.name.eq_ignore_ascii_case(name)
                || p.mediawiki_name
                    .as_deref()
                    .is_some_and(|m| m.eq_ignore_ascii_case(name))
                || p.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
    }

    /// Every page, in registry order.
    pub fn iter(&self) -> impl Iterator<Item = &SpecialPage> {
        self.rows.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded registry parses, every listed page's group is one the index lists
    /// (21 §1.1), and names resolve as MediaWiki resolves them.
    #[test]
    fn the_registry_parses_and_its_groups_are_known() {
        let r = SpecialPages::embedded();
        assert!(r.iter().count() > 100);
        for p in r.iter() {
            assert!(
                p.group == "unlisted" || GROUPS.contains(&p.group.as_str()),
                "{} is in an unknown group {}",
                p.name,
                p.group
            );
        }
        let rc = r.get("recentchanges").unwrap();
        assert_eq!(rc.name, "RecentChanges");
        assert_eq!(rc.group, "changes");
        assert!(rc.is_listed());
        assert_eq!(r.get("BrokenLinks").unwrap().name, "WantedPages");
        assert_eq!(r.get("Block").unwrap().restricted.as_deref(), Some("block"));
        assert!(!r.get("MyPage").unwrap().is_listed(), "a redirecting page");
        assert!(r.get("Nonsense").is_none());
    }
}
