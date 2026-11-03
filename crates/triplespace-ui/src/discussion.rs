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

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::Namespaces;
use serde::Serialize;
use triplespace_client::{
    CacheInfo, ClientError, Incoming, Lookup, NewThreadOn, PageInfo, PostForm, PostView,
    ThreadView, Written,
};

use crate::Site;
use crate::codex::{
    Action as ButtonAction, Button, ButtonModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::history::Choice;
use crate::frame::{Page, Tab, title_url};
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

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what the skin's `discussion/*.html` templates draw.
// ---------------------------------------------------------------------------------------

/// The status chip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusChip {
    /// `closed` or `open`.
    pub state: &'static str,
    /// The status, as the API names it.
    pub status: String,
    /// The status, as the reader's language says it.
    pub label: String,
}

/// A post's author, linked, with their talk page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Author {
    /// The user page.
    pub user: Choice,
    /// The talk page.
    pub talk: Choice,
}

/// A post's signature line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signature {
    /// The author; `None` where hidden.
    pub author: Option<Author>,
    /// What the line says of a hidden author.
    pub hidden: String,
    /// The time, which is the post's permalink on its thread page.
    pub time: Option<Choice>,
}

/// A status a post set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Event {
    /// "X set the status".
    pub text: String,
    /// The chip.
    pub chip: StatusChip,
}

/// One post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PostModel {
    /// The post ID.
    pub id: u64,
    /// The depth shown, at most [`MAX_DEPTH`].
    pub depth: usize,
    /// "Replying to X", for a post deeper than the depth shown.
    pub replying_to: Option<String>,
    /// The text, HTML: the API's rendering of the post; `None` where hidden.
    pub post_html: Option<String>,
    /// What the post says of hidden text.
    pub hidden: String,
    /// A status the post set.
    pub event: Option<Event>,
    /// The signature line.
    pub signature: Signature,
    /// **Reply**.
    pub reply: Option<Choice>,
}

/// The posts of a thread in tree order, flattened: a post, or the opening or closing of
/// a hat, the `<details>` that holds a collapsed range at the place of its first post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
#[allow(clippy::large_enum_variant)]
pub enum PostItem {
    /// A hat opens: the collapsing post's text is its summary.
    HatOpen {
        /// The depth shown.
        depth: usize,
        /// The collapsing post's text, HTML.
        post_html: String,
        /// "collapsed by X".
        by: String,
    },
    /// A post.
    Post {
        /// The post.
        post: PostModel,
    },
    /// A hat closes.
    HatClose,
}

/// The text area of a form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextArea {
    /// The `id`.
    pub id: String,
    /// The `name`.
    pub name: String,
    /// The label.
    pub label: String,
    /// The value, with the leading newline the textarea's parser eats.
    pub value: String,
    /// The rows.
    pub rows: u8,
}

/// One status the reply form offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusOption {
    /// The value.
    pub value: String,
    /// The label.
    pub label: String,
    /// Whether it is selected.
    pub selected: bool,
}

/// The reply form of a thread page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReplyForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The post replied to.
    pub reply_to: Option<u64>,
    /// The text.
    pub text: TextArea,
    /// The status menu's label.
    pub status_label: String,
    /// The menu's first entry: no change.
    pub no_change: String,
    /// The statuses.
    pub statuses: Vec<StatusOption>,
    /// The markup line.
    pub markup: String,
    /// **Reply**.
    pub submit: ButtonModel,
}

/// The new-thread form of a talk page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The subject field's label.
    pub subject_label: String,
    /// The subject.
    pub subject: String,
    /// The text.
    pub text: TextArea,
    /// The markup line.
    pub markup: String,
    /// **Cancel**.
    pub cancel: Choice,
    /// **Add topic**.
    pub submit: ButtonModel,
}

/// Where else a listed thread lives: "started on X", or "also on A, B".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Elsewhere {
    /// The label.
    pub label: String,
    /// The pages.
    pub pages: Vec<Choice>,
}

/// One thread on a talk page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListedThread {
    /// The thread ID.
    pub id: u64,
    /// The pin's label, where the thread is pinned here.
    pub pinned: Option<String>,
    /// The subject, linked to the thread page.
    pub subject: Choice,
    /// The status chip, once a post has set one.
    pub chip: Option<StatusChip>,
    /// "Latest comment … · n comments · n people in discussion".
    pub meta: String,
    /// Where else it lives.
    pub elsewhere: Option<Elsewhere>,
    /// The summary of a closed thread, which collapses its posts.
    pub closed: Option<String>,
    /// The posts.
    pub posts: Vec<PostItem>,
}

