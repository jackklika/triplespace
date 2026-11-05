//! `Special:Search` (0010 §3; 0047 §9): the full results page, readable without
//! JavaScript.
//!
//! - **Go to.** Text that names an entity (an ID such as `Q42` or `WDQ65`, a page title
//!   such as `Item:Q42` or `Domain:wikipedia.org`, or a bare domain key such as
//!   `wikipedia.org`) goes straight to its page when it exists. As in MediaWiki, the
//!   header's box asks for this (no `fulltext`), and the results page's own form does
//!   not: there, an entity the text names is offered as **Go to** above the results.
//!   Text that is sure to be an ID (a local ID, or a namespaced title) and names nothing
//!   says so; text that merely could be one (a foreign ID's shape, a dotted word) falls
//!   through to the results quietly.
//! - **Results** are grouped by kind (items, properties, domains), seven of each, with
//!   a link to the rest of a kind. `ns{n}=1` (120, 122, 210) asks for one kind, paged by
//!   `offset` and `limit` (MediaWiki's parameters).
//!
//! The search itself is the API's `wbsearchentities`: labels and aliases, by prefix, in
//! the interface language and its fallbacks (0014 §8). Page titles and page text join
//! when the pages stack does (0010 §3).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde::Serialize;
use triplespace_client::{ClientError, Fetched, Hit, Hits, SiteInfo};

use crate::Site;
use crate::codex::{
    Chip, ChipKind, ChipModel, Message, MessageKind, MessageModel, SearchInput, SearchModel,
};
use crate::entity;
use crate::entity::history::{Choice, Switch};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};

/// A kind of entity the search covers: its `wbsearchentities` type, its namespace, and
/// the messages of its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Kind {
    entity_type: &'static str,
    ns: u32,
    heading: &'static str,
    more: &'static str,
}

const KINDS: [Kind; 3] = [
    Kind {
        entity_type: "item",
        ns: 120,
        heading: "ts-search-group-item",
        more: "ts-search-more-item",
    },
    Kind {
        entity_type: "property",
        ns: 122,
        heading: "ts-search-group-property",
        more: "ts-search-more-property",
    },
    Kind {
        entity_type: "domain",
        ns: 210,
        heading: "ts-search-group-domain",
        more: "ts-search-more-domain",
    },
];

