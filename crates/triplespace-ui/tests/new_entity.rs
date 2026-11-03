//! `Special:NewItem` and `Special:NewProperty` without JavaScript, against a fake API that
//! keeps a CSRF token per session, knows three properties, and records each
//! `wbeditentity` it is sent.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tower::ServiceExt as _;
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, Client, ClientError, Transport};

#[derive(Clone, Default)]
struct FakeApi {
    /// No `wbeditentity` among the capabilities.
    no_create: bool,
    /// The `wbeditentity` forms received.
    writes: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
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
    fn answer(&self, r: &ApiRequest) -> Value {
        let q = &r.path_and_query;
        let who = session(&r.headers);
        if q.contains("meta=siteinfo") {
            let caps = if self.no_create {
                json!(["wbgetentities", "clientlogin"])
            } else {
                json!(["wbgetentities", "clientlogin", "wbeditentity"])
            };
            return json!({"query": {
                "general": {"sitename": "librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home",
                    // The types the real API registers (`siprop=general`).
                    "wikibase-propertytypes": {
                        "commonsMedia": {"valuetype": "string"}, "external-id": {"valuetype": "string"},
                        "geo-shape": {"valuetype": "string"}, "globe-coordinate": {"valuetype": "globecoordinate"},
                        "monolingualtext": {"valuetype": "monolingualtext"}, "quantity": {"valuetype": "quantity"},
                        "string": {"valuetype": "string"}, "tabular-data": {"valuetype": "string"},
                        "time": {"valuetype": "time"}, "url": {"valuetype": "string"},
                        "wikibase-domain": {"valuetype": "wikibase-entityid"},
                        "wikibase-item": {"valuetype": "wikibase-entityid"},
                        "wikibase-property": {"valuetype": "wikibase-entityid"}}},
                "triplespace": {"api_version": 2, "capabilities": caps}
            }});
        }
        if q.contains("meta=userinfo") {
            // Alice may create properties; Bob may not.
            let (id, name, rights) = match who.as_deref() {
                Some("alice") => (
                    7,
                    "Alice",
                    json!(["read", "edit", "createpage", "property-create"]),
                ),
                Some("bob") => (8, "Bob", json!(["read", "edit", "createpage"])),
                _ => {
                    return json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}});
                }
            };
            let mut u = json!({"id": id, "name": name});
            // The rights wherever `uiprop` asks for them: the page's own call asks for
            // `options|accountrequests|rights|groups`, the forms' for `rights` alone.
            if q.split('&').any(|p| {
                p.strip_prefix("uiprop=")
                    .is_some_and(|v| v.split('|').any(|x| x == "rights"))
            }) {
                u["rights"] = rights;
            }
            return json!({"query": {"userinfo": u}});
        }
        if q.contains("meta=tokens") {
            let sid = who.clone().unwrap_or_default();
            return json!({"query": {"tokens": {"csrftoken": format!("csrf-{sid}+\\")}}});
        }
        if q.contains("action=wbgetentities") {
            let mut entities = serde_json::Map::new();
            for (id, dt, label) in [
                ("P5", "external-id", "DOI"),
                ("P6", "time", "inception"),
                ("P7", "wikibase-item", "instance of"),
            ] {
                if q.contains(id) {
                    entities.insert(
                        id.into(),
                        json!({"id": id, "type": "property", "datatype": dt,
                            "labels": {"en": {"language": "en", "value": label}}}),
                    );
                }
            }
            if q.contains("P99") {
                entities.insert("P99".into(), json!({"id": "P99", "missing": ""}));
            }
            return json!({"entities": entities});
        }
        let fields = form(&r.body);
        if fields.get("action").map(String::as_str) == Some("wbeditentity") {
            let sid = who.clone().unwrap_or_default();
            if fields.get("token").map(String::as_str) != Some(&format!("csrf-{sid}+\\")) {
                return json!({"error": {"code": "badtoken", "info": "Invalid CSRF token."}});
            }
            if fields.get("new").map(String::as_str) == Some("property") && sid != "alice" {
                return json!({"error": {"code": "permissiondenied", "info": "denied"}});
            }
            let data: Value = serde_json::from_str(&fields["data"]).unwrap();
            if data["labels"]["en"]["value"] == "Taken" {
                return json!({"error": {"code": "modification-failed", "info": "Item Q1 already has label \"Taken\"."}});
            }
            self.writes.lock().unwrap().push(fields.clone());
            let (id, t) = if fields["new"] == "property" {
                ("P13", "property")
            } else {
                ("Q7", "item")
            };
            return json!({"entity": {"id": id, "type": t, "lastrevid": 40}, "success": 1});
        }
        json!({"error": {"code": "unknown_action", "info": q}})
    }
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let body = self.answer(&request);
            let mut headers = HeaderMap::new();
            let public = request.path_and_query.contains("meta=siteinfo")
                || request.path_and_query.contains("wbgetentities");
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

