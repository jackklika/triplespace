//! The Protect, Delete and Undelete forms, `index.php?title={title}&action=protect|delete|
//! undelete` (09 §4.5, §6.2; 0016 §7; 0023 §9; 0056 §13; 19 §6.3), for pages and entities
//! alike, and the view of a deleted page.
//!
//! **Protect:**
//!
//! - **One row per action** the title may be restricted on (`restrictiontypes`): a group
//!   picker whose first choice lifts the restriction. **Read** is among them: a
//!   confidential restriction (09 §5.2), with the note that the viewer must be in the
//!   group they choose (the lock-out of 09 §4.5), the viewer's own groups named so that
//!   the choice is made with eyes open.
//! - **Also the talk page** goes with the Read row, on by default, since people who hide
//!   a page usually mean its discussion too (0056 A24); it is `tstalk` on the request.
//! - **Expiry** applies to every restriction the form sets, as MediaWiki's form does
//!   without its per-row expiries; **reason** is the log's.
//! - **Enclosing restrictions** (a namespace's, a set's, the tenant's) are listed above
//!   the form and cannot be changed here: their targets have their own pages.
//!
//! The form posts to the same address; the API's answer decides, and its refusal is
//! shown on the form with what was typed kept (`ts-locked-out`, `ts-policy`,
//! `ts-unknown-group`, `protectedpage`, `badtoken`, …). A page the viewer may not read,
//! or that does not exist, answers as the API does: the form is still offered for a free
//! title, since a title may be create-protected before it exists.
//!
//! **Delete** asks for a reason and, for a page, whether the talk page goes too
//! (`deletetalk`), as MediaWiki's form does; it lands on the page, which now shows its
//! deletion. **Undelete** is a reason and a button, offered on the deleted page and at
//! `action=undelete` to a holder of `undelete`.
//!
//! **A deleted page** ([`deleted`]) shows everyone the deletion log entry in place of its
//! content (who, when, why, from the public `delete` log), and the deletion group the
//! content of the newest deleted revision under that banner (`prop=deletedrevisions`,
//! rendered as a preview), with the Undelete form. A title with no deletion the viewer may
//! see is the ordinary missing page (09 §5.3).

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use serde_json::Value;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, ProtectForm, Protection, Restriction};

use crate::Site;
use crate::codex::{Action, Button, MessageKind, Weight};
use crate::codex::{ButtonModel, Message, MessageModel};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params};
use crate::special::controls::{CheckRow, Control, ControlModel, FieldRow, Kind};

/// What the form shows when it comes back: a notice, and what was typed.
#[derive(Debug, Default)]
struct Returned {
    notice: Option<(MessageKind, String)>,
    levels: BTreeMap<String, String>,
    expiry: String,
    reason: String,
    talk: Option<bool>,
}

/// A page that asks the viewer to log in or says they may not (`moderation/gate.html`).
#[derive(Debug, Clone, Serialize)]
pub struct GateModel {
    /// What it says.
    pub notice: MessageModel,
    /// The way to log in, for a reader.
    pub login: Option<Choice>,
}

impl GateModel {
    /// Asking a reader to log in, back to `href`'s page after.
    #[must_use]
    pub fn login(m: &Messages, notice_key: &str, href: String) -> Self {
        Self {
            notice: Message::block(MessageKind::Notice, &m.get(notice_key)).model(),
            login: Some(link_to(href, m.get("ts-login"))),
        }
    }

    /// A notice alone.
    #[must_use]
    pub fn notice(kind: MessageKind, text: &str) -> Self {
        Self {
            notice: Message::block(kind, text).model(),
            login: None,
        }
    }
}

/// A link as a template draws it.
#[must_use]
pub fn link_to(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

/// The notices a form shows above it.
fn notices(n: Option<&(MessageKind, String)>) -> Vec<MessageModel> {
    n.map(|(k, t)| Message::block(*k, t).model())
        .into_iter()
        .collect()
}

/// A primary button.
fn submit(label: &str, action: Action) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action,
        weight: Weight::Primary,
    }
    .model()
}

