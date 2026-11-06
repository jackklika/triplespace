//! Document pages (0008; 19 §3.1–3.3): a page in a page namespace, read, edited and its
//! history listed, at MediaWiki's addresses: `/wiki/{title}`, `?oldid=`, `?redirect=no`,
//! `?action=edit`, `?action=history` and `?action=render`, and the edit form posted to
//! `index.php?title={title}&action=submit`.
//!
//! - **Read** is `action=parse` of the latest revision, redirects followed with
//!   "(Redirected from …)" under the title, or of one revision with a note that it is
//!   an old one. The HTML is the API's, sanitized there (0008 A39); the site adds the
//!   categories at the foot. A title with no page says so, with **Create this page** for
//!   a signed-in viewer; the main page with no page of its own is the generated one
//!   ([`crate::home`]). `?tstemplates=1` asks the API to mark each block a template drew
//!   with `data-ts-template` and `data-ts-template-at`, for template debugging (0042 §4).
//! - **About this page** (19 §3.1) is a panel under the Read tab: when and by whom the
//!   page was created, when it last changed, its size, content model, protection and
//!   categories, from one `prop=info|revisions` request of its own; a panel that cannot
//!   be fetched is left out, not an error. The counts of 19 §3.1 that need projected
//!   counters (revisions by origin, backlinks, page statements, scopes) are not shown yet.
//! - **Tabs** (19 §1.3): Read, Edit, History, **Links here** (`Special:WhatLinksHere`) and
//!   **Discussion**, the talk page, which lists its threads or offers to start one.
//! - **Edit** is a form of the source, a summary and the minor flag, posted with the
//!   viewer's CSRF token and the revision it started from, which is the edit's base
//!   (18 §2.3). It edits any text model the API edits directly (0041 §5), not only
//!   wikitext. A page being created, in a namespace that allows more than one such model,
//!   has a **Format** selector (`model`, as MediaWiki's `?action=edit&model=` reads it;
//!   0041 §10), sent as `contentmodel`. **Show preview** renders the text through
//!   `action=parse` with the pre-save transform. A save that conflicts keeps the viewer's
//!   text, shows the current one under it, and takes the current revision as the new
//!   base. `index.php?action=edit` without a title is the **New page** form, which asks
//!   for one.
//! - **History** is the page's rows of the activity feed (`target=page:{id}`), drawn as an
//!   entity's history is ([`crate::entity::history`]).
//!
//! - **Diffs** are `?diff=` and `?oldid=` as MediaWiki reads them, from `action=compare`:
//!   both revisions' headers with the way to the next diff along, the line diff, and the
//!   newer revision rendered below it unless `diffonly=1`. History and recent-changes rows
//!   link to them.
//!
//! Not yet (19 §1.4, §3.1–3.2): the Page data tab, which waits on the statement UI for
//! pages (0038 §7); changing an existing page's model (`action=changecontentmodel`); the
//! side-by-side live preview in WebAssembly, CodeMirror, link autocomplete and **Insert
//! template…** (SCA-167). Not for milestone 1: line-marked conflicts, where the current
//! text shown under the form stays the way to merge.

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::Namespaces;
use serde::Serialize;
use triplespace_client::{
    About, CacheInfo, ClientError, DiffAsk, DiffSide, EditForm, Incoming, Lookup, PageDiff, Parsed,
    Saved, Source,
};

use crate::Site;
use crate::codex::{
    Action as ButtonAction, Button, ButtonModel, Field, FieldModel, Message, MessageKind,
    MessageModel, Weight,
};
use crate::diffview::{self, View};
use crate::entity::history;
use crate::entity::history::{Choice, Switch};
use crate::entity::model::Restricted;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, ModerationTabs, Params};
use crate::special::changes::{clock, date};
use crate::special::controls::{ControlModel, FieldRow, OptionModel};
use crate::{home, special};

/// What the request asks of the title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ask {
    Read,
    Edit,
    History,
    Render,
    Diff,
}

impl Ask {
    fn of(params: &Params) -> Self {
        if params.map.contains_key("diff") && params.map.get("action").is_none_or(|a| a == "view") {
            return Self::Diff;
        }
        match params.map.get("action").map(String::as_str) {
            Some("edit" | "submit") => Self::Edit,
            Some("history") => Self::History,
            Some("render") => Self::Render,
            _ => Self::Read,
        }
    }
}

/// The tab a page is shown under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabName {
    Read,
    Edit,
    History,
}

/// A page's tabs (19 §1.3): Read, Edit (Create for a free title), History, Links here
/// (`Special:WhatLinksHere`, which a free title has too: its wanted links) and Discussion,
/// the talk page, for a page that exists. `sitename` is the site's name for `Project:`.
fn tabs(m: &Messages, sitename: &str, title: &str, exists: bool, current: TabName) -> Vec<Tab> {
    let base = format!("/wiki/{}", title_url(title));
    let tab = |name: TabName, key: &str, href: String| Tab {
        label: m.get(key),
        href,
        current: name == current,
    };
    let mut v = vec![
        tab(TabName::Read, "ts-tab-read", base.clone()),
        tab(
            TabName::Edit,
            if exists {
                "ts-tab-edit"
            } else {
                "ts-tab-create"
            },
            format!("{base}?action=edit"),
        ),
    ];
    if exists {
        v.push(tab(
            TabName::History,
            "ts-tab-history",
            format!("{base}?action=history"),
        ));
    }
    v.push(links_here_tab(m, title));
    if exists && let Some(talk) = talk_tab(m, sitename, title) {
        v.push(talk);
    }
    v
}

/// The Links here tab of a page or an entity (19 §1.3): `Special:WhatLinksHere/{title}`.
pub(crate) fn links_here_tab(m: &Messages, title: &str) -> Tab {
    Tab {
        label: m.get("ts-tab-links-here"),
        href: format!("/wiki/Special:WhatLinksHere/{}", title_url(title)),
        current: false,
    }
}

/// The Discussion tab of a page or an entity (19 §1.3; 0019 §2): its talk page, which
/// lists its threads or offers to start one. `None` where the namespace has no talk pages.
pub(crate) fn talk_tab(m: &Messages, sitename: &str, title: &str) -> Option<Tab> {
    let talk = crate::special::redirects::talk_of(&pages::canonical(title, sitename))?;
    Some(Tab {
        label: m.get("ts-tab-discussion"),
        href: format!("/wiki/{}", title_url(&talk)),
        current: false,
    })
}

/// The overflow menu (19 §1.3): **Protect…** for a holder of `protect`, a free title
/// included, which may be create-protected; **Delete…** for a holder of `delete`, on a page
/// that exists.
fn actions(
    m: &Messages,
    title: &str,
    exists: bool,
    offered: ModerationTabs,
) -> Vec<(String, String)> {
    let base = format!("/wiki/{}", title_url(title));
    let mut v = Vec::new();
    if offered.protect {
        v.push((m.get("ts-action-protect"), format!("{base}?action=protect")));
    }
    if offered.r#move && exists {
        v.push((
            m.get("ts-action-move"),
            format!("/wiki/Special:MovePage/{}", title_url(title)),
        ));
    }
    if offered.delete && exists {
        v.push((m.get("ts-action-delete"), format!("{base}?action=delete")));
    }
    v
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what the skin's `document/*.html` templates draw.
// ---------------------------------------------------------------------------------------

/// A page's content, as `action=render` and the Read tab show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Content {
    /// The rendered page, HTML: the API's, sanitized there (0008 A39).
    pub content_html: String,
    /// The categories at the foot, when the page is in any that are shown.
    pub categories: Option<Categories>,
}

