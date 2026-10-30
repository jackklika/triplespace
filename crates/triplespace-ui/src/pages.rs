//! The site's handlers, and the caching a page composes from the API responses it used
//! (0057 §6).
//!
//! A page is `public` only if every API response it drew on was; its `ETag` is a hash of
//! the build, the interface language, the status and those responses' `ETag`s, so a
//! revalidation costs the API calls but not the rendering; its `Cache-Tag`s are theirs.
//! It varies on `Cookie` and `Accept-Language`, the two inputs a shared cache can see.
//!
//! When the API cannot be reached, or is older than [`MIN_API_VERSION`], the site says so
//! with a `503` and `Retry-After`, in the frame and the shipped theme, and keeps nothing.
//!
//! Entity titles are served by [`crate::entity`], special pages by [`crate::special`],
//! and every other title, where the API serves pages, by [`crate::document`]; without
//! pages, the main page is the generated one and every other title a `404`.

use std::collections::BTreeMap;
use std::net::SocketAddr;

use axum::Extension;
use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sha2::{Digest as _, Sha256};
use triplespace_client::{CacheInfo, ClientError, Counts, Incoming, SiteInfo};

use crate::codex::{Message, MessageKind};
use crate::features::Feature;
use crate::features::Features;
use crate::frame::{self, Chrome, Page, title_url};
use crate::i18n::{self, Messages};
use crate::routes::{self, Target};
use crate::skin::Skin;
use crate::theme::Theme;
use crate::{MIN_API_VERSION, Renderer, Site, assets, parity};
use crate::{document, entity, home, moderation, special};

/// The connection address, when the server was started with `ConnectInfo`.
pub type Peer = Option<Extension<ConnectInfo<SocketAddr>>>;

/// How long a shared cache keeps a public page.
const S_MAXAGE: u32 = 60;

/// How long a client waits before trying again after a `503`.
const RETRY_AFTER: &str = "30";

/// The Content Security Policy of every response the site makes (0034 §11, A13; 0057 §7):
/// the site's own origin only and no inline script; stylesheets from the origin and no
/// `<style>` element, but `style` attributes, which wikitext writes and the API's
/// Sanitizer has checked.
pub const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; \
    style-src-attr 'unsafe-inline'; \
    img-src 'self' data:; font-src 'self'; object-src 'none'; base-uri 'self'; \
    frame-ancestors 'self'; form-action 'self'";

/// Adds the security headers to every response.
pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut r = next.run(request).await;
    let h = r.headers_mut();
    // A page that widened `form-action` for its issuers' buttons keeps its own policy.
    if !h.contains_key(header::CONTENT_SECURITY_POLICY) {
        h.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP),
        );
    }
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}

/// Refuses the API's paths: the site never answers for them, even where it is the
/// fallback of the API's own router (0057 §3).
pub async fn api_paths_refused(request: Request, next: Next) -> Response {
    if routes::target(request.uri().path()) == Target::Api {
        return (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Not found\n",
        )
            .into_response();
    }
    next.run(request).await
}

/// What every framed page needs from the API, and the caching facts of each response a
/// page draws on; a page adds its own.
pub struct Context {
    /// The site.
    pub site: SiteInfo,
    /// The signed-in viewer's name.
    pub user: Option<String>,
    /// The caching facts of every API response the page used.
    pub inputs: Vec<CacheInfo>,
    /// The interface messages.
    pub m: Messages,
    /// The theme.
    pub theme: Theme,
    /// What the site offers.
    pub features: Features,
    /// The signed-in viewer's resolved preferences (`uiprop=options`), for the frame.
    pub prefs: Option<serde_json::Map<String, serde_json::Value>>,
    /// The open account requests, where the viewer reviews them (0087 §6).
    pub account_requests: Option<u64>,
    /// A signed-in viewer's tenant-wide rights (`uiprop=rights`): what decides whether
    /// Protect, Block or `Special:UserRights` are offered. Empty for a reader.
    pub rights: Vec<String>,
    /// A signed-in viewer's groups (`uiprop=groups`), for the lock-out note of the
    /// Protect form (09 §4.5).
    pub groups: Vec<String>,
    /// A signed-in viewer's unread notifications, for the bell and the orange bar
    /// (19 §6.2); `None` where the API serves none or did not answer.
    pub notifications: Option<Counts>,
    /// The skin that draws the page (0097 §8): `useskin=`, the reader's `ui.skin`, the
    /// wiki's `skins.default`, in that order, among the skins the wiki offers; `None`
    /// only when no skin at all is loaded.
    pub skin: Option<std::sync::Arc<Skin>>,
    /// Which renderer draws the page (19 §5.10).
    pub renderer: Renderer,
}

