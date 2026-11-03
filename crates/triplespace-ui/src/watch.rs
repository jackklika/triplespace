//! Watching (0020 §3; 19 §6.2): the watch menu on a page, entity, talk page or thread,
//! without JavaScript; the forms it posts; and the seen mark a watcher's visit clears.
//!
//! - **The menu's button** is the star with **Watch**, or a filled star with
//!   **Watching**. It is a disclosure holding one form: how long (permanently, a week, a
//!   month, three months, six months) and *Notify me*, which sets `notify` on the watch
//!   (16 §4.3). Submitting it posts `action=watch` with both.
//! - **Unwatch**, in the menu of a watched title, posts `action=unwatch`.
//! - **Visiting** a watched page with changes the viewer has not seen marks them seen, as
//!   MediaWiki clears the watchlist's bold on a visit.
//!
//! A talk page and its subject share one watch, so either's star shows the same state.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, WatchForm};

use crate::Site;
use crate::codex::MessageKind;
use crate::features::Feature;
use crate::frame::{Page, WatchControl, title_url};
use crate::pages::{self, Context, Params};

/// The spans the watch menu offers, as `action=watch`'s `expiry` takes them, with their
/// labels' message keys.
pub const SPANS: &[(&str, &str)] = &[
    ("infinite", "ts-watch-permanent"),
    ("1 week", "ts-watch-1week"),
    ("1 month", "ts-watch-1month"),
    ("3 months", "ts-watch-3months"),
    ("6 months", "ts-watch-6months"),
];

/// The star's state for a title, for a signed-in viewer where the site offers watching;
/// `None` otherwise, or when the API cannot say. A visit to a page with changes the viewer
/// has not seen marks them seen.
pub async fn control(
    site: &Site,
    cx: &Context,
    incoming: &Incoming,
    title: &str,
) -> Option<WatchControl> {
    if cx.user.is_none() || !cx.features.has(Feature::Watchlist) {
        return None;
    }
    let client = site.client();
    let (state, token) = tokio::join!(
        client.watch_state(incoming, title),
        client.token(incoming, "watch")
    );
    let state = state.ok()?.value;
    let token = token.ok()?.value.value;
    if state.unseen.is_some()
        && let Ok(csrf) = client.token(incoming, "csrf").await
    {
        let _ = client
            .mark_watch_seen(incoming, Some(title), &csrf.value.value)
            .await;
    }
    Some(WatchControl {
        title: title.to_string(),
        watched: state.watched,
        expires: state.expires.filter(|e| e != "infinity"),
        notify: state.notify,
        token,
    })
}

/// A watch form posted to a title: `action=watch` or `action=unwatch`, then back to the
/// page.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: Context,
    title: &str,
    action: &str,
    form: &Params,
) -> Response {
    if cx.user.is_none() || !cx.features.has(Feature::Watchlist) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let token = form.map.get("wpEditToken").map_or("", String::as_str);
    let expiry = form
        .map
        .get("expiry")
        .map(String::as_str)
        .filter(|e| SPANS.iter().any(|(s, _)| s == e));
    // The menu's form carries `notifyfield`, so an unticked box means off; the star's
    // leaves `notify` as it was.
    let notify = form
        .map
        .contains_key("notifyfield")
        .then(|| form.map.contains_key("notify"));
    let titles = [title.to_string()];
    let result = site
        .client()
        .watch(
            incoming,
            &WatchForm {
                titles: &titles,
                unwatch: action == "unwatch",
                expiry,
                notify,
                token,
            },
        )
        .await;
    match result {
        Ok(Saved::Ok(_)) => pages::see_other(&format!("/wiki/{}", title_url(title)), &[]),
        Ok(Saved::Refused(r)) => {
            let text = r
                .iter()
                .map(|x| x.message.clone())
                .collect::<Vec<_>>()
                .join(" ");
            pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |_| Page {
                title: title.to_string(),
                returnto: Some(title.to_string()),
                body: cx.message(MessageKind::Error, &text),
                ..Page::default()
            })
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}
