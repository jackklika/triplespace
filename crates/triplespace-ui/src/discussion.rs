//! Talk pages and threads (0019 §8; 19 §6.1; 14 §1–§4): what a reader sees at
//! `Item talk:Q42` and at `Thread:2026-10-22/Why is P31 wrong here`, and the two forms a
//! reader posts from them.
//!
//! The look is a talk page's, as DiscussionTools draws one: each thread a heading with a
//! rule beneath it, posts indented by depth, each ending in its author's signature line
//! with the time as its permalink. What tells the reader this is not a wikitext page is the
//! frame: there is no Edit tab, posts have their own Reply, a status shows as a chip beside
//! the subject once a post has set one, and a thread's header names where it lives.
//!
//! - **A talk page** lists its threads pinned first, then in the order they were attached
//!   to it, newest last; `?sort=activity` orders them by their latest post instead. A
//!   closed thread is collapsed behind a summary line, readable without JavaScript
//!   (`<details>`). **Add topic** (`?action=addtopic`) opens the new-thread form.
//! - **A thread page** shows the whole tree, with ranges a post collapsed (hatting) in a
//!   `<details>` at the place of their first post, its home and listings, its status, and
//!   the reply form, with a **Status** menu: a status is set by posting (14 §2.1).
//! - **Posting** goes to the thread API with the viewer's CSRF token. The form says which
//!   markup the wiki's posts use (`thread.markup`); a trailing signature is removed by the
//!   API without comment (14 §1.6).
//!
//! - **History** (`?action=history`) is the talk page's composite history, or the thread's
//!   own, each row saying what it did ("replied", "listed the thread here").
//!
//! Not yet: the Page data tab, Pin here, List on… and Move, starting a thread on a user talk page, boards,
//! foreign threads, and a talk title in a namespace whose local name is not `… talk`.

use std::fmt::Write as _;

use askama::Template as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::Namespaces;
use triplespace_client::{
    CacheInfo, ClientError, Incoming, Lookup, NewThreadOn, PageInfo, PostForm, PostView,
    ThreadView, Written,
};

use crate::Site;
use crate::codex::{Action as ButtonAction, Button, MessageKind, Weight};
use crate::frame::{Page, Tab, title_url};
use crate::html::{esc, link};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params};
use crate::special::changes::{clock, date};

/// The deepest a post is indented; a deeper one is shown at this depth with a "replying
/// to" line. The instance's `thread.max_depth` is not reported yet.
const MAX_DEPTH: usize = 6;

/// The statuses the Status menu offers: the shipped defaults (`thread-statuses.toml`), less
/// `archived`, which imports write. A tenant's own statuses are not reported yet.
const STATUSES: &[&str] = &[
    "open",
    "in-progress",
    "blocked",
    "resolved",
    "closed",
    "wontfix",
    "declined",
    "stale",
];

/// What a title in a discussion namespace is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A talk page: the threads attached to a subject.
    Talk,
    /// A thread.
    Thread,
}

/// What a namespace's pages are, from the namespace registry's default model.
#[must_use]
pub fn kind_of(ns: i64) -> Option<Kind> {
    let n = Namespaces::embedded().get(i32::try_from(ns).ok()?)?;
    match n.default_model.as_deref() {
        Some("triplespace-talk") => Some(Kind::Talk),
        Some("triplespace-thread") => Some(Kind::Thread),
        _ => None,
    }
}

/// Whether a title may name a talk page or thread, by its prefix alone: `Talk:`,
/// `… talk:` or `Thread:`. Only these titles cost the extra lookup that decides.
#[must_use]
pub fn may_be_discussion(title: &str) -> bool {
    let Some((prefix, _)) = title.split_once(':') else {
        return false;
    };
    let p = prefix.trim().replace('_', " ").to_lowercase();
    p == "talk" || p == "thread" || p.ends_with(" talk")
}

/// The subject of a talk title: `Item talk:Q42` → `Item:Q42`, `Talk:Foo` → `Foo`.
#[must_use]
pub fn subject_of(talk: &str) -> Option<String> {
    let (prefix, rest) = talk.split_once(':')?;
    let p = prefix.replace('_', " ");
    if p.eq_ignore_ascii_case("talk") {
        return Some(rest.to_string());
    }
    let lower = p.to_lowercase();
    let cut = lower.strip_suffix(" talk")?.len();
    Some(format!("{}:{rest}", &p[..cut]))
}

fn thread_href(title: &str) -> String {
    format!("/wiki/Thread:{}", title_url(title))
}

/// The signature line's time: `14:02, 24 October 2026 (UTC)`.
fn sig_time(m: &Messages, iso: &str) -> String {
    format!("{}, {} (UTC)", clock(iso), date(m, iso))
}

fn status_label(m: &Messages, status: &str) -> String {
    let key = format!("ts-thread-status-{status}");
    if m.has(&key) {
        m.get(&key)
    } else {
        status.to_string()
    }
}

/// The status chip.
fn chip(m: &Messages, status: &str, closed: bool) -> String {
    format!(
        "<span class=\"ts-status ts-status--{} ts-status--{}\">{}</span>",
        if closed { "closed" } else { "open" },
        esc(status),
        esc(&status_label(m, status))
    )
}

