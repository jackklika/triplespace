//! The Protect form, `Special:UserRights`, `Special:ProtectedPages`, `Special:Block` and
//! the Visibility section, without JavaScript, against a fake API that answers as the real
//! one does for an owner (`alice`), a sysop in no confidential group (`bob`), and a
//! reader: what each is offered, what each form sends, and how a refusal comes back.

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

/// A write the site sent: its path and form fields.
type Write = (String, BTreeMap<String, String>);

#[derive(Clone, Default)]
struct FakeApi {
    /// Every write the site sent.
    writes: Arc<Mutex<Vec<Write>>>,
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

fn siteinfo() -> Value {
    json!({"query": {
        "general": {"sitename": "librarybase", "server": "https://librarybase.org", "lang": "en", "mainpage": "Project:Home"},
        "triplespace": {"api_version": 1, "tenant": "librarybase",
            "capabilities": ["wbgetentities", "clientlogin", "pages", "parse", "edit", "history", "recentchanges", "activity", "protect", "userrights", "block", "delete", "move", "settings"]},
        "usergroups": [
            {"name": "*", "rights": ["read"], "implicit": ""},
            {"name": "user", "rights": ["read", "edit"], "implicit": ""},
            {"name": "sysop", "rights": ["protect", "delete", "block"]},
            {"name": "board", "rights": []},
            {"name": "suppress", "rights": ["hideuser"]},
            {"name": "owner", "rights": ["*"]},
        ]
    }})
}

fn userinfo(s: Option<&str>) -> Value {
    match s {
        Some("alice") => json!({"query": {"userinfo": {"id": 7, "name": "Alice",
            "rights": ["read", "edit", "protect", "delete", "undelete", "userrights", "block", "hideuser", "ts-config", "move", "suppressredirect", "move-subpages"],
            "groups": ["*", "user", "owner", "board"],
            "changeablegroups": {"add": ["sysop", "board", "suppress"], "remove": ["sysop", "board", "suppress"], "add-self": ["sysop", "board", "suppress"], "remove-self": ["board"]}}}}),
        Some("bob") => json!({"query": {"userinfo": {"id": 8, "name": "Bob",
            "rights": ["read", "edit", "protect", "delete", "block", "move"], "groups": ["*", "user", "sysop"],
            "changeablegroups": {"add": [], "remove": [], "add-self": [], "remove-self": []}}}}),
        _ => json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
    }
}

impl FakeApi {
    #[allow(clippy::too_many_lines)]
    fn answer(&self, r: &ApiRequest) -> (StatusCode, Value) {
        let q = &r.path_and_query;
        let s = session(&r.headers);
        if q.contains("meta=siteinfo") {
            return (StatusCode::OK, siteinfo());
        }
        if q.contains("meta=userinfo") {
            return (StatusCode::OK, userinfo(s.as_deref()));
        }
        if q.contains("meta=tokens") {
            let kind = if q.contains("type=userrights") {
                "userrights"
            } else {
                "csrf"
            };
            return (
                StatusCode::OK,
                json!({"query": {"tokens": {format!("{kind}token"): format!("{kind}-{}+\\", s.clone().unwrap_or_default())}}}),
            );
        }
        if q.contains("prop=info") && q.contains("inprop=protection") {
            // Project:Secret is confidential to board: Alice sees it, Bob does not.
            let secret = q.contains("Project%3ASecret") || q.contains("Project:Secret");
            if secret && s.as_deref() != Some("alice") {
                return (
                    StatusCode::OK,
                    json!({"query": {"pages": [{"ns": 4, "title": "Project:Secret", "missing": true,
                        "protection": [], "restrictiontypes": ["edit", "move", "read", "create"]}]}}),
                );
            }
            let protection = if secret {
                json!([{"type": "read", "level": "board", "expiry": "infinity", "ts-talk": true},
                       {"type": "edit", "level": "sysop", "expiry": "infinity", "source": "acl:namespace:4"}])
            } else {
                json!([])
            };
            return (
                StatusCode::OK,
                json!({"query": {"pages": [{"pageid": 5, "ns": 4, "title": if secret { "Project:Secret" } else { "Project:Open" },
                    "protection": protection, "restrictiontypes": ["edit", "move", "read", "create"]}]}}),
            );
        }
        if q.contains("list=logevents") {
            // Project:Gone was deleted by Alice; Project:Open was not.
            if q.contains("Project%3AGone") || q.contains("Project:Gone") {
                return (
                    StatusCode::OK,
                    json!({"query": {"logevents": [{"logid": 9, "action": "delete", "type": "delete", "user": "Alice",
                        "timestamp": "2026-10-24T20:00:00Z", "comment": "/* delete/delete */ spam", "ns": 4, "pageid": 8,
                        "title": "Project:Gone", "params": {"talk": false}}]}}),
                );
            }
            return (StatusCode::OK, json!({"query": {"logevents": []}}));
        }
        if q.contains("prop=deletedrevisions") {
            return if s.as_deref() == Some("alice") {
                (
                    StatusCode::OK,
                    json!({"query": {"pages": [{"pageid": 8, "ns": 4, "title": "Project:Gone", "deletedrevisions": [
                        {"revid": 77, "user": "Alice", "timestamp": "2026-10-24T19:00:00Z", "comment": "", "slots": {"main": {"content": "the old text"}}}]}]}}),
                )
            } else {
                (
                    StatusCode::OK,
                    json!({"error": {"code": "permissiondenied", "info": "no"}}),
                )
            };
        }
        if q.contains("list=blocks") {
            let row = if s.as_deref() == Some("alice") {
                json!({"id": 11, "user": "Mallory", "userid": 9, "ts-hidden": true, "by": "Alice", "timestamp": "2026-10-24T20:00:00Z", "expiry": "infinite", "reason": "spam", "ts-removes": ["edit", "createpage"]})
            } else {
                json!({"id": 11, "userhidden": true, "userid": 9, "by": "Alice", "timestamp": "2026-10-24T20:00:00Z", "expiry": "infinite", "reason": "spam", "ts-removes": "all-but-read"})
            };
            return (StatusCode::OK, json!({"query": {"blocks": [row]}}));
        }
        if q.contains("/activity") {
            let row = json!({"kind": "log", "time": "2026-10-24T20:00:00Z", "source": "log",
                "record": {"partition": 1, "offset": 4}, "logid": 9,
                "target": {"kind": "page", "id": "8", "ns": 4, "title": "Project:Gone"},
                "actor": {"name": "Alice", "key": "x:7", "kind": "registered"},
                "summary": {"op": "delete/delete", "text": "spam"}, "comment": "spam", "tags": [],
                "flags": {"bot": false, "minor": false, "new": false}, "patrolled": true,
                "log": {"type": "delete", "action": "delete"}});
            let rows = if q.contains("log_type=rights") {
                json!([])
            } else {
                json!([row])
            };
            return (StatusCode::OK, json!({"rows": rows}));
        }
        if q.contains("prop=info") {
            return (
                StatusCode::OK,
                json!({"query": {"pages": [{"pageid": 5, "ns": 4, "title": "Project:Open", "contentmodel": "wikitext"}]}}),
            );
        }
        if q.contains("action=parse")
            && [
                "Project%3AGone",
                "Project:Gone",
                "Project%3ANever",
                "Project:Never",
            ]
            .iter()
            .any(|t| q.contains(t))
        {
            return (
                StatusCode::OK,
                json!({"error": {"code": "missingtitle", "info": "no"}}),
            );
        }
        if q.contains("action=parse") {
            return (
                StatusCode::OK,
                json!({"parse": {"title": "Project:Open", "pageid": 5, "revid": 3, "text": "<p>open</p>", "categories": [], "links": []}}),
            );
        }
        if q.contains("list=users") {
            if q.contains("Nobody") {
                return (
                    StatusCode::OK,
                    json!({"query": {"users": [{"name": "Nobody", "missing": true}]}}),
                );
            }
            let name = if q.contains("ususers=Alice") {
                "Alice"
            } else {
                "Carol"
            };
            return (
                StatusCode::OK,
                json!({"query": {"users": [{"userid": 9, "name": name,
                    "groupmemberships": [{"group": "board", "expiry": "2027-01-01T00:00:00Z"}],
                    "blockexpiry": "infinite"}]}}),
            );
        }
        if q.contains("list=protectedpages") {
            return (
                StatusCode::OK,
                json!({"query": {
                    "protectedpages": [{"pageid": 5, "ns": 4, "title": "Project:Secret", "type": "read", "level": "board", "expiry": "infinity"}],
                    "protectedtitles": [{"ns": 4, "title": "Project:Reserved", "level": "sysop", "expiry": "infinity"}]}}),
            );
        }
        if q.contains("list=protectedsets") {
            return (
                StatusCode::OK,
                json!({"query": {"protectedsets": [{"id": 3, "name": "Board docs",
                    "protection": [{"type": "read", "level": "board", "expiry": "infinity"}],
                    "members": [{"pageid": 5, "ns": 4, "title": "Project:Secret"}]}]}}),
            );
        }
        if q.contains("/acl/empty-groups") {
            return if s.as_deref() == Some("alice") {
                (
                    StatusCode::OK,
                    json!({"groups": [{"group": "vault", "restrictions": 2}]}),
                )
            } else {
                (
                    StatusCode::FORBIDDEN,
                    json!({"code": "permissiondenied", "message": "no"}),
                )
            };
        }
        if q.contains("/acl/tenant/") && r.method == axum::http::Method::GET {
            return (
                StatusCode::OK,
                json!({"target": "acl:tenant:librarybase", "acl": null, "visibility": {"public": true, "groups": []}}),
            );
        }
        if q.contains("/settings/schema") {
            return (
                StatusCode::OK,
                json!({"scope": "tenant", "groups": [{"id": "site", "label": "ts-settings-group-site"}],
                    "settings": [{"key": "site.name", "group": "site", "type": "line", "label": "ts-setting-site-name", "help": "ts-setting-site-name-help"}],
                    "template": "none"}),
            );
        }
        if q.contains("/settings") {
            return (
                StatusCode::OK,
                json!({"scope": "tenant", "base": 1, "settings": {"site.name": {"value": "librarybase", "origin": "tenant", "can_change": true}}}),
            );
        }
        if r.method == axum::http::Method::POST && q.ends_with("/w/api.php") {
            let f = form(&r.body);
            if f.get("action").map(String::as_str) == Some("parse") {
                return (
                    StatusCode::OK,
                    json!({"parse": {"title": "Project:Gone", "text": "<p>the old text</p>", "categories": [], "templates": []}}),
                );
            }
            self.writes.lock().unwrap().push((q.clone(), f.clone()));
            let action = f.get("action").cloned().unwrap_or_default();
            // Bob is outside board: a read restriction to it locks him out.
            if action == "protect"
                && s.as_deref() == Some("bob")
                && f.get("protections")
                    .is_some_and(|p| p.contains("read=board"))
            {
                return (
                    StatusCode::OK,
                    json!({"error": {"code": "ts-locked-out", "info": "locked out"}}),
                );
            }
            if action == "userrights" && f.get("add").is_some_and(|a| a.contains("owner")) {
                return (
                    StatusCode::OK,
                    json!({"error": {"code": "permissiondenied", "info": "You may not add owner."}}),
                );
            }
            if action == "block"
                && f.contains_key("hidename")
                && f.get("expiry").is_some_and(|e| e != "infinite")
            {
                return (
                    StatusCode::OK,
                    json!({"error": {"code": "ipb_expiry_temp", "info": "indefinite only"}}),
                );
            }
            if action == "move" && f.get("to").is_some_and(|t| t == "Project:Taken") {
                return (
                    StatusCode::OK,
                    json!({"error": {"code": "articleexists", "info": "taken"}}),
                );
            }
            if action == "move" {
                return (
                    StatusCode::OK,
                    json!({"move": {"from": f["from"], "to": f["to"], "redirectcreated": true}}),
                );
            }
            return (StatusCode::OK, json!({(action): {"result": "ok"}}));
        }
        if q.contains("/acl/") {
            let f: BTreeMap<String, String> = serde_json::from_slice::<Value>(&r.body)
                .ok()
                .and_then(|v| {
                    v.as_object()
                        .map(|o| o.iter().map(|(k, v)| (k.clone(), v.to_string())).collect())
                })
                .unwrap_or_default();
            self.writes.lock().unwrap().push((q.clone(), f));
            return (StatusCode::OK, json!({"id": 4, "target": "acl:set:4"}));
        }
        (
            StatusCode::OK,
            json!({"error": {"code": "unknown", "info": q}}),
        )
    }
}

impl Transport for FakeApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let (status, body) = self.answer(&request);
            let mut headers = HeaderMap::new();
            headers.insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());
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

