//! A thread's actions (0019 §8–§9, §12; 0049 §6, §13; 0069 §4; 14 §1.2, §2.1, §3.6, §4;
//! 19 §6.1): everything the records let a person do to a thread besides posting, done from
//! the page without the API.
//!
//! The thread page's overflow menu (19 §1.3) offers them, as `?action=thread&op={op}`;
//! each opens a form with a confirm button, which posts to the same address, so none needs
//! JavaScript:
//!
//! - **Rename** (`rename`): a new subject, and so a new title (14 §1.4).
//! - **Move** (`move`): a new home, named by its talk page, a board or the subject itself,
//!   with **Keep it listed on the old home** (`keep`).
//! - **List on…** (`attach`) and **Remove a listing** (`detach`): a further talk page or
//!   board, or one of the listings (14 §3.6).
//! - **Pin** and **Unpin** (`pin`, `unpin`) on one of its attachments (14 §4). A talk
//!   page's listing offers **Pin here** and **Unpin here** on each thread's head, which
//!   open the same form with the page chosen.
//! - **Close…** and **Reopen**: a post that sets a status of the closed category, with a
//!   closing statement or none, or sets `open` (14 §2.1: a status is set by posting, and a
//!   post that sets one needs no text).
//!
//! The menu shows what the viewer's tenant-wide rights allow: `move` for the first six,
//! `edit` for the last two (09 §8.1). The API decides the rest, page protection included
//! (0019 §12), and a refusal comes back as a notice on the form. Every form carries the
//! thread's base offset as it was drawn, so a change made in the meantime is an edit
//! conflict rather than a silent overwrite (14 §1.2).

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::{
    Attached, ClientError, Incoming, Lookup, NewThreadOn, PageInfo, PostForm, ThreadChange,
    ThreadView, Written,
};

use super::{Kind, choice, kind_of, message_model, status_label, subject_of, thread_href, token};
use crate::Site;
use crate::codex::{ButtonModel, MessageKind, MessageModel};
use crate::entity::history::Choice;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params};
use crate::special::controls::{CheckRow, ControlModel, FieldRow, OptionModel};

/// The statuses of the closed category that **Close…** offers, in the registry's order
/// (`thread-statuses.toml`), less `archived`, which imports write.
const CLOSING: &[&str] = &["resolved", "closed", "wontfix", "declined", "stale"];

/// What the form changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `rename`.
    Rename,
    /// `move`.
    Move,
    /// `attach`: **List on…**.
    Attach,
    /// `detach`.
    Detach,
    /// `pin`.
    Pin,
    /// `unpin`.
    Unpin,
    /// A post setting a closed status.
    Close,
    /// A post setting `open`.
    Reopen,
}

impl Op {
    /// The operation an `op=` value names.
    #[must_use]
    pub fn of(s: &str) -> Option<Self> {
        Some(match s {
            "rename" => Self::Rename,
            "move" => Self::Move,
            "attach" => Self::Attach,
            "detach" => Self::Detach,
            "pin" => Self::Pin,
            "unpin" => Self::Unpin,
            "close" => Self::Close,
            "reopen" => Self::Reopen,
            _ => return None,
        })
    }

    /// Its `op=` value.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Rename => "rename",
            Self::Move => "move",
            Self::Attach => "attach",
            Self::Detach => "detach",
            Self::Pin => "pin",
            Self::Unpin => "unpin",
            Self::Close => "close",
            Self::Reopen => "reopen",
        }
    }

    /// The tenant-wide right the menu asks of the viewer before offering it (09 §8.1).
    fn right(self) -> &'static str {
        match self {
            Self::Close | Self::Reopen => "edit",
            _ => "move",
        }
    }

    /// Whether the thread, as it stands, gives the operation something to do.
    fn applies(self, t: &ThreadView) -> bool {
        let attachments = || std::iter::once(&t.home).chain(&t.listings);
        match self {
            Self::Rename | Self::Move | Self::Attach => true,
            Self::Detach => !t.listings.is_empty(),
            Self::Pin => attachments().any(|a| a.pinned.is_none()),
            Self::Unpin => attachments().any(|a| a.pinned.is_some()),
            Self::Close => !t.closed,
            Self::Reopen => t.closed,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Rename => "ts-thread-action-rename",
            Self::Move => "ts-thread-action-move",
            Self::Attach => "ts-thread-action-attach",
            Self::Detach => "ts-thread-action-detach",
            Self::Pin => "ts-thread-action-pin",
            Self::Unpin => "ts-thread-action-unpin",
            Self::Close => "ts-thread-action-close",
            Self::Reopen => "ts-thread-action-reopen",
        }
    }
}

