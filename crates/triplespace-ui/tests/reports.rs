//! The report pages (0047 §4) against a fake API: what `Special:WantedPages` and its
//! siblings ask `list=querypage`, how their rows read, how they page, and where they are
//! offered.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

#[derive(Clone)]
struct FakeApi {
    capabilities: Vec<&'static str>,
    seen: Arc<Mutex<Vec<String>>>,
}

fn state(backing: &str, measure: &str, waiting: i64) -> Value {
    json!({"backing": backing, "measure": measure, "mode": backing, "scope": "local",
        "computed_at": "2026-10-28T09:05:00Z", "row_count": 3, "waiting": waiting})
}

fn querypage(q: &str) -> Value {
    let page = q
        .split("qppage=")
        .nth(1)
        .and_then(|r| r.split('&').next())
        .unwrap_or_default();
    let (results, st, more) = match page {
        "WantedPages" => (
            json!([
                {"value": "3", "ns": 0, "title": "A missing page"},
                {"value": "1", "ns": 4, "title": "Librarybase:Gone"}
            ]),
            state("projection", "count", 0),
            true,
        ),
        "LonelyPages" => (
            json!([{"value": "", "ns": 0, "title": "A lone page"}]),
            state("projection", "none", 7),
            false,
        ),
        "DoubleRedirects" => (
            json!([{"value": "", "ns": 0, "title": "First", "ts-target": "Second", "ts-final": "Third"}]),
            state("index", "none", 0),
            false,
        ),
        "BrokenRedirects" => (
            json!([{"value": "", "ns": 0, "title": "Old", "ts-target": "Nowhere", "ts-fragment": "Top"}]),
            state("index", "none", 0),
            false,
        ),
        "AncientPages" => (
            json!([{"value": "2025-01-02T03:04:05Z", "ns": 0, "title": "Dusty"}]),
            state("index", "time", 0),
            false,
        ),
        "ListProperties" => (
            json!([
                {"value": "", "ns": 122, "title": "Property:P1", "ts-label": "author"},
                {"value": "", "ns": 122, "title": "Property:P2"}
            ]),
            state("index", "none", 0),
            false,
        ),
        _ => (json!([]), state("projection", "none", 0), false),
    };
    let mut v = json!({"query": {"querypage": {"name": page, "results": results, "ts-state": st}}});
    if more {
        let offset: u64 = q
            .split("qpoffset=")
            .nth(1)
            .and_then(|r| r.split('&').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        let limit: u64 = q
            .split("qplimit=")
            .nth(1)
            .and_then(|r| r.split('&').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(10);
        v["continue"] = json!({"qpoffset": offset + limit, "continue": "-||"});
    }
    v
}

impl FakeApi {
    fn answer(&self, r: &ApiRequest) -> (StatusCode, Value) {
        let q = r.path_and_query.as_str();
        if q.contains("meta=siteinfo") {
            return (
                StatusCode::OK,
                json!({"query": {
                    "general": {"sitename": "Librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
                    "triplespace": {"api_version": 1, "capabilities": self.capabilities}
                }}),
            );
        }
        if q.contains("meta=userinfo") {
            return (
                StatusCode::OK,
                json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
            );
        }
        if q.contains("list=querypage") {
            return (StatusCode::OK, querypage(q));
        }
        (
            StatusCode::OK,
            json!({"error": {"code": "badvalue", "info": "not in the fake"}}),
        )
    }
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            self.seen
                .lock()
                .unwrap()
                .push(request.path_and_query.clone());
            let (status, body) = self.answer(&request);
            let mut headers = HeaderMap::new();
            headers.insert(header::CACHE_CONTROL, "public, max-age=60".parse().unwrap());
            Ok(ApiResponse {
                status,
                headers,
                body: serde_json::to_vec(&body).unwrap(),
            })
        })
    }

    fn embedded(&self) -> bool {
        false
    }
}