impl Context {
    /// Which moderation tabs the page offers the viewer (09 §4.5, §6.2; 19 §6.3):
    /// **Protect** to a holder of `protect`, **Delete** to a holder of `delete`, where
    /// the API serves the forms.
    #[must_use]
    pub fn moderation_tabs(&self) -> ModerationTabs {
        ModerationTabs {
            protect: self.holds("protect") && self.features.has(Feature::Protect),
            delete: self.holds("delete") && self.features.has(Feature::Delete),
            r#move: self.holds("move") && self.features.has(Feature::Move),
        }
    }

    /// Whether the viewer holds a right tenant-wide.
    #[must_use]
    pub fn holds(&self, right: &str) -> bool {
        self.rights.iter().any(|r| r == right)
    }

    /// What the frame needs of the site.
    #[must_use]
    pub fn chrome(&self) -> Chrome<'_> {
        Chrome {
            sitename: &self.site.sitename,
            mainpage: &self.site.mainpage,
            features: &self.features,
            prefs: self.prefs.as_ref(),
            account_requests: self.account_requests,
            notifications: self
                .notifications
                .as_ref()
                .filter(|_| self.features.has(Feature::Notifications)),
        }
    }
}

/// The browser's request, as the client forwards it.
pub fn incoming(headers: &HeaderMap, peer: Peer) -> Incoming {
    Incoming::new(headers, peer.map(|Extension(ConnectInfo(a))| a))
}

/// Asks the API who the site is and who the viewer is. A request without credentials is
/// anonymous whatever the API would say, so it is spared the second call. The interface
/// language is `uselang` where given and valid, else the site's (0034 §9).
///
/// # Errors
///
/// The `503` page when the API cannot be reached or is too old; the `502` page when its
/// `siteinfo` or `userinfo` answer cannot be read.
pub async fn context(
    site: &Site,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let client = site.client();
    // The bell's counts are asked for beside who the viewer is; an anonymous viewer's
    // answer is a refusal, which is dropped.
    let (si, ui, counts) = if incoming.credentialed() {
        let (si, ui, counts) = tokio::join!(
            client.siteinfo(incoming),
            client.userinfo(incoming),
            client.notification_counts(incoming)
        );
        (si, Some(ui), counts.ok())
    } else {
        (client.siteinfo(incoming).await, None, None)
    };
    let si = si.map_err(|e| unavailable(incoming, &e, failure_key(&e)))?;
    if si.value.api_version < MIN_API_VERSION {
        return Err(unavailable(
            incoming,
            &ClientError::Unexpected(format!(
                "the API's api_version is {}; this site needs {MIN_API_VERSION}",
                si.value.api_version
            )),
            "ts-api-too-old",
        ));
    }
    let mut inputs = vec![si.cache.clone()];
    let mut prefs = None;
    let mut account_requests = None;
    let mut rights = Vec::new();
    let mut groups = Vec::new();
    let user = match ui {
        Some(ui) => {
            let ui = ui.map_err(|e| unavailable(incoming, &e, failure_key(&e)))?;
            inputs.push(ui.cache);
            if !ui.value.anon {
                // A page drawn for a signed-in viewer is theirs alone, whatever its
                // inputs say: it may hold their name, their tokens or their controls.
                inputs.push(CacheInfo::default());
                prefs = Some(ui.value.options.clone());
                account_requests = ui.value.account_requests;
                rights.clone_from(&ui.value.rights);
                groups.clone_from(&ui.value.groups);
            }
            (!ui.value.anon).then_some(ui.value.name)
        }
        None => None,
    };
    let notifications = counts.filter(|_| user.is_some()).map(|c| c.value);
    // `uselang`, then a signed-in reader's own interface language (0085 §2.2), then the
    // site's.
    let lang = query
        .get("uselang")
        .and_then(|l| i18n::valid_code(l))
        .or_else(|| {
            prefs
                .as_ref()
                .and_then(|p: &serde_json::Map<String, serde_json::Value>| p.get("language"))
                .and_then(serde_json::Value::as_str)
                .and_then(i18n::valid_code)
        })
        .unwrap_or_else(|| si.value.lang.clone());
    let m =
        Messages::for_language(&lang).with_overrides(si.value.interface.clone(), &si.value.lang);
    let theme = Theme::from_info(si.value.theme.as_ref()).with_chips(&si.value.providers);
    let features = Features::of(&si.value);
    let pref_skin = prefs
        .as_ref()
        .and_then(|p| p.get("ui.skin"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let skin = site
        .skins()
        .choose(
            query.get("useskin").map(String::as_str),
            pref_skin.as_deref(),
            si.value.skins.default.as_deref(),
            si.value.skins.available.as_deref(),
        )
        .cloned();
    Ok(Context {
        site: si.value,
        user,
        inputs,
        m,
        theme,
        features,
        prefs,
        account_requests,
        rights,
        groups,
        notifications,
        skin,
        renderer: site.renderer(),
    })
}

/// The frame's message for a failed call: an answer the site could not read is not the
/// API being away, and says so.
fn failure_key(error: &ClientError) -> &'static str {
    match error {
        ClientError::Unexpected(_) => "ts-api-unexpected",
        _ => "ts-api-unavailable",
    }
}

/// The page when the frame's own calls fail, with the shipped theme and the reason: `503`
/// with `Retry-After` when the API is away or too old, `502` when it answered something
/// the site cannot read, which waiting will not mend.
fn unavailable(incoming: &Incoming, error: &ClientError, key: &str) -> Response {
    let unreadable = key == "ts-api-unexpected";
    if unreadable {
        eprintln!("triplespace-ui: the API's answer could not be read: {error}");
    } else {
        eprintln!("triplespace-ui: the API is unavailable: {error}");
    }
    let m = Messages::for_language("en");
    let sitename = incoming.host().unwrap_or("Triplespace").to_string();
    let text = m.get(key);
    let page = Page {
        title: m.get("ts-api-unavailable-title"),
        body: message(MessageKind::Error, &text),
        ..Page::default()
    };
    let none = Features::default();
    let chrome = Chrome {
        sitename: &sitename,
        mainpage: DEFAULT_MAINPAGE,
        features: &none,
        prefs: None,
        account_requests: None,
        notifications: None,
    };
    let html = match frame::render(&m, &chrome, &Theme::shipped(), None, &page) {
        Ok(h) => h,
        Err(e) => return render_failed(&e),
    };
    let mut r = (
        StatusCode::SERVICE_UNAVAILABLE,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        html,
    )
        .into_response();
    if unreadable {
        *r.status_mut() = StatusCode::BAD_GATEWAY;
    } else {
        r.headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER));
    }
    r
}

