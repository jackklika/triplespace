//! The History tab of an entity page (0010 §5; 19 §3.3–3.5): `?action=history` on the
//! entity's title, as MediaWiki addresses a history.
//!
//! The rows are `GET /entity/{id}/history`'s, drawn as `Special:RecentChanges` draws
//! its rows ([`crate::special::changes`]): local edits with the Local chip, mirror syncs
//! with the provider's chip and "Synced from …", log events, erased records as gap
//! rows; grouped by day, newest first, paged by the feed's cursor.
//!
//! - **The source switch** above the list offers *All sources*, *Made here* and one
//!   entry per mirror that contributes to the entity, from the provenance's graphs; it
//!   is hidden when only one source contributes, so a purely local item's history looks
//!   like MediaWiki's (19 §3.5).
//! - **Only the latest state is kept here.** When a contributing mirror's history policy
//!   is `latest`, a note says so beside the list (19 §3.5). **Keep full history here…**
//!   and the link to the provider's history wait for the retention write and the
//!   provider registry's history URL.
//!
//! Diff links, the upstream-edit fold, cluster members' rows, "History of this value"
//! and the log-only view come with the later Phase 4 pull requests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use triplespace_client::activity::ActivityQuery;
use triplespace_client::{ActivityPage, Provenance, SiteInfo};

use serde::Serialize;

use crate::frame::title_url;
use crate::html::esc;
use crate::i18n::Messages;
use crate::pages::Context;

/// Rows per page, and the most allowed.
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 500;

/// What the query asks of the history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    /// One graph, or every graph.
    pub source: Option<String>,
    /// Rows per page.
    pub limit: u32,
    /// The feed's cursor.
    pub cursor: Option<String>,
}

impl Ask {
    /// From the page's query.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Self {
        Self {
            source: query
                .get("source")
                .filter(|s| {
                    !s.is_empty()
                        && s.chars()
                            .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '-' || c == '_')
                })
                .cloned(),
            limit: query
                .get("limit")
                .and_then(|l| l.parse::<u32>().ok())
                .map_or(DEFAULT_LIMIT, |l| l.clamp(1, MAX_LIMIT)),
            cursor: query.get("continue").cloned().filter(|c| !c.is_empty()),
        }
    }

    /// The feed to fetch.
    #[must_use]
    pub fn query(&self, lang: &str) -> ActivityQuery {
        ActivityQuery {
            sources: self.source.iter().cloned().collect(),
            limit: self.limit,
            cursor: self.cursor.clone(),
            lang: lang.to_string(),
            ..ActivityQuery::default()
        }
    }

    /// The tab's URL with these parameters and a cursor.
    fn href(&self, title: &str, cursor: Option<&str>) -> String {
        let mut s = format!("/wiki/{}?action=history", title_url(title));
        if let Some(src) = &self.source {
            let _ = write!(s, "&source={}", esc(src));
        }
        if self.limit != DEFAULT_LIMIT {
            let _ = write!(s, "&limit={}", self.limit);
        }
        if let Some(c) = cursor {
            let _ = write!(
                s,
                "&continue={}",
                percent_encoding::utf8_percent_encode(c, percent_encoding::NON_ALPHANUMERIC)
            );
        }
        s
    }
}

/// The graphs that contribute to the entity, local first.
fn contributing(provenance: Option<&Provenance>) -> Vec<String> {
    let mut graphs: Vec<String> = provenance
        .map(|p| p.graphs.iter().map(|g| g.graph.clone()).collect())
        .unwrap_or_default();
    graphs.sort_by_key(|g| (g != "local", g.clone()));
    graphs.dedup();
    graphs
}

/// A mirror graph's display name: the provider's name, or the slug.
fn graph_name(site: &SiteInfo, graph: &str) -> String {
    graph
        .strip_prefix("mirror/")
        .and_then(|slug| site.providers.iter().find(|p| p.slug == slug))
        .map_or_else(|| graph.to_string(), |p| p.name.clone())
}

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `entity/history.html` draws.
// ---------------------------------------------------------------------------------------

/// A link in a switch or a pager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Choice {
    /// The page.
    pub href: String,
    /// The text.
    pub label: String,
    /// Whether it is the current one.
    pub current: bool,
}

/// The source switch (19 §3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Switch {
    /// The accessible name.
    pub label: String,
    /// *All sources*, *Made here*, then each mirror.
    pub items: Vec<Choice>,
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pager {
    /// The accessible name.
    pub label: String,
    /// The first page.
    pub newest: Option<Choice>,
    /// The next page.
    pub older: Option<Choice>,
}

/// The History tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HistoryModel {
    /// The source switch, when more than one source contributes.
    pub switch: Option<Switch>,
    /// The note that a mirror keeps only its latest state.
    pub note: Option<String>,
    /// What the tab says when it has no rows.
    pub empty: Option<String>,
    /// The rows as `Special:RecentChanges` draws them.
    pub list: Option<crate::special::changes::ListModel>,
    /// The pager, when there is another page.
    pub pager: Option<Pager>,
}

