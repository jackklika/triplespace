//! `Special:ComparePages` (0047 §9; 21 §6.1): any two pages or revisions, with the diff of
//! 0010 §6. MediaWiki's parameters: `page1` or `rev1`, and `page2` or `rev2`; a revision
//! wins over a title on the same side, and a title stands for its latest revision. The
//! form stays above the diff, filled in, so a comparison can be changed and run again.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::{ClientError, DiffAsk, DiffEnd};

use crate::Site;
use crate::codex::{Message, MessageKind, MessageModel};
use crate::document::{DiffTable, diff_table_model};
use crate::features::Feature;
use crate::frame::Page;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::FormItem;

const TITLE: &str = "Special:ComparePages";

/// One side as asked: a revision ID where one is given, else a title.
fn end(q: &BTreeMap<String, String>, n: u8) -> Result<Option<DiffEnd>, String> {
    let get = |k: &str| {
        q.get(&format!("{k}{n}"))
            .map(|v| v.replace('_', " ").trim().to_string())
            .filter(|v| !v.is_empty())
    };
    if let Some(rev) = get("rev") {
        return rev
            .parse::<u64>()
            .map(|r| Some(DiffEnd::Revision(r)))
            .map_err(|_| rev);
    }
    Ok(get("page").map(DiffEnd::Title))
}

/// `GET Special:ComparePages`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Pages) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let q = &params.map;
    let (from, to) = match (end(q, 1), end(q, 2)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(bad), _) | (_, Err(bad)) => {
            let e = cx.m.with("ts-comparepages-bad-rev", &[&bad]);
            return page(&cx, headers, q, StatusCode::BAD_REQUEST, Some(&e), None);
        }
    };
    let (Some(from), Some(to)) = (from, to) else {
        // The form alone: nothing asked yet, or only one side.
        let asked_half = q
            .keys()
            .any(|k| k.starts_with("page") || k.starts_with("rev"));
        let error = asked_half.then(|| cx.m.get("ts-comparepages-both"));
        let status = if asked_half {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::OK
        };
        return page(&cx, headers, q, status, error.as_deref(), None);
    };
    match site
        .client()
        .page_diff(&incoming, &DiffAsk::Between(from, to))
        .await
    {
        Ok(d) => {
            cx.inputs.push(d.cache);
            let table = diff_table_model(&cx.m, &d.value, None);
            page(&cx, headers, q, StatusCode::OK, None, Some(table))
        }
        Err(ClientError::Api { code, .. })
            if matches!(
                code.as_str(),
                "missingtitle" | "nosuchrevid" | "missingcontent" | "permissiondenied"
            ) =>
        {
            let e = cx.m.get("ts-comparepages-missing");
            page(&cx, headers, q, StatusCode::NOT_FOUND, Some(&e), None)
        }
        Err(ClientError::Api { code, .. }) if code == "invalidtitle" || code == "badvalue" => {
            let e = cx.m.get("ts-comparepages-missing");
            page(&cx, headers, q, StatusCode::BAD_REQUEST, Some(&e), None)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `Special:ComparePages` as Plain's `special/comparepages.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ComparePage {
    /// Why the comparison could not be made.
    pub error: Option<MessageModel>,
    /// The special page the form goes to.
    pub title: String,
    /// The two sides, each a page or a revision.
    pub sides: Vec<Side>,
    /// **Compare**.
    pub go: String,
    /// The diff.
    pub diff: Option<DiffTable>,
}

/// One side of the form.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Side {
    /// "First page" or "Second page".
    pub legend: String,
    /// Its page and revision fields.
    pub items: Vec<FormItem>,
}

/// The form, filled in as asked, an error above it, and the diff below.
fn page(
    cx: &Context,
    headers: &HeaderMap,
    q: &BTreeMap<String, String>,
    status: StatusCode,
    error: Option<&str>,
    table: Option<DiffTable>,
) -> Response {
    let m = &cx.m;
    let asked = |k: &str| q.get(k).cloned().unwrap_or_default();
    let model = ComparePage {
        error: error.map(|e| Message::block(MessageKind::Error, e).model()),
        title: TITLE.into(),
        sides: [(1, "ts-comparepages-first"), (2, "ts-comparepages-second")]
            .iter()
            .map(|(n, legend)| Side {
                legend: m.get(legend),
                items: vec![
                    FormItem::text(
                        m.get("ts-comparepages-page"),
                        &format!("page{n}"),
                        &asked(&format!("page{n}")),
                    ),
                    FormItem::numeric(
                        m.get("ts-comparepages-rev"),
                        &format!("rev{n}"),
                        &asked(&format!("rev{n}")),
                    ),
                ],
            })
            .collect(),
        go: m.get("ts-comparepages-go"),
        diff: table,
    };
    let body = cx.draw("special/comparepages.html", &model);
    pages::respond(cx, headers, status, |m| Page {
        title: m.get("ts-comparepages-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn a_revision_wins_over_a_title() {
        let both = q(&[("page1", "Foo_bar"), ("rev1", "7"), ("page2", "Baz")]);
        assert_eq!(end(&both, 1), Ok(Some(DiffEnd::Revision(7))));
        assert_eq!(end(&both, 2), Ok(Some(DiffEnd::Title("Baz".into()))));
        assert_eq!(end(&q(&[("rev1", "x")]), 1), Err("x".into()));
        assert_eq!(end(&q(&[("page1", " ")]), 1), Ok(None));
    }
}
