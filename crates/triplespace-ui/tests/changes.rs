//! `Special:RecentChanges` and `Special:Contributions` against a fake API serving a
//! recorded `GET /activity` page (`tests/fixtures/activity.json`, in the shape of
//! chapter 16 §3.1): the rows, the filters, the sync strip, the pager and the caching.
//!
//! Update the snapshots with `INSTA_UPDATE=always cargo test -p triplespace-ui --test changes`
//! and review the diff.

mod common;

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).expect("fixture")).expect("parses")
}

fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

fn param(q: &str, name: &str) -> Option<String> {
    q.split_once('?')?
        .1
        .split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
        .map(decode)
}

#[derive(Clone, Default)]
struct FakeApi {
    seen: Arc<Mutex<Vec<String>>>,
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let q = request.path_and_query.clone();
            self.seen.lock().unwrap().push(q.clone());
            let mut headers = HeaderMap::new();
            headers.insert(
                header::CACHE_CONTROL,
                "public, max-age=0, s-maxage=60".parse().unwrap(),
            );
            let (status, body) = if q.contains("meta=siteinfo") {
                headers.insert(header::ETAG, "\"si\"".parse().unwrap());
                (
                    StatusCode::OK,
                    json!({"query": {
                        "general": {"sitename": "Librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
                        "triplespace": {"api_version": 3, "insecure": false,
                            "capabilities": ["wbgetentities", "wbsearchentities", "recentchanges", "activity"],
                            "providers": [{"code": "XD", "slug": "internetdomains", "name": "Internet domains", "types": []}]}
                    }}),
                )
            } else if q.contains("/activity") {
                headers.insert(header::ETAG, "\"act\"".parse().unwrap());
                headers.insert(CACHE_TAG, "activity".parse().unwrap());
                let page = fixture("activity.json");
                let actor = param(&q, "actor");
                let cursor = param(&q, "continue");
                let kinds = param(&q, "kind").unwrap_or_default();
                let rows: Vec<Value> = page["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| kinds.split('|').any(|k| k == r["kind"].as_str().unwrap()))
                    .filter(|r| actor.as_deref().is_none_or(|a| r["actor"]["name"] == a))
                    .cloned()
                    .collect();
                if actor.as_deref() == Some("Nobody") {
                    (
                        StatusCode::OK,
                        json!({"rows": [], "set": {"actor": "Nobody"}, "kinds": []}),
                    )
                } else if cursor.is_some() {
                    (
                        StatusCode::OK,
                        json!({"rows": rows[1.min(rows.len())..], "set": "everything", "kinds": []}),
                    )
                } else {
                    (
                        StatusCode::OK,
                        json!({"rows": rows, "set": "everything", "kinds": [], "next": "1700000000000000|4098|2"}),
                    )
                }
            } else {
                headers.insert(header::ETAG, "\"anon\"".parse().unwrap());
                (
                    StatusCode::OK,
                    json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
                )
            };
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

const CACHE_TAG: header::HeaderName = header::HeaderName::from_static("cache-tag");

async fn get(fake: &FakeApi, uri: &str) -> (StatusCode, HeaderMap, String) {
    let req = Request::get(uri)
        .header(header::HOST, "librarybase.org")
        .body(Body::empty())
        .unwrap();
    let r = common::site_router(Client::new(fake.clone()))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let h = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, h, String::from_utf8(body.to_vec()).unwrap())
}

fn main_of(html: &str) -> String {
    let start = html.find("<main").expect("a <main>");
    let end = html.find("</main>").expect("a </main>") + "</main>".len();
    html[start..end].replace("><", ">\n<")
}

#[tokio::test]
async fn recent_changes_read_without_javascript() {
    let fake = FakeApi::default();
    let (status, h, html) = get(&fake, "/wiki/Special:RecentChanges").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("<title>Recent changes – Librarybase</title>"));
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public"),
        "{h:?}"
    );
    assert_eq!(h[CACHE_TAG], "activity");
    let asked = fake.seen.lock().unwrap().clone();
    let feed = asked.iter().find(|q| q.contains("/activity")).unwrap();
    assert!(
        feed.contains("kind=edit%7Cjob%7Clog%7Cerased"),
        "syncs are off by default, erased rows go with edits: {feed}"
    );
    assert!(feed.contains("since="));
    assert!(feed.contains("limit=50"));
    assert!(feed.contains("lang=en"));
    let main = main_of(&html);
    assert!(main.contains("Mirror syncs are not shown."));
    assert!(
        main.contains("Six &amp; more"),
        "the label, not the ID: {main}"
    );
    assert!(main.contains("href=\"/wiki/Item:Q6\""));
    assert!(main.contains("href=\"/wiki/Special:Contributions/Alice_bot\""));
    assert!(main.contains("(username hidden)"));
    assert!(main.contains("An edit was erased."));
    assert!(main.contains("rel=\"next\""), "a pager to the older page");
    assert!(!main.contains("ts-change--sync"));
    insta::assert_snapshot!("recent_changes", main);
}

