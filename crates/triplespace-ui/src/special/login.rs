//! `Special:UserLogin` and `Special:UserLogout` (0010 §10; 0047 §9; 0057 §4), without
//! JavaScript.
//!
//! - **Log in.** The form carries the API's login token for the viewer's session. The
//!   site posts the form to `clientlogin` with the browser's cookie, relays the API's
//!   `Set-Cookie` verbatim, and answers `303` to the page the reader came from (post,
//!   redirect, get). A refusal shows the form again, with the name kept; an expired
//!   token gets a fresh one. After repeated failures from the viewer's address, the form
//!   carries the `badlogin` challenge the browser solves (0087 §2, §5); the holder of an
//!   account request is told how it stands (0087 §6).
//! - **Log out** is a form too, as in MediaWiki: a `GET` only asks, and the `POST`
//!   carries the CSRF token to `logout`.
//!
//! A page holding a token is never cacheable by anyone but the viewer, and every
//! response that relays a cookie is `private, no-store` (0057 §6; plan §6 Phase 6).

use std::collections::BTreeMap;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::{CacheInfo, ChallengeInfo, ClientError, Incoming, LoginResult};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, Field, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};
use crate::special::register::{ChallengeModel, challenge_model};

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/login.html` and `special/logout.html` draw.
// ---------------------------------------------------------------------------------------

/// The login form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginForm {
    /// The challenge's difficulty, where one is asked for: the form's `data-ts-challenge`.
    pub challenge_bits: Option<u64>,
    /// The API's login token.
    pub token: String,
    /// The page to return to.
    pub returnto: String,
    /// The name.
    pub username: FieldModel,
    /// The password.
    pub password: FieldModel,
    /// The challenge's part of the form.
    pub challenge: Option<ChallengeModel>,
    /// **Log in**.
    pub submit: ButtonModel,
}

/// `Special:UserLogin`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginPage {
    /// A notice above the form: a refusal, an expired token, how a request stands.
    pub notice: Option<MessageModel>,
    /// The form.
    pub form: Option<LoginForm>,
    /// The external issuers' buttons, as `external.rs` draws them until group 12 switches
    /// (0097 A6).
    pub issuers_html: Option<String>,
    /// For a viewer already logged in, the way on.
    pub continue_link: Option<Choice>,
    /// The way to create an account, where the wiki takes registrations.
    pub join: Option<Choice>,
}

/// The logout form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoutForm {
    /// "Do you want to log out?"
    pub question: String,
    /// The CSRF token.
    pub token: String,
    /// The page to return to.
    pub returnto: String,
    /// **Log out**.
    pub submit: ButtonModel,
}

/// `Special:UserLogout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LogoutPage {
    /// A notice: an expired token.
    pub notice: Option<MessageModel>,
    /// That no one is logged in.
    pub not_logged_in: Option<String>,
    /// For a viewer not logged in, the way on.
    pub continue_link: Option<Choice>,
    /// The form.
    pub form: Option<LogoutForm>,
}

/// What a login page shows beneath its notice.
enum LoginBody {
    /// The form, with its token, the name kept and any challenge.
    Form {
        token: String,
        username: String,
        challenge: Option<ChallengeInfo>,
    },
    /// The viewer is logged in already: a notice and the way on.
    Already(MessageKind, String),
}

