//! `Special:BlankPage` and MediaWiki's redirecting special pages (21 §6.1) against a fake
//! API: where each sends a reader, a signed-in viewer and a bad request.

mod common;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

/// Answers as an API with document pages: page 3 is `Foo bar`, whose revisions are 5 and
/// 7; `Foo bar/de` exists; everything else is missing.
#[derive(Clone)]
struct FakeApi {
    capabilities: Vec<&'static str>,
}

const ALL: &[&str] = &[
    "wbgetentities",
    "clientlogin",
    "pages",
    "parse",
    "edit",
    "history",
    "recentchanges",
    "activity",
    "protect",
    "delete",
];

fn page(title: &str) -> Value {
    json!({"query": {"pages": [{"pageid": 3, "ns": 0, "title": title, "contentmodel": "wikitext"}]}})
}

impl FakeApi {
    fn answer(&self, r: &ApiRequest) -> Value {
        let q = &r.path_and_query;
        if q.contains("meta=siteinfo") {
            return json!({"query": {
                "general": {"sitename": "Librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
                "triplespace": {"api_version": 1, "capabilities": self.capabilities}
            }});
        }
        if q.contains("meta=userinfo") {
            return if r.headers.contains_key(header::COOKIE) {
                json!({"query": {"userinfo": {"id": 3, "name": "Alice", "rights": ["read", "edit"], "groups": ["*", "user"]}}})
            } else {
                json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}})
            };
        }
        if q.contains("prop=info") {
            if q.contains("revids=5&") || q.contains("revids=7&") || q.contains("pageids=3&") {
                return page("Foo bar");
            }
            if q.contains("titles=Foo+bar%2Fde&") {
                return page("Foo bar/de");
            }
            if let Some(rest) = q.split("revids=").nth(1) {
                let n: u64 = rest.split('&').next().unwrap().parse().unwrap();
                return json!({"query": {"pages": [{"revid": n, "missing": true}]}});
            }
            if let Some(rest) = q.split("pageids=").nth(1) {
                let n: u64 = rest.split('&').next().unwrap().parse().unwrap();
                return json!({"query": {"pages": [{"pageid": n, "missing": true}]}});
            }
            return json!({"query": {"pages": [{"ns": 0, "title": "Missing", "missing": true}]}});
        }
        json!({"error": {"code": "badvalue", "info": "not in the fake"}})
    }
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let body = self.answer(&request);
            let mut headers = HeaderMap::new();
            headers.insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());
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
        capabilities: ALL.to_vec(),
    }
}

async fn get_with(fake: &FakeApi, uri: &str, signed_in: bool) -> (StatusCode, String, String) {
    let mut req = Request::get(uri).header(header::HOST, "librarybase.org");
    if signed_in {
        req = req.header(header::COOKIE, "triplespace_session=alice");
    }
    let r = common::site_router(Client::new(fake.clone()))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let location = r
        .headers()
        .get(header::LOCATION)
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, location, String::from_utf8(body.to_vec()).unwrap())
}

async fn get(uri: &str) -> (StatusCode, String, String) {
    get_with(&api(), uri, false).await
}

/// Asserts a `302` to `to`.
async fn goes(uri: &str, signed_in: bool, to: &str) {
    let (status, location, _) = get_with(&api(), uri, signed_in).await;
    assert_eq!(status, StatusCode::FOUND, "{uri}");
    assert_eq!(location, to, "{uri}");
}

#[tokio::test]
async fn the_blank_page_reads_no_content_in_italics() {
    let (status, _, html) = get("/wiki/Special:BlankPage").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<p class=\"ts-blankpage\"><i>No content</i></p>"));
    assert!(html.contains("<title>Blank page"));
    // Unlisted: not on Special:SpecialPages.
    let (_, _, index) = get("/wiki/Special:SpecialPages").await;
    assert!(!index.contains("Special:BlankPage"));
}

#[tokio::test]
async fn the_viewers_own_pages() {
    goes("/wiki/Special:MyPage", true, "/wiki/User:Alice").await;
    goes(
        "/wiki/Special:MyPage/Notes_2026",
        true,
        "/wiki/User:Alice/Notes_2026",
    )
    .await;
    goes("/wiki/Special:MyTalk", true, "/wiki/User_talk:Alice").await;
    goes(
        "/wiki/Special:MyContributions",
        true,
        "/wiki/Special:Contributions/Alice",
    )
    .await;
    goes(
        "/wiki/Special:MyContribs",
        true,
        "/wiki/Special:Contributions/Alice",
    )
    .await;
    goes("/wiki/Special:MyLog", true, "/wiki/Special:Log?user=Alice").await;
    goes(
        "/wiki/Special:MyLog/delete",
        true,
        "/wiki/Special:Log?type=delete&user=Alice",
    )
    .await;
    // A reader is asked to log in, and comes back here.
    goes(
        "/wiki/Special:MyPage/Notes",
        false,
        "/wiki/Special:UserLogin?returnto=Special%3AMyPage%2FNotes",
    )
    .await;
}

#[tokio::test]
async fn actions_on_a_page() {
    goes(
        "/wiki/Special:EditPage/Foo_bar",
        false,
        "/wiki/Foo_bar?action=edit",
    )
    .await;
    goes(
        "/wiki/Special:Edit/Foo_bar?section=2",
        false,
        "/wiki/Foo_bar?action=edit&section=2",
    )
    .await;
    goes(
        "/wiki/Special:PageHistory/Item:Q42",
        false,
        "/wiki/Item:Q42?action=history",
    )
    .await;
    goes(
        "/wiki/Special:History/librarybase:home",
        false,
        "/wiki/Project:Home?action=history",
    )
    .await;
    goes(
        "/wiki/Special:DeletePage/Foo",
        false,
        "/wiki/Foo?action=delete",
    )
    .await;
    goes(
        "/wiki/Special:ProtectPage/Foo",
        false,
        "/wiki/Foo?action=protect",
    )
    .await;
    goes(
        "/w/index.php?title=Special:EditPage&page=Foo",
        false,
        "/wiki/Foo?action=edit",
    )
    .await;

    // No title: the form. A title no page can have: the form, with the error.
    let (status, _, html) = get("/wiki/Special:EditPage").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("name=\"page\""));
    assert!(html.contains("value=\"Special:EditPage\""));
    let (status, _, html) = get("/wiki/Special:EditPage/Special:Search").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("is not a title a page can have"));
}

