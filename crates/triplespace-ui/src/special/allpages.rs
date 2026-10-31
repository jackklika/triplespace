//! `Special:AllPages` and `Special:PrefixIndex` (0047 §4.4; 21 §2.4): a namespace's
//! document pages in title order, from `list=allpages`, with MediaWiki's parameters.
//!
//! - **`AllPages`**: `from` (or the subpage), `to`, `namespace`, `hideredirects`.
//! - **`PrefixIndex`**: `prefix` (or the subpage), `namespace`, `hideredirects`,
//!   `stripprefix`, and `from` to continue.
//!
//! A title given with a namespace (`Special:PrefixIndex/Project:Help`) picks that
//! namespace. Titles are listed without the namespace, in columns, a redirect in italics,
//! which a second, standard call finds (`apfilterredir=redirects` over the same titles),
//! since `list=allpages` does not mark redirects. **Previous page** and **Next page**
//! links walk the list. The entity namespaces hold entities, not pages (0008 §1), so they
//! are not offered; asked for one, the page says so and points to search.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::{Kind as NsKind, Namespaces};
use serde::Serialize;
use triplespace_client::{ClientError, Incoming, NamespaceRow, PageListQuery};

use crate::Site;
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::changes::FormItem;

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/allpages.html` draws.
// ---------------------------------------------------------------------------------------

/// The form: the start or prefix, the namespace, and the boxes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AllPagesForm {
    /// The page it asks.
    pub title: String,
    /// The controls, as the changes form draws them.
    pub items: Vec<FormItem>,
    /// **Go**.
    pub go: String,
}

/// One title in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Listed {
    /// Its page.
    pub href: String,
    /// The title as shown: without its namespace, and without the prefix where asked.
    pub text: String,
    /// Whether it is a redirect.
    pub redirect: bool,
}

/// **Previous page** and **Next page**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListPager {
    /// The accessible name.
    pub label: String,
    /// The previous page.
    pub previous: Option<Choice>,
    /// The next page.
    pub next: Option<Choice>,
}

/// The list of titles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TitleList {
    /// The titles, in order.
    pub titles: Vec<Listed>,
    /// The links to the pages either side, where there are any.
    pub pager: Option<ListPager>,
}

/// `Special:AllPages` and `Special:PrefixIndex`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AllPagesPage {
    /// The form.
    pub form: AllPagesForm,
    /// The list.
    pub list: Option<TitleList>,
    /// What the page says when it lists nothing.
    pub empty: Option<String>,
    /// The link to search, for a namespace that holds entities.
    pub search: Option<Choice>,
}

/// What `list=allpages` found for the page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Listing {
    /// Full title, shown text, redirect.
    titles: Vec<(String, String, bool)>,
    /// Where the previous page starts.
    previous: Option<String>,
    /// Where the next page starts.
    next: Option<String>,
}

/// Titles to a page, as MediaWiki's `AllPages` shows.
const LIMIT: u32 = 345;

/// The two pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:AllPages`.
    AllPages,
    /// `Special:PrefixIndex`.
    PrefixIndex,
}

impl Which {
    /// The page a special page's name names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "AllPages" => Some(Self::AllPages),
            "PrefixIndex" => Some(Self::PrefixIndex),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::AllPages => "Special:AllPages",
            Self::PrefixIndex => "Special:PrefixIndex",
        }
    }
}

/// What the page was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    which: Which,
    namespace: i64,
    from: String,
    to: String,
    prefix: String,
    hide_redirects: bool,
    strip_prefix: bool,
}

/// A namespace by any name a reader may type: local, canonical or alias.
fn namespace_named(rows: &[NamespaceRow], name: &str) -> Option<i64> {
    let name = name.replace('_', " ");
    let name = name.trim();
    rows.iter()
        .find(|n| {
            n.id != 0
                && (n.name.eq_ignore_ascii_case(name)
                    || n.canonical
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(name))
                    || n.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
        })
        .map(|n| n.id)
}

/// A typed title's namespace, where its prefix names one, and the rest.
fn split(rows: &[NamespaceRow], text: &str) -> (Option<i64>, String) {
    let text = text.replace('_', " ");
    if let Some((prefix, rest)) = text.split_once(':')
        && let Some(id) = namespace_named(rows, prefix)
    {
        return (Some(id), rest.trim().to_string());
    }
    (None, text.trim().to_string())
}