fn link_choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn login_form_model(
    m: &Messages,
    token: &str,
    back: &str,
    username: &str,
    challenge: Option<&ChallengeInfo>,
) -> LoginForm {
    let field = |id: &str, name: &str, kind: &str, value: &str, key: &str, auto: &str| {
        Field {
            id,
            name,
            kind,
            value,
            label: &m.get(key),
            description: None,
            help: None,
            required: true,
            autocomplete: Some(auto),
        }
        .model()
    };
    LoginForm {
        challenge_bits: challenge.map(|c| c.bits),
        token: token.to_string(),
        returnto: back.to_string(),
        username: field(
            "wpName1",
            "username",
            "text",
            username,
            "ts-login-username",
            "username",
        ),
        password: field(
            "wpPassword1",
            "password",
            "password",
            "",
            "ts-login-password",
            "current-password",
        ),
        challenge: challenge_model(m, challenge, "", "ts-challenge-nojs-login"),
        submit: Button {
            label: &m.get("ts-login-submit"),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
    }
}

fn issuers_html(cx: &Context, token: &str, back: &str) -> Option<String> {
    let html = crate::special::external::buttons(
        &cx.m,
        &cx.site.login_issuers,
        "logintoken",
        token,
        "login",
        back,
        "ts-login-external-heading",
        "ts-login-external-button",
    );
    (!html.is_empty()).then_some(html)
}

fn join_link(cx: &Context, back: &str) -> Option<Choice> {
    (cx.user.is_none() && cx.features.has(Feature::Register)).then(|| {
        link_choice(
            format!("/wiki/Special:CreateAccount?returnto={}", title_url(back)),
            cx.m.get("ts-login-join"),
        )
    })
}

/// The login page's model.
fn login_model(
    cx: &Context,
    notice: Option<(MessageKind, String)>,
    body: &LoginBody,
    back: &str,
) -> LoginPage {
    let m = &cx.m;
    match body {
        LoginBody::Form {
            token,
            username,
            challenge,
        } => LoginPage {
            notice: notice.map(|(k, t)| Message::block(k, &t).model()),
            form: Some(login_form_model(
                m,
                token,
                back,
                username,
                challenge.as_ref(),
            )),
            issuers_html: issuers_html(cx, token, back),
            continue_link: None,
            join: join_link(cx, back),
        },
        LoginBody::Already(kind, text) => LoginPage {
            notice: Some(Message::block(*kind, text).model()),
            form: None,
            issuers_html: None,
            continue_link: Some(link_choice(
                format!("/wiki/{}", title_url(back)),
                m.with("ts-login-continue", &[back]),
            )),
            join: join_link(cx, back),
        },
    }
}

/// The page to return to: `returnto` where it is a title, else the main page; never the
/// login or logout page itself.
fn returnto(cx: &Context, value: Option<&String>) -> String {
    let t = value.map_or("", |v| v.trim()).replace('_', " ");
    if t.is_empty()
        || t.contains("://")
        || t.starts_with('/')
        || t.to_ascii_lowercase().starts_with("special:userlog")
    {
        cx.site.mainpage.clone()
    } else {
        t
    }
}

/// The response that takes the reader on after a login or logout.
fn onward(returnto: &str, cookies: &[HeaderValue]) -> Response {
    pages::see_other(&format!("/wiki/{}", title_url(returnto)), cookies)
}

/// A page that holds a token is the viewer's alone.
fn private(cx: &mut Context) {
    cx.inputs.push(CacheInfo::default());
}

/// The login page: a notice, if any, the form, and where the wiki takes registrations,
/// the way to create an account.
fn login_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    body: &LoginBody,
    back: String,
) -> Response {
    let html = cx.draw("special/login.html", login_model(cx, notice, body, &back));
    crate::special::external::allow_issuer_forms(
        cx,
        pages::respond(cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-login-title"),
            returnto: Some(back),
            body: html,
            ..Page::default()
        }),
    )
}

/// A fresh login token and the form around it, with the cookies its session needs.
async fn fresh_form(
    site: &Site,
    incoming: &Incoming,
    username: &str,
) -> Result<(LoginBody, Vec<HeaderValue>), ClientError> {
    let f = site.client().login_form(incoming).await?;
    Ok((
        LoginBody::Form {
            token: f.value.token,
            username: username.to_string(),
            challenge: f.value.challenge,
        },
        f.set_cookies,
    ))
}

/// The context, or the response that ends the request: the API away, or logging in not
/// offered.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    title: &str,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Login) {
        return Err(pages::no_page(&cx, headers, title.to_string()));
    }
    private(&mut cx);
    Ok(cx)
}

