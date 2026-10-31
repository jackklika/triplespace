//! Logging in and out without JavaScript, against a fake API that keeps sessions the way
//! the real one does: a token bound to the session cookie, a new session on login, the
//! cookie cleared on logout.

mod common;

use std::collections::BTreeMap;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

#[derive(Clone, Default)]
struct FakeApi {
    no_login: bool,
}

fn session(h: &HeaderMap) -> Option<String> {
    h.get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|c| c.trim().strip_prefix("triplespace_session="))
        .map(str::to_string)
}

fn form(body: &[u8]) -> BTreeMap<String, String> {
    url::form_urlencoded::parse(body).into_owned().collect()
}

impl FakeApi {
    fn answer(&self, r: &ApiRequest) -> (Value, Vec<&'static str>, bool) {
        let q = &r.path_and_query;
        let s = session(&r.headers);
        if q.contains("meta=siteinfo") {
            let caps = if self.no_login {
                json!(["wbgetentities"])
            } else {
                json!(["wbgetentities", "clientlogin", "login"])
            };
            return (
                json!({"query": {
                    "general": {"sitename": "librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
                    "triplespace": {"api_version": 1, "capabilities": caps}
                }}),
                vec![],
                true,
            );
        }
        if q.contains("meta=userinfo") {
            return match s.as_deref() {
                Some("alice") => (
                    json!({"query": {"userinfo": {"id": 7, "name": "Alice"}}}),
                    vec![],
                    false,
                ),
                _ => (
                    json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
                    vec![],
                    false,
                ),
            };
        }
        if q.contains("meta=tokens") {
            let kind = if q.contains("type=csrf") {
                "csrf"
            } else {
                "login"
            };
            let (sid, cookies) = match s {
                Some(sid) => (sid, vec![]),
                None => (
                    "anon1".to_string(),
                    vec!["triplespace_session=anon1; Path=/; HttpOnly; SameSite=Lax"],
                ),
            };
            return (
                json!({"query": {"tokens": {format!("{kind}token"): format!("{kind}-{sid}+\\")}}}),
                cookies,
                false,
            );
        }
        let f = form(&r.body);
        match f.get("action").map(String::as_str) {
            Some("clientlogin") => {
                let sid = s.unwrap_or_default();
                if f.get("logintoken").map(String::as_str) != Some(&format!("login-{sid}+\\")) {
                    return (
                        json!({"error": {"code": "badtoken", "info": "Invalid login token."}}),
                        vec![],
                        false,
                    );
                }
                if f.get("username").map(String::as_str) == Some("Alice")
                    && f.get("password").map(String::as_str) == Some("secret")
                {
                    (
                        json!({"clientlogin": {"status": "PASS", "username": "Alice"}}),
                        vec!["triplespace_session=alice; Path=/; HttpOnly; SameSite=Lax"],
                        false,
                    )
                } else {
                    (
                        json!({"clientlogin": {"status": "FAIL", "message": "Incorrect", "messagecode": "wrongpassword"}}),
                        vec![],
                        false,
                    )
                }
            }
            Some("logout") => {
                let sid = s.unwrap_or_default();
                if f.get("token").map(String::as_str) != Some(&format!("csrf-{sid}+\\")) {
                    return (
                        json!({"error": {"code": "badtoken", "info": "Invalid CSRF token."}}),
                        vec![],
                        false,
                    );
                }
                (
                    json!({"logout": {}}),
                    vec!["triplespace_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"],
                    false,
                )
            }
            _ => (
                json!({"error": {"code": "unknown_action", "info": q}}),
                vec![],
                false,
            ),
        }
    }
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let (body, cookies, public) = self.answer(&request);
            let mut headers = HeaderMap::new();
            for c in cookies {
                headers.append(header::SET_COOKIE, c.parse().unwrap());
            }
            let cc = if public {
                "public, max-age=60, s-maxage=60"
            } else {
                "private, no-cache"
            };
            headers.insert(header::CACHE_CONTROL, cc.parse().unwrap());
            headers.insert(header::ETAG, "\"e\"".parse().unwrap());
            Ok(ApiResponse {
                status: StatusCode::OK,
                headers,
                body: serde_json::to_vec(&body).unwrap(),
            })
        })
    }

    fn embedded(&self) -> bool {
        false
    }
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Answer {
    fn cookies(&self) -> Vec<&str> {
        self.headers
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect()
    }
}

async fn send(api: FakeApi, req: Request<Body>) -> Answer {
    let r = common::site_router(Client::new(api))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    Answer {
        status,
        headers,
        body: String::from_utf8(body.to_vec()).unwrap(),
    }
}

async fn get(uri: &str, cookie: Option<&str>) -> Answer {
    let mut req = Request::get(uri).header(header::HOST, "librarybase.org");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    send(FakeApi::default(), req.body(Body::empty()).unwrap()).await
}

async fn post(uri: &str, cookie: Option<&str>, fields: &[(&str, &str)]) -> Answer {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    let mut req = Request::post(uri)
        .header(header::HOST, "librarybase.org")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    send(FakeApi::default(), req.body(Body::from(body)).unwrap()).await
}

const LOGIN: &str = "/w/index.php?title=Special:UserLogin";

