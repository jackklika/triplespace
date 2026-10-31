//! `Special:ComparePages` (0047 §9; 21 §6.1): any two pages or revisions, with the diff of
//! 0010 §6. MediaWiki's parameters: `page1` or `rev1`, and `page2` or `rev2`; a revision
//! wins over a title on the same side, and a title stands for its latest revision. The
//! form stays above the diff, filled in, so a comparison can be changed and run again.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::{ClientError, DiffAsk, DiffEnd};

use crate::Site;
use crate::codex::MessageKind;
use crate::document::diff_html;
use crate::features::Feature;
use crate::frame::Page;
use crate::html::esc;
use crate::pages::{self, Context, Params, Peer};

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
            return page(&cx, headers, q, StatusCode::BAD_REQUEST, Some(&e), "");
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
        return page(&cx, headers, q, status, error.as_deref(), "");
    };
    match site
        .client()
        .page_diff(&incoming, &DiffAsk::Between(from, to))
        .await
    {
        Ok(d) => {
            cx.inputs.push(d.cache);
            let html = format!(
                "<div class=\"ts-page-diff\">{}</div>",
                diff_html(&cx, &d.value, None)
            );
            page(&cx, headers, q, StatusCode::OK, None, &html)
        }
        Err(ClientError::Api { code, .. })
            if matches!(
                code.as_str(),
                "missingtitle" | "nosuchrevid" | "missingcontent" | "permissiondenied"
            ) =>
        {
            let e = cx.m.get("ts-comparepages-missing");
            page(&cx, headers, q, StatusCode::NOT_FOUND, Some(&e), "")
        }
        Err(ClientError::Api { code, .. }) if code == "invalidtitle" || code == "badvalue" => {
            let e = cx.m.get("ts-comparepages-missing");
            page(&cx, headers, q, StatusCode::BAD_REQUEST, Some(&e), "")
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The form, filled in as asked, an error above it, and the diff below.
fn page(
    cx: &Context,
    headers: &HeaderMap,
    q: &BTreeMap<String, String>,
    status: StatusCode,
    error: Option<&str>,
    diff: &str,
) -> Response {
    let m = &cx.m;
    let value = |k: &str| q.get(k).map(|v| esc(v)).unwrap_or_default();
    let mut body = String::new();
    if let Some(e) = error {
        body.push_str(&pages::message(MessageKind::Error, e));
    }
    let _ = write!(
        body,
        "<form class=\"ts-comparepages\" method=\"get\" action=\"/w/index.php\"><input type=\"hidden\" name=\"title\" value=\"{TITLE}\">"
    );
    for (n, legend) in [(1, "ts-comparepages-first"), (2, "ts-comparepages-second")] {
        let _ = write!(
            body,
            "<fieldset class=\"ts-comparepages__side\"><legend>{}</legend><label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" name=\"page{n}\" value=\"{}\"></label> <label class=\"ts-changes-form__field\"><span>{}</span> <input class=\"cdx-text-input__input\" type=\"text\" inputmode=\"numeric\" name=\"rev{n}\" value=\"{}\" size=\"10\"></label></fieldset>",
            esc(&m.get(legend)),
            esc(&m.get("ts-comparepages-page")),
            value(&format!("page{n}")),
            esc(&m.get("ts-comparepages-rev")),
            value(&format!("rev{n}")),
        );
    }
    let _ = write!(
        body,
        "<button class=\"cdx-button cdx-button--action-progressive\" type=\"submit\">{}</button></form>{diff}",
        esc(&m.get("ts-comparepages-go"))
    );
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