/// The categories at the foot of a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Categories {
    /// "Categories:".
    pub label: String,
    /// Each category's page.
    pub items: Vec<Choice>,
}

/// What goes under the title: where a redirect was followed from, or that the revision
/// is an old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Subtitle {
    /// "(Redirected from …)", the title linked.
    Redirected {
        /// The text before the link.
        before: String,
        /// The title it was followed from.
        from: Choice,
        /// The text after the link.
        after: String,
    },
    /// "This is an old revision of this page.", with the current one linked.
    Old {
        /// The text.
        text: String,
        /// The current revision.
        current: Choice,
    },
}

/// The identity line of a page: its kind, and the Restricted pill where its visibility
/// is not public.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    /// "page".
    pub kind: String,
    /// The pill.
    pub restricted: Option<Restricted>,
}

/// One line of the **About this page** panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    /// What it is.
    pub label: String,
    /// Its value as text.
    pub value: String,
    /// A link after the value: the creator's user page, the latest diff.
    pub link: Option<Choice>,
}

/// The **About this page** panel (19 §3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AboutPanel {
    /// "About this page".
    pub heading: String,
    /// The facts.
    pub facts: Vec<Fact>,
    /// **Page information** and the other pages about it.
    pub links: Vec<Choice>,
}

/// The Read tab: the content and the panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ReadCtx {
    content: Content,
    about: Option<AboutPanel>,
}

/// A page a title has none of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Missing {
    /// The notice.
    pub notice: MessageModel,
    /// **Create this page**, for a signed-in viewer.
    pub create: Option<Choice>,
}

/// A page asking the viewer to log in first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginNeeded {
    /// The notice.
    pub notice: MessageModel,
    /// The login page, where the site has one.
    pub login: Option<Choice>,
}

/// The edit form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditFormModel {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The base revision.
    pub base: Option<u64>,
    /// The base revision's time.
    pub base_time: Option<String>,
    /// Whether the page is being created.
    pub create: bool,
    /// The content model of the page being created: **Format**, where the namespace
    /// offers more than one (0041 §10).
    pub format: Option<FieldRow>,
    /// The text area's label.
    pub text_label: String,
    /// The text, with the leading newline the textarea's parser eats.
    pub text: String,
    /// The summary field.
    pub summary: FieldModel,
    /// Whether the minor flag is set.
    pub minor: bool,
    /// The minor flag's label.
    pub minor_label: String,
    /// **Save**.
    pub save: ButtonModel,
    /// **Show preview**.
    pub preview: ButtonModel,
    /// **Cancel**.
    pub cancel: Choice,
}

/// The edit page: notices, a preview, the form and the current text of a conflict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditPage {
    /// The notices.
    pub notices: Vec<MessageModel>,
    /// The preview.
    pub preview: Option<Preview>,
    /// The form.
    pub form: EditFormModel,
    /// The current text of a conflict.
    pub current: Option<Current>,
}

/// A preview above the form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Preview {
    /// The heading.
    pub heading: String,
    /// The note that it is only a preview.
    pub note: MessageModel,
    /// The rendered text.
    pub content: Content,
}

/// The current text of a page the viewer's edit conflicted with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Current {
    /// The heading, and the text area's accessible name.
    pub heading: String,
    /// The text, with the leading newline the textarea's parser eats.
    pub text: String,
}

/// The New page form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewPage {
    /// The introduction.
    pub intro: String,
    /// The title field.
    pub title: FieldModel,
    /// **Go**.
    pub go: ButtonModel,
}

/// One side of a page diff's header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Side {
    /// The revision, linked, as "Revision as of …".
    pub revision: Choice,
    /// Who, linked; `None` where hidden.
    pub user: Option<Choice>,
    /// What the page says of a hidden user.
    pub hidden_user: String,
    /// The summary; `None` where hidden; empty where there is none.
    pub comment: Option<String>,
    /// What the page says of a hidden summary.
    pub hidden_comment: String,
    /// The way to the next diff along.
    pub nav: Option<Choice>,
}

/// The diff table: both sides' headers, and the API's rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffTable {
    /// The table's accessible name.
    pub label: String,
    /// The old side; `None` where the new side created the page.
    pub from: Option<Side>,
    /// What the old side says of a creation.
    pub creation: String,
    /// The new side.
    pub to: Side,
    /// MediaWiki's table rows, HTML, built by `scatter_pages::diff` with every text in
    /// them escaped; empty where the two texts are equal.
    pub body_html: String,
    /// What the table says when they are equal.
    pub none: String,
    /// The same diff in MediaWiki's inline layout, HTML; empty where there is none.
    pub inline_html: String,
    /// Side by side: both revisions rendered, the old one `None` for a creation; drawn
    /// in place of the rows.
    pub columns: Option<(Option<Content>, Content)>,
}

/// `?diff=`: the table, and the newer revision rendered below.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffPage {
    /// The view switch: Source, Text only, Side by side (0010 §6).
    pub views: Switch,
    /// "More context" and "Show every line", in a line diff with changes.
    pub context: Vec<Choice>,
    /// The diff as a wiki link to copy.
    pub permalink: Permalink,
    /// The table.
    pub table: DiffTable,
    /// The newer side's heading and rendering, unless `diffonly=1`.
    pub below: Option<(String, Content)>,
}

/// The copy-ready link to a diff: `[[Special:Diff/…]]` in a read-only field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Permalink {
    /// The field's label.
    pub label: String,
    /// The link.
    pub text: String,
}