const ALL: [Op; 8] = [
    Op::Rename,
    Op::Move,
    Op::Attach,
    Op::Detach,
    Op::Pin,
    Op::Unpin,
    Op::Close,
    Op::Reopen,
];

/// The address of an operation's form.
fn href(title: &str, op: Op, on: Option<&str>) -> String {
    let mut h = format!("{}?action=thread&op={}", thread_href(title), op.name());
    if let Some(on) = on {
        h.push_str("&on=");
        h.push_str(&title_url(on));
    }
    h
}

/// The thread page's overflow menu, `(label, href)`: what the viewer may do and the
/// thread gives something to do.
#[must_use]
pub fn menu(cx: &Context, t: &ThreadView) -> Vec<(String, String)> {
    if cx.user.is_none() {
        return Vec::new();
    }
    ALL.iter()
        .filter(|op| cx.holds(op.right()) && op.applies(t))
        .map(|op| (cx.m.get(op.key()), href(&t.title, *op, None)))
        .collect()
}

/// **Pin here** or **Unpin here** on a talk page's listing of a thread, for a viewer who
/// may.
#[must_use]
pub fn pin_here(
    cx: &Context,
    t: &ThreadView,
    talk: &str,
    here: Option<&Attached>,
) -> Option<Choice> {
    if cx.user.is_none() || !cx.holds("move") {
        return None;
    }
    let pinned = here.is_some_and(|a| a.pinned.is_some());
    let (op, key) = if pinned {
        (Op::Unpin, "ts-thread-unpin-here")
    } else {
        (Op::Pin, "ts-thread-pin-here")
    };
    Some(choice(href(&t.title, op, Some(talk)), cx.m.get(key)))
}

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what the skin's `discussion/action.html` draws.
// ---------------------------------------------------------------------------------------

/// A thread action's form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActionPage {
    /// What the form does, in a sentence.
    pub intro: String,
    /// What the API refused, or why the form came back.
    pub notice: Option<MessageModel>,
    /// Where it posts.
    pub action: String,
    /// The viewer's CSRF token.
    pub token: String,
    /// The operation, `op=`.
    pub op: String,
    /// The thread's base offset as the form was drawn.
    pub base: u64,
    /// The talk page to return to, where the form was opened from one.
    pub returnto: Option<String>,
    /// The fields.
    pub fields: Vec<FieldRow>,
    /// The options.
    pub checks: Vec<CheckRow>,
    /// The thread, linked.
    pub thread: Choice,
    /// The confirm button.
    pub submit: ButtonModel,
    /// Back to the thread.
    pub cancel: Choice,
}

/// What a form keeps when it comes back.
#[derive(Debug, Default)]
struct Kept<'a> {
    notice: Option<String>,
    subject: Option<&'a str>,
    target: Option<&'a str>,
    keep: bool,
    status: Option<&'a str>,
    summary: &'a str,
    text: &'a str,
}

fn select(id: &str, name: &str, label: String, options: Vec<OptionModel>) -> FieldRow {
    FieldRow {
        id: id.to_string(),
        label,
        class: None,
        control: ControlModel::Select {
            id: Some(id.to_string()),
            name: name.to_string(),
            options,
            described_by: None,
        },
        help: None,
        help_id: None,
    }
}

