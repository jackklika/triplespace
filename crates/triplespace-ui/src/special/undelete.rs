//! `Special:Undelete` (0023 §9; 19 §6.3): the deletions the viewer may see, newest first,
//! each leading to the deleted page's view, which carries the Undelete form for a member
//! of the deletion group holding `undelete` (`moderation::deleted`).
//! `Special:Undelete/{title}` goes straight to that view.
//!
//! The list is the `delete` log (`GET /activity?kind=log&log_type=delete`), so a deletion
//! of a confidential page is listed only to its group, as every feed does (09 §5.3), and a
//! page since undeleted still shows its past deletion; its own page says it is back.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::activity::ActivityQuery;

use crate::Site;
use crate::codex::{Action, Button, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::moderation::{GateModel, link_to};
use crate::pages::{self, Params, Peer};
use crate::special::controls::{AskForm, FormItem};

const TITLE: &str = "Special:Undelete";

/// `Special:Undelete` as Plain's `special/undelete.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UndeleteIndex {
    /// What the page is.
    pub intro: String,
    /// The form asking for a page.
    pub ask: AskForm,
    /// The deletions, newest first.
    pub rows: Vec<DeletionRow>,
    /// Said where there are none.
    pub nothing: Option<String>,
}

/// One deletion.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeletionRow {
    /// When.
    pub time: String,
    /// The deleted page's view.
    pub page: Choice,
    /// Who.
    pub actor: String,
    /// Why.
    pub comment: String,
}

/// The form asking for a page to go to, which `Special:MovePage` shares.
#[must_use]
pub fn ask_form(m: &crate::i18n::Messages, title: &str, label_key: &str) -> AskForm {
    AskForm {
        class: "ts-undelete__ask".into(),
        title: title.to_string(),
        item: FormItem::text(m.get(label_key), "target", ""),
        button: Button {
            label: &m.get("ts-undelete-go"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Normal,
        }
        .model(),
    }
}

/// `GET Special:Undelete[/{title}]`.
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
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let subject = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| params.map.get("target").map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty());
    if let Some(page) = subject {
        if let Some(r) =
            crate::moderation::deleted(site, &mut cx, headers, &incoming, &page, None).await
        {
            return r;
        }
        let text = cx.m.with("ts-undelete-nothing", &[&page]);
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(crate::codex::MessageKind::Notice, &text),
        );
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-undelete-index-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let query = ActivityQuery {
        kinds: vec!["log".into()],
        log_type: Some("delete".into()),
        limit: 100,
        lang: cx.m.lang().to_string(),
        ..ActivityQuery::default()
    };
    let feed = match site.client().activity(&incoming, &query).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(feed.cache);
    let m = &cx.m;
    let model = UndeleteIndex {
        intro: m.get("ts-undelete-intro"),
        ask: ask_form(m, TITLE, "ts-undelete-page"),
        rows: feed
            .value
            .rows
            .iter()
            .filter(|r| r.summary.op.as_deref() == Some("delete/delete") || r.logid.is_some())
            .filter_map(|r| {
                let title = r.target.as_ref()?.title.clone().unwrap_or_default();
                Some(DeletionRow {
                    time: r.time.clone(),
                    page: link_to(format!("/wiki/{}/{}", TITLE, title_url(&title)), title),
                    actor: r.actor.name.clone().unwrap_or_default(),
                    comment: r.comment.clone().unwrap_or_default(),
                })
            })
            .collect(),
        nothing: None,
    };
    let model = UndeleteIndex {
        nothing: model.rows.is_empty().then(|| m.get("ts-undelete-none")),
        ..model
    };
    let body = cx.draw("special/undelete.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-undelete-index-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}