/// The context of a template drawn from one model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct ContentCtx {
    content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct SubtitleCtx {
    subtitle: Subtitle,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn message_model(kind: MessageKind, text: &str) -> MessageModel {
    Message::block(kind, text).model()
}

fn content_model(m: &Messages, parsed: &Parsed) -> Content {
    let shown: Vec<_> = parsed.categories.iter().filter(|c| !c.hidden).collect();
    Content {
        content_html: parsed.html.clone(),
        categories: (!shown.is_empty()).then(|| Categories {
            label: m.get("ts-page-categories"),
            items: shown
                .iter()
                .map(|c| {
                    choice(
                        format!("/wiki/Category:{}", title_url(&c.name)),
                        c.name.clone(),
                    )
                })
                .collect(),
        }),
    }
}

fn subtitle_model(m: &Messages, parsed: &Parsed, oldid: Option<u64>) -> Option<Subtitle> {
    if let Some(from) = &parsed.redirected_from {
        let text = m.with("ts-page-redirected-from", &["\u{1}"]);
        let (before, after) = text.split_once('\u{1}').unwrap_or((&text, ""));
        return Some(Subtitle::Redirected {
            before: before.to_string(),
            from: choice(
                format!("/wiki/{}?redirect=no", title_url(from)),
                from.clone(),
            ),
            after: after.to_string(),
        });
    }
    oldid.map(|_| Subtitle::Old {
        text: m.get("ts-page-old-revision"),
        current: choice(
            format!("/wiki/{}", title_url(&parsed.title)),
            m.get("ts-page-current-revision"),
        ),
    })
}

/// What the Read tab draws of the content, as the skin draws it.
fn content_html(cx: &Context, parsed: &Parsed) -> String {
    cx.draw(
        "document/content.html",
        ContentCtx {
            content: content_model(&cx.m, parsed),
        },
    )
}

/// The Read tab, or `action=render`.
async fn read(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    params: &Params,
    render: bool,
) -> Response {
    let oldid = params.map.get("oldid").and_then(|o| o.parse::<u64>().ok());
    let follow = params.map.get("redirect").is_none_or(|r| r != "no");
    // `tstemplates=1`: which template drew each block, for template debugging; a page
    // asks for it, so the cached public render never carries it (0042 §4).
    let templates = params.map.get("tstemplates").is_some_and(|v| v == "1");
    let parsed = match site
        .client()
        .parse_page_marked(incoming, &title, oldid, follow, templates)
        .await
    {
        Ok(p) => p,
        Err(ClientError::Api { code, .. }) => {
            return match code.as_str() {
                "missingtitle" => {
                    // A deleted page shows its deletion (19 §6.3) before it is a 404.
                    if !render
                        && let Some(r) = crate::moderation::deleted(
                            site, &mut cx, headers, incoming, &title, None,
                        )
                        .await
                    {
                        return r;
                    }
                    missing(&cx, headers, title, render)
                }
                "nosuchrevid" | "nosuchpageid" | "permissiondenied" if oldid.is_some() => {
                    no_revision(&cx, headers, title, oldid.unwrap_or(0))
                }
                // A title outside the page namespaces: the site's own `404`, which also
                // sends `Main Page` on to the main page.
                _ if render => pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new),
                _ => pages::no_page(&cx, headers, title),
            };
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(parsed.cache);
    let parsed = parsed.value;
    if render {
        return pages::fragment(&cx, headers, StatusCode::OK, || content_html(&cx, &parsed));
    }
    let watch = crate::watch::control(site, &cx, incoming, &parsed.title).await;
    // The panel is its own request (19 §3.1); a page reads without it.
    let about = match site.client().page_about(incoming, &parsed.title).await {
        Ok(f) => {
            cx.inputs.push(f.cache);
            match f.value {
                Lookup::Found(a) => Some(a),
                _ => None,
            }
        }
        Err(_) => None,
    };
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: parsed.title.clone(),
        returnto: Some(parsed.title.clone()),
        watch,
        identity_html: parsed.visibility.as_ref().map(|v| {
            cx.draw(
                "document/identity.html",
                Identity {
                    kind: m.get("ts-kind-page"),
                    restricted: Some(Restricted {
                        tip: crate::frame::restricted_tip(m, &v.groups, &v.sets),
                        text: m.get("ts-restricted"),
                    }),
                },
            )
        }),
        subtitle_html: subtitle_model(m, &parsed, oldid)
            .map(|subtitle| cx.draw("document/subtitle.html", SubtitleCtx { subtitle })),
        tabs: tabs(m, &cx.site.sitename, &parsed.title, true, TabName::Read),
        actions: actions(m, &parsed.title, true, cx.moderation_tabs()),
        body: cx.draw(
            "document/read.html",
            ReadCtx {
                content: content_model(m, &parsed),
                about: about.as_ref().map(|a| about_model(m, a, &parsed)),
            },
        ),
        ..Page::default()
    })
}

/// The **About this page** panel's model (19 §3.1).
fn about_model(m: &Messages, a: &About, parsed: &Parsed) -> AboutPanel {
    let when = |iso: &str| format!("{}, {}", clock(iso), date(m, iso));
    let mut facts = Vec::new();
    if let Some(created) = &a.created {
        facts.push(Fact {
            label: m.get("ts-about-created"),
            value: when(created),
            link: a
                .creator
                .as_ref()
                .map(|u| choice(format!("/wiki/User:{}", title_url(u)), u.clone())),
        });
    }
    if let Some(touched) = &a.touched {
        facts.push(Fact {
            label: m.get("ts-about-last-edit"),
            value: when(touched),
            link: (a.lastrevid > 0).then(|| {
                choice(
                    format!(
                        "/w/index.php?title={}&diff={}",
                        title_url(&a.title),
                        a.lastrevid
                    ),
                    m.get("ts-about-diff"),
                )
            }),
        });
    }
    facts.push(Fact {
        label: m.get("ts-about-size"),
        value: m.with("ts-about-bytes", &[&a.length.to_string()]),
        link: None,
    });
    facts.push(Fact {
        label: m.get("ts-about-model"),
        value: model_label(m, &a.model),
        link: None,
    });
    let protection: Vec<String> = a
        .protection
        .iter()
        .filter(|r| r.source.is_none())
        .map(|r| format!("{} ({})", restriction_label(m, &r.kind), r.level))
        .collect();
    facts.push(Fact {
        label: m.get("ts-about-protection"),
        value: if protection.is_empty() {
            m.get("ts-about-not-protected")
        } else {
            protection.join(", ")
        },
        link: None,
    });
    facts.push(Fact {
        label: m.get("ts-about-categories"),
        value: parsed.categories.len().to_string(),
        link: None,
    });
    AboutPanel {
        heading: m.get("ts-about-heading"),
        facts,
        links: vec![
            choice(
                format!("/wiki/{}?action=history", title_url(&a.title)),
                m.get("ts-tab-history"),
            ),
            {
                let t = links_here_tab(m, &a.title);
                choice(t.href, t.label)
            },
        ],
    }
}

/// A protected action's name, as the Protect form calls it.
fn restriction_label(m: &Messages, kind: &str) -> String {
    let key = format!("ts-protect-action-{kind}");
    if m.has(&key) {
        m.get(&key)
    } else {
        kind.to_string()
    }
}

/// A content model's name for readers: `Markdown`, `Wikitext`; its ID where the site has
/// no name for it.
fn model_label(m: &Messages, model: &str) -> String {
    let key = format!("ts-contentmodel-{model}");
    if m.has(&key) {
        m.get(&key)
    } else {
        model.to_string()
    }
}

/// Whether the edit form edits a model: a text model the API edits directly (0041 §5).
fn form_edits(model: &str) -> bool {
    scatter_pages::ContentModels::embedded()
        .get(model)
        .is_some_and(|c| {
            c.direct_editing && c.implemented && c.source == Some(scatter_pages::ModelSource::Text)
        })
}

/// The models the Format selector offers for a new page at a title, the namespace's
/// default first: those of its namespace's models the form edits, or none where there is
/// only one (19 §3.2).
fn formats(sitename: &str, title: &str) -> Vec<String> {
    let t = pages::canonical(title, sitename);
    let namespaces = Namespaces::embedded();
    let ns = t
        .split_once(':')
        .and_then(|(p, _)| {
            namespaces.iter().find(|n| {
                !n.canonical.is_empty()
                    && (n.canonical.eq_ignore_ascii_case(p.trim())
                        || n.name
                            .as_deref()
                            .is_some_and(|l| l.eq_ignore_ascii_case(p.trim())))
            })
        })
        .or_else(|| namespaces.get(0));
    let Some(ns) = ns else {
        return Vec::new();
    };
    let mut v: Vec<String> = ns
        .content_models
        .iter()
        .filter(|m| form_edits(m))
        .cloned()
        .collect();
    if let Some(d) = &ns.default_model
        && let Some(i) = v.iter().position(|m| m == d)
    {
        let first = v.remove(i);
        v.insert(0, first);
    }
    if v.len() < 2 { Vec::new() } else { v }
}