fn text_area(id: &str, name: &str, label: String, value: &str) -> FieldRow {
    FieldRow {
        id: id.to_string(),
        label,
        class: None,
        control: ControlModel::TextArea {
            id: Some(id.to_string()),
            name: name.to_string(),
            value: value.to_string(),
            rows: 4,
            no_spellcheck: false,
            described_by: None,
        },
        help: None,
        help_id: None,
    }
}

/// The attachments a select offers: pinned or not for Pin and Unpin, the listings for
/// Remove a listing; `on` chosen where it is one of them.
fn attachment_options(op: Op, t: &ThreadView, on: Option<&str>) -> Vec<OptionModel> {
    let all: Vec<&Attached> = match op {
        Op::Detach => t.listings.iter().collect(),
        Op::Pin => std::iter::once(&t.home)
            .chain(&t.listings)
            .filter(|a| a.pinned.is_none())
            .collect(),
        Op::Unpin => std::iter::once(&t.home)
            .chain(&t.listings)
            .filter(|a| a.pinned.is_some())
            .collect(),
        _ => Vec::new(),
    };
    all.iter()
        .filter_map(|a| a.title.as_deref())
        .map(|title| OptionModel {
            value: title.to_string(),
            label: title.to_string(),
            selected: on.is_some_and(|o| same_title(o, title)),
        })
        .collect()
}

fn same_title(a: &str, b: &str) -> bool {
    a.replace('_', " ") == b.replace('_', " ")
}

#[allow(clippy::too_many_lines)]
fn page_model(
    m: &Messages,
    t: &ThreadView,
    op: Op,
    tok: &str,
    on: Option<&str>,
    kept: &Kept<'_>,
) -> ActionPage {
    let mut fields = Vec::new();
    let mut checks = Vec::new();
    let summary = || {
        FieldRow::text(
            "ts-thread-action-summary",
            "summary",
            m.get("ts-thread-action-summary"),
            kept.summary,
            false,
        )
    };
    let target_field = |label: &str| {
        let mut f = FieldRow::text(
            "ts-thread-action-target",
            "target",
            m.get(label),
            kept.target.unwrap_or_default(),
            true,
        );
        f.help = Some(m.get("ts-thread-action-target-help"));
        f.help_id = Some("ts-thread-action-target-help".into());
        if let ControlModel::Input { described_by, .. } = &mut f.control {
            described_by.clone_from(&f.help_id);
        }
        f
    };
    match op {
        Op::Rename => {
            fields.push(FieldRow::text(
                "ts-thread-action-subject",
                "subject",
                m.get("ts-thread-subject"),
                kept.subject.unwrap_or(&t.subject),
                true,
            ));
            fields.push(summary());
        }
        Op::Move => {
            fields.push(target_field("ts-thread-action-move-to"));
            checks.push(CheckRow {
                class: "ts-thread-action__keep".into(),
                control: ControlModel::check(
                    "ts-thread-action-keep",
                    "keep",
                    kept.keep,
                    m.get("ts-thread-action-keep"),
                ),
                help: m.get("ts-thread-action-keep-help"),
            });
            fields.push(summary());
        }
        Op::Attach => {
            fields.push(target_field("ts-thread-action-attach-to"));
            fields.push(summary());
        }
        Op::Detach | Op::Pin | Op::Unpin => {
            let chosen = kept.target.or(on);
            fields.push(select(
                "ts-thread-action-target",
                "target",
                m.get("ts-thread-action-where"),
                attachment_options(op, t, chosen),
            ));
            fields.push(summary());
        }
        Op::Close => {
            let chosen = kept.status.unwrap_or("resolved");
            fields.push(select(
                "ts-thread-action-status",
                "status",
                m.get("ts-thread-status"),
                CLOSING
                    .iter()
                    .map(|s| OptionModel {
                        value: (*s).to_string(),
                        label: status_label(m, s),
                        selected: *s == chosen,
                    })
                    .collect(),
            ));
            fields.push(text_area(
                "ts-thread-action-text",
                "wpTextbox1",
                m.get("ts-thread-action-statement"),
                kept.text,
            ));
        }
        Op::Reopen => {
            fields.push(text_area(
                "ts-thread-action-text",
                "wpTextbox1",
                m.get("ts-thread-action-comment"),
                kept.text,
            ));
        }
    }
    let full = format!("Thread:{}", t.title);
    ActionPage {
        intro: m.with(&format!("{}-intro", op.key()), &[&t.subject]),
        notice: kept
            .notice
            .as_deref()
            .map(|n| message_model(MessageKind::Error, n)),
        action: format!("/w/index.php?title={}&action=thread", title_url(&full)),
        token: tok.to_string(),
        op: op.name().to_string(),
        base: t.base,
        returnto: on.map(str::to_string),
        fields,
        checks,
        thread: super::page_link(&full),
        submit: super::submit(&m.get(&format!("{}-submit", op.key()))),
        cancel: choice(
            on.map_or_else(
                || thread_href(&t.title),
                |o| format!("/wiki/{}", title_url(o)),
            ),
            m.get("ts-thread-cancel"),
        ),
    }
}

