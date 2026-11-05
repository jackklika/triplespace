//! Logging in through external issuers on the site (0088 §5–§7; 19 §2.3, §2.4), without
//! JavaScript.
//!
//! - **Buttons.** `Special:UserLogin` and `Special:CreateAccount` offer one form per
//!   issuer the wiki offers, posting the session's login token straight to the API's
//!   `/auth/{issuer}/start`; the provider returns to the API, which redirects back here.
//! - **Answers.** The API comes back with `tserror` or `tsnotice` in the query, which the
//!   pages show as a message.
//! - **`Special:Account`**, its Sign-in methods section: each binding with when it was
//!   added and last used, **Remove** (disabled for the last way to log in), and **Add**,
//!   a form to `/auth/{issuer}/start` with `purpose=link` and the CSRF token.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::issuers::start_path;
use triplespace_client::{AccountInfo, CacheInfo, LoginIssuer};

use crate::Site;
use crate::codex::{Action, Button, Message, MessageKind, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::link_to;
use crate::pages::{self, Context, Peer};

/// The issuers' buttons as data (19 §5.10; 0097 A6, A11): `account.html`'s `issuers`
/// draws them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssuerButtons {
    /// What the section is for: `login` or `link`, which also names its heading's id.
    pub purpose: String,
    /// The heading.
    pub heading: String,
    /// The field the token goes in.
    pub token_field: String,
    /// The token.
    pub token: String,
    /// Where the API sends the reader back to.
    pub back: String,
    /// One form per issuer.
    pub forms: Vec<IssuerForm>,
    /// The note under the buttons.
    pub note: String,
}

/// One issuer's form.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct IssuerForm {
    /// The API's start route for the issuer.
    pub action: String,
    /// Its button.
    pub button: crate::codex::ButtonModel,
}

/// The issuers' buttons, or nothing when the wiki offers none.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn issuer_buttons(
    m: &Messages,
    issuers: &[LoginIssuer],
    token_field: &str,
    token: &str,
    purpose: &str,
    back: &str,
    heading_key: &str,
    button_key: &str,
) -> Option<IssuerButtons> {
    (!issuers.is_empty()).then(|| IssuerButtons {
        purpose: purpose.to_string(),
        heading: m.get(heading_key),
        token_field: token_field.to_string(),
        token: token.to_string(),
        back: back.to_string(),
        forms: issuers
            .iter()
            .map(|i| IssuerForm {
                action: start_path(&i.issuer),
                button: Button {
                    label: &m.with(button_key, &[&i.label]),
                    kind: "submit",
                    action: Action::Default,
                    weight: Weight::Normal,
                }
                .model(),
            })
            .collect(),
        note: m.get("ts-login-external-note"),
    })
}

/// The site's policy with `form-action` widened to the authorization origins of the
/// issuers a page's buttons post for (0088 §5): the start route answers `303` to the
/// provider, and a browser applies `form-action` to a form's redirects.
#[must_use]
pub fn csp_for(issuers: &[LoginIssuer]) -> Option<String> {
    let mut origins: Vec<String> = issuers
        .iter()
        .filter_map(|i| i.origin.as_deref())
        .filter_map(|o| url::Url::parse(o).ok())
        .filter(|u| u.scheme() == "https")
        .map(|u| u.origin().ascii_serialization())
        .collect();
    origins.sort();
    origins.dedup();
    if origins.is_empty() {
        return None;
    }
    Some(pages::CSP.replace(
        "form-action 'self'",
        &format!("form-action 'self' {}", origins.join(" ")),
    ))
}

/// A page response that carries issuers' buttons, with its `form-action` widened.
#[must_use]
pub fn allow_issuer_forms(cx: &Context, mut r: Response) -> Response {
    if let Some(csp) = csp_for(&cx.site.login_issuers)
        && let Ok(v) = axum::http::HeaderValue::from_str(&csp)
    {
        r.headers_mut()
            .insert(axum::http::header::CONTENT_SECURITY_POLICY, v);
    }
    r
}