/// `GET Special:UserLogin`.
pub async fn login(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, "Special:UserLogin").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    let answer = crate::special::external::answer(&cx.m, query);
    if let Some(name) = cx.user.clone() {
        let (kind, text) = match answer {
            Some((kind, text)) => (
                kind,
                format!("{text} {}", cx.m.with("ts-login-already", &[&name])),
            ),
            None => (MessageKind::Notice, cx.m.with("ts-login-already", &[&name])),
        };
        return login_page(&cx, headers, None, &LoginBody::Already(kind, text), back);
    }
    match fresh_form(site, &incoming, "").await {
        Ok((form, cookies)) => {
            pages::with_cookies(login_page(&cx, headers, answer, &form, back), &cookies)
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `POST Special:UserLogin`.
pub async fn post_login(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, "Special:UserLogin").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let (username, token) = (field("username").trim().to_string(), field("logintoken"));
    let return_url = format!(
        "{}/wiki/{}",
        cx.site.server.trim_end_matches('/'),
        title_url(&back)
    );
    let posted = crate::special::register::posted_challenge(form);
    let result = site
        .client()
        .client_login(
            &incoming,
            &username,
            field("password"),
            token,
            &return_url,
            posted.as_ref().map(|(i, s)| (i.as_str(), s.as_str())),
        )
        .await;
    let again = |cx: &Context, notice: (MessageKind, String), challenge: Option<&ChallengeInfo>| {
        login_page(
            cx,
            headers,
            Some(notice),
            &LoginBody::Form {
                token: token.to_string(),
                username: username.clone(),
                challenge: challenge.cloned(),
            },
            back.clone(),
        )
    };
    match result {
        Ok(r) => match r.value {
            LoginResult::Pass { .. } => onward(&back, &r.set_cookies),
            LoginResult::Fail { message, code } => {
                let text = match code.as_str() {
                    "wrongpassword" => cx.m.get("ts-login-wrongpassword"),
                    "ts-subsidiary-password" => cx.m.with("ts-login-subsidiary", &[&username]),
                    _ => message,
                };
                pages::with_cookies(again(&cx, (MessageKind::Error, text), None), &r.set_cookies)
            }
            LoginResult::Challenge(c) => {
                let key = if posted.as_ref().is_none_or(|(_, s)| s.is_empty()) {
                    "ts-challenge-nojs-login"
                } else {
                    "ts-challenge-retry"
                };
                let notice = (MessageKind::Error, cx.m.get(key));
                pages::with_cookies(again(&cx, notice, Some(&c)), &r.set_cookies)
            }
            LoginResult::Pending => {
                let notice = (MessageKind::Notice, cx.m.get("ts-login-pending"));
                pages::with_cookies(again(&cx, notice, None), &r.set_cookies)
            }
            LoginResult::Rejected { reason } => {
                let notice = (
                    MessageKind::Error,
                    cx.m.with("ts-login-rejected", &[&reason]),
                );
                pages::with_cookies(again(&cx, notice, None), &r.set_cookies)
            }
            LoginResult::BadToken => match fresh_form(site, &incoming, &username).await {
                Ok((f, cookies)) => {
                    let notice = Some((MessageKind::Warning, cx.m.get("ts-login-expired")));
                    let mut all = r.set_cookies;
                    all.extend(cookies);
                    pages::with_cookies(login_page(&cx, headers, notice, &f, back), &all)
                }
                Err(e) => pages::api_failed(&cx, headers, &e),
            },
        },
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The logout page's model: the notice, then the form, or that no one is logged in.
fn logout_model(m: &Messages, notice: Option<&str>, token: Option<&str>, back: &str) -> LogoutPage {
    LogoutPage {
        notice: notice.map(|n| Message::block(MessageKind::Warning, n).model()),
        not_logged_in: token.is_none().then(|| m.get("ts-logout-not-logged-in")),
        continue_link: token.is_none().then(|| {
            link_choice(
                format!("/wiki/{}", title_url(back)),
                m.with("ts-login-continue", &[back]),
            )
        }),
        form: token.map(|t| LogoutForm {
            question: m.get("ts-logout-question"),
            token: t.to_string(),
            returnto: back.to_string(),
            submit: Button {
                label: &m.get("ts-logout-submit"),
                kind: "submit",
                action: Action::Destructive,
                weight: Weight::Primary,
            }
            .model(),
        }),
    }
}

/// The logout page.
fn logout_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<&str>,
    token: Option<&str>,
    back: String,
) -> Response {
    let body = cx.draw(
        "special/logout.html",
        logout_model(&cx.m, notice, token, &back),
    );
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-logout-title"),
        returnto: Some(back),
        body,
        ..Page::default()
    })
}