/// A talk page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TalkPage {
    /// "n threads, n pinned".
    pub summary: String,
    /// **Add topic**.
    pub add: Option<Choice>,
    /// What the API refused.
    pub notice: Option<MessageModel>,
    /// The new-thread form.
    pub form: Option<TopicForm>,
    /// The login line, for a viewer who is not signed in.
    pub login: Option<Choice>,
    /// What the page says when it has no threads.
    pub empty: Option<MessageModel>,
    /// The threads.
    pub threads: Vec<ListedThread>,
}

/// A thread page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadPage {
    /// The home: its label and the page.
    pub home: Option<(String, Choice)>,
    /// Also on: its label and the pages.
    pub also: Option<(String, Vec<Choice>)>,
    /// The status chip.
    pub chip: StatusChip,
    /// "Latest comment … · n comments · n people in discussion".
    pub meta: String,
    /// The posts.
    pub posts: Vec<PostItem>,
    /// What the API refused.
    pub notice: Option<MessageModel>,
    /// The reply form.
    pub form: Option<ReplyForm>,
    /// The login line, for a viewer who is not signed in.
    pub login: Option<Choice>,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn page_link(title: &str) -> Choice {
    choice(format!("/wiki/{}", title_url(title)), title.to_string())
}

fn message_model(kind: MessageKind, text: &str) -> MessageModel {
    Message::block(kind, text).model()
}

/// The status chip.
fn chip_model(m: &Messages, status: &str, closed: bool) -> StatusChip {
    StatusChip {
        state: if closed { "closed" } else { "open" },
        status: status.to_string(),
        label: status_label(m, status),
    }
}

/// A post's signature line: the author and their talk page, and the time, which is the
/// post's permalink on its thread page.
fn signature_model(m: &Messages, p: &PostView, thread_title: &str) -> Signature {
    Signature {
        author: p.author.as_ref().map(|a| Author {
            user: choice(format!("/wiki/User:{}", title_url(a)), a.clone()),
            talk: choice(
                format!("/wiki/User_talk:{}", title_url(a)),
                m.get("ts-thread-talk"),
            ),
        }),
        hidden: m.get("ts-thread-user-hidden"),
        time: p.published.as_deref().map(|t| {
            choice(
                format!("{}#post-{}", thread_href(thread_title), p.id),
                sig_time(m, t),
            )
        }),
    }
}

/// One post: its text, its status line, its signature, and Reply.
fn post_model(
    m: &Messages,
    p: &PostView,
    thread: &ThreadView,
    parent_name: Option<&str>,
    reply_href: Option<String>,
) -> PostModel {
    PostModel {
        id: p.id,
        depth: p.depth.min(MAX_DEPTH),
        replying_to: (p.depth > MAX_DEPTH)
            .then_some(parent_name)
            .flatten()
            .map(|name| m.with("ts-thread-replying-to", &[name])),
        post_html: p.html.clone(),
        hidden: m.get("ts-thread-text-hidden"),
        event: p.status.as_ref().map(|st| {
            let closed = !matches!(st.as_str(), "open" | "in-progress" | "blocked");
            Event {
                text: m.with(
                    "ts-thread-set-status",
                    &[p.author.as_deref().unwrap_or("?")],
                ),
                chip: chip_model(m, st, closed),
            }
        }),
        signature: signature_model(m, p, &thread.title),
        reply: reply_href.map(|href| choice(href, m.get("ts-thread-reply"))),
    }
}