/// The message key of an answer code the API sends back in `tserror` or `tsnotice`.
fn key_of(code: &str) -> Option<&'static str> {
    Some(match code {
        "ts-oauth-denied" => "ts-oauth-denied",
        "ts-oauth-state" => "ts-oauth-state",
        "ts-oauth-failed" => "ts-oauth-failed",
        "ts-oauth-unavailable" => "ts-oauth-unavailable",
        "ts-oauth-bound" => "ts-oauth-bound",
        "ts-oauth-bound-elsewhere" => "ts-oauth-bound-elsewhere",
        "ts-last-binding" => "ts-last-binding",
        "ts-reclaimed" => "ts-reclaimed",
        "ts-reclaim-taken" => "ts-reclaim-taken",
        "ts-reclaim-unavailable" => "ts-reclaim-unavailable",
        "ts-account-pending" => "ts-login-pending",
        "ts-account-rejected" => "ts-login-rejected-external",
        "ts-account-requested" => "ts-createaccount-requested",
        "ts-linked" => "ts-account-linked",
        "ts-unlinked" => "ts-account-unlinked",
        "userexists" => "ts-createaccount-userexists",
        "acct_creation_throttle_hit" => "ts-createaccount-throttle",
        _ => return None,
    })
}

/// The message an answer in the query asks for: `tserror` as an error, `tsnotice` as a
/// notice. An unknown code shows nothing.
#[must_use]
pub fn answer(m: &Messages, query: &BTreeMap<String, String>) -> Option<(MessageKind, String)> {
    if let Some(k) = query.get("tserror").and_then(|c| key_of(c)) {
        return Some((MessageKind::Error, m.get(k)));
    }
    query
        .get("tsnotice")
        .and_then(|c| key_of(c))
        .map(|k| (MessageKind::Success, m.get(k)))
}

/// `YYYY-MM-DD` of seconds since the epoch, in UTC (the civil-from-days algorithm).
#[must_use]
pub fn iso_date(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}")
}

const TITLE: &str = "Special:Account";

/// `Special:Account` as data (19 §5.10): `special/account.html` draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccountPage {
    /// What the API answered, above the page.
    pub notice: Option<crate::codex::MessageModel>,
    /// For a reader not logged in: why there is nothing here.
    pub login_needed: Option<String>,
    /// And the way to log in, where the reader has no session.
    pub login: Option<Choice>,
    /// The Sign-in methods section.
    pub methods: Option<SignInMethods>,
}

/// The Sign-in methods section.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SignInMethods {
    /// The heading.
    pub heading: String,
    /// That the section is private to the reader.
    pub private: String,
    /// The password's item, where the account has one.
    pub password: Option<String>,
    /// The bindings, each with its remove form.
    pub bindings: Vec<Binding>,
    /// Where the remove forms post.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// Remove, a destructive button; disabled for the last way to log in.
    pub remove: crate::codex::ButtonModel,
    /// Whether this is the last way to log in.
    pub last: bool,
    /// The note under the list: why Remove is disabled, or what removing does.
    pub note: Option<String>,
    /// Ways still to add.
    pub add: Option<IssuerButtons>,
}

/// One binding.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Binding {
    /// The issuer's code.
    pub issuer: String,
    /// Its name.
    pub label: String,
    /// When it was added and last used.
    pub dates: String,
    /// That the wiki no longer offers it.
    pub unavailable: Option<String>,
}