#[tokio::test]
async fn talk_pages_and_new_topics() {
    goes("/wiki/Special:TalkPage/Foo", false, "/wiki/Talk:Foo").await;
    goes(
        "/wiki/Special:TalkPage/Item:Q42",
        false,
        "/wiki/Item_talk:Q42",
    )
    .await;
    goes(
        "/wiki/Special:TalkPage/User:Alice",
        false,
        "/wiki/User_talk:Alice",
    )
    .await;
    goes(
        "/wiki/Special:TalkPage/Librarybase:Home",
        false,
        "/wiki/Project_talk:Home",
    )
    .await;
    goes(
        "/wiki/Special:TalkPage/Interface:Sidebar",
        false,
        "/wiki/MediaWiki_talk:Sidebar",
    )
    .await;
    goes("/wiki/Special:TalkPage/Talk:Foo", false, "/wiki/Talk:Foo").await;
    goes(
        "/wiki/Special:NewSection/Foo",
        false,
        "/wiki/Talk:Foo?action=addtopic",
    )
    .await;
    // A thread has no talk page.
    let (status, _, html) = get("/wiki/Special:TalkPage/Thread:Abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("has no talk page"));
}

#[tokio::test]
async fn by_revision_and_page_id() {
    goes(
        "/wiki/Special:PermanentLink/7",
        false,
        "/wiki/Foo_bar?oldid=7",
    )
    .await;
    goes("/wiki/Special:PermaLink/5", false, "/wiki/Foo_bar?oldid=5").await;
    goes("/wiki/Special:Diff/7", false, "/wiki/Foo_bar?diff=7").await;
    goes(
        "/wiki/Special:Diff/5/7",
        false,
        "/wiki/Foo_bar?diff=7&oldid=5",
    )
    .await;
    goes(
        "/wiki/Special:Diff/5/prev",
        false,
        "/wiki/Foo_bar?diff=prev&oldid=5",
    )
    .await;
    goes("/wiki/Special:Redirect/page/3", false, "/wiki/Foo_bar").await;
    goes(
        "/wiki/Special:Redirect/revision/7",
        false,
        "/wiki/Foo_bar?oldid=7",
    )
    .await;
    goes(
        "/w/index.php?title=Special:Redirect&wptype=revision&wpvalue=5",
        false,
        "/wiki/Foo_bar?oldid=5",
    )
    .await;

    let (status, _, html) = get("/wiki/Special:PermanentLink/99").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("There is no revision 99"));
    let (status, _, html) = get("/wiki/Special:Redirect/page/4").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("There is no page with ID 4"));
    let (status, _, html) = get("/wiki/Special:PermanentLink/abc").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("is not a number"));
    let (status, _, _) = get("/wiki/Special:Diff/5/sideways").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _, html) = get("/wiki/Special:Redirect/user/1").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("Only page IDs and revision IDs"));
    let (status, _, html) = get("/wiki/Special:Redirect").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("name=\"wptype\""));
}

#[tokio::test]
async fn entities_and_languages() {
    goes("/wiki/Special:EntityPage/Q42", false, "/wiki/Item:Q42").await;
    goes("/wiki/Special:EntityPage/P31", false, "/wiki/Property:P31").await;
    goes("/wiki/Special:EntityPage/q42", false, "/wiki/Item:Q42").await;
    let (status, _, _) = get("/wiki/Special:EntityPage/not_an_id").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    goes(
        "/wiki/Special:MyLanguage/Foo_bar?uselang=de",
        false,
        "/wiki/Foo_bar/de?uselang=de",
    )
    .await;
    goes(
        "/wiki/Special:MyLanguage/Foo_bar?uselang=fr",
        false,
        "/wiki/Foo_bar?uselang=fr",
    )
    .await;
    goes("/wiki/Special:MyLanguage/Foo_bar", false, "/wiki/Foo_bar").await;
}

#[tokio::test]
async fn listed_where_their_features_are() {
    let (_, _, index) = get("/wiki/Special:SpecialPages").await;
    assert!(index.contains("href=\"/wiki/Special:Redirect\""));
    assert!(index.contains("href=\"/wiki/Special:EntityPage\""));
    assert!(!index.contains("Special:MyPage"));

    // An API without document pages or histories: their pages are not served.
    let bare = FakeApi {
        capabilities: vec!["wbgetentities"],
    };
    for path in [
        "/wiki/Special:TalkPage/Foo",
        "/wiki/Special:MyPage",
        "/wiki/Special:PermanentLink/7",
        "/wiki/Special:DeletePage/Foo",
    ] {
        let (status, _, _) = get_with(&bare, path, true).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
    }
    let (_, _, index) = get_with(&bare, "/wiki/Special:SpecialPages", false).await;
    assert!(!index.contains("Special:Redirect\""));
    assert!(index.contains("Special:EntityPage\""));
    // Entities need nothing more.
    let (status, location, _) = get_with(&bare, "/wiki/Special:EntityPage/Q1", false).await;
    assert_eq!(
        (status, location.as_str()),
        (StatusCode::FOUND, "/wiki/Item:Q1")
    );
}