/// A call a page needed failed after the frame's had succeeded: the API is away (`503`),
/// refused the viewer (`403`), or answered something the site cannot read (`502`). The
/// page says which, in the frame, and keeps nothing.
pub fn api_failed(cx: &Context, headers: &HeaderMap, error: &ClientError) -> Response {
    eprintln!("triplespace-ui: an API call failed: {error}");
    let (status, key) = match error {
        ClientError::Unreachable(_) => (StatusCode::SERVICE_UNAVAILABLE, "ts-api-unavailable"),
        ClientError::Api { code, .. } if code == "permissiondenied" || code == "readapidenied" => {
            (StatusCode::FORBIDDEN, "ts-permission-denied")
        }
        _ => (StatusCode::BAD_GATEWAY, "ts-api-unexpected"),
    };
    let mut r = respond(cx, headers, status, |m| Page {
        title: m.get("ts-api-unavailable-title"),
        body: message(MessageKind::Error, &m.get(key)),
        ..Page::default()
    });
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.remove(header::ETAG);
    h.remove("cache-tag");
    if status == StatusCode::SERVICE_UNAVAILABLE {
        h.insert(header::RETRY_AFTER, HeaderValue::from_static(RETRY_AFTER));
    }
    r
}

fn render_failed(e: &askama::Error) -> Response {
    eprintln!("triplespace-ui: a template failed to render: {e}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Internal error\n").into_response()
}

/// A Codex message, as HTML.
pub fn message(kind: MessageKind, text: &str) -> String {
    use askama::Template as _;
    Message::block(kind, text).render().unwrap_or_default()
}