#[tokio::test]
async fn the_login_form_starts_a_session_and_holds_its_token() {
    let a = get("/wiki/Special:UserLogin?returnto=Item:Q6", None).await;
    assert_eq!(a.status, StatusCode::OK);
    assert_eq!(a.headers[header::CACHE_CONTROL], "private, no-store");
    assert!(a.headers.get("cache-tag").is_none());
    assert_eq!(
        a.cookies(),
        vec!["triplespace_session=anon1; Path=/; HttpOnly; SameSite=Lax"],
        "the API's cookie, verbatim"
    );
    assert!(
        a.body
            .contains("name=\"logintoken\" value=\"login-anon1+\\\"")
    );
    assert!(a.body.contains("name=\"returnto\" value=\"Item:Q6\""));
    assert!(a.body.contains("autocomplete=\"current-password\""));
    assert!(
        a.body
            .contains("action=\"/w/index.php?title=Special:UserLogin\"")
    );
}

#[tokio::test]
async fn logging_in_relays_the_new_session_and_goes_back() {
    let a = post(
        LOGIN,
        Some("triplespace_session=anon1"),
        &[
            ("username", "Alice"),
            ("password", "secret"),
            ("logintoken", "login-anon1+\\"),
            ("returnto", "Item:Q6"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::SEE_OTHER);
    assert_eq!(a.headers[header::LOCATION], "/wiki/Item:Q6");
    assert_eq!(a.headers[header::CACHE_CONTROL], "private, no-store");
    assert_eq!(
        a.cookies(),
        vec!["triplespace_session=alice; Path=/; HttpOnly; SameSite=Lax"]
    );
}

#[tokio::test]
async fn a_wrong_password_shows_the_form_again() {
    let a = post(
        LOGIN,
        Some("triplespace_session=anon1"),
        &[
            ("username", "Alice"),
            ("password", "nope"),
            ("logintoken", "login-anon1+\\"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(
        a.body
            .contains("Incorrect username or password. Please try again.")
    );
    assert!(a.body.contains("value=\"Alice\""), "the name is kept");
    assert!(!a.body.contains("nope"), "the password is not");
    assert!(
        a.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("private")
    );
}

#[tokio::test]
async fn an_expired_token_gets_a_fresh_one() {
    let a = post(
        LOGIN,
        None,
        &[
            ("username", "Alice"),
            ("password", "secret"),
            ("logintoken", "login-gone+\\"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("Your session expired"));
    assert!(a.body.contains("value=\"login-anon1+\\\""));
    assert!(!a.cookies().is_empty(), "the new session's cookie");
}

#[tokio::test]
async fn returnto_stays_on_the_site() {
    for unsafe_target in [
        "//evil.example",
        "https://evil.example/",
        "Special:UserLogout",
    ] {
        let a = post(
            LOGIN,
            Some("triplespace_session=anon1"),
            &[
                ("username", "Alice"),
                ("password", "secret"),
                ("logintoken", "login-anon1+\\"),
                ("returnto", unsafe_target),
            ],
        )
        .await;
        assert_eq!(
            a.headers[header::LOCATION],
            "/wiki/Project:Home",
            "{unsafe_target}"
        );
    }
}

#[tokio::test]
async fn the_header_offers_login_or_the_account_menu() {
    let anon = get("/wiki/Project:Home", None).await;
    assert!(
        anon.body
            .contains("href=\"/wiki/Special:UserLogin?returnto=Project:Home\">Log in</a>")
    );
    let alice = get("/wiki/Project:Home", Some("triplespace_session=alice")).await;
    assert!(alice.body.contains("aria-label=\"Account menu for Alice\""));
    assert!(
        alice
            .body
            .contains("href=\"/wiki/Special:UserLogout?returnto=Project:Home\">Log out</a>")
    );
    let already = get("/wiki/Special:UserLogin", Some("triplespace_session=alice")).await;
    assert!(already.body.contains("You are logged in as Alice."));
    assert!(!already.body.contains("logintoken"));
}

#[tokio::test]
async fn logging_out_asks_then_clears_the_cookie() {
    let ask = get(
        "/wiki/Special:UserLogout?returnto=Item:Q6",
        Some("triplespace_session=alice"),
    )
    .await;
    assert_eq!(ask.status, StatusCode::OK);
    assert!(ask.body.contains("Do you want to log out?"));
    assert!(ask.body.contains("name=\"token\" value=\"csrf-alice+\\\""));
    let done = post(
        "/w/index.php?title=Special:UserLogout",
        Some("triplespace_session=alice"),
        &[("token", "csrf-alice+\\"), ("returnto", "Item:Q6")],
    )
    .await;
    assert_eq!(done.status, StatusCode::SEE_OTHER);
    assert_eq!(done.headers[header::LOCATION], "/wiki/Item:Q6");
    assert!(done.cookies()[0].contains("Max-Age=0"));
    let forged = post(
        "/w/index.php?title=Special:UserLogout",
        Some("triplespace_session=alice"),
        &[("token", "guess")],
    )
    .await;
    assert_eq!(forged.status, StatusCode::OK, "a bad token logs no one out");
    assert!(forged.body.contains("Your session expired"));
    let nobody = get("/wiki/Special:UserLogout", None).await;
    assert!(nobody.body.contains("You are not logged in."));
}

#[tokio::test]
async fn other_posts_and_an_api_without_login_are_refused() {
    let a = post("/wiki/Item:Q6", None, &[("x", "y")]).await;
    assert_eq!(a.status, StatusCode::METHOD_NOT_ALLOWED);
    let req = Request::get("/wiki/Special:UserLogin")
        .header(header::HOST, "librarybase.org")
        .body(Body::empty())
        .unwrap();
    let r = send(FakeApi { no_login: true }, req).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(!r.body.contains(">Log in</a>"));
}