/// Whether the namespace holds document pages a list may show.
fn listable(id: i64) -> bool {
    i32::try_from(id)
        .ok()
        .and_then(|n| Namespaces::embedded().get(n))
        .is_some_and(|n| n.kind == NsKind::Pages && !n.is_entity_namespace())
}

impl Ask {
    fn of(which: Which, title: &str, q: &BTreeMap<String, String>, rows: &[NamespaceRow]) -> Self {
        let text = |k: &str| q.get(k).map(|v| v.replace('_', " ").trim().to_string());
        let subpage = title
            .strip_prefix("Special:")
            .and_then(|t| t.split_once('/'))
            .map(|(_, rest)| rest.to_string())
            .filter(|s| !s.trim().is_empty());
        let mut namespace = q
            .get("namespace")
            .and_then(|n| n.trim().parse::<i64>().ok())
            .unwrap_or(0);
        // The subpage (or `from`, or `prefix`) may carry a namespace, which wins.
        let lead = match which {
            Which::AllPages => subpage.clone().or_else(|| text("from")),
            Which::PrefixIndex => subpage.clone().or_else(|| text("prefix")),
        }
        .unwrap_or_default();
        let (ns, lead) = split(rows, &lead);
        if let Some(ns) = ns {
            namespace = ns;
        }
        let flag = |k: &str| q.get(k).is_some_and(|v| !v.is_empty() && v != "0");
        let (from, prefix) = match which {
            Which::AllPages => (lead, String::new()),
            Which::PrefixIndex => (text("from").unwrap_or_default(), lead),
        };
        Self {
            which,
            namespace,
            from,
            to: if which == Which::AllPages {
                text("to").map(|t| split(rows, &t).1).unwrap_or_default()
            } else {
                String::new()
            },
            prefix,
            hide_redirects: flag("hideredirects"),
            strip_prefix: flag("stripprefix"),
        }
    }

    fn query(&self, from: &str, descending: bool, limit: u32) -> PageListQuery {
        PageListQuery {
            namespace: self.namespace,
            from: from.to_string(),
            to: if descending {
                String::new()
            } else {
                self.to.clone()
            },
            prefix: self.prefix.clone(),
            filter_redirects: if self.hide_redirects {
                "nonredirects".into()
            } else {
                String::new()
            },
            descending,
            limit,
        }
    }

    /// This page starting at another title.
    fn href(&self, from: &str) -> String {
        let mut q: Vec<(&str, String)> = vec![("title", self.which.title().to_string())];
        if self.which == Which::PrefixIndex {
            q.push(("prefix", self.prefix.clone()));
        }
        q.push(("from", from.to_string()));
        if !self.to.is_empty() {
            q.push(("to", self.to.clone()));
        }
        q.push(("namespace", self.namespace.to_string()));
        if self.hide_redirects {
            q.push(("hideredirects", "1".into()));
        }
        if self.strip_prefix {
            q.push(("stripprefix", "1".into()));
        }
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(q)
            .finish();
        format!("/w/index.php?{query}")
    }
}

/// A title without its namespace's prefix.
fn text_of(title: &str, namespace: i64, rows: &[NamespaceRow]) -> String {
    if namespace == 0 {
        return title.to_string();
    }
    title
        .split_once(':')
        .filter(|(p, _)| namespace_named(rows, p) == Some(namespace))
        .map_or_else(|| title.to_string(), |(_, rest)| rest.to_string())
}