#[tokio::test]
async fn filters_and_paging_carry_through() {
    let fake = FakeApi::default();
    let (status, _, html) = get(
        &fake,
        "/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&hidebots=1&namespace=120&days=7&limit=100&continue=1700000000000000%7C4098%7C2",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let asked = fake.seen.lock().unwrap().clone();
    let feed = asked.iter().find(|q| q.contains("/activity")).unwrap();
    assert!(feed.contains("kind=edit%7Csync%7Cerased"), "{feed}");
    assert!(feed.contains("show=%21bot"), "{feed}");
    assert!(feed.contains("namespace=120"), "{feed}");
    assert!(feed.contains("limit=100"), "{feed}");
    assert!(
        feed.contains("continue=1700000000000000%7C4098%7C2"),
        "{feed}"
    );
    let main = main_of(&html);
    assert!(
        main.contains("ts-change--sync"),
        "syncs asked for are shown"
    );
    assert!(!main.contains("Mirror syncs are not shown."));
    assert!(main.contains(">Newest</a>"), "a way back from a later page");
    assert!(main.contains("value=\"sync\" checked"));
    // `hidebots=1` in the URL shows as the Bot edits box unchecked.
    assert!(main.contains("name=\"bots\" value=\"1\">"), "{main}");
    assert!(main.contains("<option value=\"120\" selected>"));
}

#[tokio::test]
async fn contributions_are_one_actors_feed() {
    let fake = FakeApi::default();
    let (status, _, html) = get(&fake, "/wiki/Special:Contributions/Alice_bot").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Contributions of Alice bot – Librarybase</title>"));
    let asked = fake.seen.lock().unwrap().clone();
    let feed = asked.iter().find(|q| q.contains("/activity")).unwrap();
    assert!(feed.contains("actor=Alice%20bot"), "{feed}");
    let main = main_of(&html);
    assert!(main.contains("Six &amp; more"));
    assert!(!main.contains("(username hidden)"), "only Alice bot's rows");
    insta::assert_snapshot!("contributions", main);

    let (status, _, html) = get(&fake, "/wiki/Special:Contributions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Enter a user name to see what they have changed."));
    let (status, _, html) = get(&fake, "/wiki/Special:Contributions/Nobody").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Nobody has made no changes in this period."));
}

#[tokio::test]
async fn the_pages_are_listed_and_linked() {
    let fake = FakeApi::default();
    let (_, _, html) = get(&fake, "/wiki/Special:SpecialPages").await;
    assert!(html.contains("href=\"/wiki/Special:RecentChanges\""));
    assert!(html.contains("href=\"/wiki/Special:Contributions\""));
    let (_, _, html) = get(&fake, "/wiki/Project:Home").await;
    assert!(
        html.contains("href=\"/wiki/Special:RecentChanges\""),
        "the header's link appears once the API serves the feed"
    );
}