/// Results per kind when every kind is shown.
const PER_GROUP: u32 = 7;
/// Results per page for one kind, unless `limit` says otherwise; and the most allowed.
const PER_PAGE: u32 = 20;
const MAX_PER_PAGE: u32 = 50;

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/search.html` draws.
// ---------------------------------------------------------------------------------------

/// A text in a language, which carries `lang` where it differs from the page's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LangText {
    /// The text.
    pub text: String,
    /// Its language, where it differs from the page's.
    pub lang: Option<String>,
}

/// A result's title: its label, else its key or ID in monospace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HitTitle {
    /// The text.
    pub text: String,
    /// The label's language, where it differs from the page's.
    pub lang: Option<String>,
    /// A key or an ID rather than a label.
    pub mono: bool,
    /// A keyed entity's key, left to right whatever the page's direction.
    pub ltr: bool,
}

/// "Matched the alias …": the message either side of the alias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AliasMatch {
    /// The message before the alias.
    pub before: String,
    /// The alias.
    pub alias: LangText,
    /// The message after it.
    pub after: String,
}

/// One result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HitModel {
    /// The entity's page.
    pub href: String,
    /// The title.
    pub title: HitTitle,
    /// The ID's chip; a keyed entity has none.
    pub chip: Option<ChipModel>,
    /// The description.
    pub description: Option<LangText>,
    /// The alias that matched, where an alias did.
    pub alias: Option<AliasMatch>,
}

/// One kind's results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupModel {
    /// The heading's `id`.
    pub id: String,
    /// The heading.
    pub heading: String,
    /// The results.
    pub hits: Vec<HitModel>,
    /// The link to the rest of the kind, when every kind is shown and there are more.
    pub more: Option<Choice>,
}

/// The previous and next pages of one kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchPager {
    /// The accessible name.
    pub label: String,
    /// The previous page.
    pub previous: Option<Choice>,
    /// The next page.
    pub next: Option<Choice>,
}

/// The results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResultsModel {
    /// The kind filter.
    pub filter: Switch,
    /// Each kind's group.
    pub groups: Vec<GroupModel>,
    /// What the page says when nothing matched.
    pub empty: Option<String>,
    /// The pager, for one kind.
    pub pager: Option<SearchPager>,
}

/// `Special:Search`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SearchPage {
    /// The form's accessible name.
    pub label: String,
    /// The namespaces the form keeps, for one kind.
    pub namespaces: Vec<u32>,
    /// The search box.
    pub input: SearchModel,
    /// That the text names an entity that does not exist.
    pub missing: Option<MessageModel>,
    /// **Go to** the entity the text names.
    pub go: Option<Choice>,
    /// What the page says before anything is asked.
    pub intro: Option<String>,
    /// The results.
    pub results: Option<ResultsModel>,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

/// A text's language where it differs from the page's.
fn lang_of(text_lang: &str, page_lang: &str) -> Option<String> {
    (!text_lang.is_empty() && text_lang != page_lang).then(|| text_lang.to_string())
}

/// The ID's chip: the local ID, or the foreign ID in its provider's colours. A keyed
/// entity's key is its label already, so it has none.
fn id_chip_model(id: &str) -> Option<ChipModel> {
    match EntityId::parse(id).map(|e| e.form()) {
        Ok(IdForm::Keyed) => None,
        Ok(IdForm::Foreign) => Some(Chip::new(ChipKind::Provider(id[..2].to_string()), id).model()),
        _ => Some(Chip::new(ChipKind::Local, id).model()),
    }
}

/// One result's model.
fn hit_model(m: &Messages, h: &Hit, kind: Kind) -> HitModel {
    let lang = m.lang();
    let title = match &h.label {
        Some((text, l)) => HitTitle {
            text: text.clone(),
            lang: lang_of(l, lang),
            mono: false,
            ltr: false,
        },
        None => match EntityId::parse(&h.id)
            .ok()
            .and_then(|e| e.keyed_parts().map(|(_, k)| k.to_string()))
        {
            Some(key) => HitTitle {
                text: key,
                lang: None,
                mono: true,
                ltr: true,
            },
            None => HitTitle {
                text: h.id.clone(),
                lang: None,
                mono: true,
                ltr: false,
            },
        },
    };
    let alias = (h.match_type == "alias").then(|| {
        let (text, l) = &h.match_text;
        let message = m.with("ts-search-matched-alias", &["\u{E000}"]);
        let (before, after) = message
            .split_once('\u{E000}')
            .map_or((message.as_str(), ""), |(b, a)| (b, a));
        AliasMatch {
            before: before.to_string(),
            alias: LangText {
                text: text.clone(),
                lang: lang_of(l, lang),
            },
            after: after.to_string(),
        }
    });
    HitModel {
        href: entity::entity_href(&h.id, kind.entity_type),
        title,
        chip: id_chip_model(&h.id),
        description: h.description.as_ref().map(|(text, l)| LangText {
            text: text.clone(),
            lang: lang_of(l, lang),
        }),
        alias,
    }
}

/// The results' model: the kind filter, each kind's group, and the pager.
fn results_model(m: &Messages, ask: &Ask, results: &[(Kind, Hits)]) -> ResultsModel {
    let text = &ask.text;
    let current = ask.single.then(|| ask.kinds[0]);
    let mut items = vec![Choice {
        href: results_href(text, None, 0, None),
        label: m.get("ts-search-filter-all"),
        current: current.is_none(),
    }];
    for k in KINDS {
        items.push(Choice {
            href: results_href(text, Some(k), 0, None),
            label: m.get(k.heading),
            current: current == Some(k),
        });
    }
    let filter = Switch {
        label: m.get("ts-search-filter-label"),
        items,
    };
    let any = results.iter().any(|(_, h)| !h.hits.is_empty());
    let groups = if any {
        results
            .iter()
            .filter(|(_, h)| !h.hits.is_empty() || ask.single)
            .map(|(k, h)| GroupModel {
                id: format!("ts-results-{}", k.entity_type),
                heading: m.get(k.heading),
                hits: h.hits.iter().map(|x| hit_model(m, x, *k)).collect(),
                more: (!ask.single && h.next.is_some())
                    .then(|| choice(results_href(text, Some(*k), 0, None), m.get(k.more))),
            })
            .collect()
    } else {
        Vec::new()
    };
    let pager = match (ask.single, results.first()) {
        (true, Some((k, h))) if ask.offset > 0 || h.next.is_some() => {
            let custom = (ask.limit != PER_PAGE).then_some(ask.limit);
            Some(SearchPager {
                label: m.get("ts-search-pages-label"),
                previous: (ask.offset > 0).then(|| {
                    choice(
                        results_href(text, Some(*k), ask.offset.saturating_sub(ask.limit), custom),
                        m.get("ts-search-previous"),
                    )
                }),
                next: h.next.map(|n| {
                    choice(
                        results_href(text, Some(*k), n, custom),
                        m.get("ts-search-next"),
                    )
                }),
            })
        }
        _ => None,
    };
    ResultsModel {
        filter,
        groups,
        empty: (!any).then(|| m.with("ts-search-none", &[text])),
        pager,
    }
}

/// The page's model.
fn page_model(
    m: &Messages,
    ask: &Ask,
    missing: Option<&str>,
    go: Option<&str>,
    results: &[(Kind, Hits)],
) -> SearchPage {
    SearchPage {
        label: m.get("ts-search-title"),
        namespaces: if ask.single {
            ask.kinds.iter().map(|k| k.ns).collect()
        } else {
            Vec::new()
        },
        input: SearchInput {
            name: "search",
            value: &ask.text,
            placeholder: &m.get("ts-search-placeholder"),
            label: &m.get("ts-search-label"),
            button: &m.get("ts-search-button"),
        }
        .model(),
        missing: missing.map(|title| {
            Message::block(
                MessageKind::Warning,
                &m.with("ts-search-no-entity", &[title]),
            )
            .model()
        }),
        go: go.map(|title| {
            choice(
                format!("/wiki/{}", title_url(title)),
                m.with("ts-search-go-to", &[title]),
            )
        }),
        intro: ask.text.is_empty().then(|| m.get("ts-search-intro")),
        results: (!ask.text.is_empty()).then(|| results_model(m, ask, results)),
    }
}

/// The namespaces whose pages are entities, with the type of entity each holds.
const NAMESPACES: [(&str, &str); 5] = [
    ("item", "item"),
    ("property", "property"),
    ("domain", "domain"),
    ("keyword", "keyword"),
    ("notation", "notation"),
];

/// Whether a local ID's shape: a letter, then digits only.
fn local_shape(id: &EntityId) -> bool {
    id.form() == IdForm::Local
}

/// Whether a foreign ID begins with a provider code and one of its type codes that the
/// site knows, so that `Paris` (which has the shape of a foreign ID) is not taken for one.
fn known_foreign(id: &str, site: &SiteInfo) -> bool {
    site.providers.iter().any(|p| {
        id.get(..2) == Some(p.code.as_str())
            && p.types
                .iter()
                .any(|t| id.get(2..3) == Some(t.code.as_str()))
    }) && id.len() > 3
}

/// The page title the text names, if it looks like an ID or key, and whether it surely
/// is one (a local ID, or a title in an entity namespace) rather than possibly.
#[must_use]
pub fn go_title(text: &str, site: &SiteInfo) -> Option<(String, bool)> {
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((prefix, rest)) = t.split_once(':') {
        let lower = prefix.to_ascii_lowercase();
        let (_, kind) = NAMESPACES.iter().find(|(ns, _)| *ns == lower)?;
        if rest.is_empty() {
            return None;
        }
        return match *kind {
            "item" | "property" => {
                let id = EntityId::parse(rest).ok()?;
                (id.form() != IdForm::Keyed).then(|| (entity::page_title(id.as_str(), kind), true))
            }
            keyed => {
                let key = if keyed == "domain" {
                    rest.to_lowercase()
                } else {
                    rest.to_string()
                };
                Some((entity::page_title(&format!("{keyed}:{key}"), keyed), true))
            }
        };
    }
    if let Ok(id) = EntityId::parse(t) {
        if local_shape(&id) {
            return Some((
                entity::page_title(id.as_str(), &entity::entity_type_of(id.as_str())),
                true,
            ));
        }
        if id.form() == IdForm::Foreign && known_foreign(id.as_str(), site) {
            return Some((
                entity::page_title(id.as_str(), &entity::entity_type_of(id.as_str())),
                false,
            ));
        }
    }
    let dotted = t.contains('.')
        && !t.starts_with('.')
        && !t.ends_with('.')
        && t.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '-'));
    dotted.then(|| (format!("Domain:{}", t.to_lowercase()), false))
}

/// The kinds `ns{n}` asks for; every kind when it asks for none it knows.
fn wanted(query: &BTreeMap<String, String>) -> Vec<Kind> {
    let asked: Vec<Kind> = KINDS
        .iter()
        .copied()
        .filter(|k| {
            query
                .get(&format!("ns{}", k.ns))
                .is_some_and(|v| !v.is_empty() && v != "0")
        })
        .collect();
    if asked.is_empty() {
        KINDS.to_vec()
    } else {
        asked
    }
}

/// A link to the results for `text`, optionally of one kind and from an offset.
fn results_href(text: &str, kind: Option<Kind>, offset: u32, limit: Option<u32>) -> String {
    let mut s = format!(
        "/w/index.php?title=Special:Search&search={}&fulltext=1",
        utf8_percent_encode(text, NON_ALPHANUMERIC)
    );
    if let Some(k) = kind {
        let _ = write!(s, "&ns{}=1", k.ns);
    }
    if offset > 0 {
        let _ = write!(s, "&offset={offset}");
    }
    if let Some(l) = limit {
        let _ = write!(s, "&limit={l}");
    }
    s
}

/// A number from the query, within bounds.
fn number(query: &BTreeMap<String, String>, key: &str, default: u32, max: u32) -> u32 {
    query
        .get(key)
        .and_then(|v| v.trim().parse::<u32>().ok())
        .map_or(default, |n| n.min(max))
}

/// A `302` to an entity's page, cached as its inputs allow.
fn redirect(cx: &Context, title: &str) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    if let Ok(v) = HeaderValue::from_str(&format!("/wiki/{}", title_url(title))) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    pages::caching(r.headers_mut(), None, &cx.inputs);
    r
}

/// The entity the text names, if it exists: its page title. An ID the API refuses
/// outright (`invalid-entity-id`) names nothing.
async fn existing(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    title: &str,
) -> Result<Option<String>, ClientError> {
    match site
        .client()
        .entities(incoming, &[title.to_string()], "info", "")
        .await
    {
        Ok(Fetched { value, cache }) => {
            cx.inputs.push(cache);
            Ok(value
                .found
                .iter()
                .next()
                .map(|(id, v)| entity::page_title(id, v["type"].as_str().unwrap_or("item"))))
        }
        Err(ClientError::Api { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// What the query asks for.
struct Ask {
    text: String,
    kinds: Vec<Kind>,
    single: bool,
    limit: u32,
    offset: u32,
}

impl Ask {
    fn of(query: &BTreeMap<String, String>) -> Self {
        let kinds = wanted(query);
        let single = kinds.len() == 1;
        let (limit, offset) = if single {
            (
                number(query, "limit", PER_PAGE, MAX_PER_PAGE).max(1),
                number(query, "offset", 0, 10_000),
            )
        } else {
            (PER_GROUP, 0)
        };
        Self {
            text: query.get("search").map_or("", |s| s.trim()).to_string(),
            kinds,
            single,
            limit,
            offset,
        }
    }
}

/// Searches each kind asked for, in parallel.
async fn fetch(
    site: &Site,
    incoming: &triplespace_client::Incoming,
    ask: &Ask,
    lang: &str,
) -> Result<Vec<(Kind, Fetched<Hits>)>, ClientError> {
    let client = site.client();
    let run = |k: Kind| {
        let client = &client;
        async move {
            if ask.kinds.contains(&k) {
                Some(
                    client
                        .search(
                            incoming,
                            &ask.text,
                            lang,
                            k.entity_type,
                            ask.limit,
                            ask.offset,
                        )
                        .await,
                )
            } else {
                None
            }
        }
    };
    let (a, b, c) = tokio::join!(run(KINDS[0]), run(KINDS[1]), run(KINDS[2]));
    let mut out = Vec::new();
    for (k, r) in KINDS.into_iter().zip([a, b, c]) {
        if let Some(r) = r {
            out.push((k, r?));
        }
    }
    Ok(out)
}

/// `Special:Search`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Search) {
        return pages::no_page(&cx, headers, "Special:Search".into());
    }
    let ask = Ask::of(query);

    let mut go = None;
    let mut missing = None;
    if let Some((title, sure)) = go_title(&ask.text, &cx.site) {
        match existing(site, &mut cx, &incoming, &title).await {
            Ok(Some(found)) => go = Some(found),
            Ok(None) if sure => missing = Some(title),
            Ok(None) => {}
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    if let (Some(title), false) = (&go, query.contains_key("fulltext")) {
        return redirect(&cx, title);
    }

    let mut results = Vec::new();
    if !ask.text.is_empty() {
        let lang = cx.m.lang().to_string();
        match fetch(site, &incoming, &ask, &lang).await {
            Ok(r) => {
                for (k, f) in r {
                    cx.inputs.push(f.cache);
                    results.push((k, f.value));
                }
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }

    let model = page_model(&cx.m, &ask, missing.as_deref(), go.as_deref(), &results);
    let body = cx.draw("special/search.html", model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-search-title"),
        returnto: Some("Special:Search".into()),
        body,
        search: ask.text,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::{ProviderInfo, ProviderType};

    fn site() -> SiteInfo {
        SiteInfo {
            providers: vec![ProviderInfo {
                code: "WD".into(),
                slug: "wikidata".into(),
                types: vec![ProviderType {
                    code: "Q".into(),
                    entity_type: "item".into(),
                    ..ProviderType::default()
                }],
                ..ProviderInfo::default()
            }],
            ..SiteInfo::default()
        }
    }

    #[test]
    fn go_titles() {
        let s = site();
        let go = |t: &str| go_title(t, &s);
        assert_eq!(go("Q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("P3"), Some(("Property:P3".into(), true)));
        assert_eq!(go("item:q6"), Some(("Item:Q6".into(), true)));
        assert_eq!(go("WDQ65"), Some(("Item:WDQ65".into(), false)));
        assert_eq!(
            go("Domain:Wikipedia.org"),
            Some(("Domain:wikipedia.org".into(), true))
        );
        assert_eq!(
            go("wikipedia.org"),
            Some(("Domain:wikipedia.org".into(), false))
        );
        assert_eq!(go("Paris"), None, "a word with a foreign ID's shape");
        assert_eq!(go("six and more"), None);
        assert_eq!(go("Talk:Q6"), None);
        assert_eq!(go(""), None);
    }

    #[test]
    fn kinds_from_the_query() {
        let mut q = BTreeMap::new();
        assert_eq!(wanted(&q).len(), 3);
        q.insert("ns122".into(), "1".into());
        assert_eq!(wanted(&q), vec![KINDS[1]]);
        q.insert("ns122".into(), "0".into());
        assert_eq!(wanted(&q).len(), 3);
    }

    fn hit(id: &str, label: Option<(&str, &str)>) -> Hit {
        Hit {
            id: id.into(),
            title: String::new(),
            label: label.map(|(t, l)| (t.into(), l.into())),
            description: Some(("a number".into(), "en".into())),
            match_type: "label".into(),
            match_text: (String::new(), String::new()),
        }
    }

    /// A result's model (19 §5.10): the label with its language where it differs, the
    /// alias that matched split around the message, the chip, none for a keyed entity.
    #[test]
    fn a_hit_is_a_model() {
        let m = Messages::for_language("en");
        let mut h = hit("Q6", Some(("Sechs", "de")));
        h.match_type = "alias".into();
        h.match_text = ("half dozen".into(), "en".into());
        let model = hit_model(&m, &h, KINDS[0]);
        assert_eq!(model.href, "/wiki/Item:Q6");
        assert_eq!(model.title.lang.as_deref(), Some("de"));
        assert!(!model.title.mono);
        assert_eq!(model.chip.as_ref().map(|c| c.text.as_str()), Some("Q6"));
        assert_eq!(model.description.as_ref().unwrap().lang, None);
        let alias = model.alias.unwrap();
        assert_eq!(
            (
                alias.before.as_str(),
                alias.alias.text.as_str(),
                alias.after.as_str()
            ),
            ("Also known as ", "half dozen", "")
        );
        let keyed = hit_model(&m, &hit("domain:wikipedia.org", None), KINDS[2]);
        assert_eq!(
            (keyed.title.text.as_str(), keyed.title.mono, keyed.title.ltr),
            ("wikipedia.org", true, true)
        );
        assert!(keyed.chip.is_none());
    }

    /// The results' model: the filter marks the kind asked for, a kind with more links
    /// to it, and one kind is paged.
    #[test]
    fn results_are_a_model() {
        let m = Messages::for_language("en");
        let mut q = BTreeMap::new();
        q.insert("search".to_string(), "n".to_string());
        let all = Ask::of(&q);
        let hits = |n: usize, next: Option<u32>| Hits {
            hits: (0..n).map(|i| hit(&format!("Q{}", i + 1), None)).collect(),
            next,
        };
        let r = results_model(
            &m,
            &all,
            &[
                (KINDS[0], hits(2, Some(7))),
                (KINDS[1], hits(0, None)),
                (KINDS[2], hits(1, None)),
            ],
        );
        assert!(r.filter.items[0].current);
        assert_eq!(r.groups.len(), 2, "a kind with no hits is left out");
        assert!(r.groups[0].more.is_some() && r.groups[1].more.is_none());
        assert!(r.pager.is_none() && r.empty.is_none());

        q.insert("ns122".to_string(), "1".to_string());
        q.insert("limit".to_string(), "1".to_string());
        q.insert("offset".to_string(), "1".to_string());
        let one = Ask::of(&q);
        let r = results_model(&m, &one, &[(KINDS[1], hits(1, Some(2)))]);
        assert!(r.filter.items[2].current && !r.filter.items[0].current);
        let pager = r.pager.unwrap();
        assert_eq!(
            pager.previous.unwrap().href,
            "/w/index.php?title=Special:Search&search=n&fulltext=1&ns122=1&limit=1"
        );
        assert_eq!(
            pager.next.unwrap().href,
            "/w/index.php?title=Special:Search&search=n&fulltext=1&ns122=1&offset=2&limit=1"
        );
        let none = results_model(&m, &one, &[(KINDS[1], hits(0, None))]);
        assert!(none.empty.is_some() && none.groups.is_empty());
    }

    /// The page's model: the namespaces the form keeps, **Go to**, the warning for an ID
    /// that names nothing, and the introduction before anything is asked.
    #[test]
    fn the_page_is_a_model() {
        let m = Messages::for_language("en");
        let mut q = BTreeMap::new();
        let p = page_model(&m, &Ask::of(&q), None, None, &[]);
        assert!(p.intro.is_some() && p.results.is_none() && p.namespaces.is_empty());
        q.insert("search".to_string(), "Q6".to_string());
        q.insert("ns120".to_string(), "1".to_string());
        let p = page_model(&m, &Ask::of(&q), Some("Item:Q404"), Some("Item:Q6"), &[]);
        assert_eq!(p.namespaces, vec![120]);
        assert_eq!(p.go.unwrap().href, "/wiki/Item:Q6");
        assert!(p.missing.unwrap().classes.contains("cdx-message--warning"));
        assert!(p.intro.is_none() && p.results.is_some());
    }

    #[test]
    fn links_encode_the_text() {
        assert_eq!(
            results_href("six & more", Some(KINDS[0]), 20, None),
            "/w/index.php?title=Special:Search&search=six%20%26%20more&fulltext=1&ns120=1&offset=20"
        );
    }
}