/// A title with no page: the generated main page for the main page's title, the `404`
/// of a missing page otherwise.
fn missing(cx: &Context, headers: &HeaderMap, title: String, render: bool) -> Response {
    if pages::is_mainpage(&title, &cx.site) {
        if render {
            return pages::fragment(cx, headers, StatusCode::OK, || home::content(cx));
        }
        return pages::respond(cx, headers, StatusCode::OK, |_| home::page(cx));
    }
    if cx.site.mainpage != "Main Page" && title.eq_ignore_ascii_case("Main Page") {
        return pages::redirect_to(cx, &cx.site.mainpage.clone());
    }
    if render {
        return pages::fragment(cx, headers, StatusCode::NOT_FOUND, String::new);
    }
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| {
        let body = cx.draw(
            "document/missing.html",
            Missing {
                notice: message_model(MessageKind::Notice, &m.get("ts-page-missing")),
                create: cx.user.as_ref().map(|_| {
                    choice(
                        format!("/wiki/{}?action=edit", title_url(&title)),
                        m.get("ts-page-missing-create"),
                    )
                }),
            },
        );
        Page {
            returnto: Some(title.clone()),
            tabs: tabs(m, &cx.site.sitename, &title, false, TabName::Read),
            actions: actions(m, &title, false, cx.moderation_tabs()),
            title,
            body,
            ..Page::default()
        }
    })
}

/// A revision that does not exist, or is not the viewer's to see.
fn no_revision(cx: &Context, headers: &HeaderMap, title: String, oldid: u64) -> Response {
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
        body: cx.message(
            MessageKind::Error,
            &m.with("ts-page-no-revision", &[&oldid.to_string()]),
        ),
        returnto: Some(title.clone()),
        title,
        ..Page::default()
    })
}

/// A title that cannot name a page.
fn bad_title(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    pages::respond(cx, headers, StatusCode::BAD_REQUEST, |m| Page {
        title: m.get("ts-page-bad-title-heading"),
        body: cx.message(MessageKind::Error, &m.with("ts-page-bad-title", &[title])),
        ..Page::default()
    })
}

// ---- editing ---------------------------------------------------------------------------

/// The edit form's state.
#[derive(Debug, Clone, Default)]
struct Draft {
    /// The title, as the API writes it.
    title: String,
    /// Whether the page existed when the edit began.
    exists: bool,
    /// The text.
    text: String,
    /// The summary.
    summary: String,
    /// A minor edit.
    minor: bool,
    /// The base revision.
    base: Option<u64>,
    /// The base revision's time.
    base_time: Option<String>,
    /// The content model; for a new page, the one the Format selector chose.
    model: String,
    /// What the Format selector offers a new page; empty where it is not shown.
    formats: Vec<String>,
}

impl Draft {
    fn of_source(s: &Source) -> Self {
        Self {
            title: s.title.clone(),
            exists: true,
            text: s.text.clone(),
            base: Some(s.revid),
            base_time: Some(s.timestamp.clone()),
            model: s.model.clone(),
            ..Self::default()
        }
    }

    /// A page to create at a title: the model asked for (`model=`) where the selector
    /// offers it, else the namespace's default.
    fn new_at(sitename: &str, title: String, asked: Option<&str>) -> Self {
        let formats = formats(sitename, &title);
        let model = asked
            .filter(|a| formats.iter().any(|f| f == a))
            .map(str::to_string)
            .or_else(|| formats.first().cloned())
            .unwrap_or_else(|| "wikitext".into());
        Self {
            title,
            model,
            formats,
            ..Self::default()
        }
    }

    fn of_form(sitename: &str, title: &str, form: &Params) -> Self {
        let get = |k: &str| form.map.get(k).cloned().unwrap_or_default();
        // A textarea's value comes back with CRLF line ends; the page's are LF.
        let text = get("wpTextbox1").replace("\r\n", "\n");
        let exists = !form.map.contains_key("wpCreateonly");
        let new = (!exists).then(|| {
            Self::new_at(
                sitename,
                title.to_string(),
                form.map.get("model").map(String::as_str),
            )
        });
        Self {
            title: title.to_string(),
            exists,
            model: new
                .as_ref()
                .map_or_else(|| "wikitext".into(), |n| n.model.clone()),
            formats: new.map(|n| n.formats).unwrap_or_default(),
            text,
            summary: get("wpSummary"),
            minor: form.map.contains_key("wpMinoredit"),
            base: form.map.get("baseRevId").and_then(|b| b.parse().ok()),
            base_time: form
                .map
                .get("wpEdittime")
                .filter(|t| !t.is_empty())
                .cloned(),
        }
    }
}

/// The edit form, with a preview, notices and the current text of a conflict.
fn edit_form_model(m: &Messages, token: &str, d: &Draft) -> EditFormModel {
    EditFormModel {
        action: format!("/w/index.php?title={}&action=submit", title_url(&d.title)),
        token: token.to_string(),
        base: d.base,
        base_time: d.base_time.clone(),
        create: !d.exists,
        format: (!d.exists && !d.formats.is_empty()).then(|| FieldRow {
            id: "ts-pageedit-model".into(),
            label: m.get("ts-pageedit-model"),
            class: None,
            control: ControlModel::Select {
                id: Some("ts-pageedit-model".into()),
                name: "model".into(),
                options: d
                    .formats
                    .iter()
                    .map(|f| OptionModel {
                        value: f.clone(),
                        label: model_label(m, f),
                        selected: *f == d.model,
                    })
                    .collect(),
                described_by: None,
            },
            help: None,
            help_id: None,
        }),
        text_label: m.get("ts-pageedit-text"),
        // A leading newline would be eaten by the parser of the textarea's content.
        text: format!("\n{}", d.text),
        summary: Field {
            id: "wpSummary",
            name: "wpSummary",
            kind: "text",
            value: &d.summary,
            label: &m.get("ts-pageedit-summary"),
            description: None,
            help: None,
            required: false,
            autocomplete: Some("off"),
        }
        .model(),
        minor: d.minor,
        minor_label: m.get("ts-pageedit-minor"),
        save: Button {
            label: &m.get(if d.exists {
                "ts-pageedit-save"
            } else {
                "ts-pageedit-save-new"
            }),
            kind: "submit",
            action: ButtonAction::Progressive,
            weight: Weight::Primary,
        }
        .model(),
        preview: Button::submit(&m.get("ts-pageedit-preview")).model(),
        cancel: choice(
            format!("/wiki/{}", title_url(&d.title)),
            m.get("ts-pageedit-cancel"),
        ),
    }
}

/// The edit page.
#[allow(clippy::too_many_arguments)]
fn edit_page(
    cx: &Context,
    headers: &HeaderMap,
    token: &str,
    d: &Draft,
    notices: &[(MessageKind, String)],
    preview: Option<&Parsed>,
    current: Option<&str>,
) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let body = cx.draw(
            "document/edit.html",
            EditPage {
                notices: notices.iter().map(|(k, t)| message_model(*k, t)).collect(),
                preview: preview.map(|p| Preview {
                    heading: m.get("ts-pageedit-preview-heading"),
                    note: message_model(MessageKind::Warning, &m.get("ts-pageedit-preview-note")),
                    content: content_model(m, p),
                }),
                form: edit_form_model(m, token, d),
                current: current.map(|c| Current {
                    heading: m.get("ts-pageedit-conflict-current"),
                    text: format!("\n{c}"),
                }),
            },
        );
        Page {
            title: m.with(
                if d.exists {
                    "ts-pageedit-title"
                } else {
                    "ts-pageedit-create-title"
                },
                &[&d.title],
            ),
            doc_title: None,
            returnto: Some(d.title.clone()),
            tabs: tabs(m, &cx.site.sitename, &d.title, d.exists, TabName::Edit),
            actions: actions(m, &d.title, d.exists, cx.moderation_tabs()),
            body,
            ..Page::default()
        }
    })
}

/// A page asking a viewer who is not signed in to log in first.
fn login_needed(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| {
        let body = cx.draw(
            "login.html",
            LoginNeeded {
                notice: message_model(MessageKind::Notice, &m.get("ts-pageedit-login")),
                login: cx.features.has(crate::features::Feature::Login).then(|| {
                    choice(
                        format!(
                            "/w/index.php?title=Special:UserLogin&returnto={}",
                            title_url(title)
                        ),
                        m.get("ts-login-title"),
                    )
                }),
            },
        );
        Page {
            title: title.to_string(),
            returnto: Some(title.to_string()),
            body,
            ..Page::default()
        }
    })
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

