//! `Special:SpecialPages` (0047 §1, §9; 21 §1.1): the special pages the site offers, each
//! with what it is for, under the heading the special-page registry gives it (its `group`,
//! in [`GROUPS`] order) and by name within it. A page appears only where its feature is
//! offered, where the registry lists it, and where the viewer holds the permission the
//! registry restricts it to.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::special::{GROUPS, SpecialPages};
use serde::Serialize;

use crate::Site;
use crate::features::{Feature, Features};
use crate::frame::Page;
use crate::i18n::Messages;
use crate::pages::{self, Peer};

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/index.html` draws.
// ---------------------------------------------------------------------------------------

/// One special page the site offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexEntry {
    /// Its page.
    pub href: String,
    /// Its name.
    pub label: String,
    /// What it is for.
    pub description: String,
}

/// One heading and the pages under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexGroup {
    /// The registry's group key: `changes`.
    pub key: String,
    /// The heading: "Recent changes and logs".
    pub heading: String,
    /// The pages, by name.
    pub pages: Vec<IndexEntry>,
}

/// `Special:SpecialPages`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexPage {
    /// The headings that have pages, in [`GROUPS`] order.
    pub groups: Vec<IndexGroup>,
    /// What the page says when it offers none.
    pub empty: Option<String>,
}

/// The heading a page is listed under, if it is listed for this viewer: the registry's
/// group, where the registry lists the page and the viewer holds the permission it is
/// restricted to (21 §1.1). A page the registry does not know is listed under `other`.
fn listed_under(title: &str, holds: &impl Fn(&str) -> bool) -> Option<&'static str> {
    let name = title.strip_prefix("Special:").unwrap_or(title);
    let Some(entry) = SpecialPages::embedded().get(name) else {
        return Some("other");
    };
    if !entry.is_listed() || entry.restricted.as_deref().is_some_and(|r| !holds(r)) {
        return None;
    }
    GROUPS.iter().copied().find(|g| *g == entry.group)
}

/// The pages the viewer is offered here, under their headings: a page appears only where
/// its feature is on, the instance's settings only at the farm base (0085 §4), and a
/// restricted page only to a viewer who holds its permission; `holds` says whether the
/// viewer holds a right.
#[must_use]
pub fn index_model(
    m: &Messages,
    features: &Features,
    at_farm: bool,
    holds: impl Fn(&str) -> bool,
) -> IndexPage {
    let mut offered: Vec<(&str, IndexEntry)> = Vec::new();
    for (title, label, description, feature) in PAGES {
        let elsewhere = *title == "Special:InstanceSettings" && !at_farm;
        if feature.is_none_or(|f| features.has(f))
            && !elsewhere
            && let Some(group) = listed_under(title, &holds)
        {
            offered.push((
                group,
                IndexEntry {
                    href: format!("/wiki/{title}"),
                    label: m.get(label),
                    description: m.get(description),
                },
            ));
        }
    }
    for report in super::reports::REPORTS {
        if report.offered(features)
            && let Some(group) = listed_under(&report.title(), &holds)
        {
            offered.push((
                group,
                IndexEntry {
                    href: format!("/wiki/{}", report.title()),
                    label: m.get(&report.label_key()),
                    description: m.get(&report.description_key()),
                },
            ));
        }
    }
    let groups: Vec<IndexGroup> = GROUPS
        .iter()
        .filter_map(|key| {
            let mut pages: Vec<IndexEntry> = offered
                .iter()
                .filter(|(g, _)| g == key)
                .map(|(_, e)| e.clone())
                .collect();
            if pages.is_empty() {
                return None;
            }
            pages.sort_by_cached_key(|e| e.label.to_lowercase());
            Some(IndexGroup {
                key: (*key).to_string(),
                heading: m.get(&format!("ts-specialpages-group-{key}")),
                pages,
            })
        })
        .collect();
    let empty = groups.is_empty().then(|| m.get("ts-specialpages-none"));
    IndexPage { groups, empty }
}

/// The special pages the site serves: title, label message, description message, and
/// the feature each needs, if any.
pub const PAGES: &[(&str, &str, &str, Option<Feature>)] = &[
    (
        "Special:Search",
        "ts-search-title",
        "ts-specialpages-search",
        Some(Feature::Search),
    ),
    (
        "Special:UserLogin",
        "ts-login-title",
        "ts-specialpages-login",
        Some(Feature::Login),
    ),
    (
        "Special:CreateAccount",
        "ts-createaccount-title",
        "ts-specialpages-createaccount",
        Some(Feature::Register),
    ),
    (
        "Special:ConfirmAccounts",
        "ts-confirmaccounts-title",
        "ts-specialpages-confirmaccounts",
        Some(Feature::Register),
    ),
    (
        "Special:Watchlist",
        "ts-watchlist-title",
        "ts-specialpages-watchlist",
        Some(Feature::Watchlist),
    ),
    (
        "Special:EditWatchlist",
        "ts-editwatchlist-title",
        "ts-specialpages-editwatchlist",
        Some(Feature::Watchlist),
    ),
    (
        "Special:Notifications",
        "ts-notifications-title",
        "ts-specialpages-notifications",
        Some(Feature::Notifications),
    ),
    (
        "Special:Log",
        "ts-log-title",
        "ts-specialpages-log",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:UserRights",
        "ts-userrights-title",
        "ts-specialpages-userrights",
        Some(Feature::UserRights),
    ),
    (
        "Special:ProtectedPages",
        "ts-protectedpages-title",
        "ts-specialpages-protectedpages",
        Some(Feature::Protect),
    ),
    (
        "Special:Block",
        "ts-block-title",
        "ts-specialpages-block",
        Some(Feature::Block),
    ),
    (
        "Special:BlockList",
        "ts-blocklist-title",
        "ts-specialpages-blocklist",
        Some(Feature::Block),
    ),
    (
        "Special:Undelete",
        "ts-undelete-index-title",
        "ts-specialpages-undelete",
        Some(Feature::Delete),
    ),
    (
        "Special:MovePage",
        "ts-move-index-title",
        "ts-specialpages-movepage",
        Some(Feature::Move),
    ),
    (
        "Special:NewItem",
        "ts-newitem-title",
        "ts-specialpages-newitem",
        Some(Feature::Create),
    ),
    (
        "Special:NewProperty",
        "ts-newproperty-title",
        "ts-specialpages-newproperty",
        Some(Feature::Create),
    ),
    (
        "Special:RecentChanges",
        "ts-recentchanges-title",
        "ts-specialpages-recentchanges",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:Contributions",
        "ts-contributions-title-none",
        "ts-specialpages-contributions",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:Version",
        "ts-version-title",
        "ts-specialpages-version",
        Some(Feature::Version),
    ),
    (
        "Special:Preferences",
        "ts-preferences-title",
        "ts-specialpages-preferences",
        Some(Feature::Preferences),
    ),
    (
        "Special:WikiSettings",
        "ts-wikisettings-title",
        "ts-specialpages-wikisettings",
        Some(Feature::Settings),
    ),
    (
        "Special:InstanceSettings",
        "ts-instancesettings-title",
        "ts-specialpages-instancesettings",
        Some(Feature::Settings),
    ),
    (
        "Special:Redirect",
        "ts-redirect-title",
        "ts-specialpages-redirect",
        Some(Feature::History),
    ),
    (
        "Special:EntityPage",
        "ts-entitypage-title",
        "ts-specialpages-entitypage",
        None,
    ),
    (
        "Special:ComparePages",
        "ts-comparepages-title",
        "ts-specialpages-comparepages",
        Some(Feature::Pages),
    ),
    (
        "Special:ProtectedTitles",
        "ts-protectedtitles-title",
        "ts-specialpages-protectedtitles",
        Some(Feature::Protect),
    ),
    (
        "Special:NamespaceInfo",
        "ts-namespaceinfo-title",
        "ts-specialpages-namespaceinfo",
        None,
    ),
    (
        "Special:ListGroupRights",
        "ts-listgrouprights-title",
        "ts-specialpages-listgrouprights",
        None,
    ),
    (
        "Special:AllMessages",
        "ts-allmessages-title",
        "ts-specialpages-allmessages",
        None,
    ),
    (
        "Special:ListDatatypes",
        "ts-listdatatypes-title",
        "ts-specialpages-listdatatypes",
        None,
    ),
    (
        "Special:MyLanguageFallbackChain",
        "ts-fallbackchain-title",
        "ts-specialpages-fallbackchain",
        None,
    ),
    (
        "Special:AllPages",
        "ts-allpages-title",
        "ts-specialpages-allpages",
        Some(Feature::Pages),
    ),
    (
        "Special:PrefixIndex",
        "ts-prefixindex-title",
        "ts-specialpages-prefixindex",
        Some(Feature::Pages),
    ),
    (
        "Special:NewPages",
        "ts-newpages-title",
        "ts-specialpages-newpages",
        Some(Feature::RecentChanges),
    ),
    (
        "Special:Providers",
        "ts-providers-title",
        "ts-specialpages-providers",
        None,
    ),
    (
        "Special:SetLabel",
        "ts-setlabel-title",
        "ts-specialpages-setlabel",
        Some(Feature::Edit),
    ),
    (
        "Special:SetDescription",
        "ts-setdescription-title",
        "ts-specialpages-setdescription",
        Some(Feature::Edit),
    ),
    (
        "Special:SetAliases",
        "ts-setaliases-title",
        "ts-specialpages-setaliases",
        Some(Feature::Edit),
    ),
    (
        "Special:SetLabelDescriptionAliases",
        "ts-setlabeldescriptionaliases-title",
        "ts-specialpages-setlabeldescriptionaliases",
        Some(Feature::Edit),
    ),
];

/// `Special:SpecialPages`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let body = cx.draw(
        "special/index.html",
        index_model(&cx.m, &cx.features, cx.site.at_farm, |r| cx.holds(r)),
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-nav-specialpages"),
        returnto: Some("Special:SpecialPages".into()),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hrefs(p: &IndexPage) -> Vec<&str> {
        p.groups
            .iter()
            .flat_map(|g| g.pages.iter().map(|e| e.href.as_str()))
            .collect()
    }

    /// The index's model (19 §5.10; 21 §1.1): the pages under the registry's headings in
    /// their order and by name within each, a page only where its feature is on, the
    /// instance's settings only at the farm base, a restricted page only to a viewer who
    /// holds its permission.
    #[test]
    fn the_index_is_grouped_by_the_registry() {
        let m = Messages::for_language("en");
        let all = index_model(&m, &Features::all_built(), true, |_| true);
        assert!(all.empty.is_none());
        let keys: Vec<&str> = all.groups.iter().map(|g| g.key.as_str()).collect();
        let order: Vec<usize> = keys
            .iter()
            .map(|k| GROUPS.iter().position(|g| g == k).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{keys:?}");
        let group = |k: &str| all.groups.iter().find(|g| g.key == k).unwrap();
        assert_eq!(group("changes").heading, "Recent changes and logs");
        let changes: Vec<&str> = group("changes")
            .pages
            .iter()
            .map(|e| e.href.as_str())
            .collect();
        assert!(changes.contains(&"/wiki/Special:RecentChanges"));
        let labels: Vec<String> = group("maintenance")
            .pages
            .iter()
            .map(|e| e.label.to_lowercase())
            .collect();
        assert!(labels.windows(2).all(|w| w[0] <= w[1]), "{labels:?}");
        assert!(hrefs(&all).contains(&"/wiki/Special:WantedPages"));
        assert!(hrefs(&all).contains(&"/wiki/Special:InstanceSettings"));

        let tenant = index_model(&m, &Features::all_built(), false, |_| true);
        assert!(!hrefs(&tenant).contains(&"/wiki/Special:InstanceSettings"));

        // A reader holding no rights is not offered the restricted pages.
        let reader = index_model(&m, &Features::all_built(), false, |_| false);
        for restricted in ["Block", "Preferences", "MovePage", "Watchlist"] {
            assert!(
                !hrefs(&reader).contains(&format!("/wiki/Special:{restricted}").as_str()),
                "{restricted}"
            );
        }
        assert!(hrefs(&reader).contains(&"/wiki/Special:RecentChanges"));

        let none = index_model(&m, &Features::from_capabilities(&[]), false, |_| true);
        assert!(hrefs(&none).iter().all(|h| {
            PAGES
                .iter()
                .any(|(t, _, _, f)| f.is_none() && *h == format!("/wiki/{t}"))
        }));
    }

    /// Every page the index offers is one the registry lists under a heading the index
    /// has a message for.
    #[test]
    fn every_page_has_a_heading() {
        let m = Messages::for_language("en");
        for key in GROUPS {
            assert!(
                m.has(&format!("ts-specialpages-group-{key}")),
                "no heading for {key}"
            );
        }
        for (title, ..) in PAGES {
            let name = title.strip_prefix("Special:").unwrap();
            assert!(
                SpecialPages::embedded().get(name).is_some(),
                "{name} is not in special-pages.toml"
            );
        }
    }
}