/// The reason field the moderation forms share (`reason_field`).
pub(crate) fn reason_row(m: &Messages, id: &str, value: &str) -> FieldRow {
    FieldRow::text(id, "reason", m.get("ts-moderation-reason"), value, false)
}

/// The Protect form (`moderation/protect.html`).
#[derive(Debug, Clone, Serialize)]
pub struct ProtectPage {
    /// The notice it came back with, and the note on a title with no page.
    pub notices: Vec<MessageModel>,
    /// The restrictions set on what encloses the title.
    pub enclosing: Option<Enclosing>,
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// One per action the title may be restricted on.
    pub rows: Vec<ProtectRow>,
    /// The expiry.
    pub expiry: FieldRow,
    /// The reason.
    pub reason: FieldRow,
    /// **Protect**.
    pub submit: ButtonModel,
    /// Back to the page.
    pub cancel: Choice,
}

/// The restrictions an enclosing target set.
#[derive(Debug, Clone, Serialize)]
pub struct Enclosing {
    /// The heading.
    pub heading: String,
    /// One line each.
    pub rows: Vec<String>,
}

/// One action's row of the Protect form.
#[derive(Debug, Clone, Serialize)]
pub struct ProtectRow {
    /// The picker's element ID.
    pub id: String,
    /// The action's name.
    pub label: String,
    /// The group picker.
    pub control: ControlModel,
    /// The help's element ID.
    pub help_id: String,
    /// The help.
    pub help: String,
    /// When the restriction in force ends, if it does.
    pub current_expiry: Option<String>,
    /// **Also the talk page**, on the Read row.
    pub talk: Option<CheckRow>,
}

fn enclosing_model(m: &Messages, restrictions: &[Restriction]) -> Option<Enclosing> {
    let rows: Vec<String> = restrictions
        .iter()
        .filter(|r| r.source.is_some())
        .map(|r| {
            m.with(
                "ts-protect-enclosing-row",
                &[
                    &action_label(m, &r.kind),
                    &r.level,
                    r.source.as_deref().unwrap_or_default(),
                    &expiry_text(m, &r.expiry),
                ],
            )
        })
        .collect();
    (!rows.is_empty()).then(|| Enclosing {
        heading: m.get("ts-protect-enclosing"),
        rows,
    })
}