/// A post's signature line: the author and their talk page, and the time, which is the
/// post's permalink on its thread page.
fn signature(m: &Messages, p: &PostView, thread_title: &str) -> String {
    let who = match &p.author {
        Some(a) => format!(
            "{} ({})",
            link(&format!("/wiki/User:{}", title_url(a)), &esc(a), ""),
            link(
                &format!("/wiki/User_talk:{}", title_url(a)),
                &esc(&m.get("ts-thread-talk")),
                ""
            )
        ),
        None => format!(
            "<span class=\"ts-post__hidden\">{}</span>",
            esc(&m.get("ts-thread-user-hidden"))
        ),
    };
    let when = p.published.as_deref().map_or_else(String::new, |t| {
        link(
            &format!("{}#post-{}", thread_href(thread_title), p.id),
            &esc(&sig_time(m, t)),
            "class=\"ts-post__time\"",
        )
    });
    format!("<p class=\"ts-post__sig\">{who} {when}</p>")
}

/// One post: its text, its status line, its signature, and Reply.
fn post_html(
    m: &Messages,
    p: &PostView,
    thread: &ThreadView,
    parent_name: Option<&str>,
    reply_href: Option<&str>,
) -> String {
    let depth = p.depth.min(MAX_DEPTH);
    let mut out = format!(
        "<div class=\"ts-post\" id=\"post-{}\" style=\"--ts-depth:{depth}\">",
        p.id
    );
    if p.depth > MAX_DEPTH
        && let Some(name) = parent_name
    {
        let _ = write!(
            out,
            "<p class=\"ts-post__replying\">{}</p>",
            esc(&m.with("ts-thread-replying-to", &[name]))
        );
    }
    match &p.html {
        Some(h) => out.push_str(h),
        None => {
            let _ = write!(
                out,
                "<p class=\"ts-post__hidden\">{}</p>",
                esc(&m.get("ts-thread-text-hidden"))
            );
        }
    }
    if let Some(st) = &p.status {
        let closed = !matches!(st.as_str(), "open" | "in-progress" | "blocked");
        let who = p.author.as_deref().unwrap_or("?");
        let _ = write!(
            out,
            "<p class=\"ts-post__event\">{} {}</p>",
            esc(&m.with("ts-thread-set-status", &[who])),
            chip(m, st, closed)
        );
    }
    out.push_str(&signature(m, p, &thread.title));
    if let Some(href) = reply_href {
        let _ = write!(
            out,
            "<p class=\"ts-post__actions\">{}</p>",
            link(
                href,
                &esc(&m.get("ts-thread-reply")),
                "class=\"ts-post__reply\""
            )
        );
    }
    out.push_str("</div>");
    out
}

/// The posts of a thread in tree order, with each standing collapse drawn as a
/// `<details>` at the place of its first post, under the collapsing post's text.
/// `reply` makes a post's Reply link.
fn posts_html(m: &Messages, t: &ThreadView, reply: &dyn Fn(&PostView) -> Option<String>) -> String {
    let by_id: std::collections::HashMap<u64, &PostView> =
        t.posts.iter().map(|p| (p.id, p)).collect();
    // The posts that collapse a standing range: shown at the place of the range's first.
    let mut at_first: std::collections::HashMap<u64, &PostView> = std::collections::HashMap::new();
    for p in &t.posts {
        if let Some((first, _)) = p.collapse
            && by_id.get(&first).is_some_and(|f| f.collapsed)
        {
            at_first.insert(first, p);
        }
    }
    let collapsers: std::collections::HashSet<u64> = at_first.values().map(|p| p.id).collect();
    let mut s = String::new();
    let mut open_until: Vec<u64> = Vec::new();
    for p in &t.posts {
        if collapsers.contains(&p.id) {
            continue;
        }
        if let Some(c) = at_first.get(&p.id) {
            let _ = write!(
                s,
                "<details class=\"ts-hat\" style=\"--ts-depth:{}\"><summary>{}<span class=\"ts-hat__by\">{}</span></summary>",
                p.depth.min(MAX_DEPTH),
                c.html.as_deref().unwrap_or_default(),
                esc(&m.with(
                    "ts-thread-collapsed-by",
                    &[c.author.as_deref().unwrap_or("?")]
                ))
            );
            open_until.push(c.collapse.map_or(p.id, |(_, last)| last));
        }
        let parent = p
            .in_reply_to
            .and_then(|r| by_id.get(&r))
            .and_then(|q| q.author.as_deref());
        s.push_str(&post_html(m, p, t, parent, reply(p).as_deref()));
        while open_until.last() == Some(&p.id) {
            open_until.pop();
            s.push_str("</details>");
        }
    }
    for _ in open_until {
        s.push_str("</details>");
    }
    s
}

/// "Latest comment … · n comments · n people in discussion".
fn meta(m: &Messages, t: &ThreadView) -> String {
    let latest = t
        .posts
        .iter()
        .filter_map(|p| p.published.as_deref())
        .max()
        .unwrap_or_default();
    let mut people: Vec<&str> = t.posts.iter().filter_map(|p| p.author.as_deref()).collect();
    people.sort_unstable();
    people.dedup();
    let n = t.posts.len().to_string();
    let k = people.len().to_string();
    m.with("ts-thread-meta", &[&sig_time(m, latest), &n, &k])
}

