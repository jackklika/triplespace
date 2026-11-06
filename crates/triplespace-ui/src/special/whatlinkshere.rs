//! `Special:WhatLinksHere` (0010 §12; 0008 §10; 19 §1.3): the pages that link to a title,
//! from `list=backlinks`, which reads the links projection. It is the **Links here** tab
//! of every document page and entity page.
//!
//! - The title is the subpage (`Special:WhatLinksHere/Project:Home`) or `target=`, as
//!   MediaWiki reads it; with neither, the page asks for one.
//! - Pages are listed in page-ID order, as MediaWiki lists them, 50 to a page; a redirect
//!   is marked. **Next 50** continues from where the list stopped (`from=`, the API's
//!   `blcontinue`).
//! - A free title lists its wanted links: the pages whose text links to it.
//!
//! Not yet: the `namespace`, `hidelinks`, `hideredirs` and `hidetrans` filters, and the
//! links to each redirect to the title, indented under it.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::ClientError;

use crate::Site;
use crate::codex::{Action, Button, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::FormItem;
use crate::special::controls::AskForm;

/// Linking pages to a page of the list.
const LIMIT: u32 = 50;

/// One linking page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Linking {
    /// The page, linked.
    pub page: Choice,
    /// "(redirect page)", where it is one.
    pub redirect: Option<String>,
}

/// `Special:WhatLinksHere`, as Plain's `special/whatlinkshere.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WhatLinksHerePage {
    /// The form asking for a title.
    pub form: AskForm,
    /// "The following pages link to", before the title.
    pub intro: Option<String>,
    /// The title, linked.
    pub target: Option<Choice>,
    /// The linking pages.
    pub pages: Vec<Linking>,
    /// Said where none link to it.
    pub empty: Option<String>,
    /// **Next 50**.
    pub next: Option<Choice>,
}

/// The title asked about: the subpage, or `target=`.
fn target_of(title: &str, params: &Params) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.to_string())
        .or_else(|| params.map.get("target").cloned())
        .map(|t| t.replace('_', " ").trim().to_string())
        .filter(|t| !t.is_empty())
}

fn form(m: &Messages, target: &str) -> AskForm {
    AskForm {
        class: "ts-whatlinkshere__ask".into(),
        title: "Special:WhatLinksHere".into(),
        item: FormItem::text(m.get("ts-whatlinkshere-target"), "target", target),
        button: Button {
            label: &m.get("ts-whatlinkshere-go"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Normal,
        }
        .model(),
    }
}

fn page_model(
    m: &Messages,
    target: Option<&str>,
    found: Option<&triplespace_client::Backlinks>,
) -> WhatLinksHerePage {
    let mut page = WhatLinksHerePage {
        form: form(m, target.unwrap_or_default()),
        intro: None,
        target: None,
        pages: Vec::new(),
        empty: None,
        next: None,
    };
    let (Some(target), Some(found)) = (target, found) else {
        return page;
    };
    let link = |t: &str| Choice {
        href: format!("/wiki/{}", title_url(t)),
        label: t.to_string(),
        current: false,
    };
    if found.pages.is_empty() {
        page.empty = Some(m.with("ts-whatlinkshere-none", &[target]));
        return page;
    }
    page.intro = Some(m.get("ts-whatlinkshere-intro"));
    page.target = Some(link(target));
    page.pages = found
        .pages
        .iter()
        .map(|(t, redirect)| Linking {
            page: Choice {
                href: format!("/w/index.php?title={}&redirect=no", title_url(t)),
                ..link(t)
            },
            redirect: redirect.then(|| m.get("ts-whatlinkshere-redirect")),
        })
        .collect();
    page.next = found.next.as_ref().map(|n| Choice {
        href: format!(
            "/w/index.php?title=Special:WhatLinksHere/{}&from={}",
            title_url(target),
            url::form_urlencoded::byte_serialize(n.as_bytes()).collect::<String>()
        ),
        label: m.with("ts-whatlinkshere-next", &[&LIMIT.to_string()]),
        current: false,
    });
    page
}

/// `GET Special:WhatLinksHere[/{title}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Pages) {
        return pages::no_page(&cx, headers, "Special:WhatLinksHere".into());
    }
    let target = target_of(title, params);
    let found = match &target {
        Some(t) => match site
            .client()
            .backlinks(
                &incoming,
                t,
                LIMIT,
                params.map.get("from").map(String::as_str),
            )
            .await
        {
            Ok(f) => {
                cx.inputs.push(f.cache);
                Some(f.value)
            }
            Err(ClientError::Api { code, .. }) if code == "invalidtitle" => None,
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        None => None,
    };
    let body = cx.draw(
        "special/whatlinkshere.html",
        page_model(&cx.m, target.as_deref(), found.as_ref()),
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: target.as_deref().map_or_else(
            || m.get("ts-whatlinkshere-title"),
            |t| m.with("ts-whatlinkshere-title-of", &[t]),
        ),
        returnto: Some(target.as_deref().map_or_else(
            || "Special:WhatLinksHere".to_string(),
            |t| format!("Special:WhatLinksHere/{t}"),
        )),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // 0010 §12: the subpage names the title, as `target=` does.
    #[test]
    fn the_title_is_the_subpage_or_the_target() {
        let p = Params::new(vec![("target".into(), "Project:Home".into())]);
        assert_eq!(
            target_of("Special:WhatLinksHere/Item:Q5", &p).as_deref(),
            Some("Item:Q5")
        );
        assert_eq!(
            target_of("Special:WhatLinksHere", &p).as_deref(),
            Some("Project:Home")
        );
        assert_eq!(target_of("Special:WhatLinksHere", &Params::default()), None);
    }

    #[test]
    fn linking_pages_are_listed_with_their_redirects_marked() {
        let m = Messages::for_language("en");
        let found = triplespace_client::Backlinks {
            pages: vec![("Project:Index".into(), false), ("Home".into(), true)],
            next: Some("4:Home|12".into()),
        };
        let p = page_model(&m, Some("Project:Home"), Some(&found));
        assert_eq!(p.pages.len(), 2);
        assert!(p.pages[0].redirect.is_none() && p.pages[1].redirect.is_some());
        assert!(p.next.unwrap().href.ends_with("&from=4%3AHome%7C12"));
        let none = page_model(
            &m,
            Some("Project:Home"),
            Some(&triplespace_client::Backlinks::default()),
        );
        assert!(none.empty.is_some() && none.pages.is_empty());
    }
}
