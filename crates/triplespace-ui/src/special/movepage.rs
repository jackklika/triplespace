//! `Special:MovePage/{title}` (0047 §9; 19 §6.3): the Move form, for a holder of `move`.
//! It asks for the new title and a reason, offers to leave no redirect to a holder of
//! `suppressredirect`, and to carry subpages along to a holder of `move-subpages`; the
//! API checks the rest at both ends (10 §2.5), and its refusal is shown over the form
//! with what was typed. A move that succeeds lands on the page at its new title.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::Incoming;
use triplespace_client::rights::MoveForm;
use triplespace_client::settings::Saved;

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::codex::{ButtonModel, Message, MessageModel};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::{gate, link_to, reason_row};
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{AskForm, CheckRow, ControlModel, FieldRow};

const TITLE: &str = "Special:MovePage";

/// `Special:MovePage` as Plain's `special/movepage.html` draws it: the form asking for a
/// page, or the Move form.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MovePage {
    /// The form asking for a page, where none is named.
    pub ask: Option<AskForm>,
    /// The Move form.
    pub form: Option<MoveFormModel>,
}

/// The Move form.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MoveFormModel {
    /// The notice it came back with, then what moving does.
    pub notices: Vec<MessageModel>,
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The new title.
    pub to: FieldRow,
    /// The reason.
    pub reason: FieldRow,
    /// **Leave a redirect** and **Move subpages**, as the viewer's rights allow.
    pub options: Vec<CheckRow>,
    /// **Move**.
    pub submit: ButtonModel,
    /// Back to the page.
    pub cancel: Choice,
}

/// What the form holds, typed or defaulted.
struct Typed {
    to: String,
    reason: String,
    redirect: bool,
    subpages: bool,
}

/// The page a request names: `Special:MovePage/{title}` or `?target=`.
fn subject(title: &str, params: &Params) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| params.map.get("target").map(|t| t.trim().to_string()))
        .filter(|s| !s.is_empty())
}

/// The form.
async fn form(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    from: &str,
    notice: Option<(MessageKind, String)>,
    typed: Option<Typed>,
) -> Response {
    if !cx.features.has(Feature::Move) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let heading = cx.m.with("ts-move-title", &[from]);
    if let Some(r) = gate(&cx, headers, from, &heading, "move", "move") {
        return r;
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let m = &cx.m;
    let typed = typed.unwrap_or(Typed {
        to: from.to_string(),
        reason: String::new(),
        redirect: true,
        subpages: false,
    });
    let option = |id: &str, name: &str, on: bool| CheckRow {
        class: "ts-protect__talk".into(),
        control: ControlModel::check(id, name, on, m.get(id)),
        help: m.get(&format!("{id}-help")),
    };
    let mut options_model = Vec::new();
    if cx.holds("suppressredirect") {
        options_model.push(option("ts-move-redirect", "redirect", typed.redirect));
    }
    if cx.holds("move-subpages") {
        options_model.push(option("ts-move-subpages", "subpages", typed.subpages));
    }
    let mut notices: Vec<MessageModel> = notice
        .iter()
        .map(|(k, t)| Message::block(*k, t).model())
        .collect();
    notices.push(Message::block(MessageKind::Notice, &m.get("ts-move-intro")).model());
    let model = MovePage {
        ask: None,
        form: Some(MoveFormModel {
            notices,
            action: format!("/wiki/{TITLE}/{}", title_url(from)),
            token: token.value.value.clone(),
            to: FieldRow::text("ts-move-to", "to", m.get("ts-move-to"), &typed.to, true),
            reason: reason_row(m, "ts-move-reason", &typed.reason),
            options: options_model,
            submit: Button {
                label: &m.get("ts-move-submit"),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .model(),
            cancel: link_to(
                format!("/wiki/{}", title_url(from)),
                m.get("ts-protect-cancel"),
            ),
        }),
    };
    let body = cx.draw("special/movepage.html", &model);
    let r = pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(from.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// `GET Special:MovePage/{title}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let Some(from) = subject(title, params) else {
        // No page named: ask for one.
        let m = &cx.m;
        let model = MovePage {
            ask: Some(crate::special::undelete::ask_form(
                m,
                TITLE,
                "ts-move-which",
            )),
            form: None,
        };
        let body = cx.draw("special/movepage.html", &model);
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-move-index-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    };
    form(site, cx, headers, &incoming, &from, None, None).await
}

fn refusal(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "missingtitle" | "nosuchpageid" => m.get("ts-protect-missing"),
        "articleexists" => m.get("ts-move-exists"),
        "ts-restricted-title" => m.get("ts-move-restricted"),
        "immobilenamespace" | "ts-model-not-allowed" => m.get("ts-move-immobile"),
        "selfmove" => m.get("ts-move-self"),
        "invalidtitle" => m.get("ts-move-invalid"),
        "permissiondenied" | "protectedpage" => m.with("ts-moderation-denied", &["move"]),
        "badtoken" => m.get("ts-login-expired"),
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST Special:MovePage/{title}`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Move) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let Some(from) = subject(title, params) else {
        return pages::no_page(&cx, headers, TITLE.to_string());
    };
    let field = |k: &str| params.map.get(k).map_or("", String::as_str);
    let typed = Typed {
        to: field("to").trim().to_string(),
        reason: field("reason").trim().to_string(),
        // The checkbox is offered only to a holder of `suppressredirect`; for anyone else
        // a redirect is always left, so an absent field means one.
        redirect: !cx.holds("suppressredirect") || params.map.contains_key("redirect"),
        subpages: params.map.contains_key("subpages"),
    };
    match site
        .client()
        .move_page(
            &incoming,
            &MoveForm {
                from: &from,
                to: &typed.to,
                reason: &typed.reason,
                noredirect: !typed.redirect,
                subpages: typed.subpages,
            },
            field("token"),
        )
        .await
    {
        Ok(Saved::Ok(v)) => {
            let to = v["to"].as_str().unwrap_or(&typed.to).to_string();
            pages::see_other(&format!("/wiki/{}", title_url(&to)), &[])
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            form(
                site,
                cx,
                headers,
                &incoming,
                &from,
                Some((MessageKind::Error, text)),
                Some(typed),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
