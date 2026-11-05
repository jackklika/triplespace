//! `Special:AllMessages` (0047 §9; 0095 §8): the interface messages, each with the text
//! the build ships and, where the wiki overrides it, the wiki's text and a link to the
//! Interface page that holds it, from `meta=allmessages&amprop=default`.
//!
//! MediaWiki's parameters: `prefix` (the start of the key), `filter` (`all`, `modified`,
//! `unmodified`), `lang` (the viewer's interface language by default) and `limit`; and
//! `from`, the first key shown, which the **Next page** link carries.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::{INTERFACE, Namespaces};
use triplespace_client::{MessageQuery, MessageRow};

use crate::Site;
use crate::entity::history::Choice;
use crate::frame::{Page, title_url};
use crate::i18n;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::FormItem;

const TITLE: &str = "Special:AllMessages";
const LIMITS: &[usize] = &[20, 50, 100, 250, 500, 5000];
const DEFAULT_LIMIT: usize = 50;

/// What the page was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    prefix: String,
    filter: &'static str,
    lang: String,
    limit: usize,
    from: String,
}

impl Ask {
    fn of(q: &BTreeMap<String, String>, viewer_lang: &str) -> Self {
        let text = |k: &str| q.get(k).map(|v| v.trim().to_string()).unwrap_or_default();
        Self {
            prefix: text("prefix"),
            filter: match q.get("filter").map(String::as_str) {
                Some("modified") => "modified",
                Some("unmodified") => "unmodified",
                _ => "all",
            },
            lang: q
                .get("lang")
                .and_then(|l| i18n::valid_code(l.trim()))
                .unwrap_or_else(|| viewer_lang.to_string()),
            limit: q
                .get("limit")
                .and_then(|l| l.trim().parse::<usize>().ok())
                .filter(|l| LIMITS.contains(l))
                .unwrap_or(DEFAULT_LIMIT),
            from: text("from"),
        }
    }

    /// The query of this page starting at another key.
    fn href_from(&self, from: &str) -> String {
        let mut q = BTreeMap::new();
        q.insert("title", TITLE.to_string());
        if !self.prefix.is_empty() {
            q.insert("prefix", self.prefix.clone());
        }
        if self.filter != "all" {
            q.insert("filter", self.filter.to_string());
        }
        q.insert("lang", self.lang.clone());
        if self.limit != DEFAULT_LIMIT {
            q.insert("limit", self.limit.to_string());
        }
        q.insert("from", from.to_string());
        let query: String = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(q)
            .finish();
        format!("/w/index.php?{query}")
    }
}

/// The Interface page that overrides a message in a language: `Interface:{Key}`, with
/// `/{lang}` unless the language is the wiki's own (0095 §2).
fn interface_title(key: &str, lang: &str, wiki_lang: &str) -> String {
    let ns = Namespaces::embedded()
        .get(INTERFACE)
        .and_then(|n| n.name.clone().or_else(|| Some(n.canonical.clone())))
        .unwrap_or_else(|| "Interface".to_string());
    let mut chars = key.chars();
    let key: String = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    if lang == wiki_lang {
        format!("{ns}:{key}")
    } else {
        format!("{ns}:{key}/{lang}")
    }
}

/// `GET Special:AllMessages`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let ask = Ask::of(&params.map, cx.m.lang());
    let query = MessageQuery {
        prefix: ask.prefix.clone(),
        customised: ask.filter.to_string(),
        lang: ask.lang.clone(),
        from: ask.from.clone(),
        ..MessageQuery::default()
    };
    let found = match site.client().all_messages(&incoming, &query).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(found.cache);
    let model = model(&cx, &ask, &found.value);
    let body = cx.draw("special/allmessages.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-allmessages-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

/// `Special:AllMessages` as Plain's `special/allmessages.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AllMessagesPage {
    /// What the page lists.
    pub intro: String,
    /// The filter form.
    pub form: AllMessagesForm,
    /// Said where nothing matches.
    pub nothing: Option<String>,
    /// The language of the messages shown.
    pub lang: String,
    /// The column headings.
    pub columns: Vec<String>,
    /// One row per message.
    pub rows: Vec<MessageLine>,
    /// **Next page**.
    pub next: Option<Choice>,
}