/// The posts of a thread in tree order, with each standing collapse drawn as a
/// `<details>` at the place of its first post, under the collapsing post's text.
/// `reply` makes a post's Reply link.
fn posts_model(
    m: &Messages,
    t: &ThreadView,
    reply: &dyn Fn(&PostView) -> Option<String>,
) -> Vec<PostItem> {
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
    let mut items = Vec::new();
    let mut open_until: Vec<u64> = Vec::new();
    for p in &t.posts {
        if collapsers.contains(&p.id) {
            continue;
        }
        if let Some(c) = at_first.get(&p.id) {
            items.push(PostItem::HatOpen {
                depth: p.depth.min(MAX_DEPTH),
                post_html: c.html.clone().unwrap_or_default(),
                by: m.with(
                    "ts-thread-collapsed-by",
                    &[c.author.as_deref().unwrap_or("?")],
                ),
            });
            open_until.push(c.collapse.map_or(p.id, |(_, last)| last));
        }
        let parent = p
            .in_reply_to
            .and_then(|r| by_id.get(&r))
            .and_then(|q| q.author.as_deref());
        items.push(PostItem::Post {
            post: post_model(m, p, t, parent, reply(p)),
        });
        while open_until.last() == Some(&p.id) {
            open_until.pop();
            items.push(PostItem::HatClose);
        }
    }
    for _ in open_until {
        items.push(PostItem::HatClose);
    }
    items
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
    m.get(if markup == "wikitext" {
        "ts-thread-markup-wikitext"
    } else {
        "ts-thread-markup-markdown"
    })
}

fn text_area(id: &str, name: &str, label: &str, value: &str, rows: u8) -> TextArea {
    TextArea {
        id: id.to_string(),
        name: name.to_string(),
        label: label.to_string(),
        value: format!("\n{value}"),
        rows,
    }
}

fn submit(label: &str) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action: ButtonAction::Progressive,
        weight: Weight::Primary,
    }
    .model()
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
) -> ReplyForm {
    let label = match reply_to {
        Some(p) => m.with("ts-thread-reply-to", &[p.author.as_deref().unwrap_or("?")]),
        None => m.get("ts-thread-reply-thread"),
    };
    ReplyForm {
        action: format!(
            "/w/index.php?title=Thread:{}&action=reply",
            title_url(&t.title)
        ),
        token: token.to_string(),
        reply_to: reply_to.map(|p| p.id),
        text: text_area("ts-reply-text", "wpTextbox1", &label, text, 5),
        status_label: m.get("ts-thread-status"),
        no_change: m.get("ts-thread-status-nochange"),
        statuses: STATUSES
            .iter()
            .map(|st| StatusOption {
                value: (*st).to_string(),
                label: status_label(m, st),
                selected: *st == status,
            })
            .collect(),
        markup: markup_line(m, markup),
        submit: submit(&m.get("ts-thread-reply")),
    }
}