/// The New page form: a title to create.
fn new_page(cx: &Context, headers: &HeaderMap) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-newpage-title"),
        body: cx.draw(
            "document/new.html",
            NewPage {
                intro: m.get("ts-newpage-intro"),
                title: Field {
                    id: "ts-newpage-title",
                    name: "title",
                    kind: "text",
                    value: "",
                    label: &m.get("ts-newpage-label"),
                    description: None,
                    help: None,
                    required: true,
                    autocomplete: Some("off"),
                }
                .model(),
                go: Button {
                    label: &m.get("ts-newpage-go"),
                    kind: "submit",
                    action: ButtonAction::Progressive,
                    weight: Weight::Primary,
                }
                .model(),
            },
        ),
        ..Page::default()
    })
}

/// The Edit tab.
async fn edit(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    asked_model: Option<&str>,
) -> Response {
    if cx.user.is_none() {
        return login_needed(&cx, headers, &title);
    }
    let source = match site.client().page_source(incoming, &title).await {
        Ok(s) => s.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let draft = match source {
        Lookup::Found(s) => {
            if !form_edits(&s.model) {
                return unsupported(&cx, headers, &s);
            }
            Draft::of_source(&s)
        }
        Lookup::Missing { title, .. } => Draft::new_at(&cx.site.sitename, title, asked_model),
        Lookup::Invalid => return bad_title(&cx, headers, &title),
    };
    let (token, cookies) = match token(site, &mut cx, incoming).await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    pages::with_cookies(
        edit_page(&cx, headers, &token, &draft, &[], None, None),
        &cookies,
    )
}

/// A page whose content model the form does not edit.
fn unsupported(cx: &Context, headers: &HeaderMap, s: &Source) -> Response {
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: s.title.clone(),
        returnto: Some(s.title.clone()),
        tabs: tabs(m, &cx.site.sitename, &s.title, true, TabName::Edit),
        actions: actions(m, &s.title, true, cx.moderation_tabs()),
        body: cx.message(
            MessageKind::Notice,
            &m.with("ts-pageedit-unsupported", &[&s.model]),
        ),
        ..Page::default()
    })
}

