//! `Special:Watchlist` and `Special:EditWatchlist` (0020 §4; 19 §6.2), for a signed-in
//! viewer, labelled **Private** as 0010 §1 asks.
//!
//! - **`Special:Watchlist`**: the changes to what the viewer watches, newest first, fifty
//!   to a page, as recent changes shows them, mirror syncs included (16 §4.2); a change
//!   they have not seen is marked, and **Mark all changes as seen** clears the marks.
//! - **`Special:EditWatchlist`**: every watch, with how long it lasts and whether it
//!   notifies, a box to tick beside each, and **Remove selected**.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::activity::ActivityQuery;
use triplespace_client::settings::Saved;
use triplespace_client::{ClientError, WatchForm};

use crate::Site;
use crate::codex::{Message, MessageKind, MessageModel};
use crate::entity::history::{Choice, Switch};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::changes::{ListModel, date, list_model};

const WATCHLIST: &str = "Special:Watchlist";
const EDIT: &str = "Special:EditWatchlist";

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/watchlist.html` and
// `special/editwatchlist.html` draw.
// ---------------------------------------------------------------------------------------

/// A form with a token and one button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ButtonForm {
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The button.
    pub label: String,
}

/// `Special:Watchlist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchlistPage {
    /// The tabs: the changes, and editing the list.
    pub tabs: Switch,
    /// **Mark all changes as seen**, when any is unseen.
    pub seen: Option<ButtonForm>,
    /// What the page says when it has no rows.
    pub empty: Option<String>,
    /// The rows.
    pub list: Option<ListModel>,
    /// **Older**.
    pub older: Option<Choice>,
}

/// One watch on `Special:EditWatchlist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Watch {
    /// The box's number.
    pub i: usize,
    /// The title.
    pub title: String,
    /// The page.
    pub href: String,
    /// The box's hidden label.
    pub remove: String,
    /// When it expires, or that it is permanent.
    pub expires: String,
    /// Whether it notifies.
    pub notify: String,
}

/// `Special:EditWatchlist`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditWatchlistPage {
    /// The tabs.
    pub tabs: Switch,
    /// What the page says when nothing is watched.
    pub empty: Option<String>,
    /// "n pages watched".
    pub count: Option<String>,
    /// Where the form posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The table's hidden caption.
    pub caption: String,
    /// The column headings.
    pub columns: Vec<String>,
    /// The watches.
    pub watches: Vec<Watch>,
    /// **Remove selected**.
    pub remove: String,
}

/// The **Private** label beside the title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
struct Private {
    label: String,
}

/// A page asking the viewer to log in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginPage {
    /// The notice.
    pub notice: MessageModel,
    /// The login page.
    pub login: Option<Choice>,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn private_label(cx: &Context) -> String {
    cx.draw(
        "special/private.html",
        Private {
            label: cx.m.get("ts-account-private"),
        },
    )
}

/// The login page, for a viewer who is not signed in.
fn login(cx: &Context, headers: &HeaderMap, title: &str, key: &str) -> Response {
    let body = cx.draw(
        "login.html",
        LoginPage {
            notice: Message::block(MessageKind::Notice, &cx.m.get("ts-watchlist-login")).model(),
            login: Some(choice(
                format!("/wiki/Special:UserLogin?returnto={title}"),
                cx.m.get("ts-login"),
            )),
        },
    );
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get(key),
        returnto: Some(title.to_string()),
        body,
        ..Page::default()
    })
}

fn tabs(m: &Messages, current: &str) -> Switch {
    let tab = |title: &str, key: &str| Choice {
        href: format!("/wiki/{title}"),
        label: m.get(key),
        current: title == current,
    };
    Switch {
        label: m.get("ts-watchlist-title"),
        items: vec![
            tab(WATCHLIST, "ts-watchlist-changes"),
            tab(EDIT, "ts-editwatchlist-title"),
        ],
    }
}

async fn context(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    params: &Params,
) -> Result<(Context, triplespace_client::Incoming), Response> {
    let incoming = pages::incoming(headers, peer);
    let cx = pages::context(site, &incoming, &params.map).await?;
    Ok((cx, incoming))
}

/// `GET Special:Watchlist`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (mut cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) {
        return pages::no_page(&cx, headers, WATCHLIST.to_string());
    }
    if cx.user.is_none() {
        return login(&cx, headers, WATCHLIST, "ts-watchlist-title");
    }
    let query = ActivityQuery {
        set: Some("watchlist".into()),
        limit: 50,
        cursor: params.map.get("from").cloned(),
        lang: cx.m.lang().to_string(),
        ..ActivityQuery::default()
    };
    let page = match site.client().activity(&incoming, &query).await {
        Ok(p) => p,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(page.cache);
    let token = match site.client().token(&incoming, "csrf").await {
        Ok(t) => t.value.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let m = &cx.m;
    let rows = &page.value.rows;
    let body = cx.draw(
        "special/watchlist.html",
        WatchlistPage {
            tabs: tabs(m, WATCHLIST),
            seen: rows.iter().any(|r| r.unseen).then(|| ButtonForm {
                action: format!("/w/index.php?title={WATCHLIST}"),
                token: token.clone(),
                label: m.get("ts-watchlist-markseen"),
            }),
            empty: rows.is_empty().then(|| m.get("ts-watchlist-none")),
            list: list_model(m, &cx.site, rows),
            older: page.value.next.as_ref().map(|next| {
                let q: String = url::form_urlencoded::byte_serialize(next.as_bytes()).collect();
                choice(
                    format!("/wiki/{WATCHLIST}?from={q}"),
                    m.get("ts-watchlist-older"),
                )
            }),
        },
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-watchlist-title"),
        returnto: Some(WATCHLIST.to_string()),
        identity_html: Some(private_label(&cx)),
        body,
        ..Page::default()
    })
}