/// The thread a title names, or the response that says why there is none.
async fn the_thread(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    info: &Lookup<PageInfo>,
) -> Result<ThreadView, Response> {
    let Lookup::Found(page) = info else {
        let title = match info {
            Lookup::Missing { title, .. } => title.clone(),
            _ => String::new(),
        };
        return Err(pages::no_page(cx, headers, title));
    };
    match site.client().thread(incoming, page.page_id).await {
        Ok(t) => {
            cx.inputs.push(t.cache);
            Ok(t.value)
        }
        Err(ClientError::Api { .. }) => Err(pages::no_page(cx, headers, page.title.clone())),
        Err(e) => Err(pages::api_failed(cx, headers, &e)),
    }
}

fn to_login(title: &str) -> Response {
    pages::see_other(
        &format!(
            "/w/index.php?title=Special:UserLogin&returnto={}",
            title_url(title)
        ),
        &[],
    )
}

/// `GET Thread:…?action=thread&op={op}`: the operation's form.
pub async fn form(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    info: Lookup<PageInfo>,
    params: &Params,
) -> Response {
    let Some(op) = params.map.get("op").and_then(|o| Op::of(o)) else {
        return super::thread(site, cx, headers, incoming, info, &super::Kept::default()).await;
    };
    let t = match the_thread(site, &mut cx, headers, incoming, &info).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    if cx.user.is_none() {
        return to_login(&format!("Thread:{}", t.title));
    }
    draw(
        site,
        cx,
        headers,
        incoming,
        &t,
        op,
        params.map.get("on").map(String::as_str),
        &Kept::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn draw(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    t: &ThreadView,
    op: Op,
    on: Option<&str>,
    kept: &Kept<'_>,
) -> Response {
    let (tok, cookies) = match token(site, &mut cx, incoming).await {
        Ok(x) => x,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let status = if kept.notice.is_some() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    let full = format!("Thread:{}", t.title);
    let r = pages::respond(&cx, headers, status, |m| {
        let body = cx.draw(
            "discussion/action.html",
            page_model(m, t, op, &tok, on, kept),
        );
        Page {
            title: m.get(&format!("{}-title", op.key())),
            returnto: Some(full.clone()),
            body,
            ..Page::default()
        }
    });
    pages::with_cookies(r, &cookies)
}

/// Where a title the reader typed or chose leads a thread: the talk page of a page (a
/// talk title is its subject's), a board, which is its own, or a user's talk page. `None`
/// for a title that leads nowhere a thread can be.
async fn target_of(
    site: &Site,
    incoming: &Incoming,
    title: &str,
) -> Result<Option<Target>, ClientError> {
    let info = site.client().page_info(incoming, title).await?.value;
    let (ns, prefixed, found) = match &info {
        Lookup::Found(p) => (p.ns, p.title.clone(), Some(p.page_id)),
        Lookup::Missing { ns, title } => (*ns, title.clone(), None),
        Lookup::Invalid => return Ok(None),
    };
    match kind_of(ns) {
        Some(Kind::Thread) => Ok(None),
        Some(Kind::Talk) => {
            let Some(subject) = subject_of(&prefixed) else {
                return Ok(None);
            };
            if let Some((p, name)) = subject.split_once(':')
                && p.eq_ignore_ascii_case("user")
            {
                return Ok(Some(Target::User(name.to_string())));
            }
            Ok(
                match site.client().page_info(incoming, &subject).await?.value {
                    Lookup::Found(p) => Some(Target::Page(p.page_id)),
                    _ => None,
                },
            )
        }
        None => Ok(found.map(Target::Page)),
    }
}

/// A target, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Page(u64),
    User(String),
}

impl Target {
    fn on(&self) -> NewThreadOn<'_> {
        match self {
            Self::Page(id) => NewThreadOn::Page(*id),
            Self::User(name) => NewThreadOn::User(name),
        }
    }
}

/// The message a refusal shows.
fn refusal(m: &Messages, code: &str, message: &str) -> String {
    let key = format!("ts-thread-action-refused-{code}");
    if m.has(&key) {
        m.get(&key)
    } else {
        m.with("ts-thread-action-refused", &[message])
    }
}

/// `POST Thread:…?action=thread`: the operation, then back to the thread, or to the talk
/// page the form was opened from; the form again, with the reason, where it was refused.
#[allow(clippy::too_many_lines)]
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    info: Lookup<PageInfo>,
    form: &Params,
) -> Response {
    let get = |k: &str| form.map.get(k).map(String::as_str);
    let Some(op) = get("op").and_then(Op::of) else {
        return pages::no_page(&cx, headers, String::new());
    };
    let t = match the_thread(site, &mut cx, headers, incoming, &info).await {
        Ok(t) => t,
        Err(r) => return r,
    };
    let token = get("wpEditToken").unwrap_or_default();
    let base = get("base").and_then(|b| b.parse().ok()).unwrap_or(t.base);
    let summary = get("summary").unwrap_or_default().trim();
    let target_text = get("target").map(str::trim).filter(|s| !s.is_empty());
    let keep = get("keep").is_some();
    let status = get("status");
    let text = get("wpTextbox1").map_or("", |t| t.trim_start_matches('\n'));
    let returnto = get("returnto").filter(|r| !r.is_empty());
    let mut kept = Kept {
        subject: get("subject"),
        target: target_text,
        keep,
        status,
        summary,
        text,
        ..Kept::default()
    };
    macro_rules! back {
        ($cx:expr, $kept:expr) => {
            draw(site, $cx, headers, incoming, &t, op, returnto, &$kept).await
        };
    }
    let target = match op {
        Op::Move | Op::Attach | Op::Detach | Op::Pin | Op::Unpin => {
            let Some(text) = target_text else {
                kept.notice = Some(cx.m.get("ts-thread-action-no-target"));
                return back!(cx, kept);
            };
            match target_of(site, incoming, text).await {
                Ok(Some(t)) => Some(t),
                Ok(None) => {
                    kept.notice = Some(cx.m.with("ts-thread-action-no-such-target", &[text]));
                    return back!(cx, kept);
                }
                Err(e) => return pages::api_failed(&cx, headers, &e),
            }
        }
        _ => None,
    };
    let written = match (op, &target) {
        (Op::Close | Op::Reopen, _) => {
            let st = match op {
                Op::Close => {
                    let Some(s) = status.filter(|s| CLOSING.contains(s)) else {
                        kept.notice = Some(cx.m.get("ts-thread-action-no-status"));
                        return back!(cx, kept);
                    };
                    s
                }
                _ => "open",
            };
            site.client()
                .reply(
                    incoming,
                    t.id,
                    &PostForm {
                        text,
                        in_reply_to: None,
                        status: Some(st),
                        token,
                    },
                )
                .await
        }
        (Op::Rename, _) => {
            let subject = get("subject").unwrap_or_default().trim();
            site.client()
                .change_thread(
                    incoming,
                    t.id,
                    &ThreadChange::Rename(subject),
                    base,
                    summary,
                    token,
                )
                .await
        }
        (_, Some(target)) => {
            let on = target.on();
            let change = match op {
                Op::Move => ThreadChange::Move(on, keep),
                Op::Attach => ThreadChange::Attach(on),
                Op::Detach => ThreadChange::Detach(on),
                Op::Pin => ThreadChange::Pin(on),
                _ => ThreadChange::Unpin(on),
            };
            site.client()
                .change_thread(incoming, t.id, &change, base, summary, token)
                .await
        }
        (_, None) => unreachable!("every other operation has a target"),
    };
    match written {
        Ok(Written::Done { revid, title, .. }) => {
            // A rename changes the title: the API answers with the new one.
            let where_to = match (returnto, op) {
                (Some(r), Op::Pin | Op::Unpin) => {
                    format!("/wiki/{}#thread-{}", title_url(r), t.id)
                }
                (_, Op::Close | Op::Reopen) => {
                    format!("{}#post-{revid}", thread_href(&t.title))
                }
                _ => thread_href(title.as_deref().unwrap_or(&t.title)),
            };
            pages::see_other(&where_to, &[])
        }
        Ok(Written::Refused { code, message }) => {
            kept.notice = Some(refusal(&cx.m, &code, &message));
            back!(cx, kept)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread() -> ThreadView {
        ThreadView {
            id: 12,
            title: "2026-11-05/Scope".into(),
            subject: "Scope".into(),
            status: "open".into(),
            home: Attached {
                talk_page_id: Some(9),
                title: Some("Item talk:Q5".into()),
                attached: Some("2026-11-05T10:00:00Z".into()),
                pinned: None,
            },
            listings: vec![Attached {
                talk_page_id: Some(30),
                title: Some("Board:Ontology".into()),
                attached: Some("2026-11-05T11:00:00Z".into()),
                pinned: Some("2026-11-05T12:00:00Z".into()),
            }],
            base: 40,
            ..ThreadView::default()
        }
    }

    // 0019 §8, 14 §3.6, §4.2: what each operation finds to do on a thread as it stands.
    #[test]
    fn an_operation_applies_where_the_thread_gives_it_something_to_do() {
        let mut t = thread();
        assert!(Op::Detach.applies(&t));
        assert!(Op::Pin.applies(&t) && Op::Unpin.applies(&t));
        assert!(Op::Close.applies(&t) && !Op::Reopen.applies(&t));
        t.listings.clear();
        t.closed = true;
        assert!(!Op::Detach.applies(&t) && !Op::Unpin.applies(&t));
        assert!(!Op::Close.applies(&t) && Op::Reopen.applies(&t));
        for op in ALL {
            assert_eq!(Op::of(op.name()), Some(op));
        }
        assert_eq!(Op::Close.right(), "edit");
        assert_eq!(Op::Pin.right(), "move");
    }

    // 14 §4.2: Pin offers the attachments it is not pinned on, Unpin those it is, Remove a
    // listing the listings; the page a talk page's Pin here names is chosen.
    #[test]
    fn the_attachment_choices_follow_the_operation() {
        let t = thread();
        let labels = |o: Vec<OptionModel>| o.into_iter().map(|x| x.label).collect::<Vec<_>>();
        assert_eq!(
            labels(attachment_options(Op::Pin, &t, None)),
            ["Item talk:Q5"]
        );
        assert_eq!(
            labels(attachment_options(Op::Unpin, &t, None)),
            ["Board:Ontology"]
        );
        assert_eq!(
            labels(attachment_options(Op::Detach, &t, None)),
            ["Board:Ontology"]
        );
        let chosen = attachment_options(Op::Pin, &t, Some("Item_talk:Q5"));
        assert!(chosen[0].selected);
    }

    #[test]
    fn a_form_address_names_the_operation_and_the_page() {
        assert_eq!(
            href("2026-11-05/Scope", Op::Pin, Some("Item talk:Q5")),
            "/wiki/Thread:2026-11-05/Scope?action=thread&op=pin&on=Item_talk:Q5"
        );
    }
}