/// Whether any post has set a status: until one has, a thread reads as an ordinary
/// section and shows no chip.
fn status_set(t: &ThreadView) -> bool {
    t.posts.iter().any(|p| p.status.is_some())
}

/// The markup line under a text area.
fn markup_line(m: &Messages, markup: &str) -> String {
    let key = if markup == "wikitext" {
        "ts-thread-markup-wikitext"
    } else {
        "ts-thread-markup-markdown"
    };
    format!("<p class=\"ts-composer__markup\">{}</p>", esc(&m.get(key)))
}

/// A text area with its label.
fn text_area(id: &str, name: &str, label: &str, value: &str, rows: u8) -> String {
    format!(
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"{id}\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-area\"><textarea class=\"cdx-text-area__textarea\" id=\"{id}\" name=\"{name}\" rows=\"{rows}\" dir=\"auto\" spellcheck=\"true\" required>{}</textarea></div></div></div>",
        esc(label),
        esc(&format!("\n{value}"))
    )
}

fn submit(label: &str) -> String {
    Button {
        label,
        kind: "submit",
        action: ButtonAction::Progressive,
        weight: Weight::Primary,
    }
    .render()
    .unwrap_or_default()
}

/// The reply form of a thread page.
fn reply_form(
    m: &Messages,
    t: &ThreadView,
    markup: &str,
    token: &str,
    reply_to: Option<&PostView>,
    text: &str,
    status: &str,
) -> String {
    let label = match reply_to {
        Some(p) => m.with("ts-thread-reply-to", &[p.author.as_deref().unwrap_or("?")]),
        None => m.get("ts-thread-reply-thread"),
    };
    let mut s = format!(
        "<form class=\"ts-composer\" id=\"reply\" method=\"post\" action=\"/w/index.php?title=Thread:{}&amp;action=reply\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\">",
        title_url(&t.title),
        esc(token)
    );
    if let Some(p) = reply_to {
        let _ = write!(
            s,
            "<input type=\"hidden\" name=\"replyto\" value=\"{}\">",
            p.id
        );
    }
    s.push_str(&text_area("ts-reply-text", "wpTextbox1", &label, text, 5));
    let _ = write!(
        s,
        "<div class=\"ts-composer__bar\"><div class=\"ts-composer__status\"><label for=\"ts-reply-status\">{}</label> <select class=\"cdx-select\" id=\"ts-reply-status\" name=\"status\"><option value=\"\">{}</option>",
        esc(&m.get("ts-thread-status")),
        esc(&m.get("ts-thread-status-nochange"))
    );
    for st in STATUSES {
        let _ = write!(
            s,
            "<option value=\"{st}\"{}>{}</option>",
            if *st == status { " selected" } else { "" },
            esc(&status_label(m, st))
        );
    }
    s.push_str("</select></div>");
    s.push_str(&markup_line(m, markup));
    let _ = write!(
        s,
        "<div class=\"ts-composer__actions\">{}</div></div></form>",
        submit(&m.get("ts-thread-reply"))
    );
    s
}

/// The new-thread form of a talk page.
fn topic_form(
    m: &Messages,
    talk: &str,
    markup: &str,
    token: &str,
    subject: &str,
    text: &str,
) -> String {
    let mut s = format!(
        "<form class=\"ts-composer\" id=\"new-topic\" method=\"post\" action=\"/w/index.php?title={}&amp;action=addtopic\"><input type=\"hidden\" name=\"wpEditToken\" value=\"{}\">",
        title_url(talk),
        esc(token)
    );
    let _ = write!(
        s,
        "<div class=\"cdx-field\"><div class=\"cdx-label\"><label class=\"cdx-label__label\" for=\"ts-topic-subject\"><span class=\"cdx-label__label__text\">{}</span></label></div><div class=\"cdx-field__control\"><div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" id=\"ts-topic-subject\" name=\"wpSummary\" value=\"{}\" required autocomplete=\"off\"></div></div></div>",
        esc(&m.get("ts-thread-subject")),
        esc(subject)
    );
    s.push_str(&text_area(
        "ts-topic-text",
        "wpTextbox1",
        &m.get("ts-thread-description"),
        text,
        6,
    ));
    let _ = write!(
        s,
        "<div class=\"ts-composer__bar\">{}<div class=\"ts-composer__actions\">{}{}</div></div></form>",
        markup_line(m, markup),
        link(
            &format!("/wiki/{}", title_url(talk)),
            &esc(&m.get("ts-thread-cancel")),
            "class=\"cdx-button cdx-button--fake-button cdx-button--fake-button--enabled\""
        ),
        submit(&m.get("ts-talk-add-topic"))
    );
    s
}

/// A fresh CSRF token, the page made the viewer's alone.
async fn token(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
) -> Result<(String, Vec<axum::http::HeaderValue>), ClientError> {
    let t = site.client().token(incoming, "csrf").await?;
    cx.inputs.push(t.value.cache);
    cx.inputs.push(CacheInfo::default());
    Ok((t.value.value, t.set_cookies))
}