/// The Sign-in methods section's model.
fn sign_in_model(m: &Messages, a: &AccountInfo, token: &str) -> SignInMethods {
    let last = a.ways <= 1;
    SignInMethods {
        heading: m.get("ts-account-signin-heading"),
        private: m.get("ts-account-private"),
        password: a.password.then(|| m.get("ts-account-password")),
        bindings: a
            .bindings
            .iter()
            .map(|b| {
                let used = b.last_used.map_or_else(
                    || m.get("ts-account-never-used"),
                    |t| m.with("ts-account-last-used", &[&iso_date(t)]),
                );
                Binding {
                    issuer: b.issuer.clone(),
                    label: b.label.clone(),
                    dates: format!(
                        "{} · {used}",
                        m.with("ts-account-added", &[&iso_date(b.added)])
                    ),
                    unavailable: (!b.available).then(|| m.get("ts-account-unavailable")),
                }
            })
            .collect(),
        action: format!("/w/index.php?title={}", title_url(TITLE)),
        token: token.to_string(),
        remove: Button {
            label: &m.get("ts-account-remove"),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Normal,
        }
        .model(),
        last,
        note: if last {
            Some(m.get("ts-account-remove-last"))
        } else if a.bindings.is_empty() {
            None
        } else {
            Some(m.get("ts-account-remove-note"))
        },
        add: issuer_buttons(
            m,
            &a.can_add,
            "token",
            token,
            "link",
            TITLE,
            "ts-account-add-heading",
            "ts-account-add-button",
        ),
    }
}

fn account_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    mut model: AccountPage,
) -> Response {
    model.notice = notice.map(|(k, t)| Message::block(k, &t).model());
    let body = cx.draw("special/account.html", &model);
    allow_issuer_forms(
        cx,
        pages::respond(cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-account-title"),
            body,
            ..Page::default()
        }),
    )
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Result<(Context, triplespace_client::Incoming), Response> {
    let incoming = pages::incoming(headers, peer);
    let mut cx = pages::context(site, &incoming, query).await?;
    if !cx.features.has(Feature::Login) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok((cx, incoming))
}

/// The page, with a fresh CSRF token.
async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    notice: Option<(MessageKind, String)>,
) -> Response {
    if cx.user.is_none() {
        let model = AccountPage {
            notice: None,
            login_needed: Some(cx.m.get("ts-account-login-needed")),
            login: Some(link_to(
                format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE)),
                cx.m.get("ts-login"),
            )),
            methods: None,
        };
        return account_page(cx, headers, notice, model);
    }
    let account = match site.client().account(incoming).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            let model = AccountPage {
                notice: None,
                login_needed: Some(cx.m.get("ts-account-login-needed")),
                login: None,
                methods: None,
            };
            return account_page(cx, headers, notice, model);
        }
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let t = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let model = AccountPage {
        notice: None,
        login_needed: None,
        login: None,
        methods: Some(sign_in_model(&cx.m, &account, &t.value.value)),
    };
    pages::with_cookies(account_page(cx, headers, notice, model), &t.set_cookies)
}

/// `GET Special:Account`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let (mut cx, incoming) = match start(site, headers, peer, query).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let notice = answer(&cx.m, query);
    render(site, &mut cx, headers, &incoming, notice).await
}