fn api(capabilities: &[&'static str]) -> FakeApi {
    FakeApi {
        capabilities: capabilities.to_vec(),
        seen: Arc::default(),
    }
}

async fn get_from(fake: &FakeApi, uri: &str) -> (StatusCode, String) {
    let req = Request::get(uri).header(header::HOST, "librarybase.org");
    let r = common::site_router(Client::new(fake.clone()))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

const ALL: &[&str] = &["wbgetentities", "pages", "reports"];

#[tokio::test]
async fn wanted_pages_count_links_and_page_through() {
    let fake = api(ALL);
    let (status, html) = get_from(&fake, "/wiki/Special:WantedPages?limit=20&offset=40").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Wanted pages"), "{html}");
    assert!(
        fake.seen
            .lock()
            .unwrap()
            .iter()
            .any(|q| q.contains("qppage=WantedPages&qpoffset=40&qplimit=20")),
        "{:?}",
        fake.seen
    );
    assert!(html.contains("<ol class=\"ts-report\" start=\"41\">"));
    assert!(
        html.contains("class=\"new\">A missing page</a> (3 links)"),
        "{html}"
    );
    assert!(html.contains("Librarybase:Gone</a> (1 link)"));
    assert!(html.contains("Showing up to 2 results in range #41 to #42."));
    assert!(html.contains(
        "kept up to date as pages change; it was last built in full at 09:05, 28 October 2026"
    ));
    assert!(html.contains(
        "href=\"/w/index.php?title=Special:WantedPages&amp;limit=20&amp;offset=20\" rel=\"prev\""
    ), "{html}");
    assert!(html.contains(
        "href=\"/w/index.php?title=Special:WantedPages&amp;limit=20&amp;offset=60\" rel=\"next\""
    ));
}

#[tokio::test]
async fn names_and_aliases_reach_the_same_report() {
    for path in [
        "/wiki/Special:Wantedpages",
        "/wiki/Special:BrokenLinks",
        "/wiki/Special:wantedpages",
    ] {
        let fake = api(ALL);
        let (status, html) = get_from(&fake, path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(html.contains("A missing page"), "{path}");
        assert!(
            fake.seen
                .lock()
                .unwrap()
                .iter()
                .any(|q| q.contains("qppage=WantedPages&qpoffset=0&qplimit=50")),
            "{path}"
        );
    }
}

#[tokio::test]
async fn lonely_pages_say_what_they_count_and_how_far_behind() {
    let (status, html) = get_from(&api(ALL), "/wiki/Special:LonelyPages").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Only links count here"));
    assert!(html.contains("7 changes are still being applied"));
    assert!(html.contains(">A lone page</a></li>"));
}

#[tokio::test]
async fn redirects_times_and_properties() {
    let (_, html) = get_from(&api(ALL), "/wiki/Special:DoubleRedirects").await;
    assert!(html.contains(">First</a> \u{2192} <a href=\"/wiki/Second\">Second</a> \u{2192} <a href=\"/wiki/Third\">Third</a>"), "{html}");
    // A live query says nothing of its freshness.
    assert!(!html.contains("ts-report__state"));
    let (_, html) = get_from(&api(ALL), "/wiki/Special:BrokenRedirects").await;
    assert!(
        html.contains("<a href=\"/wiki/Nowhere#Top\" class=\"new\">Nowhere#Top</a>"),
        "{html}"
    );
    let (_, html) = get_from(&api(ALL), "/wiki/Special:AncientPages").await;
    assert!(
        html.contains(">Dusty</a> (03:04, 2 January 2025)"),
        "{html}"
    );
    let (_, html) = get_from(&api(ALL), "/wiki/Special:ListProperties").await;
    assert!(
        html.contains("<a href=\"/wiki/Property:P1\">author</a> (P1)"),
        "{html}"
    );
    assert!(html.contains("<a href=\"/wiki/Property:P2\">P2</a>"));
}

#[tokio::test]
async fn an_empty_report_says_so() {
    let (status, html) = get_from(&api(ALL), "/wiki/Special:UnusedCategories").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("There are no results for this report."));
    assert!(!html.contains("<ol class=\"ts-report\""));
}

#[tokio::test]
async fn reports_are_offered_only_where_served() {
    // No reports from the API: no pages, and none on Special:SpecialPages.
    let bare = api(&["wbgetentities", "pages"]);
    let (status, _) = get_from(&bare, "/wiki/Special:WantedPages").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, html) = get_from(&bare, "/wiki/Special:SpecialPages").await;
    assert!(!html.contains("Special:WantedPages"));
    // Reports but no document pages: only the entity report.
    let entities = api(&["wbgetentities", "reports"]);
    let (status, _) = get_from(&entities, "/wiki/Special:ShortPages").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = get_from(&entities, "/wiki/Special:ListProperties").await;
    assert_eq!(status, StatusCode::OK);
    let (_, html) = get_from(&api(ALL), "/wiki/Special:SpecialPages").await;
    assert!(html.contains("href=\"/wiki/Special:WantedPages\">Wanted pages</a>"));
    assert!(html.contains("Every redirect and where it leads."));
}