/// The composed `ETag`, if every input had one. The build it hashes is the binary's
/// and, where the skin draws the page, the skin's name and version (0097 §15), so a
/// skin upgrade or a change of skin is build skew.
fn composed_etag(
    m: &Messages,
    status: StatusCode,
    inputs: &[CacheInfo],
    skin: Option<&str>,
) -> Option<String> {
    let mut h = Sha256::new();
    for part in [
        assets::build_id(),
        m.lang(),
        status.as_str(),
        skin.unwrap_or_default(),
    ] {
        h.update(part.as_bytes());
        h.update([0]);
    }
    for i in inputs {
        h.update(i.etag.as_deref()?.as_bytes());
        h.update([0]);
    }
    Some(format!("\"{}\"", crate::hex(&h.finalize()[..16])))
}

/// Whether `If-None-Match` names the `ETag`.
fn not_modified(request: &HeaderMap, etag: &str) -> bool {
    let want = etag.trim_start_matches("W/");
    request
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|t| t == "*" || t.trim_start_matches("W/") == want)
}

/// The caching headers of a page drawn from `inputs`.
pub(crate) fn caching(h: &mut HeaderMap, etag: Option<&str>, inputs: &[CacheInfo]) {
    let public = inputs.iter().all(|i| i.public);
    let cc = if public {
        format!("public, max-age=0, s-maxage={S_MAXAGE}, stale-while-revalidate=300")
    } else {
        "private, no-cache".to_string()
    };
    if let Ok(v) = HeaderValue::from_str(&cc) {
        h.insert(header::CACHE_CONTROL, v);
    }
    if let Some(v) = etag.and_then(|e| HeaderValue::from_str(e).ok()) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::VARY,
        HeaderValue::from_static("Cookie, Accept-Language"),
    );
    if public {
        let mut tags: Vec<&str> = inputs
            .iter()
            .flat_map(|i| i.tags.iter().map(String::as_str))
            .collect();
        tags.sort_unstable();
        tags.dedup();
        if !tags.is_empty()
            && let Ok(v) = HeaderValue::from_str(&tags.join(", "))
        {
            h.insert("cache-tag", v);
        }
    }
}

/// A `304` for a client that holds the response already, when the status allows one.
fn revalidated(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
) -> (Option<String>, Option<Response>) {
    let skin_build = match cx.renderer {
        Renderer::Skin => cx.skin.as_ref().map(|s| s.build()),
        Renderer::Builtin | Renderer::Compare => None,
    };
    let etag = composed_etag(&cx.m, status, &cx.inputs, skin_build.as_deref());
    if status == StatusCode::OK
        && let Some(e) = &etag
        && not_modified(headers, e)
    {
        let mut r = Response::new(Body::empty());
        *r.status_mut() = StatusCode::NOT_MODIFIED;
        caching(r.headers_mut(), Some(e), &cx.inputs);
        return (etag, Some(r));
    }
    (etag, None)
}

/// A page in the frame, with the caching its inputs allow: a `304` before rendering when
/// the client holds it.
pub fn respond(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    build: impl FnOnce(&Messages) -> Page,
) -> Response {
    let (etag, fresh) = revalidated(cx, headers, status);
    if let Some(r) = fresh {
        return r;
    }
    let page = build(&cx.m);
    let html = match draw(cx, &page) {
        Ok(h) => h,
        Err(r) => return *r,
    };
    let mut r = Response::new(Body::from(html));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    caching(r.headers_mut(), etag.as_deref(), &cx.inputs);
    r
}

/// Draws a page in the frame with the renderer the deployment runs (19 §5.10): the
/// built-in rendering, the skin, or both under `compare`, which serves the built-in
/// result and logs the route when they differ. `Err` is the internal error to answer
/// with.
fn draw(cx: &Context, page: &Page) -> Result<String, Box<Response>> {
    let model = frame::model(
        &cx.m,
        &cx.chrome(),
        &cx.theme,
        cx.user.as_deref(),
        page,
        cx.skin.as_deref(),
    );
    let failed =
        || Box::new((StatusCode::INTERNAL_SERVER_ERROR, "Internal error\n").into_response());
    let builtin = || {
        frame::render_builtin(&cx.m, &model).map_err(|e| {
            eprintln!("triplespace-ui: a template failed to render: {e}");
            failed()
        })
    };
    let skin = || {
        let Some(s) = &cx.skin else {
            eprintln!("triplespace-ui: no skin is loaded");
            return Err(failed());
        };
        frame::render_skin(s, &cx.m, &model).map_err(|e| {
            eprintln!(
                "triplespace-ui: the skin `{}` failed to render: {e:#}",
                s.name()
            );
            failed()
        })
    };
    match cx.renderer {
        Renderer::Builtin => builtin(),
        Renderer::Skin => skin(),
        Renderer::Compare => {
            let b = builtin()?;
            match skin() {
                Ok(s) => {
                    if let Some(d) = parity::compare(&b, &s, parity::Kind::Document) {
                        eprintln!(
                            "triplespace-ui: compare: {} differs for {}: {}",
                            cx.skin.as_ref().map_or("?", |s| s.name()),
                            page.title,
                            d.to_string().replace('\n', " ")
                        );
                    }
                }
                Err(_) => eprintln!(
                    "triplespace-ui: compare: the skin failed for {}",
                    page.title
                ),
            }
            Ok(b)
        }
    }
}