/// `POST Special:Watchlist`: **Mark all changes as seen**.
pub async fn post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) || cx.user.is_none() {
        return pages::no_page(&cx, headers, WATCHLIST.to_string());
    }
    let token = params.map.get("wpEditToken").map_or("", String::as_str);
    match site.client().mark_watch_seen(&incoming, None, token).await {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{WATCHLIST}"), &[]),
        Ok(Saved::Refused(r)) => refused(&cx, headers, WATCHLIST, &r),
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

fn refused(
    cx: &Context,
    headers: &HeaderMap,
    title: &str,
    r: &[triplespace_client::settings::Refusal],
) -> Response {
    let text = r
        .iter()
        .map(|x| x.message.clone())
        .collect::<Vec<_>>()
        .join(" ");
    pages::respond(cx, headers, StatusCode::BAD_REQUEST, |_| Page {
        title: title.to_string(),
        returnto: Some(title.to_string()),
        body: pages::message(MessageKind::Error, &text),
        ..Page::default()
    })
}

/// `GET Special:EditWatchlist`.
pub async fn edit(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (mut cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) {
        return pages::no_page(&cx, headers, EDIT.to_string());
    }
    if cx.user.is_none() {
        return login(&cx, headers, EDIT, "ts-editwatchlist-title");
    }
    let (watches, token) = tokio::join!(
        site.client().watches(&incoming),
        site.client().token(&incoming, "watch")
    );
    let watches = match watches {
        Ok(w) => w,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let token = match token {
        Ok(t) => t.value.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(watches.cache);
    let m = &cx.m;
    let mut entries: Vec<_> = watches
        .value
        .iter()
        .filter_map(|w| w.title.clone().map(|t| (t, w)))
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let body = cx.draw(
        "special/editwatchlist.html",
        EditWatchlistPage {
            tabs: tabs(m, EDIT),
            empty: entries.is_empty().then(|| m.get("ts-editwatchlist-none")),
            count: (!entries.is_empty())
                .then(|| m.with("ts-editwatchlist-count", &[&entries.len().to_string()])),
            action: format!("/w/index.php?title={EDIT}"),
            token: token.clone(),
            caption: m.get("ts-editwatchlist-title"),
            columns: [
                "ts-editwatchlist-remove-col",
                "ts-editwatchlist-page",
                "ts-editwatchlist-expires",
                "ts-editwatchlist-notify",
            ]
            .iter()
            .map(|k| m.get(k))
            .collect(),
            watches: entries
                .iter()
                .enumerate()
                .map(|(i, (title, w))| Watch {
                    i,
                    title: title.clone(),
                    href: format!("/wiki/{}", title_url(title)),
                    remove: m.with("ts-editwatchlist-remove-one", &[title]),
                    expires: w
                        .expires
                        .as_deref()
                        .map_or_else(|| m.get("ts-watch-permanent"), |e| date(m, e)),
                    notify: m.get(if w.notify {
                        "ts-editwatchlist-yes"
                    } else {
                        "ts-editwatchlist-no"
                    }),
                })
                .collect(),
            remove: m.get("ts-editwatchlist-remove"),
        },
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-editwatchlist-title"),
        returnto: Some(EDIT.to_string()),
        identity_html: Some(private_label(&cx)),
        body,
        ..Page::default()
    })
}

/// `POST Special:EditWatchlist`: **Remove selected**.
pub async fn edit_post(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let (cx, incoming) = match context(site, headers, peer, params).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Watchlist) || cx.user.is_none() {
        return pages::no_page(&cx, headers, EDIT.to_string());
    }
    let titles: Vec<String> = params
        .pairs
        .iter()
        .filter(|(k, _)| k == "titles")
        .map(|(_, v)| v.clone())
        .collect();
    if titles.is_empty() {
        return pages::see_other(&format!("/wiki/{EDIT}"), &[]);
    }
    let token = params.map.get("wpEditToken").map_or("", String::as_str);
    let r = site
        .client()
        .watch(
            &incoming,
            &WatchForm {
                titles: &titles,
                unwatch: true,
                expiry: None,
                notify: None,
                token,
            },
        )
        .await;
    match r {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{EDIT}"), &[]),
        Ok(Saved::Refused(r)) => refused(&cx, headers, EDIT, &r),
        Err(ClientError::Api { info, .. }) => refused(
            &cx,
            headers,
            EDIT,
            &[triplespace_client::settings::Refusal {
                code: "api".into(),
                key: None,
                message: info,
            }],
        ),
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