/// `GET Special:AllPages` and `Special:PrefixIndex`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Pages) {
        return pages::no_page(&cx, headers, which.title().to_string());
    }
    let rows = match site.client().namespaces(&incoming).await {
        Ok(f) => {
            cx.inputs.push(f.cache);
            f.value
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let ask = Ask::of(which, title, &params.map, &rows);
    let listing = if listable(ask.namespace) {
        match list(site, &mut cx, &incoming, &ask, &rows).await {
            Ok(l) => Some(l),
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    } else {
        None
    };
    let model = page_model(&cx.m, &ask, &rows, listing.as_ref());
    let body = cx.draw("special/allpages.html", model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(match which {
            Which::AllPages => "ts-allpages-title",
            Which::PrefixIndex => "ts-prefixindex-title",
        }),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    })
}

/// The page's model.
fn page_model(
    m: &Messages,
    ask: &Ask,
    rows: &[NamespaceRow],
    listing: Option<&Listing>,
) -> AllPagesPage {
    let form = form_model(m, ask, rows);
    let Some(listing) = listing else {
        return AllPagesPage {
            form,
            list: None,
            empty: Some(m.get("ts-allpages-not-pages")),
            search: Some(Choice {
                href: "/wiki/Special:Search".into(),
                label: m.get("ts-search-title"),
                current: false,
            }),
        };
    };
    if listing.titles.is_empty() {
        return AllPagesPage {
            form,
            list: None,
            empty: Some(m.get("ts-allpages-none")),
            search: None,
        };
    }
    let titles = listing
        .titles
        .iter()
        .map(|(full, shown, redirect)| Listed {
            href: format!("/wiki/{}", title_url(full)),
            text: shown.clone(),
            redirect: *redirect,
        })
        .collect();
    let previous = listing.previous.as_ref().map(|p| Choice {
        href: ask.href(p),
        label: m.with("ts-allpages-previous", &[p]),
        current: false,
    });
    let next = listing.next.as_ref().map(|n| Choice {
        href: ask.href(n),
        label: m.with("ts-allpages-next", &[n]),
        current: false,
    });
    let pager = (previous.is_some() || next.is_some()).then(|| ListPager {
        label: m.get("ts-search-pages-label"),
        previous,
        next,
    });
    AllPagesPage {
        form,
        list: Some(TitleList { titles, pager }),
        empty: None,
        search: None,
    }
}

/// The form's model.
fn form_model(m: &Messages, ask: &Ask, rows: &[NamespaceRow]) -> AllPagesForm {
    let text = |name: &str, label: &str, value: &str| FormItem::Text {
        label: m.get(label),
        name: name.to_string(),
        value: value.to_string(),
    };
    let check = |name: &str, label: &str, on: bool| FormItem::Check {
        name: name.to_string(),
        value: "1".into(),
        checked: on,
        label: m.get(label),
    };
    let mut items = match ask.which {
        Which::AllPages => vec![
            text("from", "ts-allpages-from", &ask.from),
            text("to", "ts-allpages-to", &ask.to),
        ],
        Which::PrefixIndex => vec![text("prefix", "ts-prefixindex-prefix", &ask.prefix)],
    };
    items.push(FormItem::Select {
        label: m.get("ts-changes-namespace"),
        name: "namespace".into(),
        options: rows
            .iter()
            .filter(|n| listable(n.id))
            .map(|n| {
                let name = if n.id == 0 {
                    m.get("ts-namespaceinfo-main")
                } else {
                    n.name.clone()
                };
                (n.id.to_string(), name, n.id == ask.namespace)
            })
            .collect(),
    });
    items.push(check(
        "hideredirects",
        "ts-allpages-hideredirects",
        ask.hide_redirects,
    ));
    if ask.which == Which::PrefixIndex {
        items.push(check(
            "stripprefix",
            "ts-prefixindex-stripprefix",
            ask.strip_prefix,
        ));
    }
    AllPagesForm {
        title: ask.which.title().to_string(),
        items,
        go: m.get("ts-allpages-go"),
    }
}

/// The list, its redirects marked, and where the pages either side start.
async fn list(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    ask: &Ask,
    rows: &[NamespaceRow],
) -> Result<Listing, ClientError> {
    let client = site.client();
    let found = client
        .all_pages(incoming, &ask.query(&ask.from, false, LIMIT))
        .await?;
    cx.inputs.push(found.cache);
    let titles = found.value.titles;
    if titles.is_empty() {
        return Ok(Listing::default());
    }
    let texts: Vec<String> = titles
        .iter()
        .map(|t| text_of(t, ask.namespace, rows))
        .collect();
    // The redirects among them: one more call over the same range.
    let mut redirects = Vec::new();
    if !ask.hide_redirects {
        let mut q = ask.query(&texts[0], false, LIMIT);
        q.to.clone_from(&texts[texts.len() - 1]);
        q.filter_redirects = "redirects".into();
        let r = client.all_pages(incoming, &q).await?;
        cx.inputs.push(r.cache);
        redirects = r.value.titles;
    }
    // Where the previous page starts: `LIMIT` titles back from this one's first.
    let mut previous = None;
    if !ask.from.is_empty() {
        let back = client
            .all_pages(incoming, &ask.query(&texts[0], true, LIMIT + 1))
            .await?;
        cx.inputs.push(back.cache);
        let earlier: Vec<&String> = back
            .value
            .titles
            .iter()
            .filter(|t| text_of(t, ask.namespace, rows) != texts[0])
            .collect();
        if let Some(start) = earlier.get(LIMIT as usize - 1).or(earlier.last()) {
            previous = Some(text_of(start, ask.namespace, rows));
        }
    }
    let listed = titles
        .iter()
        .zip(&texts)
        .map(|(full, text)| {
            let shown = if ask.strip_prefix && ask.which == Which::PrefixIndex {
                text.strip_prefix(&ask.prefix)
                    .filter(|t| !t.is_empty())
                    .unwrap_or(text)
            } else {
                text
            };
            (full.clone(), shown.to_string(), redirects.contains(full))
        })
        .collect();
    Ok(Listing {
        titles: listed,
        previous,
        next: found.value.next,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<NamespaceRow> {
        vec![
            NamespaceRow {
                id: 0,
                ..NamespaceRow::default()
            },
            NamespaceRow {
                id: 4,
                name: "Librarybase".into(),
                canonical: Some("Project".into()),
                aliases: vec!["LB".into()],
                ..NamespaceRow::default()
            },
        ]
    }

    fn q(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_namespace_in_the_title_wins() {
        let a = Ask::of(
            Which::PrefixIndex,
            "Special:PrefixIndex/Project:Help_desk",
            &q(&[("namespace", "0")]),
            &rows(),
        );
        assert_eq!((a.namespace, a.prefix.as_str()), (4, "Help desk"));
        let a = Ask::of(
            Which::AllPages,
            "Special:AllPages",
            &q(&[("from", "LB:M"), ("hideredirects", "1")]),
            &rows(),
        );
        assert_eq!(
            (a.namespace, a.from.as_str(), a.hide_redirects),
            (4, "M", true)
        );
        let a = Ask::of(
            Which::AllPages,
            "Special:AllPages/Douglas: a life",
            &q(&[]),
            &rows(),
        );
        assert_eq!((a.namespace, a.from.as_str()), (0, "Douglas: a life"));
    }

    /// The page's model (19 §5.10): the form's controls, the titles with redirects
    /// marked, the pager, and what the page says for an empty list or an entity namespace.
    #[test]
    fn the_page_is_a_model() {
        let m = Messages::for_language("en");
        let a = Ask::of(
            Which::PrefixIndex,
            "Special:PrefixIndex",
            &q(&[
                ("prefix", "Analytics/"),
                ("namespace", "4"),
                ("stripprefix", "1"),
            ]),
            &rows(),
        );
        let listing = Listing {
            titles: vec![
                ("Librarybase:Analytics/Orbit".into(), "Orbit".into(), false),
                ("Librarybase:Analytics/Orb".into(), "Orb".into(), true),
            ],
            previous: None,
            next: Some("Analytics/P".into()),
        };
        let p = page_model(&m, &a, &rows(), Some(&listing));
        let names: Vec<&str> = p
            .form
            .items
            .iter()
            .map(|i| match i {
                FormItem::Text { name, .. }
                | FormItem::Check { name, .. }
                | FormItem::Select { name, .. } => name.as_str(),
                FormItem::Kinds { .. } => "kinds",
            })
            .collect();
        assert_eq!(
            names,
            ["prefix", "namespace", "hideredirects", "stripprefix"]
        );
        let list = p.list.unwrap();
        assert_eq!(list.titles[0].href, "/wiki/Librarybase:Analytics/Orbit");
        assert!(list.titles[1].redirect && !list.titles[0].redirect);
        let pager = list.pager.unwrap();
        assert!(pager.previous.is_none());
        assert!(pager.next.unwrap().href.contains("from=Analytics%2FP"));

        let empty = page_model(&m, &a, &rows(), Some(&Listing::default()));
        assert!(empty.list.is_none() && empty.empty.is_some() && empty.search.is_none());
        let entities = page_model(&m, &a, &rows(), None);
        assert_eq!(entities.search.unwrap().href, "/wiki/Special:Search");
    }

    #[test]
    fn titles_are_shown_without_their_namespace() {
        assert_eq!(text_of("Librarybase:Home", 4, &rows()), "Home");
        assert_eq!(text_of("Douglas: a life", 0, &rows()), "Douglas: a life");
        assert!(listable(0) && listable(4) && !listable(120) && !listable(-1));
    }
}