/// A region for `action=render` (0057 §8): its HTML alone, with the caching of the page
/// it belongs to.
pub fn fragment(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    build: impl FnOnce() -> String,
) -> Response {
    let (etag, fresh) = revalidated(cx, headers, status);
    if let Some(r) = fresh {
        return r;
    }
    let mut r = Response::new(Body::from(build()));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    caching(r.headers_mut(), etag.as_deref(), &cx.inputs);
    r
}

/// A component built for another release asked for a region: `409` with no body, so it
/// reloads the page instead of swapping in markup its styles do not match (0057 §8).
#[must_use]
pub fn build_skew() -> Response {
    (StatusCode::CONFLICT, [(header::CACHE_CONTROL, "no-store")]).into_response()
}

/// `400` for a region no page has, MediaWiki-shaped.
#[must_use]
pub fn unknown_region(region: &str) -> Response {
    let body = serde_json::json!({"error": {
        "code": "badregion",
        "info": format!("Unrecognized value for parameter \"region\": {region}."),
    }})
    .to_string();
    (
        StatusCode::BAD_REQUEST,
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        body,
    )
        .into_response()
}

/// The main page's title when the API does not say (a stock Wikibase says `Main Page`).
pub const DEFAULT_MAINPAGE: &str = "Project:Home";

/// A title from a URL: underscores as spaces; empty for the main page.
fn display_title(raw: &str) -> String {
    raw.replace('_', " ").trim().to_string()
}

/// A title with its first letter upper-cased, as MediaWiki's titles have it.
fn first_upper(t: &str) -> String {
    let mut c = t.chars();
    c.next()
        .map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

/// A title with the project namespace written canonically: `librarybase:Home` and
/// `project:home` are `Project:Home`.
pub(crate) fn canonical(title: &str, sitename: &str) -> String {
    let t = title.replace('_', " ");
    if let Some((ns, rest)) = t.split_once(':')
        && (ns.trim().eq_ignore_ascii_case("project") || ns.trim().eq_ignore_ascii_case(sitename))
    {
        return format!("Project:{}", first_upper(rest.trim()));
    }
    first_upper(t.trim())
}

/// Whether a title is the main page's.
#[must_use]
pub fn is_mainpage(title: &str, site: &SiteInfo) -> bool {
    canonical(title, &site.sitename) == canonical(&site.mainpage, &site.sitename)
}

/// A `302` to a page, cached as its inputs allow.
pub fn redirect_to(cx: &Context, title: &str) -> Response {
    redirect_to_with(cx, title, &BTreeMap::new())
}

/// A `302` to a title's page, keeping the request's query (`?tab=…`, `?diff=…`).
pub fn redirect_to_with(cx: &Context, title: &str, query: &BTreeMap<String, String>) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::FOUND;
    let mut location = format!("/wiki/{}", title_url(title));
    for (i, (k, v)) in query.iter().enumerate() {
        location.push(if i == 0 { '?' } else { '&' });
        location.push_str(
            &percent_encoding::utf8_percent_encode(k, percent_encoding::NON_ALPHANUMERIC)
                .to_string(),
        );
        location.push('=');
        location.push_str(
            &percent_encoding::utf8_percent_encode(v, percent_encoding::NON_ALPHANUMERIC)
                .to_string(),
        );
    }
    if let Ok(v) = HeaderValue::from_str(&location) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    caching(r.headers_mut(), None, &cx.inputs);
    r
}

