//! The read-only special pages of 21 §6.1 against a fake API: `NamespaceInfo`,
//! `ListGroupRights`, `ListDatatypes`, `MyLanguageFallbackChain`, `ProtectedTitles`,
//! `AllMessages` and `ComparePages`, what each asks the API and what it shows.

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

const ALL: &[&str] = &["wbgetentities", "pages", "parse", "history", "protect"];

fn messages(q: &str) -> Value {
    // Twenty-one `ts-a…` messages, the second overridden by the wiki.
    let mut all: Vec<Value> = (0..21)
        .map(|i| {
            let name = format!("ts-a{i:02}");
            if i == 1 {
                json!({"name": name, "content": "Ours", "default": "Shipped one", "customised": true})
            } else {
                json!({"name": name, "content": format!("Text {i}"), "default": format!("Text {i}")})
            }
        })
        .collect();
    if q.contains("amcustomised=modified") {
        all.retain(|m| m.get("customised").is_some());
    }
    if let Some(from) = q.split("amfrom=").nth(1).and_then(|r| r.split('&').next()) {
        all.retain(|m| m["name"].as_str().unwrap() >= from);
    }
    json!({"query": {"allmessages": all}})
}

impl FakeApi {
    fn answer(&self, r: &ApiRequest) -> (StatusCode, Value) {
        let q = r.path_and_query.as_str();
        if q.contains("siprop=namespaces") {
            return (
                StatusCode::OK,
                json!({"query": {
                    "namespaces": {
                        "0": {"id": 0, "name": "", "case": "first-letter", "subpages": false, "content": true},
                        "4": {"id": 4, "name": "Librarybase", "canonical": "Project", "case": "first-letter", "subpages": true, "content": false},
                        "120": {"id": 120, "name": "Item", "canonical": "Item", "case": "first-letter", "subpages": false, "content": false, "defaultcontentmodel": "wikibase-item"}
                    },
                    "namespacealiases": [{"id": 4, "alias": "Project"}, {"id": 4, "alias": "LB"}]
                }}),
            );
        }
        if q.contains("siprop=usergroups") {
            return (
                StatusCode::OK,
                json!({"query": {"usergroups": [
                    {"name": "*", "rights": ["read", "createaccount"], "implicit": ""},
                    {"name": "user", "rights": ["edit"], "implicit": ""},
                    {"name": "sysop", "rights": ["protect", "delete", "block"]},
                    {"name": "board", "rights": []}
                ]}}),
            );
        }
        if q.contains("meta=siteinfo") {
            return (
                StatusCode::OK,
                json!({"query": {
                    "general": {"sitename": "Librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home",
                        "wikibase-propertytypes": {"wikibase-item": {"valuetype": "wikibase-entityid"}, "string": {"valuetype": "string"}, "made-up": {"valuetype": "string"}}},
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
        if q.contains("list=protectedtitles") {
            return (
                StatusCode::OK,
                json!({"query": {"protectedtitles": [
                    {"title": "Project:Reserved", "ns": 4, "level": "sysop", "expiry": "infinity"}
                ]}}),
            );
        }
        if q.contains("meta=allmessages") {
            return (StatusCode::OK, messages(q));
        }
        if q.contains("action=compare") {
            if q.contains("torev=99") {
                return (
                    StatusCode::OK,
                    json!({"error": {"code": "nosuchrevid", "info": "There is no revision with ID 99."}}),
                );
            }
            let body = if q.contains("difftype=inline") {
                "<div class=\"mw-diff-inline-added\">new line</div>"
            } else {
                "<tr><td class=\"diff-marker\">+</td><td class=\"diff-addedline\">new line</td></tr>"
            };
            return (
                StatusCode::OK,
                json!({"compare": {
                    "fromrevid": 5, "fromtitle": "Foo bar", "fromtimestamp": "2026-10-01T00:00:00Z", "fromuser": "Alice", "fromcomment": "",
                    "torevid": 7, "totitle": "Baz", "totimestamp": "2026-10-02T00:00:00Z", "touser": "Bob", "tocomment": "more",
                    "body": body
                }}),
            );
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

fn api() -> FakeApi {
    FakeApi {
        capabilities: ALL.to_vec(),
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

async fn get(uri: &str) -> (StatusCode, String) {
    get_from(&api(), uri).await
}

#[tokio::test]
async fn namespace_info_lists_names_aliases_and_properties() {
    let (status, html) = get("/wiki/Special:NamespaceInfo").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<i>(Main)</i>"), "{html}");
    assert!(html.contains("<td>Librarybase</td><td>Project</td><td>Project, LB</td>"));
    assert!(html.contains("Subpages allowed"));
    assert!(html.contains("Default content model: wikibase-item"));
    // `Item`'s canonical name is its name, so not repeated.
    assert!(html.contains("<td>120</td><td>Item</td><td></td>"));
}

#[tokio::test]
async fn group_rights_list_each_group() {
    for path in [
        "/wiki/Special:ListGroupRights",
        "/wiki/Special:UserGroupRights",
    ] {
        let (status, html) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(html.contains("<code>*</code> (everyone)"));
        assert!(html.contains("<code>user</code> (implicit)"));
        assert!(html.contains("<li><code>block</code></li><li><code>delete</code></li>"));
        assert!(html.contains("<i>No permissions</i>"));
    }
}

#[tokio::test]
async fn datatypes_come_from_the_repository() {
    let (status, html) = get("/wiki/Special:ListDatatypes").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<td>Item</td><td><code>wikibase-item</code></td><td><code>wikibase-entityid</code></td>"), "{html}");
    // A type the interface has no name for shows its ID.
    assert!(html.contains("<td>made-up</td><td><code>made-up</code></td>"));
}

#[tokio::test]
async fn the_fallback_chain_follows_the_interface_language() {
    let (status, html) = get("/wiki/Special:MyLanguageFallbackChain?uselang=de-at").await;
    assert_eq!(status, StatusCode::OK);
    let order: Vec<usize> = ["\"de-at\">de-at", "\"de\">de", "\"mul\">mul", "\"en\">en"]
        .iter()
        .map(|c| html.find(c).unwrap_or_else(|| panic!("{c} in {html}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{order:?}");
    // English readers get `mul` after English.
    let (_, html) = get("/wiki/Special:MyLanguageFallbackChain").await;
    assert!(html.find("\"en\">en").unwrap() < html.find("\"mul\">mul").unwrap());
}

#[tokio::test]
async fn protected_titles_are_the_reservations() {
    let (status, html) = get("/wiki/Special:ProtectedTitles").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("href=\"/wiki/Project:Reserved\""));
    assert!(html.contains("<td>sysop</td>"));
    assert!(html.contains("href=\"/wiki/Special:ProtectedPages\""));
    let bare = FakeApi {
        capabilities: vec!["wbgetentities"],
        seen: Arc::default(),
    };
    let (status, _) = get_from(&bare, "/wiki/Special:ProtectedTitles").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn all_messages_show_defaults_overrides_and_pages() {
    let fake = api();
    let (status, html) = get_from(&fake, "/wiki/Special:AllMessages?limit=20").await;
    assert_eq!(status, StatusCode::OK);
    let asked = fake.seen.lock().unwrap().clone();
    let asked = asked
        .iter()
        .find(|q| q.contains("meta=allmessages"))
        .unwrap();
    assert!(asked.contains("amprop=default"), "{asked}");
    assert!(asked.contains("amlang=en"), "{asked}");
    // The overridden message: both texts, and a link to the page that overrides it.
    assert!(html.contains(
        "<tr class=\"ts-allmessages__customised\"><td><a href=\"/wiki/Interface:Ts-a01\"><code>ts-a01</code></a></td><td>Shipped one</td><td>Ours</td></tr>"
    ), "{html}");
    assert!(html.contains("<td>Text 2</td><td></td>"));
    // Twenty shown, and the next page starts at the twenty-first.
    assert_eq!(html.matches("<tr").count(), 21, "a header and twenty rows");
    assert!(html.contains("from=ts-a20"), "{html}");
    let (_, next) = get_from(
        &fake,
        "/w/index.php?title=Special:AllMessages&limit=20&from=ts-a20",
    )
    .await;
    assert!(next.contains("<code>ts-a20</code>"));
    assert!(!next.contains("Next page"));

    // Another language links to that language's page; `filter` reaches the API.
    let fake = api();
    let (_, html) = get_from(&fake, "/wiki/Special:AllMessages?lang=de&filter=modified").await;
    assert!(
        html.contains("href=\"/wiki/Interface:Ts-a01/de\""),
        "{html}"
    );
    assert_eq!(html.matches("<tr").count(), 2);
    let seen = fake.seen.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|q| q.contains("amcustomised=modified") && q.contains("amlang=de"))
    );
}

#[tokio::test]
async fn compare_pages_takes_titles_or_revisions() {
    let fake = api();
    let (status, html) = get_from(
        &fake,
        "/w/index.php?title=Special:ComparePages&page1=Foo_bar&page2=Baz&rev2=7",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains("ts-page-diff__table"));
    assert!(html.contains("diff-addedline"));
    // The form stays, filled in.
    assert!(html.contains("name=\"page1\" value=\"Foo_bar\""));
    let seen = fake.seen.lock().unwrap().clone();
    assert!(
        seen.iter().any(|q| q.contains("fromtitle=Foo+bar&torev=7")),
        "{seen:?}"
    );

    let (status, html) = get("/wiki/Special:ComparePages").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("name=\"rev2\""));
    assert!(!html.contains("ts-page-diff__table"));
    let (status, html) = get("/w/index.php?title=Special:ComparePages&page1=Foo").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("for both sides"));
    let (status, html) = get("/w/index.php?title=Special:ComparePages&rev1=x&page2=Baz").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(html.contains("is not a revision ID"));
    let (status, html) = get("/w/index.php?title=Special:ComparePages&rev1=5&rev2=99").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("does not exist"));
}

#[tokio::test]
async fn the_index_lists_them() {
    let (_, index) = get("/wiki/Special:SpecialPages").await;
    for t in [
        "Special:ComparePages",
        "Special:ProtectedTitles",
        "Special:NamespaceInfo",
        "Special:ListGroupRights",
        "Special:AllMessages",
        "Special:ListDatatypes",
        "Special:MyLanguageFallbackChain",
    ] {
        assert!(index.contains(&format!("href=\"/wiki/{t}\"")), "{t}");
    }
}