/// The Protect form's model, as [`form_html`] draws it.
#[allow(clippy::too_many_lines)] // one form's model, read top to bottom
fn protect_model(
    cx: &Context,
    title: &str,
    p: &Protection,
    groups: &[String],
    token: &str,
    back: &Returned,
) -> ProtectPage {
    let m = &cx.m;
    let mut notices = notices(back.notice.as_ref());
    if !p.exists {
        notices.push(Message::block(MessageKind::Notice, &m.get("ts-protect-new-title")).model());
    }
    let own: BTreeMap<&str, &Restriction> = p
        .restrictions
        .iter()
        .filter(|r| r.source.is_none())
        .map(|r| (r.kind.as_str(), r))
        .collect();
    let lift = m.get("ts-protect-level-all");
    let rows = p
        .types
        .iter()
        .filter(|kind| !(*kind == "create" && p.exists))
        .map(|kind| {
            let id = format!("ts-protect-level-{kind}");
            let help_id = format!("{id}-help");
            let current = back
                .levels
                .get(kind)
                .cloned()
                .or_else(|| own.get(kind.as_str()).map(|r| r.level.clone()))
                .unwrap_or_default();
            let value = Value::String(current);
            let control = Control {
                id: &id,
                name: &format!("level-{kind}"),
                kind: Kind::Choice,
                choices: groups,
                value: Some(&value),
                empty_choice: Some(&lift),
                described_by: Some(&help_id),
                choice_label: None,
            }
            .model();
            let help = if kind == "read" {
                m.with("ts-protect-read-help", &[&cx.groups.join(", ")])
            } else {
                m.get(&format!("ts-protect-help-{kind}"))
            };
            let talk = (kind == "read").then(|| CheckRow {
                class: "ts-protect__talk".into(),
                control: ControlModel::check(
                    "ts-protect-talk",
                    "talk",
                    back.talk
                        .unwrap_or_else(|| own.get("read").is_none_or(|r| r.talk)),
                    m.get("ts-protect-talk"),
                ),
                help: m.get("ts-protect-talk-help"),
            });
            ProtectRow {
                label: action_label(m, kind),
                control,
                help,
                current_expiry: own
                    .get(kind.as_str())
                    .filter(|r| r.expiry != "infinity")
                    .map(|r| m.with("ts-protect-current-expiry", &[&r.expiry])),
                talk,
                id,
                help_id,
            }
        })
        .collect();
    let expiry = if back.expiry.is_empty() {
        "infinite"
    } else {
        &back.expiry
    };
    let mut expiry_row = FieldRow::text(
        "ts-protect-expiry",
        "expiry",
        m.get("ts-protect-expiry"),
        expiry,
        false,
    );
    if let ControlModel::Input { described_by, .. } = &mut expiry_row.control {
        *described_by = Some("ts-protect-expiry-help".into());
    }
    expiry_row.help = Some(m.get("ts-protect-expiry-help"));
    expiry_row.help_id = Some("ts-protect-expiry-help".into());
    ProtectPage {
        notices,
        enclosing: enclosing_model(m, &p.restrictions),
        action: format!("/w/index.php?title={}&action=protect", title_url(title)),
        token: token.to_string(),
        rows,
        expiry: expiry_row,
        reason: FieldRow::text(
            "ts-protect-reason",
            "reason",
            m.get("ts-protect-reason"),
            &back.reason,
            false,
        ),
        submit: submit(&m.get("ts-protect-submit"), Action::Progressive),
        cancel: link_to(
            format!("/wiki/{}", title_url(title)),
            m.get("ts-protect-cancel"),
        ),
    }
}

/// The Delete form (`moderation/delete.html`).
#[derive(Debug, Clone, Serialize)]
pub struct DeletePage {
    /// The notice it came back with, then the warning.
    pub notices: Vec<MessageModel>,
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The reason.
    pub reason: FieldRow,
    /// **Also delete the talk page**.
    pub talk: CheckRow,
    /// **Delete**.
    pub submit: ButtonModel,
    /// Back to the page.
    pub cancel: Choice,
}

/// The view of a deleted page (`moderation/deleted.html`).
#[derive(Debug, Clone, Serialize)]
pub struct DeletedPage {
    /// The notice it came back with.
    pub notices: Vec<MessageModel>,
    /// Who deleted it, when and why.
    pub banner: String,
    /// The deletion log.
    pub log: Choice,
    /// The newest deleted revision as the API renders it, for the deletion group.
    pub content_html: Option<String>,
    /// Its source, where the API would not render it.
    pub source: Option<String>,
    /// The Undelete form.
    pub undelete: Option<UndeleteForm>,
}

/// The Undelete form.
#[derive(Debug, Clone, Serialize)]
pub struct UndeleteForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The reason.
    pub reason: FieldRow,
    /// **Undelete**.
    pub submit: ButtonModel,
}