/// The `404` for a title the site has no page for: a special page it does not serve, or,
/// where the API serves no pages, any other title (0008).
pub(crate) fn no_page(cx: &Context, headers: &HeaderMap, title: String) -> Response {
    let special = title.starts_with("Special:");
    if cx.site.mainpage != "Main Page" && title.eq_ignore_ascii_case("Main Page") {
        return redirect_to(cx, &cx.site.mainpage.clone());
    }
    respond(cx, headers, StatusCode::NOT_FOUND, |m| {
        let text = if special {
            m.with("ts-no-special-page", &[&title])
        } else {
            m.with("ts-no-page", &[&title])
        };
        Page {
            returnto: Some(title.clone()),
            title,
            body: message(MessageKind::Notice, &text),
            ..Page::default()
        }
    })
}

/// A request's query or form: by name, the last value of each, and in order, every
/// value, for the few parameters that repeat (`statement=` on `Special:NewItem`).
#[derive(Debug, Clone, Default)]
pub struct Params {
    /// The last value of each name.
    pub map: BTreeMap<String, String>,
    /// Every pair, in order.
    pub pairs: Vec<(String, String)>,
}

impl Params {
    /// From the pairs, in order.
    #[must_use]
    pub fn new(pairs: Vec<(String, String)>) -> Self {
        Self {
            map: pairs.iter().cloned().collect(),
            pairs,
        }
    }

