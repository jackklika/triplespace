//! `Special:Notifications` (0021 §6; 19 §6.2): the viewer's inbox as a page, newest first,
//! fifty to a page, everything or the unread only, with **Mark as read** on each unread
//! item and **Mark all as read** above them. Opening it is looking: the bell's mark goes
//! (`echomarkseen`). Each item says who did what where, and links to it: a thread's post,
//! or the user page an item about an account names.
//!
//! The list is what the API lets the viewer see now: an item about a thread they may no
//! longer read, or a post since hidden, is not there (16 §5.2).

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::Notification;
use triplespace_client::settings::Saved;

use crate::Site;
use crate::codex::{Message, MessageKind, MessageModel};
use crate::entity::history::{Choice, Switch};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};
use crate::special::watchlist::LoginPage;

const TITLE: &str = "Special:Notifications";

/// How many items a page shows.
const PER_PAGE: u32 = 50;

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/notifications.html` draws.
// ---------------------------------------------------------------------------------------

/// A part of an item's sentence: text, or a link in its place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Part {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// A link.
    Link {
        /// The page.
        href: String,
        /// The text.
        text: String,
        /// The `class`, for the thread link.
        class: Option<&'static str>,
    },
}

/// **Mark as read** or **Mark all as read**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MarkForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// `markread` or `markall`.
    pub name: String,
    /// The ID, or `1`.
    pub value: String,
    /// Whether to come back to the unread view.
    pub unread: bool,
    /// The button.
    pub label: String,
}

/// One item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// The reason, as the API names it.
    pub reason: String,
    /// Whether it is unread.
    pub unread: bool,
    /// The sentence.
    pub sentence: Vec<Part>,
    /// ISO 8601.
    pub datetime: String,
    /// "HH:MM, date".
    pub time: String,
    /// The talk page, where the item has one.
    pub talk: Option<Choice>,
    /// **Mark as read**, for an unread item.
    pub mark: Option<MarkForm>,
}

/// `Special:Notifications`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NotificationsPage {
    /// The filters: all, unread.
    pub filters: Switch,
    /// **Mark all as read**, when any is unread.
    pub markall: Option<MarkForm>,
    /// What the page says when it has no items.
    pub empty: Option<MessageModel>,
    /// The items.
    pub items: Vec<Item>,
    /// **Older**.
    pub older: Option<Choice>,
}

/// One item's sentence, its links in place.
fn sentence(m: &Messages, n: &Notification) -> Vec<Part> {
    let who = match &n.agent {
        Some(agent) => Part::Link {
            href: format!("/wiki/User:{}", title_url(agent)),
            text: agent.clone(),
            class: None,
        },
        None => Part::Text {
            text: m.get("ts-notif-someone"),
        },
    };
    let thread = match (&n.title, &n.subject) {
        (Some(title), Some(subject)) => Some(Part::Link {
            href: match n.revid {
                Some(rev) => format!("/wiki/{}#post-{rev}", title_url(title)),
                None => format!("/wiki/{}", title_url(title)),
            },
            text: format!("“{subject}”"),
            class: Some("ts-notif__thread"),
        }),
        _ => None,
    };
    let account = n.title.as_deref().map(|t| Part::Link {
        href: format!("/wiki/{}", title_url(t)),
        text: t.split_once(':').map_or(t, |(_, n)| n).to_string(),
        class: None,
    });
    let key = match n.reason.as_str() {
        "talk" => "ts-notif-talk",
        "reply" => "ts-notif-reply",
        "mention" => "ts-notif-mention",
        "thread-status" => "ts-notif-thread-status",
        "rights" => "ts-notif-rights",
        "group-join" => "ts-notif-group-join",
        "watch" if n.subject.is_some() => "ts-notif-watch-thread",
        "watch" => "ts-notif-watch",
        _ => "ts-notif-other",
    };
    let page = n.title.as_deref().map(|t| Part::Link {
        href: match n.revid {
            Some(rev) => format!("/w/index.php?title={}&oldid={rev}", title_url(t)),
            None => format!("/wiki/{}", title_url(t)),
        },
        text: t.to_string(),
        class: None,
    });
    let second = match n.reason.as_str() {
        "group-join" => account,
        "watch" if n.subject.is_none() => page,
        _ => thread,
    };
    // The message, its `$1` and `$2` replaced by the parts.
    let text = m.get(key);
    let mut parts = Vec::new();
    let mut rest = text.as_str();
    while let Some(i) = rest.find("$1").into_iter().chain(rest.find("$2")).min() {
        if i > 0 {
            parts.push(Part::Text {
                text: rest[..i].to_string(),
            });
        }
        let which = &rest[i..i + 2];
        let part = if which == "$1" {
            Some(&who)
        } else {
            second.as_ref()
        };
        if let Some(p) = part {
            parts.push(p.clone());
        }
        rest = &rest[i + 2..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text {
            text: rest.to_string(),
        });
    }
    parts
}

fn mark_form(token: &str, id: Option<i64>, label: &str, unread_only: bool) -> MarkForm {
    let (name, value) = match id {
        Some(i) => ("markread", i.to_string()),
        None => ("markall", "1".to_string()),
    };
    MarkForm {
        action: format!("/w/index.php?title={TITLE}"),
        token: token.to_string(),
        name: name.to_string(),
        value,
        unread: unread_only,
        label: label.to_string(),
    }
}

/// `GET Special:Notifications`.
#[allow(clippy::too_many_lines)]
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    if cx.user.is_none() {
        let body = cx.draw(
            "login.html",
            LoginPage {
                notice: Message::block(MessageKind::Notice, &cx.m.get("ts-notifications-login"))
                    .model(),
                login: Some(Choice {
                    href: format!("/wiki/Special:UserLogin?returnto={TITLE}"),
                    label: cx.m.get("ts-login"),
                    current: false,
                }),
            },
        );
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-notifications-title"),
            returnto: Some(TITLE.to_string()),
            body,
            ..Page::default()
        });
    }
    let unread_only = params.map.get("unread").is_some_and(|v| v == "1");
    let from = params.map.get("from").map(String::as_str);
    let listing = match site
        .client()
        .notifications(&incoming, unread_only, PER_PAGE, from)
        .await
    {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(listing.cache);
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(token.value.cache.clone());
    let cookies = token.set_cookies.clone();
    let tok = token.value.value;
    // Opening the inbox is looking at it: the bell's mark goes.
    if let Ok(Saved::Ok(_)) = site.client().mark_seen(&incoming, &tok).await
        && let Some(n) = cx.notifications.as_mut()
    {
        n.seen = n.newest_unread.clone();
    }
    let m = &cx.m;
    let items = &listing.value.items;
    let body = cx.draw(
        "special/notifications.html",
        NotificationsPage {
            filters: Switch {
                label: m.get("ts-notifications-filter"),
                items: vec![
                    Choice {
                        href: format!("/wiki/{TITLE}"),
                        label: m.get("ts-notifications-all"),
                        current: !unread_only,
                    },
                    Choice {
                        href: format!("/wiki/{TITLE}?unread=1"),
                        label: m.get("ts-notifications-unread"),
                        current: unread_only,
                    },
                ],
            },
            markall: items
                .iter()
                .any(|n| !n.read)
                .then(|| mark_form(&tok, None, &m.get("ts-notifications-markall"), unread_only)),
            empty: items.is_empty().then(|| {
                Message::block(
                    MessageKind::Notice,
                    &m.get(if unread_only {
                        "ts-notifications-none-unread"
                    } else {
                        "ts-notifications-empty"
                    }),
                )
                .model()
            }),
            items: items
                .iter()
                .map(|n| Item {
                    reason: n.reason.clone(),
                    unread: !n.read,
                    sentence: sentence(m, n),
                    datetime: n.timestamp.clone(),
                    time: format!("{}, {}", clock(&n.timestamp), date(m, &n.timestamp)),
                    talk: n.talk.as_deref().map(|t| Choice {
                        href: format!("/wiki/{}", title_url(t)),
                        label: t.to_string(),
                        current: false,
                    }),
                    mark: (!n.read).then(|| {
                        mark_form(
                            &tok,
                            Some(n.id),
                            &m.get("ts-notifications-markread"),
                            unread_only,
                        )
                    }),
                })
                .collect(),
            older: listing.value.next.as_ref().map(|next| Choice {
                href: format!(
                    "/wiki/{TITLE}?from={}{}",
                    url_q(next),
                    if unread_only { "&unread=1" } else { "" }
                ),
                label: m.get("ts-notifications-older"),
                current: false,
            }),
        },
    );
    let r = pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-notifications-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &cookies)
}

fn url_q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// `POST Special:Notifications`: **Mark as read** (`markread`, an ID) or **Mark all as
/// read** (`markall`), then back to the list.
pub async fn post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Notifications) || cx.user.is_none() {
        return pages::no_page(&cx, headers, TITLE.to_string());
    }
    let form = &params.map;
    let token = form.get("wpEditToken").map_or("", String::as_str);
    let one = form.get("markread").and_then(|i| i.parse::<i64>().ok());
    let ids = one.map(|i| vec![i]);
    if ids.is_none() && !form.contains_key("markall") {
        return pages::see_other(&format!("/wiki/{TITLE}"), &[]);
    }
    match site
        .client()
        .mark_read(&incoming, ids.as_deref(), None, token)
        .await
    {
        Ok(Saved::Ok(_)) => {
            let back = if form.get("unread").is_some_and(|v| v == "1") {
                format!("/wiki/{TITLE}?unread=1")
            } else {
                format!("/wiki/{TITLE}")
            };
            pages::see_other(&back, &[])
        }
        Ok(Saved::Refused(r)) => {
            let text = r
                .iter()
                .map(|x| x.message.clone())
                .collect::<Vec<_>>()
                .join(" ");
            pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |m| Page {
                title: m.get("ts-notifications-title"),
                returnto: Some(TITLE.to_string()),
                body: pages::message(MessageKind::Error, &text),
                ..Page::default()
            })
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