fn login_line(cx: &Context, title: &str, key: &str) -> String {
    if !cx.features.has(crate::features::Feature::Login) {
        return String::new();
    }
    format!(
        "<p class=\"ts-composer__login\">{}</p>",
        link(
            &format!(
                "/w/index.php?title=Special:UserLogin&returnto={}",
                title_url(title)
            ),
            &esc(&cx.m.get(key)),
            ""
        )
    )
}

fn talk_tabs(m: &Messages, talk: &str, subject: &str) -> Vec<Tab> {
    talk_tabs_at(m, talk, subject, false)
}

fn talk_tabs_at(m: &Messages, talk: &str, subject: &str, history: bool) -> Vec<Tab> {
    let base = format!("/wiki/{}", title_url(talk));
    vec![
        Tab {
            label: m.get("ts-tab-subject"),
            href: format!("/wiki/{}", title_url(subject)),
            current: false,
        },
        Tab {
            label: m.get("ts-tab-discussion"),
            href: base.clone(),
            current: !history,
        },
        Tab {
            label: m.get("ts-tab-history"),
            href: format!("{base}?action=history"),
            current: history,
        },
    ]
}

fn thread_tabs(m: &Messages, title: &str) -> Vec<Tab> {
    thread_tabs_at(m, title, false)
}

fn thread_tabs_at(m: &Messages, title: &str, history: bool) -> Vec<Tab> {
    let base = thread_href(title);
    vec![
        Tab {
            label: m.get("ts-tab-thread"),
            href: base.clone(),
            current: !history,
        },
        Tab {
            label: m.get("ts-tab-history"),
            href: format!("{base}?action=history"),
            current: history,
        },
    ]
}

/// The History tab of a talk page or thread (16 §6.1): `GET /page/{id}/history`, which for
/// a talk page is composite (the rows of every thread attached to it, and the events of
/// threads leaving it), drawn as every history is, each row saying what it did to which
/// thread. A talk page no thread was ever attached to has no history yet.
async fn history(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    info: Lookup<PageInfo>,
    kind: Kind,
    params: &Params,
) -> Response {
    let (prefixed, id) = match info {
        Lookup::Found(p) => (p.title, Some(p.page_id)),
        Lookup::Missing { title, .. } => (title, None),
        Lookup::Invalid => return pages::no_page(&cx, headers, String::new()),
    };
    if kind == Kind::Thread && id.is_none() {
        return pages::no_page(&cx, headers, prefixed);
    }
    let ask = crate::entity::history::Ask::of(&params.map);
    let query = ask.query(cx.m.lang());
    let page = match id {
        Some(id) => match site.client().page_history(incoming, id, &query).await {
            Ok(f) => {
                cx.inputs.push(f.cache);
                f.value.unwrap_or_default()
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        None => triplespace_client::ActivityPage::default(),
    };
    let subject = subject_of(&prefixed).unwrap_or_else(|| prefixed.clone());
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: prefixed.clone(),
        returnto: Some(prefixed.clone()),
        tabs: match kind {
            Kind::Talk => talk_tabs_at(m, &prefixed, &subject, true),
            Kind::Thread => thread_tabs_at(
                m,
                prefixed.strip_prefix("Thread:").unwrap_or(&prefixed),
                true,
            ),
        },
        body: crate::entity::history::body_here(&cx, &prefixed, &ask, id, &page),
        ..Page::default()
    })
}

/// What a form keeps when the API refuses it.
#[derive(Debug, Default)]
struct Kept<'a> {
    notice: Option<String>,
    subject: &'a str,
    text: &'a str,
    status: &'a str,
    reply_to: Option<u64>,
}

/// Serves a discussion title's Read and Add topic, or `None` for every other title and
/// ask, which the document pages serve.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: &str,
    params: &Params,
) -> Result<Response, Context> {
    let action = params.map.get("action").map(String::as_str);
    if !matches!(action, None | Some("view" | "addtopic" | "history"))
        || params.map.contains_key("diff")
        || params.map.contains_key("oldid")
        || !may_be_discussion(title)
    {
        return Err(cx);
    }
    let info = match site.client().page_info(incoming, title).await {
        Ok(i) => i,
        Err(e) => return Ok(pages::api_failed(&cx, headers, &e)),
    };
    cx.inputs.push(info.cache);
    let ns = match &info.value {
        Lookup::Found(p) => p.ns,
        Lookup::Missing { ns, .. } => *ns,
        Lookup::Invalid => return Err(cx),
    };
    let kept = Kept {
        subject: params.map.get("subject").map_or("", String::as_str),
        ..Kept::default()
    };
    let reply_to = params.map.get("replyto").and_then(|r| r.parse().ok());
    if action == Some("history") {
        return match kind_of(ns) {
            Some(k) => Ok(history(site, cx, headers, incoming, info.value, k, params).await),
            None => Err(cx),
        };
    }
    match kind_of(ns) {
        Some(Kind::Talk) => Ok(talk(
            site,
            cx,
            headers,
            incoming,
            info.value,
            action == Some("addtopic"),
            &kept,
            params.map.get("sort").is_some_and(|s| s == "activity"),
        )
        .await),
        Some(Kind::Thread) => Ok(thread(
            site,
            cx,
            headers,
            incoming,
            info.value,
            &Kept { reply_to, ..kept },
        )
        .await),
        None => Err(cx),
    }
}