/// `POST Special:Account`: removes a sign-in method.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let (mut cx, incoming) = match start(site, headers, peer, query).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    match site
        .client()
        .unlink(&incoming, field("issuer"), field("token"))
        .await
    {
        Ok(Ok(())) => pages::see_other(
            &format!(
                "/wiki/{}?tsnotice=ts-unlinked#sign-in-methods",
                title_url(TITLE)
            ),
            &[],
        ),
        Ok(Err(code)) => {
            let text = match code.as_str() {
                "ts-last-binding" => cx.m.get("ts-last-binding"),
                "badtoken" => cx.m.get("ts-login-expired"),
                _ => cx.m.get("ts-account-remove-failed"),
            };
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(iso_date(0), "1970-01-01");
        assert_eq!(iso_date(1_792_281_600), "2026-10-18");
        assert_eq!(iso_date(951_782_400), "2000-02-29");
    }

    /// What Plain draws: the issuers' buttons alone, or Special:Account.
    fn drawn(m: &Messages, template: &str, model: impl serde::Serialize) -> String {
        let skins = crate::skin::Skins::load(None);
        let plain = skins.get("plain").expect("plain");
        if template == "issuers" {
            plain
                .render_str(
                    "{% import \"account.html\" as account %}{{ account.issuers(x) }}",
                    m,
                    serde_json::json!({ "x": model }),
                )
                .unwrap()
        } else {
            plain.render(template, m, model).unwrap()
        }
    }

    #[test]
    fn buttons_post_to_the_api() {
        let m = Messages::for_language("en");
        let model = issuer_buttons(
            &m,
            &[LoginIssuer {
                issuer: "wikimedia-central".into(),
                label: "Wikimedia".into(),
                origin: None,
            }],
            "logintoken",
            "tok+\\",
            "login",
            "Main Page",
            "ts-login-external-heading",
            "ts-login-external-button",
        );
        let html = drawn(&m, "issuers", model);
        assert!(
            html.contains("action=\"/w/rest.php/triplespace/v0/auth/wikimedia-central/start\""),
            "{html}"
        );
        assert!(
            html.contains("name=\"logintoken\" value=\"tok+\\\""),
            "{html}"
        );
        assert!(html.contains("Log in with Wikimedia"));
        assert!(issuer_buttons(&m, &[], "t", "t", "login", "x", "a", "b").is_none());
    }

    #[test]
    fn form_action_widens_to_https_origins_only() {
        let i = |o: &str| LoginIssuer {
            issuer: "x".into(),
            label: "X".into(),
            origin: Some(o.into()),
        };
        let csp = csp_for(&[
            i("https://meta.wikimedia.org"),
            i("https://meta.wikimedia.org"),
            i("http://insecure.example"),
        ])
        .unwrap();
        assert!(
            csp.ends_with("form-action 'self' https://meta.wikimedia.org"),
            "{csp}"
        );
        assert_eq!(csp_for(&[]), None);
        assert_eq!(csp_for(&[i("javascript:alert(1)")]), None);
    }

    #[test]
    fn answers_are_known_codes_only() {
        let m = Messages::for_language("en");
        let q = |k: &str, v: &str| BTreeMap::from([(k.to_string(), v.to_string())]);
        assert!(matches!(
            answer(&m, &q("tserror", "ts-oauth-denied")),
            Some((MessageKind::Error, _))
        ));
        assert!(matches!(
            answer(&m, &q("tsnotice", "ts-linked")),
            Some((MessageKind::Success, _))
        ));
        assert!(answer(&m, &q("tserror", "<script>")).is_none());
    }

    /// Special:Account's sign-in methods: Remove disabled for the last way to log in, with
    /// the note that says why; a binding the wiki no longer offers says so.
    #[test]
    fn the_sign_in_methods() {
        let m = Messages::for_language("en");
        let binding = |code: &str, available: bool| triplespace_client::issuers::Binding {
            issuer: code.into(),
            label: code.to_uppercase(),
            added: 1_792_281_600,
            last_used: None,
            available,
        };
        let page = |a: &AccountInfo| AccountPage {
            notice: None,
            login_needed: None,
            login: None,
            methods: Some(sign_in_model(&m, a, "t&k")),
        };
        let last = AccountInfo {
            name: None,
            password: false,
            bindings: vec![binding("orcid", true)],
            can_add: Vec::new(),
            ways: 1,
        };
        let html = drawn(&m, "special/account.html", page(&last));
        assert!(
            html.contains("disabled aria-describedby=\"ts-signin-last\""),
            "{html}"
        );
        assert!(html.contains("id=\"ts-signin-last\""), "{html}");
        assert!(html.contains("2026-10-18"), "{html}");
        assert!(html.contains("value=\"t&amp;k\""), "{html}");
        let two = AccountInfo {
            password: true,
            bindings: vec![binding("orcid", true), binding("gone", false)],
            can_add: vec![LoginIssuer {
                issuer: "wikimedia-central".into(),
                label: "Wikimedia".into(),
                origin: None,
            }],
            ways: 3,
            ..last
        };
        let html = drawn(&m, "special/account.html", page(&two));
        assert!(!html.contains(" disabled"), "{html}");
        assert!(html.contains("cdx-button--action-destructive"), "{html}");
        assert!(html.contains("ts-signin__unavailable"), "{html}");
        assert!(html.contains("ts-signin__note"), "{html}");
        assert!(html.contains("Add Wikimedia"), "{html}");
    }
}