    /// Every value of a name, in order.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.pairs
            .iter()
            .filter(move |(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Which moderation tabs a page offers (see [`Context::moderation_tabs`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModerationTabs {
    /// **Protect**.
    pub protect: bool,
    /// **Delete**.
    pub delete: bool,
    /// **Move**.
    pub r#move: bool,
}

/// A title: an entity's page, a special page, the main page, or the `404`.
async fn dispatch(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: String,
    params: &Params,
) -> Response {
    let query = &params.map;
    if let Some(form) = query.get("action").and_then(|a| moderation::Form::of(a))
        && !title.is_empty()
        && special::name_of(&title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return moderation::serve(site, headers, &incoming, cx, form, title, params).await;
    }
    if entity::is_entity_title(&title) {
        return entity::serve(site, headers, peer, title, query).await;
    }
    if let Some(r) = special::serve(site, headers, peer, &title, params).await {
        return r;
    }
    let incoming = incoming(headers, peer);
    let cx = match context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if cx.features.has(Feature::Pages) {
        // A talk page or a thread (19 §6.1); every other title is a document page's.
        let cx = match crate::discussion::serve(site, headers, &incoming, cx, &title, params).await
        {
            Ok(r) => return r,
            Err(cx) => cx,
        };
        return document::serve(site, headers, &incoming, cx, title, params).await;
    }
    if title.is_empty() {
        return redirect_to(&cx, &cx.site.mainpage);
    }
    if is_mainpage(&title, &cx.site) {
        if query.get("action").is_some_and(|a| a == "render") {
            return fragment(&cx, headers, StatusCode::OK, || home::body(&cx));
        }
        return respond(&cx, headers, StatusCode::OK, |_| home::page(&cx));
    }
    if query.get("action").is_some_and(|a| a == "render") {
        return fragment(&cx, headers, StatusCode::NOT_FOUND, String::new);
    }
    no_page(&cx, headers, title)
}

/// Relays the API's `Set-Cookie` headers on a response, which is then the viewer's alone
/// and kept by no one (0057 §4, §6).
#[must_use]
pub fn with_cookies(mut r: Response, cookies: &[HeaderValue]) -> Response {
    if cookies.is_empty() {
        return r;
    }
    let h = r.headers_mut();
    for c in cookies {
        h.append(header::SET_COOKIE, c.clone());
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.remove("cache-tag");
    h.remove(header::ETAG);
    r
}

/// A `303` after a form: on to `href`, with the cookies the API set.
#[must_use]
pub fn see_other(href: &str, cookies: &[HeaderValue]) -> Response {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = StatusCode::SEE_OTHER;
    if let Ok(v) = HeaderValue::from_str(href) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    with_cookies(r, cookies)
}

/// A form posted to a title: a document page's edit form, a special page that takes one,
/// or `405`.
async fn dispatch_post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let action = query.get("action").or_else(|| form.map.get("action"));
    if let Some(which) = action.and_then(|a| moderation::Form::of(a))
        && !title.is_empty()
        && special::name_of(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return moderation::post(site, headers, &incoming, cx, which, title.to_string(), form)
            .await;
    }
    if !title.is_empty()
        && special::name_of(title).is_none()
        && !entity::is_entity_title(title)
        && action.is_some_and(|a| a == "submit" || a == "edit")
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        if cx.features.has(Feature::Pages) {
            return document::post(site, headers, &incoming, cx, title, form).await;
        }
        return no_page(&cx, headers, title.to_string());
    }
    if let Some(a) = action.filter(|a| *a == "watch" || *a == "unwatch")
        && !title.is_empty()
        && special::name_of(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        return crate::watch::post(site, headers, &incoming, cx, title, a, form).await;
    }
    if let Some(a) = action.filter(|a| *a == "addtopic" || *a == "reply")
        && !title.is_empty()
        && special::name_of(title).is_none()
    {
        let incoming = incoming(headers, peer);
        let cx = match context(site, &incoming, query).await {
            Ok(cx) => cx,
            Err(r) => return r,
        };
        if cx.user.is_none() || !cx.features.has(Feature::Pages) {
            return no_page(&cx, headers, title.to_string());
        }
        return match crate::discussion::post(site, headers, &incoming, cx, title, a, form).await {
            Ok(r) => r,
            Err(cx) => no_page(&cx, headers, title.to_string()),
        };
    }
    if let Some(r) = special::post(site, headers, peer, title, query, form).await {
        return r;
    }
    (
        StatusCode::METHOD_NOT_ALLOWED,
        [
            (header::ALLOW, "GET, HEAD"),
            (header::CACHE_CONTROL, "no-store"),
        ],
    )
        .into_response()
}

/// `POST /wiki/{title}`.
pub async fn wiki_post(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let form = Params::new(form);
    dispatch_post(&site, &headers, peer, &display_title(&title), &query, &form).await
}

/// `POST /w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_post(
    State(site): State<Site>,
    Query(query): Query<BTreeMap<String, String>>,
    headers: HeaderMap,
    peer: Peer,
    Form(form): Form<Vec<(String, String)>>,
) -> Response {
    let form = Params::new(form);
    let title = display_title(
        query
            .get("title")
            .or_else(|| form.map.get("title"))
            .map_or("", String::as_str),
    );
    dispatch_post(&site, &headers, peer, &title, &query, &form).await
}

/// `/`: a redirect to the main page.
pub async fn root(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    dispatch(&site, &headers, peer, String::new(), &Params::default()).await
}

/// `/wiki/{title}`.
pub async fn wiki(
    State(site): State<Site>,
    Path(title): Path<String>,
    Query(query): Query<Vec<(String, String)>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    dispatch(
        &site,
        &headers,
        peer,
        display_title(&title),
        &Params::new(query),
    )
    .await
}

/// `/w/index.php` and `/index.php`: the title in `title=`.
pub async fn index_php(
    State(site): State<Site>,
    Query(query): Query<Vec<(String, String)>>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let params = Params::new(query);
    let title = display_title(params.map.get("title").map_or("", String::as_str));
    dispatch(&site, &headers, peer, title, &params).await
}

/// Everything else: the site's own 404.
pub async fn not_found(State(site): State<Site>, headers: HeaderMap, peer: Peer) -> Response {
    let incoming = incoming(&headers, peer);
    let cx = match context(&site, &incoming, &BTreeMap::new()).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    respond(&cx, &headers, StatusCode::NOT_FOUND, |m| Page {
        title: m.get("ts-not-found-title"),
        body: message(MessageKind::Warning, &m.get("ts-not-found")),
        ..Page::default()
    })
}

/// `/ui/assets/{path}`: a file of the build, kept for a year, since its name is its hash.
pub async fn asset(Path(path): Path<String>) -> Response {
    match assets::get(&path) {
        Some((data, content_type)) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            ],
            data.into_owned(),
        )
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not found\n").into_response(),
    }
}

/// `/ui/skins/{skin}/{hash}/{path}`: a file of a loaded skin, content-hashed in its URL
/// (19 §5.10). A stale hash gets the current file for a minute, as the theme's
/// stylesheets do.
pub async fn skin_asset(
    State(site): State<Site>,
    Path((skin, hash, path)): Path<(String, String, String)>,
) -> Response {
    let Some(s) = site.skins().get(&skin) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let Some(data) = s.file(&path) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let cache = if hash == s.hash() {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    (
        [
            (header::CONTENT_TYPE, assets::content_type(&path)),
            (header::CACHE_CONTROL, cache),
        ],
        data.to_vec(),
    )
        .into_response()
}

/// `/ui/theme/{p|t}-{name}-{hash}.css`: a palette's or a type set's stylesheet (0086 §5).
/// Kept for a year when the name is the hash of the text served; a stale name gets the
/// current text for a minute, so that a page cached across a change still looks right.
/// A name from before palettes gets the wiki's palette, briefly.
pub async fn theme_css(
    State(site): State<Site>,
    Path(file): Path<String>,
    headers: HeaderMap,
    peer: Peer,
) -> Response {
    let incoming = incoming(&headers, peer);
    let theme = match site.client().siteinfo(&incoming).await {
        Ok(si) => Theme::from_info(si.value.theme.as_ref()).with_chips(&si.value.providers),
        Err(e) => {
            eprintln!("triplespace-ui: the API is unavailable: {e}");
            Theme::shipped()
        }
    };
    let Some((css, current)) = theme.file(&file) else {
        return (StatusCode::NOT_FOUND, "Not found\n").into_response();
    };
    let cache = if current {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    };
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, cache),
        ],
        css,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(public: bool, etag: Option<&str>, tags: &[&str]) -> CacheInfo {
        CacheInfo {
            public,
            etag: etag.map(str::to_string),
            tags: tags.iter().map(|t| (*t).to_string()).collect(),
        }
    }