/// A message for an API refusal.
fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        "ts-policy" => m.get("ts-protect-policy"),
        "ts-unknown-group" | "badvalue" => m.get("ts-protect-unknown-group"),
        "invalidexpiry" => m.get("ts-protect-invalid-expiry"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" | "protectedpage" | "protectednamespace" => m.get("ts-protect-denied"),
        "ts-restricted-title" | "missingtitle" | "nosuchpageid" => m.get("ts-protect-missing"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

fn action_label(m: &Messages, kind: &str) -> String {
    let key = format!("ts-protect-action-{kind}");
    if m.has(&key) {
        m.get(&key)
    } else {
        kind.to_string()
    }
}

fn expiry_text(m: &Messages, expiry: &str) -> String {
    if expiry == "infinity" || expiry == "infinite" {
        m.get("ts-protect-expiry-never")
    } else {
        expiry.to_string()
    }
}

/// The groups a restriction may name: the tenant's, implicit ones left out.
async fn group_names(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
) -> Result<Vec<String>, Response> {
    match site.client().groups(incoming).await {
        Ok(g) => {
            cx.inputs.push(g.cache);
            Ok(g.value
                .into_iter()
                .filter(|g| !g.implicit && g.name != "*")
                .map(|g| g.name)
                .collect())
        }
        Err(e) => Err(pages::api_failed(cx, headers, &e)),
    }
}

/// The page around the form, or the page that says the viewer may not protect.
async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    back: Returned,
    status: StatusCode,
) -> Response {
    let m = &cx.m;
    let heading = m.with("ts-protect-title", &[title]);
    if cx.user.is_none() {
        let href = format!(
            "/wiki/Special:UserLogin?returnto={}&returntoquery=action%3Dprotect",
            title_url(title)
        );
        let model = GateModel::login(m, "ts-protect-login", href.clone());
        let body = cx.draw("moderation/gate.html", &model);
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        });
    }
    if !cx.holds("protect") {
        let text = m.get("ts-protect-denied");
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Error, &text),
        );
        return pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| Page {
            title: heading.clone(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        });
    }
    let (p, token) = tokio::join!(
        site.client().protection(incoming, title),
        site.client().token(incoming, "csrf")
    );
    let (p, token) = match (p, token) {
        (Ok(p), Ok(t)) => (p, t),
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(p.cache);
    cx.inputs.push(token.value.cache);
    let groups = match group_names(site, cx, headers, incoming).await {
        Ok(g) => g,
        Err(r) => return r,
    };
    let model = protect_model(
        cx,
        &p.value.title,
        &p.value,
        &groups,
        &token.value.value,
        &back,
    );
    let body = cx.draw("moderation/protect.html", &model);
    let r = pages::respond(cx, headers, status, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

/// `GET …&action=protect`.
async fn serve_protect(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: String,
    params: &Params,
) -> Response {
    if !cx.features.has(Feature::Protect) {
        return pages::no_page(&cx, headers, title);
    }
    let notice = params
        .map
        .get("done")
        .map(|_| (MessageKind::Success, cx.m.get("ts-protect-done")));
    let back = Returned {
        notice,
        ..Returned::default()
    };
    page(
        site,
        &mut cx,
        headers,
        incoming,
        &title,
        back,
        StatusCode::OK,
    )
    .await
}

/// `POST …&action=protect`.
async fn post_protect(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: String,
    form: &Params,
) -> Response {
    if !cx.features.has(Feature::Protect) {
        return pages::no_page(&cx, headers, title);
    }
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let mut levels = BTreeMap::new();
    let mut protections = Vec::new();
    for (k, v) in &form.map {
        if let Some(kind) = k.strip_prefix("level-") {
            levels.insert(kind.to_string(), v.clone());
            let group = if v.trim().is_empty() { "all" } else { v.trim() };
            protections.push((kind.to_string(), group.to_string()));
        }
    }
    let talk = form.map.contains_key("talk");
    let expiry = field("expiry").trim();
    let reason = field("reason").trim();
    let sent = ProtectForm {
        title: &title,
        protections,
        expiry: if expiry.is_empty() {
            "infinite"
        } else {
            expiry
        },
        reason,
        talk,
        token: field("token"),
    };
    match site.client().protect(incoming, &sent).await {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/w/index.php?title={}&action=protect&done=1",
                title_url(&title)
            ),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            let back = Returned {
                notice: Some((MessageKind::Error, text)),
                levels,
                expiry: expiry.to_string(),
                reason: reason.to_string(),
                talk: Some(talk),
            };
            page(
                site,
                &mut cx,
                headers,
                incoming,
                &title,
                back,
                StatusCode::OK,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// Which form an `action=` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// `action=protect`.
    Protect,
    /// `action=delete`.
    Delete,
    /// `action=undelete`.
    Undelete,
}

impl Form {
    /// The form an `action=` names, if any.
    #[must_use]
    pub fn of(action: &str) -> Option<Self> {
        match action {
            "protect" => Some(Self::Protect),
            "delete" => Some(Self::Delete),
            "undelete" => Some(Self::Undelete),
            _ => None,
        }
    }
}

/// `GET …&action=protect|delete|undelete`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    form: Form,
    title: String,
    params: &Params,
) -> Response {
    match form {
        Form::Protect => serve_protect(site, headers, incoming, cx, title, params).await,
        Form::Delete => delete_page(site, cx, headers, incoming, &title, None, None).await,
        Form::Undelete => undelete_page(site, cx, headers, incoming, &title, None).await,
    }
}

/// `POST …&action=protect|delete|undelete`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    form: Form,
    title: String,
    fields: &Params,
) -> Response {
    match form {
        Form::Protect => post_protect(site, headers, incoming, cx, title, fields).await,
        Form::Delete => post_delete(site, cx, headers, incoming, &title, fields).await,
        Form::Undelete => post_undelete(site, cx, headers, incoming, &title, fields).await,
    }
}

/// A page that asks the viewer to log in, or says they lack a right.
pub(crate) fn gate(
    cx: &Context,
    headers: &HeaderMap,
    title: &str,
    heading: &str,
    right: &str,
    action: &str,
) -> Option<Response> {
    let m = &cx.m;
    if cx.user.is_none() {
        let href = format!(
            "/wiki/Special:UserLogin?returnto={}&returntoquery=action%3D{action}",
            title_url(title)
        );
        let model = GateModel::login(m, "ts-moderation-login", href.clone());
        let body = cx.draw("moderation/gate.html", &model);
        return Some(pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.to_string(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        }));
    }
    if !cx.holds(right) {
        let text = m.with("ts-moderation-denied", &[right]);
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Error, &text),
        );
        return Some(pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| {
            Page {
                title: heading.to_string(),
                returnto: Some(title.to_string()),
                body,
                ..Page::default()
            }
        }));
    }
    None
}