/// A CSRF token for the logout form, or `None` when no one is logged in.
async fn logout_token(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
) -> Result<(Option<String>, Vec<HeaderValue>), ClientError> {
    if cx.user.is_none() {
        return Ok((None, Vec::new()));
    }
    let t = site.client().token(incoming, "csrf").await?;
    cx.inputs.push(t.value.cache);
    Ok((Some(t.value.value), t.set_cookies))
}

/// `GET Special:UserLogout`: asks.
pub async fn logout(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogout").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    match logout_token(site, &mut cx, &incoming).await {
        Ok((token, cookies)) => pages::with_cookies(
            logout_page(&cx, headers, None, token.as_deref(), back),
            &cookies,
        ),
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `POST Special:UserLogout`: logs out.
pub async fn post_logout(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, "Special:UserLogout").await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    let token = form.get("token").map_or("", String::as_str);
    match site.client().logout(&incoming, token).await {
        Ok(r) => onward(&back, &r.set_cookies),
        Err(ClientError::Api { code, .. }) if code == "badtoken" => {
            let notice = cx.m.get("ts-login-expired");
            match logout_token(site, &mut cx, &incoming).await {
                Ok((token, cookies)) => pages::with_cookies(
                    logout_page(&cx, headers, Some(&notice), token.as_deref(), back),
                    &cookies,
                ),
                Err(e) => pages::api_failed(&cx, headers, &e),
            }
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The login and logout models (19 §5.10): the form's fields and token, the challenge
    /// where one is asked for, and the logout page's two states.
    #[test]
    fn login_and_logout_are_models() {
        let m = Messages::for_language("en");
        let f = login_form_model(&m, "tok+\\", "Item:Q6", "Alice", None);
        assert_eq!(
            (f.token.as_str(), f.returnto.as_str()),
            ("tok+\\", "Item:Q6")
        );
        assert_eq!(f.username.value, "Alice");
        assert_eq!(f.password.value, "");
        assert_eq!(f.password.autocomplete.as_deref(), Some("current-password"));
        assert!(f.challenge.is_none() && f.challenge_bits.is_none());
        let c = ChallengeInfo {
            id: "1.w.badlogin.12.9.s.m".into(),
            bits: 12,
        };
        let f = login_form_model(&m, "t", "Main Page", "", Some(&c));
        assert_eq!(f.challenge_bits, Some(12));
        let ch = f.challenge.unwrap();
        assert_eq!(
            (ch.id.as_str(), ch.bits, ch.solution.as_str()),
            (c.id.as_str(), 12, "")
        );
        assert_eq!(ch.nojs, m.get("ts-challenge-nojs-login"));

        let asks = logout_model(&m, None, Some("csrf"), "Item:Q6");
        assert!(asks.not_logged_in.is_none() && asks.continue_link.is_none());
        assert_eq!(asks.form.unwrap().token, "csrf");
        let nobody = logout_model(&m, Some("expired"), None, "Item:Q6");
        assert!(nobody.form.is_none());
        assert_eq!(nobody.continue_link.unwrap().href, "/wiki/Item:Q6");
        assert!(
            nobody
                .notice
                .unwrap()
                .classes
                .contains("cdx-message--warning")
        );
    }
}