    #[test]
    fn a_page_is_public_only_if_every_input_was() {
        let mut h = HeaderMap::new();
        caching(
            &mut h,
            Some("\"x\""),
            &[
                info(true, Some("\"a\""), &["entity:Q2"]),
                info(true, Some("\"b\""), &["entity:Q1", "entity:Q2"]),
            ],
        );
        assert!(
            h[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public")
        );
        assert_eq!(h["cache-tag"], "entity:Q1, entity:Q2");
        assert_eq!(h[header::VARY], "Cookie, Accept-Language");
        let mut h = HeaderMap::new();
        caching(
            &mut h,
            None,
            &[info(true, None, &["entity:Q2"]), info(false, None, &[])],
        );
        assert_eq!(h[header::CACHE_CONTROL], "private, no-cache");
        assert!(h.get("cache-tag").is_none());
        assert!(h.get(header::ETAG).is_none());
    }

    #[test]
    fn the_composed_etag_needs_every_input_etag() {
        let m = Messages::for_language("en");
        let a = composed_etag(&m, StatusCode::OK, &[info(true, Some("\"a\""), &[])], None).unwrap();
        let b = composed_etag(&m, StatusCode::OK, &[info(true, Some("\"b\""), &[])], None).unwrap();
        let n = composed_etag(
            &m,
            StatusCode::NOT_FOUND,
            &[info(true, Some("\"a\""), &[])],
            None,
        )
        .unwrap();
        assert_ne!(a, b);
        assert_ne!(a, n);
        assert!(composed_etag(&m, StatusCode::OK, &[info(true, None, &[])], None).is_none());
        // The skin that drew the page is part of the build it hashes (0097 §15).
        let worksheet = composed_etag(
            &m,
            StatusCode::OK,
            &[info(true, Some("\"a\""), &[])],
            Some("worksheet 0.1.0"),
        )
        .unwrap();
        let plain = composed_etag(
            &m,
            StatusCode::OK,
            &[info(true, Some("\"a\""), &[])],
            Some("plain 0.1.0"),
        )
        .unwrap();
        assert_ne!(worksheet, plain);
        assert_ne!(worksheet, a);
        let mut req = HeaderMap::new();
        req.insert(header::IF_NONE_MATCH, format!("W/{a}").parse().unwrap());
        assert!(not_modified(&req, &a));
        assert!(!not_modified(&req, &b));
    }

    #[test]
    fn the_policy_allows_style_attributes_only() {
        assert!(CSP.contains("style-src 'self';"));
        assert!(CSP.contains("style-src-attr 'unsafe-inline';"));
        assert!(!CSP.contains("script-src 'self' 'unsafe-inline'"));
        assert!(!CSP.contains("style-src-elem"));
    }

    #[test]
    fn titles() {
        assert_eq!(display_title("Main_Page"), "Main Page");
        assert_eq!(display_title(""), "");
        let site = SiteInfo {
            sitename: "librarybase".into(),
            mainpage: "Project:Home".into(),
            ..SiteInfo::default()
        };
        for t in [
            "Project:Home",
            "project:home",
            "librarybase:Home",
            "Project:_Home",
        ] {
            assert!(is_mainpage(t, &site), "{t}");
        }
        assert!(!is_mainpage("Home", &site));
        assert!(!is_mainpage("Main Page", &site));
    }
}