async fn send(api: &FakeApi, req: Request<Body>) -> Answer {
    let r = common::site_router(Client::new(api.clone()))
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

async fn get(api: &FakeApi, uri: &str, who: Option<&str>) -> Answer {
    let mut req = Request::get(uri).header(header::HOST, "librarybase.org");
    if let Some(s) = who {
        req = req.header(header::COOKIE, format!("triplespace_session={s}"));
    }
    send(api, req.body(Body::empty()).unwrap()).await
}

async fn post(api: &FakeApi, title: &str, who: &str, fields: &[(&str, &str)]) -> Answer {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    let req = Request::post(format!("/w/index.php?title={title}"))
        .header(header::HOST, "librarybase.org")
        .header(header::COOKIE, format!("triplespace_session={who}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    send(api, req).await
}

/// A page with its quotation marks spelled as the skin spells them, whichever renderer
/// drew it.
fn spelled(html: &str) -> String {
    html.replace("&#34;", "&quot;")
}

#[tokio::test]
async fn a_reader_is_asked_to_log_in() {
    let api = FakeApi::default();
    let a = get(&api, "/wiki/Special:NewItem", None).await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("Log in to create items and properties."));
    assert!(
        a.body
            .contains("href=\"/w/index.php?title=Special:UserLogin&amp;returnto=Special:NewItem\"")
    );
    assert!(!a.body.contains("name=\"token\""));
    // The New menu offers both forms.
    assert!(a.body.contains("href=\"/wiki/Special:NewItem\""));
    assert!(a.body.contains("href=\"/wiki/Special:NewProperty\""));
}

#[tokio::test]
async fn not_offered_without_the_capability() {
    let api = FakeApi {
        no_create: true,
        ..FakeApi::default()
    };
    let a = get(&api, "/wiki/Special:NewItem", Some("alice")).await;
    assert_eq!(a.status, StatusCode::NOT_FOUND);
    assert!(!a.body.contains("Special:NewProperty"));
    let p = post(&api, "Special:NewItem", "alice", &[("label", "x")]).await;
    assert_eq!(p.status, StatusCode::NOT_FOUND);
    assert!(api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_form_with_its_prefills() {
    let api = FakeApi::default();
    let a = get(
        &api,
        "/wiki/Special:CreateItem/Seven/a%20number?statement=P5:10.1000/182&statement=P6:1952&statement=P99:x&statement=nonsense&statement=P7:Q2",
        Some("alice"),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("<h1"), "{}", a.body);
    assert!(a.body.contains("Create a new item"));
    assert!(a.body.contains("value=\"csrf-alice+\\\""));
    assert!(
        a.body
            .contains("action=\"/w/index.php?title=Special:NewItem\"")
    );
    assert!(
        a.body
            .contains("name=\"label\" type=\"text\" value=\"Seven\"")
    );
    assert!(
        a.body
            .contains("name=\"description\" type=\"text\" value=\"a number\"")
    );
    assert!(a.body.contains("name=\"lang\" type=\"text\" value=\"en\""));
    assert!(
        !a.body.contains("name=\"datatype\""),
        "an item has no data type"
    );
    // Two can be added; three cannot, each saying why.
    assert!(a.body.contains("value=\"P5:10.1000/182\" checked"));
    assert!(a.body.contains("DOI (P5): 10.1000/182"));
    assert!(a.body.contains("instance of (P7): Q2"));
    assert!(
        a.body
            .contains("values of type Point in time cannot be filled in here yet")
    );
    assert!(a.body.contains("there is no property P99"));
    // Either renderer's spelling of a quotation mark (19 §5.10).
    assert!(spelled(&a.body).contains("&quot;nonsense&quot; is left out"));
    assert_eq!(a.headers[header::CACHE_CONTROL], "private, no-cache");
}

#[tokio::test]
async fn creating_an_item_goes_to_its_page() {
    let api = FakeApi::default();
    let a = post(
        &api,
        "Special:NewItem",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("lang", "en"),
            ("label", " Seven "),
            ("description", ""),
            ("aliases", "7|VII"),
            ("statement", "P5:10.1000/182"),
            ("statement", "P7:Q2"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::SEE_OTHER, "{}", a.body);
    assert_eq!(a.headers[header::LOCATION], "/wiki/Item:Q7");
    assert_eq!(a.headers[header::CACHE_CONTROL], "private, no-store");
    let writes = api.writes.lock().unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0]["new"], "item");
    let data: Value = serde_json::from_str(&writes[0]["data"]).unwrap();
    assert_eq!(
        data,
        json!({
            "labels": {"en": {"language": "en", "value": "Seven"}},
            "aliases": {"en": [{"language": "en", "value": "7"}, {"language": "en", "value": "VII"}]},
            "claims": [
                {"mainsnak": {"snaktype": "value", "property": "P5", "datavalue": {"type": "string", "value": "10.1000/182"}}, "type": "statement", "rank": "normal"},
                {"mainsnak": {"snaktype": "value", "property": "P7", "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "Q2"}}}, "type": "statement", "rank": "normal"},
            ],
        })
    );
}

#[tokio::test]
async fn refusals_show_the_form_again() {
    let api = FakeApi::default();
    // Nothing to create: refused before the API is asked.
    let a = post(
        &api,
        "Special:NewItem",
        "alice",
        &[("token", "csrf-alice+\\"), ("lang", "en"), ("label", " ")],
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(
        a.body
            .contains("Give at least a label, a description or an alias.")
    );
    // The API's refusal, with what was typed kept.
    let a = post(
        &api,
        "Special:NewItem",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("lang", "en"),
            ("label", "Taken"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(
        spelled(&a.body)
            .contains("Nothing was created: Item Q1 already has label &quot;Taken&quot;.")
    );
    assert!(a.body.contains("value=\"Taken\""));
    // An expired token: a fresh one, and the form as sent.
    let a = post(
        &api,
        "Special:NewItem",
        "alice",
        &[("token", "stale"), ("lang", "en"), ("label", "Eight")],
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("Your session expired"));
    assert!(a.body.contains("value=\"csrf-alice+\\\""));
    assert!(a.body.contains("value=\"Eight\""));
    assert!(api.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_property_needs_property_create() {
    let api = FakeApi::default();
    let a = get(&api, "/wiki/Special:NewProperty", Some("bob")).await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);
    assert!(
        a.body
            .contains("You do not have permission to create properties.")
    );
    assert!(!a.body.contains("name=\"token\""));
    let a = post(
        &api,
        "Special:NewProperty",
        "bob",
        &[("token", "csrf-bob+\\"), ("label", "x")],
    )
    .await;
    assert_eq!(a.status, StatusCode::FORBIDDEN);

    let a = get(
        &api,
        "/wiki/Special:NewProperty?datatype=external-id",
        Some("alice"),
    )
    .await;
    assert_eq!(a.status, StatusCode::OK);
    assert!(a.body.contains("Create a new property"));
    assert!(
        a.body
            .contains("<option value=\"external-id\" selected>External identifier</option>")
    );
    // Only the types the API registers: no `math`, which it does not.
    let select = a.body.split("name=\"datatype\">").nth(1).unwrap();
    let select = &select[..select.find("</select>").unwrap()];
    assert_eq!(select.matches("<option value=").count(), 13, "{select}");
    assert!(select.starts_with("<option value=\"wikibase-item\">Item</option>"));
    assert!(!a.body.contains("value=\"math\""));
    let a = post(
        &api,
        "Special:NewProperty",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("lang", "de"),
            ("label", "ISBN-13"),
            ("datatype", "external-id"),
        ],
    )
    .await;
    assert_eq!(a.status, StatusCode::SEE_OTHER, "{}", a.body);
    assert_eq!(a.headers[header::LOCATION], "/wiki/Property:P13");
    let writes = api.writes.lock().unwrap();
    let data: Value = serde_json::from_str(&writes[0]["data"]).unwrap();
    assert_eq!(
        data,
        json!({"labels": {"de": {"language": "de", "value": "ISBN-13"}}, "datatype": "external-id"})
    );
}

/// Both pages are listed under Wikibase's heading; `Special:NewProperty` only to a viewer
/// who may create properties, since the registry restricts it to `property-create`
/// (21 §1.1).
#[tokio::test]
async fn listed_on_special_pages() {
    let api = FakeApi::default();
    let new_item = "href=\"/wiki/Special:NewItem\">Create a new item</a>";
    let new_property = "href=\"/wiki/Special:NewProperty\">Create a new property</a>";
    let a = get(&api, "/wiki/Special:SpecialPages", Some("alice")).await;
    assert!(a.body.contains("id=\"mw-specialpagesgroup-wikibaserepo\""));
    assert!(a.body.contains(new_item) && a.body.contains(new_property));
    for who in [None, Some("bob")] {
        let a = get(&api, "/wiki/Special:SpecialPages", who).await;
        assert!(a.body.contains(new_item), "{who:?}");
        assert!(!a.body.contains(new_property), "{who:?}");
    }
}