/// A talk page.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn talk(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    info: Lookup<PageInfo>,
    adding: bool,
    kept: &Kept<'_>,
    by_activity: bool,
) -> Response {
    let (prefixed, talk_id) = match info {
        Lookup::Found(p) => (p.title, Some(p.page_id)),
        Lookup::Missing { title, .. } => (title, None),
        Lookup::Invalid => unreachable!("checked by the caller"),
    };
    let subject = subject_of(&prefixed).unwrap_or_else(|| prefixed.clone());
    let user_talk = subject
        .split_once(':')
        .is_some_and(|(p, _)| p.eq_ignore_ascii_case("user"));
    let subject_id = match site.client().page_info(incoming, &subject).await {
        Ok(f) => {
            cx.inputs.push(f.cache);
            match f.value {
                Lookup::Found(p) => Some(p.page_id),
                _ => None,
            }
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    // The talk page of a subject that does not exist does not exist either (14 §1.3).
    if subject_id.is_none() && talk_id.is_none() && !user_talk {
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            body: pages::message(MessageKind::Notice, &m.get("ts-talk-no-subject")),
            tabs: talk_tabs(m, &prefixed, &subject),
            returnto: Some(prefixed.clone()),
            title: prefixed.clone(),
            ..Page::default()
        });
    }
    let mut threads: Vec<ThreadView> = Vec::new();
    if let Some(id) = talk_id {
        match site.client().talk_threads(incoming, id).await {
            Ok(l) => {
                cx.inputs.push(l.cache);
                for listed in l.value.threads {
                    match site.client().thread(incoming, listed.id).await {
                        Ok(t) => {
                            cx.inputs.push(t.cache);
                            threads.push(t.value);
                        }
                        // A thread gone between the listing and the read is left out.
                        Err(ClientError::Api { .. }) => {}
                        Err(e) => return pages::api_failed(&cx, headers, &e),
                    }
                }
            }
            Err(ClientError::Api { .. }) => {}
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    order(&mut threads, talk_id.unwrap_or(0), by_activity);
    // A user talk page takes a topic whether or not the user page exists; the API refuses
    // a name no account holds.
    let can_add = cx.user.is_some() && (subject_id.is_some() || user_talk);
    let mut cookies = Vec::new();
    let mut tok = String::new();
    // Reading one's own talk page reads its messages: the orange bar goes (19 §6.2), as
    // MediaWiki's did.
    let own = user_talk
        && cx
            .user
            .as_deref()
            .is_some_and(|u| subject.split_once(':').is_some_and(|(_, n)| n == u));
    if own && cx.notifications.as_ref().is_some_and(|n| n.messages > 0) {
        match token(site, &mut cx, incoming).await {
            Ok((t, c)) => {
                if let Ok(triplespace_client::settings::Saved::Ok(_)) = site
                    .client()
                    .mark_read(incoming, None, Some("message"), &t)
                    .await
                    && let Some(n) = cx.notifications.as_mut()
                {
                    n.messages = 0;
                }
                tok = t;
                cookies = c;
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    if adding && can_add && tok.is_empty() {
        match token(site, &mut cx, incoming).await {
            Ok((t, c)) => {
                tok = t;
                cookies = c;
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    let markup = cx.site.thread_markup.clone();
    let tid = talk_id.unwrap_or(0);
    let status = if kept.notice.is_some() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    let watch = crate::watch::control(site, &cx, incoming, &prefixed).await;
    let r = pages::respond(&cx, headers, status, |m| {
        let mut body = String::from("<div class=\"ts-talk\">");
        let pinned = threads
            .iter()
            .filter(|t| t.attached_to(tid).is_some_and(|a| a.pinned.is_some()))
            .count();
        let _ = write!(
            body,
            "<div class=\"ts-talk__bar\"><p class=\"ts-talk__summary\">{}</p>",
            esc(&m.with(
                "ts-talk-summary",
                &[&threads.len().to_string(), &pinned.to_string()]
            ))
        );
        if can_add && !adding {
            let _ = write!(
                body,
                "<a class=\"cdx-button cdx-button--fake-button cdx-button--fake-button--enabled cdx-button--action-progressive cdx-button--weight-primary\" href=\"/wiki/{}?action=addtopic#new-topic\">{}</a>",
                title_url(&prefixed),
                esc(&m.get("ts-talk-add-topic"))
            );
        }
        body.push_str("</div>");
        if let Some(n) = &kept.notice {
            body.push_str(&pages::message(MessageKind::Error, n));
        }
        if adding {
            if can_add {
                body.push_str(&topic_form(
                    m,
                    &prefixed,
                    &markup,
                    &tok,
                    kept.subject,
                    kept.text,
                ));
            } else if cx.user.is_none() {
                body.push_str(&login_line(&cx, &prefixed, "ts-thread-login"));
            }
        }
        if threads.is_empty() && !adding {
            body.push_str(&pages::message(
                MessageKind::Notice,
                &m.get("ts-talk-empty"),
            ));
        }
        for t in &threads {
            body.push_str(&listed_thread(m, t, tid));
        }
        body.push_str("</div>");
        Page {
            title: prefixed.clone(),
            returnto: Some(prefixed.clone()),
            tabs: talk_tabs(m, &prefixed, &subject),
            body,
            watch,
            ..Page::default()
        }
    });
    pages::with_cookies(r, &cookies)
}

/// Pinned first, in the order they were pinned; then in the order they were attached to
/// this page, newest last, or by their latest post, newest first (19 §6.1).
fn order(threads: &mut [ThreadView], talk_id: u64, by_activity: bool) {
    let latest = |t: &ThreadView| {
        t.posts
            .iter()
            .filter_map(|p| p.published.clone())
            .max()
            .unwrap_or_default()
    };
    threads.sort_by(|a, b| {
        let (aa, ba) = (a.attached_to(talk_id), b.attached_to(talk_id));
        let (ap, bp) = (
            aa.and_then(|x| x.pinned.clone()),
            ba.and_then(|x| x.pinned.clone()),
        );
        match (&ap, &bp) {
            (Some(x), Some(y)) => return x.cmp(y),
            (Some(_), None) => return std::cmp::Ordering::Less,
            (None, Some(_)) => return std::cmp::Ordering::Greater,
            (None, None) => {}
        }
        if by_activity {
            latest(b).cmp(&latest(a))
        } else {
            let at = |x: Option<&triplespace_client::Attached>| {
                x.and_then(|y| y.attached.clone()).unwrap_or_default()
            };
            at(aa).cmp(&at(ba)).then(a.id.cmp(&b.id))
        }
    });
}

/// One thread on a talk page.
fn listed_thread(m: &Messages, t: &ThreadView, talk_id: u64) -> String {
    let here = t.attached_to(talk_id);
    let pinned = here.is_some_and(|a| a.pinned.is_some());
    let mut s = format!(
        "<section class=\"ts-thread\" id=\"thread-{}\"><div class=\"ts-thread__head\">",
        t.id
    );
    if pinned {
        let _ = write!(
            s,
            "<span class=\"ts-thread__pin\" role=\"img\" aria-label=\"{0}\" title=\"{0}\"></span>",
            esc(&m.get("ts-thread-pinned"))
        );
    }
    let _ = write!(
        s,
        "<h2 class=\"ts-thread__subject\">{}</h2>",
        link(&thread_href(&t.title), &esc(&t.subject), "")
    );
    if status_set(t) {
        s.push_str(&chip(m, &t.status, t.closed));
    }
    s.push_str("</div>");
    let mut line = meta(m, t);
    let elsewhere: Vec<&triplespace_client::Attached> = std::iter::once(&t.home)
        .chain(&t.listings)
        .filter(|a| a.talk_page_id != Some(talk_id))
        .collect();
    let started_here = t.home.talk_page_id == Some(talk_id);
    let mut extra = String::new();
    if !started_here && let Some(h) = &t.home.title {
        let _ = write!(
            extra,
            " · {} {}",
            esc(&m.get("ts-thread-started-on")),
            link(&format!("/wiki/{}", title_url(h)), &esc(h), "")
        );
    } else if !elsewhere.is_empty() {
        let names: Vec<String> = elsewhere
            .iter()
            .filter_map(|a| a.title.as_ref())
            .map(|n| link(&format!("/wiki/{}", title_url(n)), &esc(n), ""))
            .collect();
        let _ = write!(
            extra,
            " · {} {}",
            esc(&m.get("ts-thread-also-on")),
            names.join(", ")
        );
    }
    line = esc(&line);
    let _ = write!(s, "<p class=\"ts-thread__meta\">{line}{extra}</p>");
    let posts = posts_html(m, t, &|p| {
        Some(format!("{}?replyto={}#reply", thread_href(&t.title), p.id))
    });
    if t.closed {
        let _ = write!(
            s,
            "<details class=\"ts-thread__closed\"><summary>{}</summary>{posts}</details>",
            esc(&m.with(
                "ts-thread-show",
                &[&status_label(m, &t.status), &t.posts.len().to_string()]
            ))
        );
    } else {
        s.push_str(&posts);
    }
    s.push_str("</section>");
    s
}

/// A thread page.
#[allow(clippy::too_many_lines)]
async fn thread(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    info: Lookup<PageInfo>,
    kept: &Kept<'_>,
) -> Response {
    let Lookup::Found(page) = info else {
        let title = match info {
            Lookup::Missing { title, .. } => title,
            _ => String::new(),
        };
        return pages::no_page(&cx, headers, title);
    };
    let t = match site.client().thread(incoming, page.page_id).await {
        Ok(t) => {
            cx.inputs.push(t.cache);
            t.value
        }
        Err(ClientError::Api { .. }) => return pages::no_page(&cx, headers, page.title),
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let mut cookies = Vec::new();
    let mut tok = String::new();
    if cx.user.is_some() {
        match token(site, &mut cx, incoming).await {
            Ok((v, c)) => {
                tok = v;
                cookies = c;
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    let markup = cx.site.thread_markup.clone();
    let status = if kept.notice.is_some() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    };
    let watch = crate::watch::control(site, &cx, incoming, &format!("Thread:{}", t.title)).await;
    let r = pages::respond(&cx, headers, status, |m| {
        let mut body = String::from(
            "<div class=\"ts-thread-page\"><div class=\"ts-thread-page__head\"><dl class=\"ts-thread-page__where\">",
        );
        if let Some(h) = &t.home.title {
            let _ = write!(
                body,
                "<div><dt>{}</dt><dd>{}</dd></div>",
                esc(&m.get("ts-thread-home")),
                link(&format!("/wiki/{}", title_url(h)), &esc(h), "")
            );
        }
        let also: Vec<String> = t
            .listings
            .iter()
            .filter_map(|a| a.title.as_ref())
            .map(|n| link(&format!("/wiki/{}", title_url(n)), &esc(n), ""))
            .collect();
        if !also.is_empty() {
            let _ = write!(
                body,
                "<div><dt>{}</dt><dd>{}</dd></div>",
                esc(&m.get("ts-thread-also-on")),
                also.join(", ")
            );
        }
        let _ = write!(
            body,
            "</dl><p class=\"ts-thread-page__state\">{} <span class=\"ts-thread__meta\">{}</span></p></div>",
            chip(m, &t.status, t.closed),
            esc(&meta(m, &t))
        );
        let title = t.title.clone();
        body.push_str("<div class=\"ts-thread-page__posts\">");
        body.push_str(&posts_html(m, &t, &|p| {
            cx.user
                .as_ref()
                .map(|_| format!("{}?replyto={}#reply", thread_href(&title), p.id))
        }));
        body.push_str("</div>");
        if let Some(n) = &kept.notice {
            body.push_str(&pages::message(MessageKind::Error, n));
        }
        if cx.user.is_some() {
            let reply_to = kept
                .reply_to
                .and_then(|r| t.posts.iter().find(|p| p.id == r));
            body.push_str(&reply_form(
                m,
                &t,
                &markup,
                &tok,
                reply_to,
                kept.text,
                kept.status,
            ));
        } else {
            body.push_str(&login_line(
                &cx,
                &format!("Thread:{}", t.title),
                "ts-thread-login",
            ));
        }
        body.push_str("</div>");
        let full = format!("Thread:{}", t.title);
        Page {
            title: full.clone(),
            returnto: Some(full),
            tabs: thread_tabs(m, &t.title),
            body,
            watch,
            ..Page::default()
        }
    });
    pages::with_cookies(r, &cookies)
}

/// The message a refusal shows.
fn refusal(m: &Messages, code: &str, message: &str) -> String {
    let key = format!("ts-thread-refused-{code}");
    if m.has(&key) {
        m.get(&key)
    } else {
        m.with("ts-thread-refused", &[message])
    }
}

/// A form posted to a discussion title: `action=addtopic` on a talk page, `action=reply`
/// on a thread; `None` for anything else.
#[allow(clippy::too_many_lines)]
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: &str,
    action: &str,
    form: &Params,
) -> Result<Response, Context> {
    if !matches!(action, "addtopic" | "reply") || !may_be_discussion(title) {
        return Err(cx);
    }
    // The page is the viewer's alone: it holds their token and their text.
    cx.inputs.push(CacheInfo::default());
    let info = match site.client().page_info(incoming, title).await {
        Ok(i) => i.value,
        Err(e) => return Ok(pages::api_failed(&cx, headers, &e)),
    };
    let token = form.map.get("wpEditToken").map_or("", String::as_str);
    let text = form
        .map
        .get("wpTextbox1")
        .map_or("", |t| t.trim_start_matches('\n'));
    let status = form.map.get("status").map_or("", String::as_str);
    let reply_to = form.map.get("replyto").and_then(|r| r.parse::<u64>().ok());
    let ns = match &info {
        Lookup::Found(p) => p.ns,
        Lookup::Missing { ns, .. } => *ns,
        Lookup::Invalid => return Err(cx),
    };
    match (kind_of(ns), action) {
        (Some(Kind::Talk), "addtopic") => {
            let prefixed = match &info {
                Lookup::Found(p) => p.title.clone(),
                Lookup::Missing { title, .. } => title.clone(),
                Lookup::Invalid => return Err(cx),
            };
            let subject = form.map.get("wpSummary").map_or("", String::as_str);
            let subject_title = subject_of(&prefixed).unwrap_or_default();
            let user = subject_title
                .split_once(':')
                .filter(|(p, _)| p.eq_ignore_ascii_case("user"))
                .map(|(_, name)| name.to_string());
            let on = match &user {
                Some(name) => NewThreadOn::User(name),
                None => match site.client().page_info(incoming, &subject_title).await {
                    Ok(f) => match f.value {
                        Lookup::Found(p) => NewThreadOn::Page(p.page_id),
                        _ => return Ok(pages::no_page(&cx, headers, prefixed)),
                    },
                    Err(e) => return Ok(pages::api_failed(&cx, headers, &e)),
                },
            };
            let w = site
                .client()
                .start_thread(
                    incoming,
                    on,
                    subject,
                    &PostForm {
                        text,
                        in_reply_to: None,
                        status: None,
                        token,
                    },
                )
                .await;
            match w {
                Ok(Written::Done { thread, .. }) => Ok(pages::see_other(
                    &format!("/wiki/{}#thread-{thread}", title_url(&prefixed)),
                    &[],
                )),
                Ok(Written::Refused { code, message }) => {
                    let notice = refusal(&cx.m, &code, &message);
                    let kept = Kept {
                        notice: Some(notice),
                        subject,
                        text,
                        ..Kept::default()
                    };
                    Ok(talk(site, cx, headers, incoming, info, true, &kept, false).await)
                }
                Err(e) => Ok(pages::api_failed(&cx, headers, &e)),
            }
        }
        (Some(Kind::Thread), "reply") => {
            let Lookup::Found(page) = &info else {
                return Ok(pages::no_page(&cx, headers, title.to_string()));
            };
            let w = site
                .client()
                .reply(
                    incoming,
                    page.page_id,
                    &PostForm {
                        text,
                        in_reply_to: reply_to,
                        status: Some(status),
                        token,
                    },
                )
                .await;
            match w {
                Ok(Written::Done { revid, .. }) => Ok(pages::see_other(
                    &format!("/wiki/{}#post-{revid}", title_url(&page.title)),
                    &[],
                )),
                Ok(Written::Refused { code, message }) => {
                    let notice = refusal(&cx.m, &code, &message);
                    let kept = Kept {
                        notice: Some(notice),
                        text,
                        status,
                        reply_to,
                        ..Kept::default()
                    };
                    Ok(thread(site, cx, headers, incoming, info, &kept).await)
                }
                Err(e) => Ok(pages::api_failed(&cx, headers, &e)),
            }
        }
        _ => Err(cx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talk_titles_and_their_subjects() {
        assert!(may_be_discussion("Item talk:Q42"));
        assert!(may_be_discussion("Talk:Foo"));
        assert!(may_be_discussion("Thread:2026-10-22/Why"));
        assert!(may_be_discussion("Librarybase_talk:Scope"));
        assert!(!may_be_discussion("Item:Q42"));
        assert!(!may_be_discussion("Talkative"));
        assert_eq!(subject_of("Item talk:Q42").as_deref(), Some("Item:Q42"));
        assert_eq!(subject_of("Talk:Foo").as_deref(), Some("Foo"));
        assert_eq!(subject_of("User talk:Alice").as_deref(), Some("User:Alice"));
        assert_eq!(
            subject_of("Librarybase talk:Scope").as_deref(),
            Some("Librarybase:Scope")
        );
        assert_eq!(subject_of("Item:Q42"), None);
        assert_eq!(kind_of(1), Some(Kind::Talk));
        assert_eq!(kind_of(214), Some(Kind::Thread));
        assert_eq!(kind_of(0), None);
    }

    fn post(id: u64, depth: usize, author: &str) -> PostView {
        PostView {
            id,
            in_reply_to: (id > 1).then_some(id - 1),
            author: Some(author.into()),
            published: Some(format!("2026-10-2{id}T10:0{id}:00Z")),
            media_type: "text/markdown".into(),
            html: Some(format!("<div class=\"ts-markdown\"><p>Post {id}</p></div>")),
            depth,
            ..PostView::default()
        }
    }

    #[test]
    fn a_hat_wraps_its_range_at_the_place_of_its_first_post() {
        let m = Messages::for_language("en");
        let mut hat = post(4, 0, "MiraK");
        hat.in_reply_to = None;
        hat.collapse = Some((2, 3));
        hat.html = Some("<p>Off topic</p>".into());
        let mut p2 = post(2, 1, "Hollis");
        p2.collapsed = true;
        let mut p3 = post(3, 2, "James");
        p3.collapsed = true;
        let t = ThreadView {
            id: 9,
            title: "2026-10-22/Why".into(),
            subject: "Why".into(),
            status: "open".into(),
            posts: vec![post(1, 0, "Hollis"), p2, p3, hat],
            ..ThreadView::default()
        };
        let html = posts_html(&m, &t, &|_| None);
        let hat_at = html.find("<details class=\"ts-hat\"").unwrap();
        assert!(html.find("Post 1").unwrap() < hat_at);
        assert!(hat_at < html.find("Post 2").unwrap());
        assert!(html.find("Post 3").unwrap() < html.rfind("</details>").unwrap());
        assert!(html.contains("<p>Off topic</p>"));
        assert_eq!(
            html.matches("id=\"post-4\"").count(),
            0,
            "the collapsing post is the hat's summary"
        );
        assert!(html.contains("Hollis"), "{html}");
        assert!(html.contains("10:01, 21 October 2026 (UTC)"), "{html}");
    }

    #[test]
    fn a_chip_only_once_a_status_is_set() {
        let mut t = ThreadView {
            posts: vec![post(1, 0, "Hollis")],
            status: "open".into(),
            ..ThreadView::default()
        };
        assert!(!status_set(&t));
        t.posts[0].status = Some("in-progress".into());
        assert!(status_set(&t));
    }

    #[test]
    fn pinned_first_then_attached_order() {
        let att = |id: u64, at: &str, pinned: Option<&str>| ThreadView {
            id,
            home: triplespace_client::Attached {
                talk_page_id: Some(7),
                attached: Some(at.into()),
                pinned: pinned.map(str::to_string),
                ..Default::default()
            },
            ..ThreadView::default()
        };
        let mut v = vec![
            att(1, "2026-01-01", None),
            att(2, "2026-03-01", Some("2026-03-02")),
            att(3, "2026-02-01", None),
        ];
        order(&mut v, 7, false);
        assert_eq!(v.iter().map(|t| t.id).collect::<Vec<_>>(), [2, 1, 3]);
    }
}