/// The Delete form.
async fn delete_page(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
    typed: Option<(&str, bool)>,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let heading = cx.m.with("ts-delete-title", &[title]);
    if let Some(r) = gate(&cx, headers, title, &heading, "delete", "delete") {
        return r;
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let m = &cx.m;
    let (reason, talk) = typed.unwrap_or(("", true));
    let mut notices_now = notices(notice.as_ref());
    notices_now.push(Message::block(MessageKind::Warning, &m.get("ts-delete-warning")).model());
    let model = DeletePage {
        notices: notices_now,
        action: format!("/w/index.php?title={}&action=delete", title_url(title)),
        token: token.value.value.clone(),
        reason: reason_row(m, "ts-delete-reason", reason),
        talk: CheckRow {
            class: "ts-protect__talk".into(),
            control: ControlModel::check("ts-delete-talk", "talk", talk, m.get("ts-delete-talk")),
            help: m.get("ts-delete-talk-help"),
        },
        submit: submit(&m.get("ts-delete-submit"), Action::Destructive),
        cancel: link_to(
            format!("/wiki/{}", title_url(title)),
            m.get("ts-protect-cancel"),
        ),
    };
    let body = cx.draw("moderation/delete.html", &model);
    let r = pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

fn deletion_refusal(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "missingtitle" | "nosuchpageid" | "cantundelete" | "ts-restricted-title" => {
            m.get("ts-protect-missing")
        }
        "permissiondenied" | "protectedpage" => m.with("ts-moderation-denied", &["delete"]),
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

/// `POST …&action=delete`.
async fn post_delete(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    fields: &Params,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let field = |k: &str| fields.map.get(k).map_or("", String::as_str);
    let reason = field("reason").trim();
    let talk = fields.map.contains_key("talk");
    match site
        .client()
        .delete_page(incoming, title, reason, talk, field("token"))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{}", title_url(title)), &[]),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = deletion_refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            delete_page(
                site,
                cx,
                headers,
                incoming,
                title,
                Some((MessageKind::Error, text)),
                Some((reason, talk)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The Undelete form on its own page (`action=undelete`): the deleted page's view, which
/// carries it, or the missing page.
async fn undelete_page(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let heading = cx.m.with("ts-undelete-title", &[title]);
    if let Some(r) = gate(&cx, headers, title, &heading, "undelete", "undelete") {
        return r;
    }
    let mut cx = cx;
    match deleted(site, &mut cx, headers, incoming, title, notice).await {
        Some(r) => r,
        None => pages::no_page(&cx, headers, title.to_string()),
    }
}

/// `POST …&action=undelete`.
async fn post_undelete(
    site: &Site,
    cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    fields: &Params,
) -> Response {
    if !cx.features.has(Feature::Delete) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let field = |k: &str| fields.map.get(k).map_or("", String::as_str);
    match site
        .client()
        .undelete_page(incoming, title, field("reason").trim(), field("token"))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{}", title_url(title)), &[]),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = deletion_refusal(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            undelete_page(
                site,
                cx,
                headers,
                incoming,
                title,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The view of a deleted title (19 §6.3), or `None` where the viewer may see no deletion
/// of it: the banner naming who deleted it, when and why; for the deletion group the
/// newest deleted revision's content under it; and the Undelete form for a holder of
/// `undelete`. An entity's deleted content is not shown: `deletedrevisions` has none for it.
pub async fn deleted(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: &str,
    notice: Option<(MessageKind, String)>,
) -> Option<Response> {
    if !cx.features.has(Feature::Delete) {
        return None;
    }
    let deletion = match site.client().deletion_of(incoming, title).await {
        Ok(d) => d,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    cx.inputs.push(deletion.cache);
    let d = deletion.value?;
    let m = &cx.m;
    let by =
        d.by.clone()
            .unwrap_or_else(|| m.get("ts-changes-hidden-user"));
    let mut model = DeletedPage {
        notices: notices(notice.as_ref()),
        banner: if d.reason.is_empty() {
            m.with("ts-deleted-banner-no-reason", &[&by, &d.time])
        } else {
            m.with("ts-deleted-banner", &[&by, &d.time, &d.reason])
        },
        log: link_to(
            format!("/wiki/Special:Log?type=delete&page={}", title_url(title)),
            m.get("ts-deleted-log-link"),
        ),
        content_html: None,
        source: None,
        undelete: None,
    };
    // The deletion group reads the content; others see nothing more.
    let revision = match site.client().deleted_revision(incoming, title).await {
        Ok(r) => r,
        Err(e) => return Some(pages::api_failed(cx, headers, &e)),
    };
    cx.inputs.push(revision.cache);
    let member = revision.value.is_some();
    if let Some(rev) = &revision.value
        && let Some(text) = &rev.content
    {
        if let Ok(parsed) = site
            .client()
            .preview(incoming, title, text, "wikitext")
            .await
        {
            model.content_html = Some(parsed.html);
        } else {
            model.source = Some(text.clone());
        }
    }
    let mut cookies = Vec::new();
    if member && cx.holds("undelete") {
        let token = match site.client().token(incoming, "csrf").await {
            Ok(t) => t,
            Err(e) => return Some(pages::api_failed(cx, headers, &e)),
        };
        cx.inputs.push(token.value.cache);
        cookies = token.set_cookies;
        let m = &cx.m;
        model.undelete = Some(UndeleteForm {
            action: format!("/w/index.php?title={}&action=undelete", title_url(title)),
            token: token.value.value,
            reason: reason_row(m, "ts-undelete-reason", ""),
            submit: submit(&m.get("ts-undelete-submit"), Action::Progressive),
        });
    }
    // The group has a page to read; everyone else a title with no page, as the API says.
    let status = if member {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    };
    let heading = title.to_string();
    let body = cx.draw("moderation/deleted.html", &model);
    let r = pages::respond(cx, headers, status, |_| Page {
        title: heading.clone(),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    });
    Some(pages::with_cookies(r, &cookies))
}
