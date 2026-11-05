//! The site's router against a fake API: the frame, its caching, the `503` when the API is
//! away, and the API's paths refused.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::json;
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

/// Answers `siteinfo` and `userinfo`, and records what it was asked.
#[derive(Clone, Default)]
struct FakeApi {
    down: bool,
    api_version: u64,
    no_search: bool,
    /// Sends a provider whose `types` is not a list, as a mistyped field would.
    bad_providers: bool,
    seen: Arc<Mutex<Vec<ApiRequest>>>,
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(request.clone());
            if self.down {
                return Err(ClientError::Unreachable("connection refused".into()));
            }
            let credentialed = request.headers.contains_key(header::COOKIE);
            let (body, etag) = if request.path_and_query.contains("meta=siteinfo") {
                (
                    json!({"query": {
                        "general": {"sitename": "Librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
                        "triplespace": {"api_version": self.api_version, "insecure": false,
                            "capabilities": if self.no_search { json!(["wbgetentities"]) } else { json!(["wbgetentities", "wbsearchentities"]) }},
                        "providers": if self.bad_providers { json!([{"code": "WD", "slug": "wikidata", "name": "Wikidata", "types": "item"}]) } else { json!(null) }
                    }}),
                    "\"si\"",
                )
            } else if credentialed {
                (
                    json!({"query": {"userinfo": {"id": 3, "name": "Alice"}}}),
                    "\"alice\"",
                )
            } else {
                (
                    json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
                    "\"anon\"",
                )
            };
            let mut headers = HeaderMap::new();
            headers.insert(header::ETAG, etag.parse().unwrap());
            let cc = if credentialed && !request.path_and_query.contains("meta=siteinfo") {
                "private, no-cache"
            } else {
                "public, max-age=60, s-maxage=60"
            };
            headers.insert(header::CACHE_CONTROL, cc.parse().unwrap());
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

fn api() -> FakeApi {
    FakeApi {
        api_version: 1,
        ..FakeApi::default()
    }
}

async fn get(
    fake: &FakeApi,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let mut req = Request::get(uri).header(header::HOST, "librarybase.org");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let r = common::site_router(Client::new(fake.clone()))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let h = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, h, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn an_anonymous_page_is_public_and_revalidates() {
    let fake = api();
    let (status, h, html) = get(&fake, "/wiki/Project:Home", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Project:Home – Librarybase</title>"));
    assert!(html.contains(">Librarybase</h1>"));
    assert!(html.contains(
        "This page is generated because the page Project:Home has not been written yet."
    ));
    assert!(
        !html.contains("Special:UserLogin"),
        "logging in is not offered by this API"
    );
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    assert_eq!(h[header::VARY], "Cookie, Accept-Language");
    assert_eq!(
        h[header::CONTENT_SECURITY_POLICY],
        triplespace_ui::pages::CSP
    );
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    // Anonymous: only siteinfo was asked, with the host forwarded.
    {
        let seen = fake.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].headers["x-forwarded-host"], "librarybase.org");
    }
    let etag = h[header::ETAG].to_str().unwrap().to_string();
    let (status, h, body) = get(&fake, "/wiki/Project:Home", &[("if-none-match", &etag)]).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert!(body.is_empty());
    assert_eq!(h[header::ETAG].to_str().unwrap(), etag);
}

#[tokio::test]
async fn a_signed_in_page_is_private_and_names_the_viewer() {
    let fake = api();
    let (status, h, html) = get(
        &fake,
        "/wiki/Project:Home",
        &[("cookie", "triplespace_session=s")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<span class=\"ts-header__user\">Alice</span>"));
    assert!(!html.contains("Special:UserLogin"));
    assert_eq!(h[header::CACHE_CONTROL], "private, no-cache");
    // Siteinfo, userinfo, and the bell's counts.
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 3);
    assert!(
        seen.iter()
            .any(|r| r.path_and_query.contains("meta=notifications")),
        "the bell's counts are asked for"
    );
}

#[tokio::test]
async fn the_main_page_has_its_old_names_and_other_titles_are_404() {
    let fake = api();
    for path in ["/", "/wiki/Main_Page", "/wiki/", "/w/index.php"] {
        let (status, h, _) = get(&fake, path, &[]).await;
        assert_eq!(status, StatusCode::FOUND, "{path}");
        assert_eq!(h[header::LOCATION], "/wiki/Project:Home", "{path}");
    }
    let (status, _, html) = get(&fake, "/wiki/Librarybase:Home", &[]).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the project namespace by the site's name"
    );
    assert!(html.contains("This page is generated"));
    let (status, _, html) = get(&fake, "/wiki/Project:About", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("There is no page called Project:About."));
    for absent in [
        "Special:Jobs",
        "Special:RecentChanges",
        "Special:UserLogin",
        "Special:NewItem",
    ] {
        let (status, _, html) = get(&fake, &format!("/wiki/{absent}"), &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{absent}");
        assert!(html.contains(&format!("There is no special page called {absent}.")));
    }
    let (status, _, html) = get(&fake, "/wiki/Special:SpecialPages", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("href=\"/wiki/Special:Search\""));
}

#[tokio::test]
async fn features_the_api_lacks_are_hidden() {
    let fake = FakeApi {
        no_search: true,
        ..api()
    };
    let (_, _, home) = get(&fake, "/wiki/Project:Home", &[]).await;
    assert!(!home.contains("name=\"search\""), "no search box anywhere");
    let (status, _, _) = get(&fake, "/wiki/Special:Search", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, index) = get(&fake, "/wiki/Special:SpecialPages", &[]).await;
    assert!(!index.contains("Special:Search\""));
    // An API without `GET /version` (API level 1): no Special:Version.
    let (status, _, _) = get(&fake, "/wiki/Special:Version", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!index.contains("Special:Version\""));
    let (_, _, full) = get(&api(), "/wiki/Project:Home", &[]).await;
    for absent in [
        "Special:RecentChanges",
        "Special:Jobs",
        "Special:NewItem",
        "action=edit",
        "Special:UserLogin",
    ] {
        assert!(!full.contains(absent), "{absent} is not built yet");
    }
}

#[tokio::test]
async fn the_api_away_or_too_old_is_a_503() {
    let fake = FakeApi {
        down: true,
        ..api()
    };
    let (status, h, html) = get(&fake, "/wiki/Main_Page", &[]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(h[header::RETRY_AFTER], "30");
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    assert!(html.contains("cannot reach its server"));
    let old = FakeApi {
        api_version: 0,
        ..api()
    };
    let (status, _, html) = get(&old, "/wiki/Main_Page", &[]).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(html.contains("older than this version"));
}

#[tokio::test]
async fn a_siteinfo_section_that_does_not_decode_is_a_502_not_an_empty_list() {
    let fake = FakeApi {
        bad_providers: true,
        ..api()
    };
    let (status, h, html) = get(&fake, "/wiki/Main_Page", &[]).await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert!(
        h.get(header::RETRY_AFTER).is_none(),
        "waiting will not mend it"
    );
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
    assert!(html.contains("could not read"), "{html}");
}

#[tokio::test]
async fn the_api_paths_are_refused_and_the_rest_is_a_framed_404() {
    let fake = api();
    for path in [
        "/w/api.php",
        "/w/rest.php/triplespace/v0/nothing",
        "/entity/Q1",
        "/wiki/Special:EntityData/Q1.json",
    ] {
        let (status, h, body) = get(&fake, path, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body, "Not found\n", "{path}");
        assert!(h.get(header::CONTENT_SECURITY_POLICY).is_some());
    }
    assert!(fake.seen.lock().unwrap().is_empty());
    let (status, _, html) = get(&fake, "/no/such/thing", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("There is no page at this address."));
}

#[tokio::test]
async fn root_search_and_theme() {
    let fake = api();
    let (status, h, _) = get(&fake, "/", &[]).await;
    assert_eq!(status, StatusCode::FOUND);
    assert_eq!(h[header::LOCATION], "/wiki/Project:Home");
    let (status, _, html) = get(
        &fake,
        "/w/index.php?title=Special:Search&search=Douglas+Adams",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("value=\"Douglas Adams\""));
    assert!(
        !html.contains("ts-tabs__link"),
        "a special page has no tabs"
    );
    // The fake API reports no theme, so the site serves the shipped one: a stylesheet
    // per palette and per type set, named by their hashes (0086 §5).
    let theme = triplespace_ui::theme::Theme::shipped();
    let href = theme
        .href(triplespace_ui::theme::Sheet::Palette, "parchment")
        .unwrap();
    let (status, _, html) = get(&fake, "/wiki/Special:Search", &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains(&format!(
        "<link rel=\"stylesheet\" id=\"ts-palette\" href=\"{href}\">"
    )));
    assert!(html.contains("<script type=\"application/json\" id=\"ts-theme\">"));
    let (status, h, css) = get(&fake, &href, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    assert!(css.contains("--color-progressive: #2B559E;"));
    let typeset = theme
        .href(triplespace_ui::theme::Sheet::Typeset, "opendyslexic")
        .unwrap();
    let (status, _, css) = get(&fake, &typeset, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(css.contains("--font-family-base: 'OpenDyslexic'"));
    let (status, _, _) = get(&fake, "/ui/theme/p-nope-0000000000000000.css", &[]).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, h, _) = get(&fake, "/ui/theme/p-parchment-0000000000000000.css", &[]).await;
    assert_eq!(h[header::CACHE_CONTROL], "public, max-age=60");
    let (_, h, _) = get(&fake, "/ui/theme/0000000000000000.css", &[]).await;
    assert_eq!(h[header::CACHE_CONTROL], "public, max-age=60");
}