/// The new-thread form of a talk page.
fn topic_form(
    m: &Messages,
    talk: &str,
    markup: &str,
    token: &str,
    subject: &str,
    text: &str,
) -> TopicForm {
    TopicForm {
        action: format!("/w/index.php?title={}&action=addtopic", title_url(talk)),
        token: token.to_string(),
        subject_label: m.get("ts-thread-subject"),
        subject: subject.to_string(),
        text: text_area(
            "ts-topic-text",
            "wpTextbox1",
            &m.get("ts-thread-description"),
            text,
            6,
        ),
        markup: markup_line(m, markup),
        cancel: choice(
            format!("/wiki/{}", title_url(talk)),
            m.get("ts-thread-cancel"),
        ),
        submit: submit(&m.get("ts-talk-add-topic")),
    }
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

fn login_line(cx: &Context, title: &str, key: &str) -> Option<Choice> {
    cx.features.has(crate::features::Feature::Login).then(|| {
        choice(
            format!(
                "/w/index.php?title=Special:UserLogin&returnto={}",
                title_url(title)
            ),
            cx.m.get(key),
        )
    })
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
            body: cx.message(MessageKind::Notice, &m.get("ts-talk-no-subject")),
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
        let pinned = threads
            .iter()
            .filter(|t| t.attached_to(tid).is_some_and(|a| a.pinned.is_some()))
            .count();
        let body = cx.draw(
            "discussion/talk.html",
            TalkPage {
                summary: m.with(
                    "ts-talk-summary",
                    &[&threads.len().to_string(), &pinned.to_string()],
                ),
                add: (can_add && !adding).then(|| {
                    choice(
                        format!("/wiki/{}?action=addtopic#new-topic", title_url(&prefixed)),
                        m.get("ts-talk-add-topic"),
                    )
                }),
                notice: kept
                    .notice
                    .as_deref()
                    .map(|n| message_model(MessageKind::Error, n)),
                form: (adding && can_add)
                    .then(|| topic_form(m, &prefixed, &markup, &tok, kept.subject, kept.text)),
                login: (adding && !can_add && cx.user.is_none())
                    .then(|| login_line(&cx, &prefixed, "ts-thread-login"))
                    .flatten(),
                empty: (threads.is_empty() && !adding)
                    .then(|| message_model(MessageKind::Notice, &m.get("ts-talk-empty"))),
                threads: threads.iter().map(|t| listed_thread(m, t, tid)).collect(),
            },
        );
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
fn listed_thread(m: &Messages, t: &ThreadView, talk_id: u64) -> ListedThread {
    let here = t.attached_to(talk_id);
    let elsewhere: Vec<&triplespace_client::Attached> = std::iter::once(&t.home)
        .chain(&t.listings)
        .filter(|a| a.talk_page_id != Some(talk_id))
        .collect();
    let started_here = t.home.talk_page_id == Some(talk_id);
    let elsewhere = if !started_here && let Some(h) = &t.home.title {
        Some(Elsewhere {
            label: m.get("ts-thread-started-on"),
            pages: vec![page_link(h)],
        })
    } else if !elsewhere.is_empty() {
        Some(Elsewhere {
            label: m.get("ts-thread-also-on"),
            pages: elsewhere
                .iter()
                .filter_map(|a| a.title.as_deref())
                .map(page_link)
                .collect(),
        })
    } else {
        None
    };
    ListedThread {
        id: t.id,
        pinned: here
            .is_some_and(|a| a.pinned.is_some())
            .then(|| m.get("ts-thread-pinned")),
        subject: choice(thread_href(&t.title), t.subject.clone()),
        chip: status_set(t).then(|| chip_model(m, &t.status, t.closed)),
        meta: meta(m, t),
        elsewhere,
        closed: t.closed.then(|| {
            m.with(
                "ts-thread-show",
                &[&status_label(m, &t.status), &t.posts.len().to_string()],
            )
        }),
        posts: posts_model(m, t, &|p| {
            Some(format!("{}?replyto={}#reply", thread_href(&t.title), p.id))
        }),
    }
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
        let title = t.title.clone();
        let body = cx.draw(
            "discussion/thread.html",
            ThreadPage {
                home: t
                    .home
                    .title
                    .as_deref()
                    .map(|h| (m.get("ts-thread-home"), page_link(h))),
                also: {
                    let also: Vec<Choice> = t
                        .listings
                        .iter()
                        .filter_map(|a| a.title.as_deref())
                        .map(page_link)
                        .collect();
                    (!also.is_empty()).then(|| (m.get("ts-thread-also-on"), also))
                },
                chip: chip_model(m, &t.status, t.closed),
                meta: meta(m, &t),
                posts: posts_model(m, &t, &|p| {
                    cx.user
                        .as_ref()
                        .map(|_| format!("{}?replyto={}#reply", thread_href(&title), p.id))
                }),
                notice: kept
                    .notice
                    .as_deref()
                    .map(|n| message_model(MessageKind::Error, n)),
                form: cx.user.as_ref().map(|_| {
                    let reply_to = kept
                        .reply_to
                        .and_then(|r| t.posts.iter().find(|p| p.id == r));
                    reply_form(m, &t, &markup, &tok, reply_to, kept.text, kept.status)
                }),
                login: cx
                    .user
                    .is_none()
                    .then(|| login_line(&cx, &format!("Thread:{}", t.title), "ts-thread-login"))
                    .flatten(),
            },
        );
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
        let items = posts_model(&m, &t, &|_| None);
        let kinds: Vec<String> = items
            .iter()
            .map(|i| match i {
                PostItem::HatOpen { post_html, .. } => format!("hat({post_html})"),
                PostItem::Post { post } => format!("post {}", post.id),
                PostItem::HatClose => "close".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "post 1",
                "hat(<p>Off topic</p>)",
                "post 2",
                "post 3",
                "close"
            ],
            "the hat wraps its range at the place of its first post; the collapsing post is its summary"
        );
        let PostItem::Post { post } = &items[0] else {
            unreachable!()
        };
        let author = post.signature.author.as_ref().expect("an author");
        assert_eq!(author.user.label, "Hollis");
        assert_eq!(
            post.signature.time.as_ref().map(|t| t.label.as_str()),
            Some("10:01, 21 October 2026 (UTC)")
        );
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