/// The filter form: prefix, filter, language, limit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AllMessagesForm {
    /// The page it filters.
    pub title: String,
    /// The fields.
    pub items: Vec<FormItem>,
    /// **Go**.
    pub go: String,
}

/// One message.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageLine {
    /// Its key.
    pub name: String,
    /// The Interface page that overrides it.
    pub href: String,
    /// The text the build ships.
    pub default: String,
    /// The wiki's own text, where it overrides the default.
    pub current: String,
    /// Whether the wiki overrides it.
    pub customised: bool,
}

fn model(cx: &Context, ask: &Ask, rows: &[MessageRow]) -> AllMessagesPage {
    let m = &cx.m;
    let options = |list: Vec<(String, String)>, current: &str| {
        list.into_iter()
            .map(|(v, label)| {
                let on = v == current;
                (v, label, on)
            })
            .collect()
    };
    let filters = [
        ("all", "ts-allmessages-filter-all"),
        ("modified", "ts-allmessages-filter-modified"),
        ("unmodified", "ts-allmessages-filter-unmodified"),
    ]
    .iter()
    .map(|(v, k)| ((*v).to_string(), m.get(k)))
    .collect();
    let limits = LIMITS
        .iter()
        .map(|l| (l.to_string(), l.to_string()))
        .collect();
    let shown = &rows[..rows.len().min(ask.limit)];
    AllMessagesPage {
        intro: m.get("ts-allmessages-intro"),
        form: AllMessagesForm {
            title: TITLE.into(),
            items: vec![
                FormItem::text(m.get("ts-allmessages-prefix"), "prefix", &ask.prefix),
                FormItem::select(
                    m.get("ts-allmessages-filter"),
                    "filter",
                    options(filters, ask.filter),
                ),
                FormItem::text(m.get("ts-allmessages-lang"), "lang", &ask.lang),
                FormItem::select(
                    m.get("ts-allmessages-limit"),
                    "limit",
                    options(limits, &ask.limit.to_string()),
                ),
            ],
            go: m.get("ts-allmessages-go"),
        },
        nothing: shown.is_empty().then(|| m.get("ts-allmessages-none")),
        lang: ask.lang.clone(),
        columns: [
            "ts-allmessages-col-name",
            "ts-allmessages-col-default",
            "ts-allmessages-col-current",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: shown
            .iter()
            .map(|r| MessageLine {
                name: r.name.clone(),
                href: format!(
                    "/wiki/{}",
                    title_url(&interface_title(&r.name, &ask.lang, &cx.site.lang))
                ),
                default: r.default.clone().unwrap_or_default(),
                current: if r.customised {
                    r.content.clone().unwrap_or_default()
                } else {
                    String::new()
                },
                customised: r.customised,
            })
            .collect(),
        next: rows.get(ask.limit).map(|next| Choice {
            href: ask.href_from(&next.name),
            label: m.get("ts-allmessages-next"),
            current: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_titles_follow_0095() {
        assert_eq!(interface_title("ts-new", "en", "en"), "Interface:Ts-new");
        assert_eq!(interface_title("ts-new", "de", "en"), "Interface:Ts-new/de");
    }

    #[test]
    fn the_ask_keeps_only_what_mediawiki_takes() {
        let q: BTreeMap<String, String> = [
            ("filter", "modified"),
            ("limit", "7"),
            ("lang", "not a code"),
            ("prefix", " ts-"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
        let a = Ask::of(&q, "fr");
        assert_eq!(a.filter, "modified");
        assert_eq!(a.limit, DEFAULT_LIMIT);
        assert_eq!(a.lang, "fr");
        assert_eq!(a.prefix, "ts-");
        assert!(
            a.href_from("ts-x")
                .contains("filter=modified&from=ts-x&lang=fr&prefix=ts-")
        );
    }
}