async fn send(api: &FakeApi, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let r = common::site_router(Client::new(api.clone()))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

async fn get(api: &FakeApi, uri: &str, who: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::get(uri).header(header::HOST, "librarybase.org");
    if let Some(w) = who {
        req = req.header(header::COOKIE, format!("triplespace_session={w}"));
    }
    let (s, _, b) = send(api, req.body(Body::empty()).unwrap()).await;
    (s, b)
}

async fn post(
    api: &FakeApi,
    uri: &str,
    who: &str,
    fields: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish();
    let req = Request::post(uri)
        .header(header::HOST, "librarybase.org")
        .header(header::COOKIE, format!("triplespace_session={who}"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap();
    send(api, req).await
}

fn last_write(api: &FakeApi) -> Write {
    api.writes.lock().unwrap().last().cloned().expect("a write")
}

#[tokio::test]
async fn the_protect_form_shows_the_rows_and_the_enclosing_restrictions() {
    let api = FakeApi::default();
    let (status, body) = get(
        &api,
        "/w/index.php?title=Project:Secret&action=protect",
        Some("alice"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Protect Project:Secret"), "{body}");
    for row in ["level-edit", "level-move", "level-read"] {
        assert!(body.contains(&format!("name=\"{row}\"")), "{row}: {body}");
    }
    // The page exists: nothing to create.
    assert!(!body.contains("name=\"level-create\""), "{body}");
    // The read row shows board selected, the talk checkbox on, and the viewer's groups.
    assert!(body.contains("<option value=\"board\" selected>"), "{body}");
    assert!(body.contains("name=\"talk\" value=\"1\" checked"), "{body}");
    assert!(
        body.contains("Your groups: *, user, owner, board"),
        "{body}"
    );
    // The namespace's edit restriction is listed, not editable.
    assert!(body.contains("acl:namespace:4"), "{body}");
    // Implicit groups are not offered.
    assert!(!body.contains("<option value=\"user\">"), "{body}");
}

#[tokio::test]
async fn the_protect_form_posts_and_shows_a_refusal() {
    let api = FakeApi::default();
    let (status, headers, _) = post(
        &api,
        "/w/index.php?title=Project:Open&action=protect",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("level-edit", "sysop"),
            ("level-read", ""),
            ("talk", "1"),
            ("expiry", "1 week"),
            ("reason", "why"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("action=protect&done=1")
    );
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "protect");
    assert_eq!(f["title"], "Project:Open");
    assert_eq!(f["protections"], "edit=sysop|read=all");
    assert_eq!(f["expiry"], "1 week");
    assert_eq!(f["tstalk"], "1");
    assert_eq!(f["token"], "csrf-alice+\\");
    // Bob, outside board, is locked out, and the form comes back with his choice kept.
    let (status, _, body) = post(
        &api,
        "/w/index.php?title=Project:Open&action=protect",
        "bob",
        &[
            ("token", "csrf-bob+\\"),
            ("level-read", "board"),
            ("expiry", "infinite"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("You must be a member of the group you restrict reading to"),
        "{body}"
    );
    assert!(body.contains("<option value=\"board\" selected>"), "{body}");
}

#[tokio::test]
async fn protect_is_offered_to_its_holders_only() {
    let api = FakeApi::default();
    let (_, body) = get(&api, "/wiki/Project:Open", Some("alice")).await;
    assert!(body.contains("action=protect"), "{body}");
    let (_, body) = get(&api, "/wiki/Project:Open", None).await;
    assert!(!body.contains("action=protect"), "{body}");
    let (status, body) = get(&api, "/w/index.php?title=Project:Open&action=protect", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Log in to protect pages."), "{body}");
}

#[tokio::test]
async fn userrights_lists_groups_as_checkboxes_the_viewer_may_change() {
    let api = FakeApi::default();
    let (status, body) = get(&api, "/wiki/Special:UserRights/Carol", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("User rights of Carol"), "{body}");
    // A member of board, until its expiry; changeable by Alice.
    assert!(
        body.contains("name=\"group-board\" value=\"1\" checked>"),
        "{body}"
    );
    assert!(body.contains("until 2027-01-01T00:00:00Z"), "{body}");
    // Alice may not change owner, which is disabled, not hidden.
    assert!(
        body.contains("name=\"group-owner\" value=\"1\" disabled>"),
        "{body}"
    );
    assert!(body.contains("Carol is blocked"), "{body}");
    assert!(
        body.contains("name=\"expiry\" type=\"text\" value=\"infinite\""),
        "{body}"
    );
    // Her own page proposes a short expiry.
    let (_, body) = get(&api, "/wiki/Special:UserRights/Alice", Some("alice")).await;
    assert!(body.contains("value=\"1 day\""), "{body}");
    // A sysop without userrights sees every checkbox disabled.
    let (_, body) = get(&api, "/wiki/Special:UserRights/Carol", Some("bob")).await;
    assert!(
        body.contains("You may not change any of these groups."),
        "{body}"
    );
    let (_, body) = get(&api, "/wiki/Special:UserRights/Nobody", Some("alice")).await;
    assert!(body.contains("There is no account by that name."), "{body}");
}

#[tokio::test]
async fn userrights_sends_the_difference() {
    let api = FakeApi::default();
    let (status, headers, _) = post(
        &api,
        "/wiki/Special:UserRights/Carol",
        "alice",
        &[
            ("token", "userrights-alice+\\"),
            ("was-board", "1"),
            ("was-sysop", "0"),
            ("group-sysop", "1"),
            ("expiry", "1 week"),
            ("reason", "r"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with("Special:UserRights/Carol?done=1")
    );
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "userrights");
    assert_eq!(f["user"], "Carol");
    assert_eq!(f["add"], "sysop");
    assert_eq!(f["remove"], "board");
    assert_eq!(f["expiry"], "1 week");
    let (status, _, body) = post(
        &api,
        "/wiki/Special:UserRights/Carol",
        "alice",
        &[
            ("token", "userrights-alice+\\"),
            ("was-owner", "0"),
            ("group-owner", "1"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("You may not make that change: You may not add owner."),
        "{body}"
    );
}

#[tokio::test]
async fn protectedpages_lists_and_manages_sets() {
    let api = FakeApi::default();
    let (status, body) = get(&api, "/wiki/Special:ProtectedPages", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("title=Project:Secret&amp;action=protect"),
        "{body}"
    );
    assert!(body.contains("Project:Reserved"), "{body}");
    assert!(body.contains("Board docs"), "{body}");
    assert!(body.contains("vault: named by 2 restrictions"), "{body}");
    assert!(body.contains("name=\"do\" value=\"create\""), "{body}");
    // Bob holds protect but not userrights: forms, no empty-group report.
    let (_, body) = get(&api, "/wiki/Special:ProtectedPages", Some("bob")).await;
    assert!(body.contains("name=\"do\" value=\"create\""), "{body}");
    assert!(!body.contains("id=\"empty-groups\""), "{body}");
    // A reader: no forms.
    let (_, body) = get(&api, "/wiki/Special:ProtectedPages", None).await;
    assert!(!body.contains("name=\"do\""), "{body}");
    let (status, _, _) = post(
        &api,
        "/wiki/Special:ProtectedPages",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("do", "add-member"),
            ("set", "3"),
            ("member", "Project:Open"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (path, f) = last_write(&api);
    assert!(path.ends_with("/acl/set/3/members"), "{path}");
    assert_eq!(f["id"], "5");
    assert_eq!(f["kind"], "\"page\"");
}

#[tokio::test]
async fn block_offers_hiding_to_its_holders_and_relays_refusals() {
    let api = FakeApi::default();
    let (_, body) = get(&api, "/wiki/Special:Block/Carol", Some("alice")).await;
    assert!(body.contains("value=\"Carol\""), "{body}");
    assert!(body.contains("name=\"hidename\""), "{body}");
    let (_, body) = get(&api, "/wiki/Special:Block", Some("bob")).await;
    assert!(!body.contains("name=\"hidename\""), "{body}");
    let (_, body) = get(&api, "/wiki/Special:Unblock", Some("bob")).await;
    assert!(body.contains("it stays hidden"), "{body}");
    let (status, _, body) = post(
        &api,
        "/wiki/Special:Block",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("user", "Carol"),
            ("expiry", "1 week"),
            ("hidename", "1"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("only with an indefinite block"), "{body}");
    let (status, headers, _) = post(
        &api,
        "/wiki/Special:Block",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("user", "Carol"),
            ("expiry", "infinite"),
            ("nocreate", "1"),
            ("reason", "spam"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .contains("done=1")
    );
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "block");
    assert_eq!(f["nocreate"], "1");
    assert!(!f.contains_key("hidename"));
    let (status, _, _) = post(
        &api,
        "/wiki/Special:Unblock",
        "alice",
        &[("token", "csrf-alice+\\"), ("user", "Carol")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert_eq!(last_write(&api).1["action"], "unblock");
}

#[tokio::test]
async fn the_visibility_section_sets_the_tenant_acl() {
    let api = FakeApi::default();
    let (status, body) = get(&api, "/wiki/Special:WikiSettings", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("id=\"visibility\""), "{body}");
    assert!(
        body.contains("Who may read this wiki now: Anyone"),
        "{body}"
    );
    let (status, headers, _) = post(
        &api,
        "/wiki/Special:WikiSettings",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("section", "visibility"),
            ("visibility", "group"),
            ("group", "board"),
            ("reason", "r"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "{}",
        headers[header::LOCATION].to_str().unwrap()
    );
    let (path, f) = last_write(&api);
    assert!(path.ends_with("/acl/tenant/librarybase"), "{path}");
    assert_eq!(f["restrictions"], "{\"read\":{\"group\":\"board\"}}");
    let (status, _, body) = post(
        &api,
        "/wiki/Special:WikiSettings",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("section", "visibility"),
            ("visibility", "group"),
            ("group", ""),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("Choose a group."), "{body}");
}

#[tokio::test]
async fn a_deleted_page_shows_its_deletion_and_the_way_back() {
    let api = FakeApi::default();
    // Everyone sees the deletion; only the group reads the content and may undelete.
    let (status, body) = get(&api, "/wiki/Project:Gone", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(
        body.contains("This page was deleted by Alice on 2026-10-24T20:00:00Z: spam"),
        "{body}"
    );
    assert!(!body.contains("the old text"), "{body}");
    assert!(!body.contains("class=\"ts-undelete\""), "{body}");
    let (status, body) = get(&api, "/wiki/Project:Gone", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("the old text"), "{body}");
    assert!(body.contains("action=undelete"), "{body}");
    // A page with no deletion is the ordinary missing page.
    let (status, body) = get(&api, "/wiki/Project:Never", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("was deleted"), "{body}");
    let (status, _, _) = post(
        &api,
        "/w/index.php?title=Project:Gone&action=undelete",
        "alice",
        &[("token", "csrf-alice+\\"), ("reason", "back")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "undelete");
    assert_eq!(f["title"], "Project:Gone");
    assert_eq!(f["reason"], "back");
}

#[tokio::test]
async fn the_delete_form_posts_with_the_talk_page() {
    let api = FakeApi::default();
    let (_, body) = get(&api, "/wiki/Project:Open", Some("alice")).await;
    assert!(body.contains("action=delete"), "{body}");
    let (_, body) = get(&api, "/wiki/Project:Open", Some("bob")).await;
    assert!(
        body.contains("action=delete"),
        "a sysop holds delete: {body}"
    );
    let (status, body) = get(
        &api,
        "/w/index.php?title=Project:Open&action=delete",
        Some("alice"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Delete Project:Open"), "{body}");
    assert!(body.contains("name=\"talk\" value=\"1\" checked"), "{body}");
    let (status, headers, _) = post(
        &api,
        "/w/index.php?title=Project:Open&action=delete",
        "alice",
        &[("token", "csrf-alice+\\"), ("reason", "why"), ("talk", "1")],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with("/wiki/Project:Open")
    );
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "delete");
    assert_eq!(f["deletetalk"], "1");
    assert_eq!(f["reason"], "why");
    let (status, body) = get(&api, "/w/index.php?title=Project:Open&action=delete", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Log in to do this."), "{body}");
}

#[tokio::test]
async fn blocklist_and_log_and_undelete_index() {
    let api = FakeApi::default();
    let (status, body) = get(&api, "/wiki/Special:BlockList", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("(username hidden)"), "{body}");
    assert!(body.contains("everything but reading"), "{body}");
    assert!(!body.contains("Special:Unblock"), "{body}");
    let (_, body) = get(&api, "/wiki/Special:BlockList", Some("alice")).await;
    assert!(body.contains("Mallory"), "{body}");
    assert!(body.contains("(name hidden)"), "{body}");
    assert!(body.contains("/wiki/Special:Unblock/Mallory"), "{body}");
    let (status, body) = get(
        &api,
        "/w/index.php?title=Special:Log&type=delete&page=Project:Gone",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<option value=\"delete\" selected"), "{body}");
    assert!(body.contains("value=\"Project:Gone\""), "{body}");
    assert!(body.contains("ts-change--log"), "{body}");
    let (_, body) = get(&api, "/wiki/Special:Log/rights", None).await;
    assert!(body.contains("<option value=\"rights\" selected"), "{body}");
    assert!(body.contains("No changes match these filters."), "{body}");
    let (_, body) = get(
        &api,
        "/w/index.php?title=Special:Log&page=Project:Never",
        None,
    )
    .await;
    assert!(body.contains("No changes match these filters."), "{body}");
    let (status, body) = get(&api, "/wiki/Special:Undelete", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("/wiki/Special:Undelete/Project:Gone"),
        "{body}"
    );
    let (status, body) = get(&api, "/wiki/Special:Undelete/Project:Gone", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("the old text"), "{body}");
}

#[tokio::test]
async fn the_move_form_posts_and_lands_on_the_new_title() {
    let api = FakeApi::default();
    // The overflow item, for a holder of `move`.
    let (_, body) = get(&api, "/wiki/Project:Open", Some("bob")).await;
    assert!(
        body.contains("/wiki/Special:MovePage/Project:Open"),
        "{body}"
    );
    let (_, body) = get(&api, "/wiki/Project:Open", None).await;
    assert!(!body.contains("Special:MovePage"), "{body}");
    // The form: the options only for their holders.
    let (status, body) = get(&api, "/wiki/Special:MovePage/Project:Open", Some("alice")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Move Project:Open"), "{body}");
    assert!(
        body.contains("name=\"to\" value=\"Project:Open\""),
        "{body}"
    );
    assert!(
        body.contains("name=\"redirect\" value=\"1\" checked"),
        "{body}"
    );
    assert!(body.contains("name=\"subpages\""), "{body}");
    let (_, body) = get(&api, "/wiki/Special:MovePage/Project:Open", Some("bob")).await;
    assert!(!body.contains("name=\"redirect\""), "{body}");
    assert!(!body.contains("name=\"subpages\""), "{body}");
    let (status, body) = get(&api, "/wiki/Special:MovePage/Project:Open", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Log in to do this."), "{body}");
    // The post: unchecking the redirect box sends `noredirect`; the move lands on the
    // new title.
    let (status, headers, _) = post(
        &api,
        "/wiki/Special:MovePage/Project:Open",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("to", "Project:Opened"),
            ("reason", "why"),
            ("subpages", "1"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        headers[header::LOCATION]
            .to_str()
            .unwrap()
            .ends_with("/wiki/Project:Opened")
    );
    let (_, f) = last_write(&api);
    assert_eq!(f["action"], "move");
    assert_eq!(f["from"], "Project:Open");
    assert_eq!(f["to"], "Project:Opened");
    assert_eq!(f["noredirect"], "1");
    assert_eq!(f["movesubpages"], "1");
    // Bob cannot suppress the redirect, so an absent box means one is left.
    post(
        &api,
        "/wiki/Special:MovePage/Project:Open",
        "bob",
        &[("token", "csrf-bob+\\"), ("to", "Project:Opened")],
    )
    .await;
    assert!(!last_write(&api).1.contains_key("noredirect"));
    // A refusal is shown over the form, with what was typed.
    let (status, _, body) = post(
        &api,
        "/wiki/Special:MovePage/Project:Open",
        "alice",
        &[
            ("token", "csrf-alice+\\"),
            ("to", "Project:Taken"),
            ("redirect", "1"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("already exists"), "{body}");
    assert!(body.contains("value=\"Project:Taken\""), "{body}");
}