/// `POST …&action=submit`: a preview, or a save.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    mut cx: Context,
    title: &str,
    form: &Params,
) -> Response {
    if cx.user.is_none() {
        return login_needed(&cx, headers, title);
    }
    let mut draft = Draft::of_form(&cx.site.sitename, title, form);
    let posted = form.map.get("wpEditToken").cloned().unwrap_or_default();
    // The page is the viewer's alone: it holds their token and their text.
    cx.inputs.push(CacheInfo::default());
    if form.map.contains_key("wpPreview") {
        return match site
            .client()
            .preview(incoming, title, &draft.text, &draft.model)
            .await
        {
            Ok(p) => edit_page(&cx, headers, &posted, &draft, &[], Some(&p), None),
            Err(ClientError::Api { info, .. }) => {
                let notices = [(MessageKind::Error, info)];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
            Err(e) => pages::api_failed(&cx, headers, &e),
        };
    }
    let saved = site
        .client()
        .save_page(
            incoming,
            &EditForm {
                title,
                text: &draft.text,
                summary: &draft.summary,
                minor: draft.minor,
                baserevid: draft.base,
                basetimestamp: draft.base_time.as_deref(),
                createonly: !draft.exists,
                content_model: (!draft.exists).then_some(draft.model.as_str()),
                token: &posted,
            },
        )
        .await;
    match saved {
        Ok(Saved::Saved { title, .. } | Saved::NoChange { title }) => {
            pages::see_other(&format!("/wiki/{}", title_url(&title)), &[])
        }
        Err(ClientError::Api { code, info }) => match code.as_str() {
            "editconflict" | "articleexists" => {
                // The current text, and the base moved to it: saving again replaces it
                // with what the viewer merged.
                let current = match site.client().page_source(incoming, title).await {
                    Ok(f) => f.value,
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                };
                let current_text = match current {
                    Lookup::Found(s) => {
                        draft.exists = true;
                        draft.base = Some(s.revid);
                        draft.base_time = Some(s.timestamp.clone());
                        draft.title = s.title;
                        Some(s.text)
                    }
                    _ => None,
                };
                let notices = [(MessageKind::Error, cx.m.get("ts-pageedit-conflict"))];
                edit_page(
                    &cx,
                    headers,
                    &posted,
                    &draft,
                    &notices,
                    None,
                    current_text.as_deref(),
                )
            }
            "badtoken" => {
                let (token, cookies) = match token(site, &mut cx, incoming).await {
                    Ok(t) => t,
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                };
                let notices = [(MessageKind::Warning, cx.m.get("ts-new-expired"))];
                pages::with_cookies(
                    edit_page(&cx, headers, &token, &draft, &notices, None, None),
                    &cookies,
                )
            }
            "permissiondenied" => {
                let notices = [(MessageKind::Error, cx.m.get("ts-permission-denied"))];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
            _ => {
                let notices = [(
                    MessageKind::Error,
                    cx.m.with("ts-pageedit-failed", &[&info]),
                )];
                edit_page(&cx, headers, &posted, &draft, &notices, None, None)
            }
        },
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

// ---- history ---------------------------------------------------------------------------

/// The History tab: the page's rows of the activity feed.
async fn page_history(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    params: &Params,
) -> Response {
    let info = match site.client().page_info(incoming, &title).await {
        Ok(i) => i,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(info.cache);
    let info = match info.value {
        Lookup::Found(i) => i,
        Lookup::Missing { .. } => return missing(&cx, headers, title, false),
        Lookup::Invalid => return bad_title(&cx, headers, &title),
    };
    let ask = history::Ask::of(&params.map);
    let mut query = ask.query(cx.m.lang());
    query.target = Some(format!("page:{}", info.page_id));
    let rows = match site.client().activity(incoming, &query).await {
        Ok(r) => r,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(rows.cache);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: info.title.clone(),
        returnto: Some(info.title.clone()),
        tabs: tabs(m, &cx.site.sitename, &info.title, true, TabName::History),
        actions: actions(m, &info.title, true, cx.moderation_tabs()),
        body: history::body(&cx, &info.title, &ask, None, &rows.value),
        ..Page::default()
    })
}

// ---- diffs -------------------------------------------------------------------------------

/// What `?diff=` and `?oldid=` ask to compare, as MediaWiki reads them: `diff=X` alone is
/// X against the revision before it; with `oldid=Y`, Y against X; `diff=prev|next|cur`
/// names X from Y; `diff=cur` or `diff=prev` alone, the latest revision against the one
/// before it. `None` for a value that names nothing.
fn diff_ask(title: &str, params: &Params) -> Option<DiffAsk> {
    let diff = params.map.get("diff").map_or("", String::as_str);
    let oldid = params.map.get("oldid").and_then(|o| o.parse::<u64>().ok());
    let rel = |d: &str| match d {
        "prev" => Some("prev"),
        "next" => Some("next"),
        "cur" | "0" | "" => Some("cur"),
        _ => None,
    };
    match (diff.parse::<u64>().ok().filter(|d| *d > 0), oldid) {
        // MediaWiki compares the older with the newer, whichever order they came in, as a
        // history's compare form may send them (0010 §5.6).
        (Some(d), Some(o)) => Some(DiffAsk::Revisions(o.min(d), o.max(d))),
        (Some(d), None) => Some(DiffAsk::Relative(d, "prev")),
        (None, Some(o)) => rel(diff).map(|r| DiffAsk::Relative(o, r)),
        (None, None) => rel(diff).map(|_| DiffAsk::Latest(title.to_string())),
    }
}

/// When a revision was made, as `15:04, 20 October 2026`.
fn when(m: &Messages, iso: &str) -> String {
    format!(
        "{}, {}",
        special::changes::clock(iso),
        special::changes::date(m, iso)
    )
}

/// One side's header: the revision linked, who, the summary, and the way to the next
/// diff along.
fn side_model(m: &Messages, side: &DiffSide, latest: bool, nav: Option<Choice>) -> Side {
    let key = if latest {
        "ts-pagediff-latest-as-of"
    } else {
        "ts-pagediff-revision-as-of"
    };
    Side {
        revision: choice(
            format!("/wiki/{}?oldid={}", title_url(&side.title), side.revid),
            m.with(key, &[&when(m, &side.timestamp)]),
        ),
        user: side.user.as_ref().map(|u| {
            choice(
                format!("/wiki/Special:Contributions/{}", title_url(u)),
                u.clone(),
            )
        }),
        hidden_user: m.get("ts-changes-hidden-user"),
        comment: side.comment.clone(),
        hidden_comment: m.get("ts-changes-hidden-comment"),
        nav,
    }
}

/// The diff table's model: both sides' headers, and the API's rows.
pub(crate) fn diff_table_model(m: &Messages, d: &PageDiff, latest_revid: Option<u64>) -> DiffTable {
    diff_table_keeping(m, d, latest_revid, "")
}

/// The diff table's model, its links to the next diff along keeping the query `keep`
/// (the view and its context, [`diffview::keep`]).
fn diff_table_keeping(
    m: &Messages,
    d: &PageDiff,
    latest_revid: Option<u64>,
    keep: &str,
) -> DiffTable {
    let title = &d.to.title;
    DiffTable {
        label: m.get("ts-pagediff-label"),
        from: d.from.as_ref().map(|f| {
            side_model(
                m,
                f,
                false,
                d.prev.map(|_| {
                    choice(
                        format!(
                            "/wiki/{}?diff=prev&oldid={}{keep}",
                            title_url(title),
                            f.revid
                        ),
                        m.get("ts-pagediff-older"),
                    )
                }),
            )
        }),
        creation: m.get("ts-pagediff-creation"),
        to: side_model(
            m,
            &d.to,
            latest_revid == Some(d.to.revid),
            d.next.map(|_| {
                choice(
                    format!(
                        "/wiki/{}?diff=next&oldid={}{keep}",
                        title_url(title),
                        d.to.revid
                    ),
                    m.get("ts-pagediff-newer"),
                )
            }),
        ),
        body_html: d.body.clone(),
        none: m.get("ts-pagediff-none"),
        inline_html: d.inline.clone(),
        columns: None,
    }
}

/// The address of this diff in another view or context.
fn diff_href(title: &str, d: &PageDiff, view: View, context: diffview::Context) -> String {
    let mut s = format!("/wiki/{}?diff={}", title_url(title), d.to.revid);
    if let Some(f) = &d.from {
        let _ = write!(s, "&oldid={}", f.revid);
    }
    s.push_str(&diffview::keep(view, context));
    s
}

/// The view switch.
fn views_model(
    m: &Messages,
    title: &str,
    d: &PageDiff,
    view: View,
    context: diffview::Context,
) -> Switch {
    let item = |v: View, key: &str| Choice {
        href: diff_href(title, d, v, context),
        label: m.get(key),
        current: v == view,
    };
    Switch {
        label: m.get("ts-pagediff-views"),
        items: vec![
            item(View::Source, "ts-pagediff-view-source"),
            item(View::Text, "ts-pagediff-view-text"),
            item(View::Rendered, "ts-pagediff-view-rendered"),
        ],
    }
}

/// "More context" and "Show every line", for a line diff that has changes.
fn context_model(
    m: &Messages,
    title: &str,
    d: &PageDiff,
    view: View,
    context: diffview::Context,
    has_rows: bool,
) -> Vec<Choice> {
    if !view.has_lines() || !has_rows {
        return Vec::new();
    }
    let mut out = Vec::new();
    if let Some(more) = context.more() {
        out.push(choice(
            diff_href(title, d, view, more),
            m.get("ts-pagediff-more-context"),
        ));
        if !more.is_all() {
            out.push(choice(
                diff_href(title, d, view, diffview::Context::all()),
                m.get("ts-pagediff-all-context"),
            ));
        }
    }
    out
}

/// The diff page's answer when a revision it needs is missing or hidden from the viewer.
fn diff_missing(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
        body: cx.message(MessageKind::Error, &m.get("ts-pagediff-missing")),
        returnto: Some(title.to_string()),
        title: title.to_string(),
        ..Page::default()
    })
}

/// A revision's rendering: `Ok(None)` when the API refuses it (missing or hidden).
async fn rendering(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    title: &str,
    revid: u64,
) -> Result<Option<Parsed>, ClientError> {
    match site
        .client()
        .parse_page(incoming, title, Some(revid), false)
        .await
    {
        Ok(p) => {
            cx.inputs.push(p.cache);
            Ok(Some(p.value))
        }
        Err(ClientError::Api { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A revision's source: `Ok(None)` when it is missing or hidden.
async fn source_text(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    revid: u64,
) -> Result<Option<String>, ClientError> {
    match site.client().revision_text(incoming, revid).await {
        Ok(t) => {
            cx.inputs.push(t.cache);
            Ok(t.value)
        }
        Err(ClientError::Api { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// `?diff=`: two revisions compared, the newer rendered below unless `diffonly=1`; with
/// `view=` and `context=`, the other views of 0010 §6 ([`diffview`]).
#[allow(clippy::too_many_lines)]
async fn page_diff(
    site: &Site,
    mut cx: Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    title: String,
    params: &Params,
) -> Response {
    let Some(ask) = diff_ask(&title, params) else {
        return pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |m| Page {
            body: cx.message(MessageKind::Error, &m.get("ts-pagediff-bad")),
            returnto: Some(title.clone()),
            title: title.clone(),
            ..Page::default()
        });
    };
    let view = View::of(&params.map);
    let context = diffview::Context::of(&params.map);
    // The API's diff serves the default view; the others draw their own from the sides.
    let own = view != View::Source || context.0.is_some();
    let fetched = if own {
        site.client().page_diff_sides(incoming, &ask).await
    } else {
        site.client().page_diff(incoming, &ask).await
    };
    let diff = match fetched {
        Ok(d) => d,
        Err(ClientError::Api { code, .. }) => {
            return match code.as_str() {
                "missingtitle" => missing(&cx, headers, title, false),
                "nosuchrevid" | "missingcontent" | "permissiondenied" => {
                    diff_missing(&cx, headers, &title)
                }
                _ => pages::no_page(&cx, headers, title),
            };
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(diff.cache);
    let mut diff = diff.value;
    let page_title = diff.to.title.clone();
    // Whether the newer side is the page's latest revision.
    let latest = diff.next.is_none().then_some(diff.to.revid);
    let diffonly = params.map.get("diffonly").is_some_and(|v| v == "1");
    let from_rev = diff.from.as_ref().map(|f| f.revid);
    let to_rev = diff.to.revid;
    let lineno = |m: &Messages, n: usize| m.with("ts-pagediff-lineno", &[&n.to_string()]);
    let mut below: Option<Parsed> = None;
    let mut columns: Option<(Option<Parsed>, Parsed)> = None;
    match view {
        View::Source => {
            if own {
                let old = match from_rev {
                    Some(r) => match source_text(site, &mut cx, incoming, r).await {
                        Ok(Some(t)) => t,
                        Ok(None) => return diff_missing(&cx, headers, &page_title),
                        Err(e) => return pages::api_failed(&cx, headers, &e),
                    },
                    None => String::new(),
                };
                let new = match source_text(site, &mut cx, incoming, to_rev).await {
                    Ok(Some(t)) => t,
                    Ok(None) => return diff_missing(&cx, headers, &page_title),
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                };
                let m = &cx.m;
                (diff.body, diff.inline) = diffview::rows(&old, &new, context, |n| lineno(m, n));
            }
            if !diffonly {
                match rendering(site, &mut cx, incoming, &page_title, to_rev).await {
                    Ok(p) => below = p,
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                }
            }
        }
        View::Text | View::Rendered => {
            let old = match from_rev {
                Some(r) => match rendering(site, &mut cx, incoming, &page_title, r).await {
                    Ok(Some(p)) => Some(p),
                    Ok(None) => return diff_missing(&cx, headers, &page_title),
                    Err(e) => return pages::api_failed(&cx, headers, &e),
                },
                None => None,
            };
            let new = match rendering(site, &mut cx, incoming, &page_title, to_rev).await {
                Ok(Some(p)) => p,
                Ok(None) => return diff_missing(&cx, headers, &page_title),
                Err(e) => return pages::api_failed(&cx, headers, &e),
            };
            if view == View::Text {
                let old_text = old
                    .as_ref()
                    .map(|p| diffview::prose(&p.html))
                    .unwrap_or_default();
                let new_text = diffview::prose(&new.html);
                let m = &cx.m;
                (diff.body, diff.inline) =
                    diffview::rows(&old_text, &new_text, context, |n| lineno(m, n));
                if !diffonly {
                    below = Some(new);
                }
            } else {
                columns = Some((old, new));
            }
        }
    }
    pages::respond(&cx, headers, StatusCode::OK, |m| {
        let keep = diffview::keep(view, context);
        let mut table = diff_table_keeping(m, &diff, latest, &keep);
        table.columns = columns.as_ref().map(|(old, new)| {
            (
                old.as_ref().map(|p| content_model(m, p)),
                content_model(m, new),
            )
        });
        let body = cx.draw(
            "document/diff.html",
            DiffPage {
                views: views_model(m, &page_title, &diff, view, context),
                context: context_model(m, &page_title, &diff, view, context, !diff.body.is_empty()),
                permalink: Permalink {
                    label: m.get("ts-pagediff-permalink"),
                    text: diffview::permalink(from_rev, to_rev),
                },
                table,
                below: below.as_ref().map(|p| {
                    let key = if latest.is_some() {
                        "ts-pagediff-latest-as-of"
                    } else {
                        "ts-pagediff-revision-as-of"
                    };
                    (
                        m.with(key, &[&when(m, &diff.to.timestamp)]),
                        content_model(m, p),
                    )
                }),
            },
        );
        Page {
            doc_title: Some(m.with("ts-pagediff-title", &[&page_title])),
            returnto: Some(page_title.clone()),
            tabs: tabs(m, &cx.site.sitename, &page_title, true, TabName::History),
            actions: actions(m, &page_title, true, cx.moderation_tabs()),
            title: page_title.clone(),
            body,
            ..Page::default()
        }
    })
}

// ---- dispatch --------------------------------------------------------------------------

/// Serves a title that is neither an entity's nor a special page's, where the API serves
/// pages.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    title: String,
    params: &Params,
) -> Response {
    let ask = Ask::of(params);
    if title.is_empty() {
        return if ask == Ask::Edit {
            new_page(&cx, headers)
        } else {
            pages::redirect_to(&cx, &cx.site.mainpage.clone())
        };
    }
    if special::name_of(&title).is_some() {
        return pages::no_page(&cx, headers, title);
    }
    match ask {
        Ask::Read => read(site, cx, headers, incoming, title, params, false).await,
        Ask::Render => read(site, cx, headers, incoming, title, params, true).await,
        Ask::Edit => {
            let model = params.map.get("model").map(String::as_str);
            edit(site, cx, headers, incoming, title, model).await
        }
        Ask::History => page_history(site, cx, headers, incoming, title, params).await,
        Ask::Diff => page_diff(site, cx, headers, incoming, title, params).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::PageCategory;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn diff_addresses_read_as_mediawiki_reads_them() {
        let ask = |pairs: &[(&str, &str)]| diff_ask("Project:Home", &params(pairs));
        assert_eq!(ask(&[("diff", "7")]), Some(DiffAsk::Relative(7, "prev")));
        assert_eq!(
            ask(&[("diff", "9"), ("oldid", "7")]),
            Some(DiffAsk::Revisions(7, 9))
        );
        // 0010 §5.6: a history's compare form may send the newer as `oldid`.
        assert_eq!(
            ask(&[("diff", "7"), ("oldid", "9")]),
            Some(DiffAsk::Revisions(7, 9))
        );
        assert_eq!(
            ask(&[("diff", "prev"), ("oldid", "7")]),
            Some(DiffAsk::Relative(7, "prev"))
        );
        assert_eq!(
            ask(&[("diff", "next"), ("oldid", "7")]),
            Some(DiffAsk::Relative(7, "next"))
        );
        assert_eq!(
            ask(&[("diff", "cur"), ("oldid", "7")]),
            Some(DiffAsk::Relative(7, "cur"))
        );
        assert_eq!(
            ask(&[("diff", "cur")]),
            Some(DiffAsk::Latest("Project:Home".into()))
        );
        assert_eq!(ask(&[("diff", "x")]), None);
        assert_eq!(Ask::of(&params(&[("diff", "7")])), Ask::Diff);
        assert_eq!(
            Ask::of(&params(&[("diff", "7"), ("action", "history")])),
            Ask::History
        );
    }

    #[test]
    fn a_diff_shows_both_sides_and_the_way_along() {
        let m = Messages::for_language("en");
        let side = |revid, user: Option<&str>| DiffSide {
            revid,
            title: "Project:Home".into(),
            timestamp: "2026-10-20T15:04:00Z".into(),
            user: user.map(str::to_string),
            comment: Some("<b>sum</b>".into()),
            size: 3,
        };
        let d = PageDiff {
            from: Some(side(7, Some("Harej"))),
            to: side(9, None),
            body: "<tr><td>row</td></tr>".into(),
            inline: "<div class=\"mw-diff-inline-context\">row</div>".into(),
            prev: Some(5),
            next: None,
        };
        let t = diff_table_model(&m, &d, Some(9));
        let from = t.from.as_ref().expect("an old side");
        assert_eq!(from.revision.href, "/wiki/Project:Home?oldid=7");
        assert_eq!(from.revision.label, "Revision as of 15:04, 20 October 2026");
        assert_eq!(
            t.to.revision.label,
            "Latest revision as of 15:04, 20 October 2026"
        );
        assert_eq!(
            from.user.as_ref().map(|u| u.href.as_str()),
            Some("/wiki/Special:Contributions/Harej")
        );
        assert_eq!(t.to.user, None, "a hidden user says so");
        assert_eq!(from.comment.as_deref(), Some("<b>sum</b>"));
        assert_eq!(
            from.nav.as_ref().map(|n| n.href.as_str()),
            Some("/wiki/Project:Home?diff=prev&oldid=7")
        );
        assert_eq!(t.to.nav, None, "nothing newer");
        assert_eq!(t.body_html, "<tr><td>row</td></tr>");
        assert!(!t.inline_html.is_empty());
        let created = PageDiff {
            from: None,
            body: String::new(),
            inline: String::new(),
            ..d
        };
        let t = diff_table_model(&m, &created, None);
        assert_eq!(t.from, None);
        assert_eq!(t.creation, "(Page created)");
        assert_eq!(t.none, "(No difference)");
        assert!(t.inline_html.is_empty(), "nothing to show in one column");
    }

    #[test]
    fn what_is_asked() {
        assert_eq!(Ask::of(&params(&[])), Ask::Read);
        assert_eq!(Ask::of(&params(&[("action", "edit")])), Ask::Edit);
        assert_eq!(Ask::of(&params(&[("action", "submit")])), Ask::Edit);
        assert_eq!(Ask::of(&params(&[("action", "history")])), Ask::History);
        assert_eq!(Ask::of(&params(&[("action", "render")])), Ask::Render);
    }

    #[test]
    fn tabs_of_a_page_and_a_missing_one() {
        let m = Messages::for_language("en");
        let t = tabs(&m, "Librarybase", "Project:Home", true, TabName::History);
        assert_eq!(t.len(), 5);
        // 19 §1.3: Links here and the talk page close the row.
        assert_eq!(t[3].href, "/wiki/Special:WhatLinksHere/Project:Home");
        assert_eq!(t[4].href, "/wiki/Project_talk:Home");
        assert_eq!(t[4].label, m.get("ts-tab-discussion"));
        // The site's own name for the namespace leads to the same talk page.
        let own = tabs(&m, "Librarybase", "Librarybase:Home", true, TabName::Read);
        assert_eq!(own[4].href, "/wiki/Project_talk:Home");
        let a = actions(
            &m,
            "Project:Home",
            true,
            ModerationTabs {
                protect: true,
                delete: true,
                r#move: true,
            },
        );
        assert_eq!(a[0].1, "/wiki/Project:Home?action=protect");
        assert_eq!(a[1].1, "/wiki/Special:MovePage/Project:Home");
        assert_eq!(a[2].1, "/wiki/Project:Home?action=delete");
        assert_eq!(
            actions(&m, "Project:Home", true, ModerationTabs::default()).len(),
            0
        );
        assert_eq!(t[1].href, "/wiki/Project:Home?action=edit");
        assert_eq!(t[1].label, m.get("ts-tab-edit"));
        assert!(t[2].current);
        let t = tabs(&m, "Librarybase", "Project:New one", false, TabName::Read);
        // A free title has no talk page (14 §1.3), but its wanted links are listed.
        assert_eq!(t.len(), 3);
        assert_eq!(t[2].label, m.get("ts-tab-links-here"));
        assert_eq!(t[1].label, m.get("ts-tab-create"));
        assert_eq!(t[1].href, "/wiki/Project:New_one?action=edit");
    }

    #[test]
    fn categories_are_listed_but_not_hidden_ones() {
        let m = Messages::for_language("en");
        let p = Parsed {
            categories: vec![
                PageCategory {
                    name: "Policy pages".into(),
                    hidden: false,
                },
                PageCategory {
                    name: "Tracking".into(),
                    hidden: true,
                },
            ],
            ..Parsed::default()
        };
        let c = content_model(&m, &p).categories.expect("shown categories");
        assert_eq!(c.items.len(), 1);
        assert_eq!(c.items[0].href, "/wiki/Category:Policy_pages");
        assert_eq!(c.items[0].label, "Policy pages");
        assert_eq!(content_model(&m, &Parsed::default()).categories, None);
    }

    // 0041 §10, 19 §3.2: a new page's Format selector offers its namespace's text models,
    // the default first, and only where there is more than one.
    #[test]
    fn a_new_page_offers_the_formats_its_namespace_allows() {
        let main = formats("Librarybase", "Sandbox");
        assert_eq!(main.first().map(String::as_str), Some("wikitext"));
        assert!(main.iter().any(|f| f == "markdown"));
        assert!(!main.iter().any(|f| f == "triplespace-talk"));
        assert!(formats("Librarybase", "Librarybase:Sandbox").len() > 1);
        assert!(formats("Librarybase", "Thread:2026-11-05/Scope").is_empty());
        let d = Draft::new_at("Librarybase", "Sandbox".into(), Some("markdown"));
        assert_eq!(d.model, "markdown");
        let d = Draft::new_at("Librarybase", "Sandbox".into(), Some("triplespace-talk"));
        assert_eq!(d.model, "wikitext");
        let m = Messages::for_language("en");
        let f = edit_form_model(&m, "t", &d).format.expect("a selector");
        assert_eq!(f.label, m.get("ts-pageedit-model"));
        assert!(form_edits("markdown") && form_edits("wikitext"));
        assert!(!form_edits("wikibase-item") && !form_edits("triplespace-talk"));
    }

    // 0038 §7, 19 §3.1: the panel's facts, from prop=info|revisions.
    #[test]
    fn the_about_panel_says_what_the_page_is() {
        let m = Messages::for_language("en");
        let a = About {
            title: "Project:Home".into(),
            model: "wikitext".into(),
            length: 1234,
            touched: Some("2026-11-05T10:00:00Z".into()),
            lastrevid: 99,
            created: Some("2026-01-02T03:04:05Z".into()),
            creator: Some("Alice".into()),
            protection: vec![],
        };
        let p = about_model(&m, &a, &Parsed::default());
        assert_eq!(
            p.facts[0].link.as_ref().map(|l| l.label.as_str()),
            Some("Alice")
        );
        assert_eq!(
            p.facts[1].link.as_ref().map(|l| l.href.as_str()),
            Some("/w/index.php?title=Project:Home&diff=99")
        );
        assert!(
            p.facts
                .iter()
                .any(|f| f.value == m.get("ts-about-not-protected"))
        );
        assert_eq!(p.links[1].href, "/wiki/Special:WhatLinksHere/Project:Home");
    }

    #[test]
    fn the_form_round_trips_a_draft() {
        let m = Messages::for_language("en");
        let d = Draft {
            title: "Project:Sandbox".into(),
            exists: true,
            text: "<b>a</b> & b".into(),
            summary: "x\"y".into(),
            minor: true,
            base: Some(7),
            base_time: Some("2026-10-19T00:00:00Z".into()),
            ..Draft::default()
        };
        let f = edit_form_model(&m, "tok+\\", &d);
        assert_eq!(f.action, "/w/index.php?title=Project:Sandbox&action=submit");
        assert_eq!(f.token, "tok+\\");
        assert_eq!(
            f.text, "\n<b>a</b> & b",
            "the leading newline the textarea eats"
        );
        assert_eq!(f.base, Some(7));
        assert_eq!(f.summary.value, "x\"y");
        assert!(f.minor);
        assert!(!f.create);
        assert_eq!(f.save.label, m.get("ts-pageedit-save"));
        let back = Draft::of_form(
            "Librarybase",
            "Project:Sandbox",
            &params(&[
                ("wpTextbox1", "line\r\nline"),
                ("wpSummary", "s"),
                ("baseRevId", "7"),
                ("wpEdittime", "2026-10-19T00:00:00Z"),
            ]),
        );
        assert_eq!(back.text, "line\nline");
        assert_eq!(back.base, Some(7));
        assert!(back.exists);
        assert!(!back.minor);
        let new = Draft::of_form(
            "Librarybase",
            "Project:X",
            &params(&[("wpCreateonly", "1")]),
        );
        assert!(!new.exists);
        assert_eq!(new.base, None);
    }
}