fn switch_model(
    m: &Messages,
    site: &SiteInfo,
    title: &str,
    ask: &Ask,
    graphs: &[String],
) -> Option<Switch> {
    if graphs.len() < 2 {
        return None;
    }
    let item = |label: String, source: Option<&str>| Choice {
        href: Ask {
            source: source.map(str::to_string),
            limit: ask.limit,
            cursor: None,
        }
        .href(title, None),
        label,
        current: ask.source.as_deref() == source,
    };
    let mut items = vec![item(m.get("ts-history-all-sources"), None)];
    for g in graphs {
        let label = if g == "local" {
            m.get("ts-history-made-here")
        } else {
            graph_name(site, g)
        };
        items.push(item(label, Some(g)));
    }
    Some(Switch {
        label: m.get("ts-history-switch-label"),
        items,
    })
}

fn latest_only_model(
    m: &Messages,
    site: &SiteInfo,
    provenance: Option<&Provenance>,
) -> Option<String> {
    let latest: Vec<String> = provenance?
        .graphs
        .iter()
        .filter(|g| g.graph.starts_with("mirror/") && g.history.as_deref() == Some("latest"))
        .map(|g| graph_name(site, &g.graph))
        .collect();
    if latest.is_empty() {
        return None;
    }
    Some(m.with("ts-history-latest-only", &[&latest.join(", ")]))
}

fn pager_model(m: &Messages, title: &str, ask: &Ask, next: Option<&str>) -> Option<Pager> {
    if ask.cursor.is_none() && next.is_none() {
        return None;
    }
    Some(Pager {
        label: m.get("ts-search-pages-label"),
        newest: ask.cursor.as_ref().map(|_| Choice {
            href: ask.href(title, None),
            label: m.get("ts-changes-newest"),
            current: false,
        }),
        older: next.map(|n| Choice {
            href: ask.href(title, Some(n)),
            label: m.get("ts-changes-older"),
            current: false,
        }),
    })
}

/// The tab's model: the rows read relative to the talk page `here` when there is one.
fn model(
    cx: &Context,
    title: &str,
    ask: &Ask,
    provenance: Option<&Provenance>,
    here: Option<u64>,
    page: &ActivityPage,
    switch: bool,
) -> HistoryModel {
    let (m, site) = (&cx.m, &cx.site);
    let graphs = contributing(provenance);
    HistoryModel {
        switch: switch
            .then(|| switch_model(m, site, title, ask, &graphs))
            .flatten(),
        note: switch
            .then(|| latest_only_model(m, site, provenance))
            .flatten(),
        empty: page.rows.is_empty().then(|| m.get("ts-history-none")),
        list: crate::special::changes::list_model_here(m, site, &page.rows, here),
        pager: if page.rows.is_empty() {
            None
        } else {
            pager_model(m, title, ask, page.next.as_deref())
        },
    }
}

/// The History tab of a talk page or thread (16 §6.1): the rows as [`body`] draws them, read
/// relative to the talk page `here`, with the pager; no source switch, since every row of a
/// discussion is local.
#[must_use]
pub fn body_here(
    cx: &Context,
    title: &str,
    ask: &Ask,
    here: Option<u64>,
    page: &ActivityPage,
) -> String {
    cx.draw(
        "entity/history.html",
        model(cx, title, ask, None, here, page, false),
    )
}

/// The tab's content.
#[must_use]
pub fn body(
    cx: &Context,
    title: &str,
    ask: &Ask,
    provenance: Option<&Provenance>,
    page: &ActivityPage,
) -> String {
    cx.draw(
        "entity/history.html",
        model(cx, title, ask, provenance, None, page, true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::entity::GraphStanding;

    #[test]
    fn the_ask_reads_the_query() {
        let mut q = BTreeMap::new();
        let a = Ask::of(&q);
        assert_eq!(a.source, None);
        assert_eq!(a.limit, DEFAULT_LIMIT);
        assert_eq!(a.href("Item:Q6", None), "/wiki/Item:Q6?action=history");
        q.insert("source".into(), "mirror/wikidata".into());
        q.insert("limit".into(), "9999".into());
        q.insert("continue".into(), "1|2|3".into());
        let a = Ask::of(&q);
        assert_eq!(a.source.as_deref(), Some("mirror/wikidata"));
        assert_eq!(a.limit, MAX_LIMIT);
        assert_eq!(
            a.href("Item:Q6", Some("4|5|6")),
            "/wiki/Item:Q6?action=history&source=mirror/wikidata&limit=500&continue=4%7C5%7C6"
        );
        q.insert("source".into(), "<x>".into());
        assert_eq!(Ask::of(&q).source, None, "a source is a graph name");
    }

    #[test]
    fn contributing_graphs_are_local_first() {
        let p = Provenance {
            graphs: vec![
                GraphStanding {
                    graph: "mirror/wikidata".into(),
                    ..GraphStanding::default()
                },
                GraphStanding {
                    graph: "local".into(),
                    ..GraphStanding::default()
                },
            ],
            ..Provenance::default()
        };
        assert_eq!(contributing(Some(&p)), vec!["local", "mirror/wikidata"]);
        assert!(contributing(None).is_empty());
    }
}
