//! The thread routes end to end (0019 §9; 18 §3.2): starting a thread on a page's talk
//! page, reading it in tree order, replies, statuses, collapsing, editing with a base,
//! pinning and the talk page's listing, and the refusals. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::instance;

const DB: &str = "tsthreads_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
const REST: &str = "/w/rest.php/triplespace/v0";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

/// A fresh database for one test; each test in this file has its own, so they run in
/// parallel.
async fn fresh_database(name: &str) -> String {
    let db = format!("{DB}_{name}");
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {db} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {db}"))
        .await
        .expect("create database");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{db}"))
        .unwrap()
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A client with one session cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: Value,
}

impl Client {
    async fn send(
        &mut self,
        method: &str,
        uri: &str,
        content_type: Option<&str>,
        body: Option<String>,
        extra: &[(header::HeaderName, &str)],
    ) -> Reply {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(ct) = content_type {
            req = req.header(header::CONTENT_TYPE, ct);
        }
        for (name, value) in extra {
            req = req.header(name, *value);
        }
        let mut req = req.body(body.map_or_else(Body::empty, Body::from)).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([203, 0, 113, 9], 50000))));
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(sc) = headers.get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        Reply { status, body }
    }

    async fn api(&mut self, query: &str) -> Value {
        let r = self
            .send(
                "GET",
                &format!("/w/api.php?{query}&format=json&formatversion=2"),
                None,
                None,
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn api_post(&mut self, form: &str) -> Value {
        let r = self
            .send(
                "POST",
                "/w/api.php",
                Some("application/x-www-form-urlencoded"),
                Some(format!("{form}&format=json&formatversion=2")),
                &[],
            )
            .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.body);
        r.body
    }

    async fn rest(&mut self, method: &str, path: &str, body: Option<Value>) -> Reply {
        let body = body.map(|b| b.to_string());
        let ct = body.as_ref().map(|_| "application/json");
        self.send(method, &format!("{REST}{path}"), ct, body, &[])
            .await
    }

    async fn csrf(&mut self) -> String {
        self.api("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string()
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn threads_over_rest() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database("rest").await;
    let dir = std::env::temp_dir().join(format!("tsthreads-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let writer_app = app.clone();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };

    // Anyone may read; writing needs an account.
    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Hi", "target": {"kind": "page", "id": 1}, "text": "x"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(
        c.rest("GET", "/thread/999999", None).await.status,
        StatusCode::NOT_FOUND
    );

    let t = c.api("action=query&meta=tokens&type=login").await;
    let login = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c
        .api_post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&login)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let token = c.csrf().await;

    // A page to talk about.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=What%20we%20cover.&createonly=1&token={}",
            enc(&token)
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().expect("the page was created");

    // A session's write needs its token.
    let start = json!({"subject": "why_is P31 wrong", "target": {"kind": "page", "id": page}, "text": "Because."});
    let r = c.rest("POST", "/thread", Some(start.clone())).await;
    assert_eq!(r.body["code"], "badtoken", "{}", r.body);

    let mut with = start.clone();
    with["token"] = json!(token);
    let r = c.rest("POST", "/thread", Some(with)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    assert!(
        r.body["title"]
            .as_str()
            .unwrap()
            .ends_with("/Why is P31 wrong"),
        "{}",
        r.body
    );

    // Replies, a status, and a collapse.
    let path = format!("/thread/{thread}");
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": opening, "text": "Really?", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let reply = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": reply, "text": "Yes.", "status": "resolved", "token": token})),
        )
        .await;
    let answer = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest("POST", &format!("{path}/post"), Some(json!({"text": "Off topic.", "collapse": {"first": reply, "last": answer}, "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"inReplyTo": 12_345_678, "text": "?", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-not-a-post", "{}", r.body);
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"text": "x", "status": "nonsense", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-unknown-status", "{}", r.body);

    // The thread in tree order.
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let t = &r.body;
    assert_eq!(t["subject"], "Why is P31 wrong");
    assert_eq!(t["status"], "resolved");
    assert_eq!(t["category"], "closed");
    assert_eq!(t["home"]["talk_page"]["ns"], 5, "{t}");
    assert_eq!(t["home"]["talk_page"]["title"], "Scope");
    let posts = t["posts"].as_array().unwrap();
    let ids: Vec<u64> = posts.iter().map(|p| p["id"].as_u64().unwrap()).collect();
    assert_eq!(ids[..3], [opening, reply, answer]);
    assert_eq!(posts[2]["depth"], 2);
    assert_eq!(posts[1]["collapsed"], true);
    assert_eq!(posts[0]["content"], "Because.");
    assert_eq!(posts[3]["collapse"]["first"], reply);
    let base = t["base"].as_u64().unwrap();

    // An edit needs a base, and the thread as it stands.
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-base-required", "{}", r.body);
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "base": 1, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{opening}"),
            Some(json!({"text": "Because, really.", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let base = r.body["base"].as_u64().unwrap();
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.body["posts"][0]["content"], "Because, really.");
    assert!(r.body["posts"][0]["updated"].is_string());

    // Pin it, and read the talk page's listing: a second thread, pinned first.
    let r = c
        .rest("POST", &format!("{path}/pin"), Some(json!({"target": {"kind": "page", "id": page}, "base": base, "summary": "FAQ", "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let mut second = start.clone();
    second["subject"] = json!("Another question");
    second["token"] = json!(token);
    assert_eq!(
        c.rest("POST", "/thread", Some(second)).await.status,
        StatusCode::CREATED
    );
    let talk = c.rest("GET", &path, None).await.body["home"]["talk_page_id"]
        .as_u64()
        .unwrap();
    let r = c.rest("GET", &format!("/page/{talk}/threads"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let listed = r.body["threads"].as_array().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["id"], thread);
    assert!(listed[0]["pinned"].is_string());
    assert_eq!(listed[0]["category"], "closed");
    assert_eq!(listed[1]["subject"], "Another question");
    assert_eq!(
        c.rest("GET", &format!("/page/{page}/threads"), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // Rename keeps the date.
    let base = c.rest("GET", &path, None).await.body["base"]
        .as_u64()
        .unwrap();
    let r = c
        .rest(
            "POST",
            &format!("{path}/rename"),
            Some(json!({"subject": "P31 is fine", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(r.body["title"].as_str().unwrap().ends_with("/P31 is fine"));

    // Every thread record is an activity row on the thread (16 §6.1). The talk page's
    // history is composite: its threads' rows, oldest first here.
    let r = c
        .rest("GET", &format!("/page/{talk}/history?dir=newer"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let rows = r.body["rows"].as_array().unwrap();
    let ops: Vec<&str> = rows
        .iter()
        .map(|row| row["summary"]["op"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(
        ops,
        [
            "create", "post", "post", "post", "edit", "pin", "create", "rename"
        ],
        "{}",
        r.body
    );
    assert_eq!(rows[0]["target"]["id"], thread.to_string());
    assert_eq!(rows[0]["flags"]["new"], true);
    assert_eq!(rows[0]["log"]["type"], "create");
    assert!(rows[0]["logid"].is_u64(), "{}", rows[0]);
    assert!(rows[1].get("logid").is_none() && rows[1].get("log").is_none());
    assert_eq!(rows[5]["log"]["action"], "pin");
    assert_eq!(rows[7]["log"]["type"], "move");
    // A thread's own history is its rows; the subject page's is its own.
    let r = c
        .rest("GET", &format!("/page/{thread}/history"), None)
        .await;
    assert_eq!(r.body["rows"].as_array().unwrap().len(), 7, "{}", r.body);
    let r = c.rest("GET", &format!("/page/{page}/history"), None).await;
    assert_eq!(r.body["rows"].as_array().unwrap().len(), 1, "{}", r.body);
    assert_eq!(
        c.rest("GET", "/page/999999/history", None).await.status,
        StatusCode::NOT_FOUND
    );
    // The log events, in the log; posts are not log events.
    let l = c
        .api("action=query&list=logevents&leprop=type%7Ctitle%7Cids&lelimit=50")
        .await;
    let events: Vec<String> = l["query"]["logevents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            format!(
                "{}/{}",
                e["type"].as_str().unwrap_or("?"),
                e["action"].as_str().unwrap_or("?")
            )
        })
        .collect();
    for want in ["create/create", "thread/pin", "move/move"] {
        assert!(events.iter().any(|e| e == want), "{want} in {events:?}");
    }
    assert!(!events.iter().any(|e| e.starts_with("thread/post")));
    // Recent changes show posts as edits to the thread.
    let rc = c
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=50")
        .await;
    assert!(
        rc["query"]["recentchanges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["revid"] == reply && e["ns"] == 214),
        "{rc}"
    );

    // `prop=revisions` lists a thread's revisions, and each one's content is the thread as
    // it stood after it (0041 §5): the `GET /thread` body.
    let title = c.rest("GET", &path, None).await.body["title"]
        .as_str()
        .unwrap()
        .to_string();
    let r = c
        .api(&format!(
            "action=query&prop=revisions&titles={}&rvprop=ids%7Ccontent&rvslots=main&rvlimit=50&rvdir=newer",
            enc(&format!("Thread:{title}"))
        ))
        .await;
    let revs = r["query"]["pages"][0]["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 7, "{r}");
    let as_of = |k: usize| -> Value {
        let slot = &revs[k]["slots"]["main"];
        assert_eq!(slot["contentmodel"], "triplespace-thread", "{slot}");
        serde_json::from_str(slot["content"].as_str().unwrap()).unwrap()
    };
    assert_eq!(as_of(0)["posts"].as_array().unwrap().len(), 1);
    assert_eq!(as_of(0)["status"], "open");
    assert_eq!(as_of(2)["status"], "resolved");
    assert!(as_of(4)["home"]["pinned"].is_null());
    assert!(as_of(5)["home"]["pinned"].is_string());
    assert_eq!(as_of(6)["subject"], "P31 is fine");
    let cmp = c
        .api(&format!("action=compare&fromrev={opening}&torev={reply}"))
        .await;
    assert!(cmp["compare"]["body"].is_string(), "{cmp}");
    let r = c
        .send(
            "GET",
            &format!("/w/api.php?action=parse&pageid={thread}&format=json&formatversion=2"),
            None,
            None,
            &[],
        )
        .await;
    assert_eq!(r.body["error"]["code"], "ts-not-supported", "{}", r.body);

    // A post's text can be revision-deleted now that it has an activity row.
    let off_topic = c.rest("GET", &path, None).await.body["posts"][3]["id"]
        .as_u64()
        .unwrap();
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={off_topic}&hide=content&reason=Spam&token={}",
            enc(&token)
        ))
        .await;
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true, "{h}");
    let mut viewer = Client {
        router: c.router.clone(),
        cookie: None,
    };
    let r = viewer.rest("GET", &path, None).await;
    let hidden = r.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == off_topic)
        .unwrap();
    assert_eq!(hidden["texthidden"], true, "{hidden}");
    assert!(hidden["content"].is_null(), "{hidden}");

    // Another actor's post, written through the one writer as Bob: Alice holds
    // `ts-editpost` (as `owner`), so she may edit it, and the post then names her.
    let bob = {
        let store = writer_app.store();
        let mut cx = scatter_projection::Backend::begin(store).await.unwrap();
        let w = triplespace_api_ingest::threads::write(
            store,
            writer_app.pipeline(),
            &mut cx,
            &scatter_ingest::Request::new(
                TENANT,
                scatter_ingest::Attestation::by("lbwiki:8"),
                triplespace_api_action::edit::now_micros(),
            ),
            triplespace_api_ingest::threads::ThreadWrite {
                thread: Some(thread),
                op: scatter_threads::ThreadOp::Post {
                    in_reply_to: Some(opening),
                    status: None,
                    media_type: None,
                    heading: None,
                    collapse: None,
                    uncollapse: None,
                    imported_from: None,
                },
                text: Some("Bob here.".into()),
                writer: scatter_threads::Writer::Editor,
                limits: scatter_threads::Limits::default(),
                date: None,
                signer: None,
            },
        )
        .await
        .unwrap();
        scatter_projection::Backend::commit(store, cx)
            .await
            .unwrap();
        w.revid
    };
    let read = c.rest("GET", &path, None).await;
    let alice = read.body["posts"][0]["attributedTo"].clone();
    let base = read.body["base"].as_u64().unwrap();
    let r = c
        .rest(
            "PUT",
            &format!("{path}/post/{bob}"),
            Some(json!({"text": "Bob here, typo fixed.", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let read = c.rest("GET", &path, None).await;
    let post = read.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == bob)
        .unwrap()
        .clone();
    assert_eq!(post["attributedTo"], "lbwiki:8", "{post}");
    assert_eq!(post["content"], "Bob here, typo fixed.");
    assert_eq!(post["editedBy"], json!([alice]), "{post}");
    // Her own edits name nobody.
    assert!(read.body["posts"][0].get("editedBy").is_none());

    // ACLs, as the moderation modules will write them (0023 §3–§5); written to view.acl
    // directly here, since those modules do not exist yet.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let acl =
        |target: String, group: &'static str, kind: &'static str, extra: Option<Value>| {
            let db = &db;
            async move {
                db.execute(
                "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
                 VALUES ($1, $2, $3, $4, $5, 0)
                 ON CONFLICT (tenant, target) DO UPDATE SET restrictions = EXCLUDED.restrictions,
                   read_kind = EXCLUDED.read_kind, extra = EXCLUDED.extra",
                &[&TENANT, &target, &json!({"read": {"group": group}}), &kind, &extra],
            )
            .await
            .unwrap();
            }
        };
    let pages: i64 = db
        .query_one(
            "SELECT partition FROM log.partition WHERE tenant = $1 AND name = 'pages'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    // Partition IDs are stored as `bigint`, bit for bit.
    let pages = pages.cast_unsigned();
    let offset_of = |revid: u64| {
        let db = &db;
        async move {
            let r = i64::try_from(revid).unwrap();
            db.query_one(
                "SELECT latest_offset FROM view.post WHERE tenant = $1 AND revid = $2",
                &[&TENANT, &r],
            )
            .await
            .unwrap()
            .get::<_, i64>(0)
        }
    };
    let mut anon = Client {
        router: c.router.clone(),
        cookie: None,
    };

    // A reply's text hidden from everyone but `owner`, which Alice is in; the answer's
    // author hidden by the suppression group, which nobody is in and `owner` does not
    // read as (0016 A37).
    let reply_offset = offset_of(reply).await;
    acl(
        format!("acl:record:{pages}:{reply_offset}"),
        "owner",
        "moderation",
        Some(json!({"parts": ["text"]})),
    )
    .await;
    let answer_offset = offset_of(answer).await;
    acl(
        format!("acl:record:{pages}:{answer_offset}"),
        "suppress",
        "moderation",
        Some(json!({"parts": ["attestation"]})),
    )
    .await;
    let note = |body: &Value, id: u64| {
        body["posts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == id)
            .cloned()
            .unwrap()
    };
    let seen = anon.rest("GET", &path, None).await.body;
    assert_eq!(note(&seen, reply)["texthidden"], true, "{seen}");
    assert_eq!(note(&seen, reply)["content"], Value::Null);
    assert_eq!(note(&seen, answer)["userhidden"], true);
    assert_eq!(note(&seen, answer)["attributedTo"], Value::Null);
    assert_eq!(note(&seen, answer)["content"], "Yes.");
    let seen = c.rest("GET", &path, None).await.body;
    assert_eq!(note(&seen, reply)["texthidden"], true);
    assert_eq!(
        note(&seen, reply)["content"],
        "Really?",
        "the group sees the text"
    );
    assert_eq!(note(&seen, answer)["attributedTo"], Value::Null);

    // A deleted thread: 410 outside its deletion group, readable and marked for it, off
    // the listing for everyone else, and closed to every write.
    acl(format!("acl:page:{thread}"), "owner", "moderation", None).await;
    let r = anon.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-deleted");
    let r = c.rest("GET", &path, None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["deleted"], true);
    let listed = anon
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"].as_array().unwrap().len(), 1, "{listed}");
    let listed = c
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"][0]["deleted"], true, "{listed}");
    let r = c
        .rest(
            "POST",
            &format!("{path}/post"),
            Some(json!({"text": "Still here?", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-deleted");

    // A confidential thread is absent for those outside its group; `owner` reads it by
    // definition (0016 A37).
    let other = listed["threads"][1]["id"].as_u64().unwrap();
    acl(format!("acl:page:{other}"), "board", "confidential", None).await;
    let r = anon.rest("GET", &format!("/thread/{other}"), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(r.body["code"], "ts-thread-not-found");
    let listed = anon
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await
        .body;
    assert_eq!(listed["threads"].as_array().unwrap().len(), 0, "{listed}");
    let r = c.rest("GET", &format!("/thread/{other}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    // A deleted page takes no new threads, and its talk page answers 410 outside the
    // group that deleted it.
    acl(
        format!("acl:page:{page}"),
        "suppress",
        "moderation",
        Some(json!({"talk": true})),
    )
    .await;
    let mut fresh = start.clone();
    fresh["subject"] = json!("A third question");
    fresh["token"] = json!(token);
    let r = c.rest("POST", "/thread", Some(fresh)).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(r.body["code"], "ts-no-target");
    let r = c.rest("GET", &format!("/page/{talk}/threads"), None).await;
    assert_eq!(r.status, StatusCode::GONE, "{}", r.body);
}

/// A fresh instance with Alice signed in: her client, its CSRF token, and a page to talk
/// about.
async fn signed_in(tag: &str) -> (Client, String, u64) {
    let database = fresh_database(tag).await;
    let dir = std::env::temp_dir().join(format!("tsthreads-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: Some("Library Base".into()),
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: None,
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
    };
    let t = c.api("action=query&meta=tokens&type=login").await;
    let login = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c
        .api_post(&format!(
            "action=clientlogin&username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(&login)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let token = c.csrf().await;
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=What%20we%20cover.&createonly=1&token={}",
            enc(&token)
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().expect("the page was created");
    (c, token, page)
}

/// The text and media type of each post, in tree order.
async fn posts(c: &mut Client, thread: u64) -> (Vec<(String, String)>, u64) {
    let r = c.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let posts = r.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["content"].as_str().unwrap_or_default().to_string(),
                p["mediaType"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    (posts, r.body["base"].as_u64().unwrap())
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn signatures_and_markup() {
    // 14 §1.6; 0019 A28.
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (mut c, token, page) = signed_in("markup").await;

    // A trailing signature is removed without comment.
    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Scope", "target": {"kind": "page", "id": page}, "text": "Does this cover maps? --~~~~", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    let path = format!("/thread/{thread}/post");
    let r = c
        .rest(
            "POST",
            &path,
            Some(json!({"inReplyTo": opening, "text": "It does. ~~~~ and ~~~ stay mid-text in markdown. \u{2014} ~~~~~\n", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    // A post that is only a signature is empty, unless it sets a status.
    let r = c
        .rest("POST", &path, Some(json!({"text": "~~~~", "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
    assert_eq!(r.body["code"], "ts-empty-post", "{}", r.body);
    let r = c
        .rest(
            "POST",
            &path,
            Some(json!({"text": " ~~~~ ", "status": "resolved", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    // A markdown tenant refuses a wikitext post.
    let r = c
        .rest(
            "POST",
            &path,
            Some(json!({"text": "'''Bold'''", "mediaType": "text/x-wiki", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-bad-media-type", "{}", r.body);

    let (p, _) = posts(&mut c, thread).await;
    assert_eq!(
        p[0],
        ("Does this cover maps?".into(), "text/markdown".into())
    );
    assert_eq!(
        p[1].0, "It does. ~~~~ and ~~~ stay mid-text in markdown.",
        "only the trailing signature goes"
    );
    assert_eq!(p[2].0, "");

    // The tenant switches to wikitext: new posts are wikitext and get the pre-save
    // transform; the markdown posts keep their markup.
    let base = c.rest("GET", "/settings", None).await.body["base"].clone();
    let r = c
        .rest(
            "PATCH",
            "/settings",
            Some(json!({"set": {"thread.markup": "wikitext"}, "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let si = c.api("action=query&meta=siteinfo&siprop=triplespace").await;
    assert_eq!(
        si["query"]["triplespace"]["thread"]["markup"], "wikitext",
        "{si}"
    );
    let r = c
        .rest(
            "POST",
            &path,
            Some(json!({"inReplyTo": opening, "text": "'''Yes.''' <small>Struck by ~~~</small> --~~~~", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let wikitext_post = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest(
            "POST",
            &path,
            Some(json!({"text": "plain", "mediaType": "text/markdown", "token": token})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-bad-media-type", "{}", r.body);

    let (p, base) = posts(&mut c, thread).await;
    let w = p
        .iter()
        .find(|(t, _)| t.starts_with("'''Yes.'''"))
        .expect("the wikitext post");
    assert_eq!(w.1, "text/x-wiki");
    assert_eq!(
        w.0, "'''Yes.''' <small>Struck by [[User:Alice|Alice]] ([[User talk:Alice|talk]])</small>",
        "a mid-text signature expands; the trailing one is removed"
    );
    assert_eq!(
        p[0].1, "text/markdown",
        "posts keep the markup they were written in"
    );

    // An edit takes its post's markup without naming it, and loses a trailing signature.
    let r = c
        .rest(
            "PUT",
            &format!("/thread/{thread}/post/{wikitext_post}"),
            Some(json!({"text": "'''Yes, really.''' ~~~~", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (p, base) = posts(&mut c, thread).await;
    assert!(
        p.iter()
            .any(|(t, m)| t == "'''Yes, really.'''" && m == "text/x-wiki"),
        "{p:?}"
    );

    // Back to markdown: editing the opening post keeps it markdown.
    let r = c
        .rest(
            "PUT",
            &format!("/thread/{thread}/post/{opening}"),
            Some(
                json!({"text": "Does this cover maps and atlases?", "base": base, "token": token}),
            ),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (p, _) = posts(&mut c, thread).await;
    assert_eq!(
        p[0],
        (
            "Does this cover maps and atlases?".into(),
            "text/markdown".into()
        )
    );

    // Read for display: each post rendered by its own markup.
    let r = c
        .rest("GET", &format!("/thread/{thread}?html=1"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let html: Vec<&str> = r.body["posts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["html"].as_str().unwrap_or_default())
        .collect();
    assert!(html[0].contains("ts-markdown"), "{}", html[0]);
    assert!(
        html.iter()
            .any(|h| h.contains("<b>Yes, really.</b>") && h.contains("mw-parser-output")),
        "{html:?}"
    );
    assert!(
        c.rest("GET", &format!("/thread/{thread}"), None).await.body["posts"][0]
            .get("html")
            .is_none(),
        "html only when asked"
    );
}

/// Creates a document page and answers its ID.
async fn new_page(c: &mut Client, token: &str, title: &str) -> u64 {
    let e = c
        .api_post(&format!(
            "action=edit&title={title}&text=x&createonly=1&token={}",
            enc(token)
        ))
        .await;
    e["edit"]["pageid"].as_u64().expect("created")
}

/// A thread's current base.
async fn thread_base(c: &mut Client, thread: u64) -> u64 {
    c.rest("GET", &format!("/thread/{thread}"), None).await.body["base"]
        .as_u64()
        .unwrap()
}

/// A page's history as `(log action or "edit", revid)` pairs, newest first.
async fn history(c: &mut Client, talk: u64) -> Vec<(String, u64)> {
    let r = c
        .rest("GET", &format!("/page/{talk}/history?limit=50"), None)
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    r.body["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["log"]["action"]
                    .as_str()
                    .map_or_else(|| "edit".to_string(), str::to_string),
                row["revid"].as_u64().unwrap_or(0),
            )
        })
        .collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn a_talk_pages_history_is_composite() {
    // 16 §6.1; 14 §1.3, §3.7.
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (mut c, token, scope) = signed_in("history").await;
    let maps = new_page(&mut c, &token, "Project:Maps").await;
    let atlas = new_page(&mut c, &token, "Project:Atlas").await;

    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Coverage", "target": {"kind": "page", "id": scope}, "text": "Maps too?", "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/post"),
            Some(json!({"inReplyTo": opening, "text": "Yes.", "token": token})),
        )
        .await;
    let reply = r.body["revid"].as_u64().unwrap();
    let b = thread_base(&mut c, thread).await;
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/attach"),
            Some(json!({"target": {"kind": "page", "id": maps}, "base": b, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let t = c.rest("GET", &format!("/thread/{thread}"), None).await.body;
    let scope_talk = t["home"]["talk_page_id"].as_u64().unwrap();
    let maps_talk = t["listings"][0]["talk_page_id"].as_u64().unwrap();

    // Listed, the thread brings its whole history to the listing page.
    let h = history(&mut c, maps_talk).await;
    assert_eq!(
        h.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
        ["attach", "edit", "create"],
        "{h:?}"
    );
    assert_eq!(h[1].1, reply);
    assert_eq!(
        h,
        history(&mut c, scope_talk).await,
        "the home shows the same rows"
    );

    // Detached, it takes its rows away; the detach stays on the page it left.
    let b = thread_base(&mut c, thread).await;
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/detach"),
            Some(json!({"target": {"kind": "page", "id": maps}, "base": b, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let h = history(&mut c, maps_talk).await;
    assert_eq!(
        h.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
        ["detach"],
        "{h:?}"
    );
    assert_eq!(history(&mut c, scope_talk).await.len(), 4);

    // Moved, it takes its rows to its new home; the move stays on the old one too.
    let b = thread_base(&mut c, thread).await;
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/move"),
            Some(json!({"target": {"kind": "page", "id": atlas}, "base": b, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let atlas_talk =
        c.rest("GET", &format!("/thread/{thread}"), None).await.body["home"]["talk_page_id"]
            .as_u64()
            .unwrap();
    let old = history(&mut c, scope_talk).await;
    assert_eq!(
        old.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
        ["move"],
        "{old:?}"
    );
    let new = history(&mut c, atlas_talk).await;
    assert_eq!(
        new.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>(),
        ["move", "detach", "attach", "edit", "create"],
        "{new:?}"
    );
    // A thread's own history is all its rows, wherever it lives.
    assert_eq!(history(&mut c, thread).await, new);
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn a_talk_pages_revisions_and_diffs_are_wikitext() {
    // 16 §6.1; 0019 A29.
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (mut c, token, scope) = signed_in("talktext").await;
    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Coverage", "target": {"kind": "page", "id": scope}, "text": "Maps too?", "token": token})),
        )
        .await;
    let first = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    let talk = c.rest("GET", &format!("/thread/{first}"), None).await.body["home"]["talk_page_id"]
        .as_u64()
        .unwrap();
    let r = c
        .rest(
            "POST",
            &format!("/thread/{first}/post"),
            Some(json!({"inReplyTo": opening, "text": "Yes, and atlases.", "token": token})),
        )
        .await;
    let reply = r.body["revid"].as_u64().unwrap();
    let r = c
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Scope of dates", "target": {"kind": "page", "id": scope}, "text": "Before 1900?", "token": token})),
        )
        .await;
    let second = r.body["revid"].as_u64().unwrap();

    // The talk page's revisions are its threads', newest first, with parents and content.
    let v = c
        .api(&format!(
            "action=query&prop=revisions&pageids={talk}&rvprop=ids|content&rvslots=main&rvlimit=10"
        ))
        .await;
    let page = &v["query"]["pages"][0];
    let revs = page["revisions"].as_array().expect("revisions");
    let ids: Vec<u64> = revs.iter().map(|r| r["revid"].as_u64().unwrap()).collect();
    assert_eq!(ids, [second, reply, opening], "{v}");
    assert_eq!(revs[0]["parentid"], reply);
    assert_eq!(revs[2]["parentid"], 0);
    let content = |r: &Value| r["slots"]["main"]["content"].as_str().unwrap().to_string();
    assert_eq!(revs[0]["slots"]["main"]["contentmodel"], "triplespace-talk");
    assert_eq!(revs[0]["slots"]["main"]["contentformat"], "text/x-wiki");
    let latest = content(&revs[0]);
    assert!(
        latest.starts_with(
            "== Coverage ==\nMaps too? [[User:Alice|Alice]] ([[User talk:Alice|talk]]) "
        ),
        "{latest}"
    );
    assert!(
        latest.contains("\n:Yes, and atlases. [[User:Alice|Alice]]"),
        "{latest}"
    );
    assert!(
        latest.contains("\n\n== Scope of dates ==\nBefore 1900? "),
        "{latest}"
    );
    let oldest = content(&revs[2]);
    assert!(
        !oldest.contains("atlases") && !oldest.contains("Scope of dates"),
        "{oldest}"
    );

    // A diff between two of the talk page's revisions: the reply, as added lines.
    let d = c
        .api(&format!(
            "action=compare&fromid={talk}&fromrev={opening}&toid={talk}&torev={reply}&prop=diff|ids"
        ))
        .await;
    let diff = d["compare"]["body"].as_str().unwrap_or_default();
    assert!(diff.contains("Yes, and atlases."), "{d}");
    assert!(!diff.contains("Scope of dates"), "{d}");
    assert_eq!(d["compare"]["torevid"], reply);
    // From the talk page's latest revision to the one before it.
    let d = c
        .api(&format!(
            "action=compare&fromid={talk}&torelative=prev&prop=diff|ids"
        ))
        .await;
    assert_eq!(d["compare"]["fromrevid"], reply, "{d}");
    assert_eq!(d["compare"]["torevid"], second, "{d}");
    assert!(
        d["compare"]["body"]
            .as_str()
            .unwrap_or_default()
            .contains("Scope of dates"),
        "{d}"
    );
    // A revision named alone is its thread's, compared as the thread's JSON.
    let d = c
        .api(&format!(
            "action=compare&fromrev={opening}&torev={reply}&prop=diff|title"
        ))
        .await;
    assert!(
        d["compare"]["totitle"]
            .as_str()
            .unwrap_or_default()
            .starts_with("Thread:"),
        "{d}"
    );
}

/// A second account, signed in.
async fn account(router: &Router, name: &str, password: &str) -> Client {
    let mut a = Client {
        router: router.clone(),
        cookie: None,
    };
    let v = a
        .api("action=query&meta=authmanagerinfo&amirequestsfor=create&amimergerequestfields=1")
        .await;
    let challenge = v["query"]["authmanagerinfo"]["fields"]["tschallengeid"]["value"]
        .as_str()
        .map(|id| {
            let bits = scatter_challenge::Challenge::parse(id).unwrap().bits;
            format!(
                "&tschallengeid={}&tschallengesolution={}",
                enc(id),
                scatter_challenge::solve(id, bits)
            )
        })
        .unwrap_or_default();
    let token = a.api("action=query&meta=tokens&type=createaccount").await["query"]["tokens"]
        ["createaccounttoken"]
        .as_str()
        .unwrap()
        .to_string();
    let r = a
        .api_post(&format!(
            "action=createaccount&username={}&password={}&retype={}&createtoken={}&createreturnurl=https://{HOST}/{challenge}",
            enc(name),
            enc(password),
            enc(password),
            enc(&token)
        ))
        .await;
    assert_eq!(r["createaccount"]["status"], "PASS", "{r}");
    let token = a.api("action=query&meta=tokens&type=login").await["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = a
        .api_post(&format!(
            "action=clientlogin&username={}&password={}&logintoken={}&loginreturnurl=https://{HOST}/",
            enc(name),
            enc(password),
            enc(&token)
        ))
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    a
}

/// One account's notifications as `(ts-reason, section, thread subject, agent)`, newest
/// first, and the unread counts by section.
async fn inbox(c: &mut Client) -> (Vec<(String, String, String, String)>, Value) {
    let n = c
        .api("action=query&meta=notifications&notprop=list%7Ccount&notgroupbysection=1&notlimit=50")
        .await;
    let n = &n["query"]["notifications"];
    let mut items: Vec<(i64, (String, String, String, String))> = Vec::new();
    for s in ["alert", "message"] {
        // The instance's making Alice owner is no thread's.
        for i in n[s]["list"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["ts-reason"] != "rights")
        {
            items.push((
                i["id"].as_i64().unwrap(),
                (
                    i["ts-reason"].as_str().unwrap().to_string(),
                    i["section"].as_str().unwrap().to_string(),
                    i["ts-thread"]["subject"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    i["agent"]["name"].as_str().unwrap_or_default().to_string(),
                ),
            ));
        }
    }
    items.sort_by_key(|(id, _)| -id);
    let counts = json!({"alert": n["alert"]["rawcount"], "message": n["message"]["rawcount"]});
    (items.into_iter().map(|(_, i)| i).collect(), counts)
}

fn note(
    reason: &str,
    section: &str,
    subject: &str,
    agent: &str,
) -> (String, String, String, String) {
    (reason.into(), section.into(), subject.into(), agent.into())
}

/// Thread notifications (0019 §8; 16 §5.2; 0021 A20): `talk` on a user talk page, `reply`,
/// `mention` (and an edit's new mentions only), `thread-status`, one notification per
/// account per record, nothing of one's own acts, and what the reader may no longer see
/// leaving the inbox and its counts: a post hidden by revision deletion, a thread deleted.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn thread_notifications() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (mut alice, at, page) = signed_in("notify").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let bt = bob.csrf().await;
    let ct = carol.csrf().await;
    let r = alice
        .api_post(&format!("action=echomarkread&all=1&token={}", enc(&at)))
        .await;
    assert_eq!(r["echomarkread"]["rawcount"], 0, "{r}");

    // Bob starts a topic on Alice's user talk page by her name, mentioning Carol, himself
    // and nobody.
    let r = bob
        .rest(
            "POST",
            "/thread",
            Some(json!({
                "subject": "Your edits",
                "target": {"kind": "actor", "name": "Alice"},
                "text": "Hello. [[User:Carol]] saw them too; so did [[User:Bob|I]] and [[User:Nobody]].",
                "token": bt,
            })),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let talk_thread = r.body["thread"].as_u64().unwrap();
    let opening = r.body["revid"].as_u64().unwrap();
    let r = bob
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "x", "target": {"kind": "actor", "name": "Nobody"}, "text": "Hi", "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    assert_eq!(r.body["code"], "ts-no-target", "{}", r.body);

    // The change routes name a user talk page by its account too, as the site's forms do
    // (0019 §9, A30): Alice pins the thread on her talk page and unpins it, by her name.
    let at2 = alice.csrf().await;
    for (op, status) in [("pin", StatusCode::OK), ("unpin", StatusCode::OK)] {
        let base = alice
            .rest("GET", &format!("/thread/{talk_thread}"), None)
            .await
            .body["base"]
            .as_u64()
            .unwrap();
        let r = alice
            .rest(
                "POST",
                &format!("/thread/{talk_thread}/{op}"),
                Some(json!({"target": {"kind": "actor", "name": "Alice"}, "base": base, "token": at2})),
            )
            .await;
        assert_eq!(r.status, status, "{op}: {}", r.body);
    }
    let r = alice
        .rest(
            "POST",
            &format!("/thread/{talk_thread}/pin"),
            Some(json!({"target": {"kind": "actor", "name": "Nobody"}, "base": 0, "token": at2})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-no-target", "{}", r.body);

    let (items, counts) = inbox(&mut alice).await;
    assert_eq!(
        items,
        vec![note("talk", "message", "Your edits", "Bob")],
        "{items:?}"
    );
    assert_eq!(counts, json!({"alert": 0, "message": 1}));
    let (items, _) = inbox(&mut carol).await;
    assert_eq!(items, vec![note("mention", "alert", "Your edits", "Bob")]);
    assert!(inbox(&mut bob).await.0.is_empty(), "nothing of his own");

    // Alice answers Bob on her own talk page: Bob has a reply, Alice nothing.
    let r = alice
        .rest(
            "POST",
            &format!("/thread/{talk_thread}/post"),
            Some(json!({"inReplyTo": opening, "text": "Thanks, [[User:Bob]].", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (items, _) = inbox(&mut bob).await;
    assert_eq!(
        items,
        vec![note("reply", "alert", "Your edits", "Alice")],
        "one per record: {items:?}"
    );
    assert_eq!(inbox(&mut alice).await.0.len(), 1);

    // Carol posts there too: Alice's talk, and Bob nothing (a top-level post answers no one).
    let r = carol
        .rest(
            "POST",
            &format!("/thread/{talk_thread}/post"),
            Some(json!({"text": "Agreed.", "token": ct})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let carols = r.body["revid"].as_u64().unwrap();
    let (items, counts) = inbox(&mut alice).await;
    assert_eq!(
        items,
        vec![
            note("talk", "message", "Your edits", "Carol"),
            note("talk", "message", "Your edits", "Bob")
        ]
    );
    assert_eq!(counts, json!({"alert": 0, "message": 2}));
    assert_eq!(inbox(&mut bob).await.0.len(), 1);

    // Bob's thread on a page's talk page, closed by Carol's answer to him: thread-status,
    // the first reason that applies, not also a reply.
    let r = bob
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Scope question", "target": {"kind": "page", "id": page}, "text": "Is this in scope?", "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let scope_thread = r.body["thread"].as_u64().unwrap();
    let question = r.body["revid"].as_u64().unwrap();
    let scope_title = r.body["title"].as_str().unwrap().to_string();
    let r = carol
        .rest(
            "POST",
            &format!("/thread/{scope_thread}/post"),
            Some(json!({"inReplyTo": question, "text": "Yes.", "status": "resolved", "token": ct})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let (items, _) = inbox(&mut bob).await;
    assert_eq!(
        items[0],
        note("thread-status", "alert", "Scope question", "Carol")
    );
    assert_eq!(items.len(), 2);
    // Resolving again is not a move from open to closed: nothing more.
    let r = alice
        .rest(
            "POST",
            &format!("/thread/{scope_thread}/post"),
            Some(json!({"text": "Closing for good.", "status": "resolved", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(inbox(&mut bob).await.0.len(), 2);

    // An edit notifies only the mentions it adds.
    let base = thread_base(&mut bob, talk_thread).await;
    let r = bob
        .rest(
            "PUT",
            &format!("/thread/{talk_thread}/post/{opening}"),
            Some(json!({"text": "Hello. [[User:Carol]] saw them; so did [[User:Alice]].", "base": base, "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        inbox(&mut carol).await.0.len(),
        1,
        "Carol was mentioned before"
    );
    // Alice owns the talk page, but an edit is not a new post there: a mention.
    let (items, counts) = inbox(&mut alice).await;
    assert_eq!(items[0], note("mention", "alert", "Your edits", "Bob"));
    assert_eq!(counts, json!({"alert": 1, "message": 2}));

    // A hidden post leaves every inbox it reached, and the counts.
    let r = alice
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={carols}&hide=content&reason=Test&token={}",
            enc(&at)
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let (items, counts) = inbox(&mut alice).await;
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(!items.contains(&note("talk", "message", "Your edits", "Carol")));
    assert_eq!(counts, json!({"alert": 1, "message": 1}));

    // A deleted thread leaves the inbox of whoever may no longer read it.
    let r = alice
        .api_post(&format!(
            "action=delete&title={}&token={}",
            enc(&format!("Thread:{scope_title}")),
            enc(&at)
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let (items, counts) = inbox(&mut bob).await;
    assert_eq!(
        items,
        vec![note("reply", "alert", "Your edits", "Alice")],
        "{items:?}"
    );
    assert_eq!(counts, json!({"alert": 1, "message": 0}));

    // Reading the orange bar's messages: `echomarkread` narrowed to messages.
    let r = alice
        .api_post(&format!(
            "action=echomarkread&all=1&sections=message&token={}",
            enc(&at)
        ))
        .await;
    assert_eq!(r["echomarkread"]["message"]["rawcount"], 0, "{r}");
    assert_eq!(r["echomarkread"]["alert"]["rawcount"], 1, "{r}");
}

/// One account's notifications as `(ts-reason, title)`, newest first.
async fn reasons(c: &mut Client) -> Vec<(String, String)> {
    let n = c
        .api("action=query&meta=notifications&notprop=list&notlimit=50")
        .await;
    n["query"]["notifications"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["ts-reason"] != "rights")
        .map(|i| {
            (
                i["ts-reason"].as_str().unwrap().to_string(),
                i["title"]["full"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

/// The watch set (0020 §3, §5; 16 §4.2–4.3; 0021 A21): `action=watch` with expiry and
/// `tsnotify`, a talk page watched as its subject, a title watched before its page
/// exists, `prop=info` `watched` and `notificationtimestamp`, `list=watchlist` and
/// `list=watchlistraw`, `setnotificationtimestamp`, auto-watch on writes and the
/// `watchlist` parameter, the REST routes, and `watch` notifications for threads and pages.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_watch_set() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (mut alice, at, page) = signed_in("watch").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let bt = bob.csrf().await;
    let awt =
        alice.api("action=query&meta=tokens&type=watch").await["query"]["tokens"]["watchtoken"]
            .as_str()
            .unwrap()
            .to_string();

    // Alice watches the talk page, which is watching its subject, and asks to be notified.
    let r = alice
        .api_post(&format!(
            "action=watch&titles=Project%20talk:Scope&tsnotify=1&token={}",
            enc(&awt)
        ))
        .await;
    assert_eq!(r["watch"][0]["watched"], true, "{r}");
    assert_eq!(r["watch"][0]["watchlistexpiry"], "infinity", "{r}");
    let info = alice
        .api("action=query&prop=info&titles=Project:Scope&inprop=watched%7Cnotificationtimestamp")
        .await;
    let p = &info["query"]["pages"][0];
    assert_eq!(p["watched"], true, "{info}");
    assert_eq!(p["ts-notify"], true, "{info}");
    assert_eq!(p["notificationtimestamp"], "", "{info}");
    // A title with no page yet, with an expiry.
    let r = alice
        .api_post(&format!(
            "action=watch&titles=Project:Later&expiry=1%20week&tsnotify=1&token={}",
            enc(&awt)
        ))
        .await;
    assert!(
        r["watch"][0]["watchlistexpiry"]
            .as_str()
            .unwrap()
            .starts_with("20"),
        "{r}"
    );
    let r = alice
        .api_post(&format!(
            "action=watch&titles=Special:Nothing&token={}",
            enc(&awt)
        ))
        .await;
    assert_eq!(r["watch"][0]["invalid"], true, "{r}");

    // Bob starts a thread on the talk page: Alice is notified, and Bob watches the thread
    // by his auto-watch preference.
    let r = bob
        .rest(
            "POST",
            "/thread",
            Some(json!({"subject": "Scope again", "target": {"kind": "page", "id": page}, "text": "A question.", "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread_title = format!("Thread:{}", r.body["title"].as_str().unwrap());
    assert_eq!(
        reasons(&mut alice).await,
        vec![("watch".to_string(), thread_title.clone())]
    );
    let raw = bob.api("action=query&list=watchlistraw").await;
    let titles: Vec<&str> = raw["watchlistraw"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, vec![thread_title.as_str()], "{raw}");

    // Alice's watchlist has the thread's row, not yet seen; her own edits are seen.
    let wl = alice
        .api("action=query&list=watchlist&wlprop=title%7Cnotificationtimestamp%7Cuser")
        .await;
    let rows = wl["query"]["watchlist"].as_array().unwrap();
    let row = rows
        .iter()
        .find(|r| r["title"] == thread_title.as_str())
        .unwrap_or_else(|| panic!("{wl}"));
    assert_ne!(row["notificationtimestamp"], "", "{wl}");
    let info = alice
        .api("action=query&prop=info&titles=Project:Scope&inprop=notificationtimestamp")
        .await;
    assert_ne!(
        info["query"]["pages"][0]["notificationtimestamp"], "",
        "{info}"
    );
    let r = alice
        .api_post(&format!(
            "action=setnotificationtimestamp&entirewatchlist=1&token={}",
            enc(&at)
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let info = alice
        .api("action=query&prop=info&titles=Project:Scope&inprop=notificationtimestamp")
        .await;
    assert_eq!(
        info["query"]["pages"][0]["notificationtimestamp"], "",
        "{info}"
    );

    // A page edit by Bob reaches Alice's watch on the page; her own does not.
    let r = bob
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Wider.&watchlist=nochange&token={}",
            enc(&bt)
        ))
        .await;
    assert_eq!(r["edit"]["result"], "Success", "{r}");
    assert!(
        r["edit"].get("watched").is_none(),
        "nochange leaves Bob unwatched: {r}"
    );
    alice
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Wider still.&token={}",
            enc(&at)
        ))
        .await;
    let got = reasons(&mut alice).await;
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        got[0],
        ("watch".to_string(), "Library Base:Scope".to_string())
    );

    // Bob creates the page Alice watched before it existed; he watches it (autowatch.create).
    let r = bob
        .api_post(&format!(
            "action=edit&title=Project:Later&text=Now.&token={}",
            enc(&bt)
        ))
        .await;
    assert_eq!(r["edit"]["watched"], true, "{r}");
    let got = reasons(&mut alice).await;
    assert_eq!(
        got[0],
        ("watch".to_string(), "Library Base:Later".to_string()),
        "{got:?}"
    );

    // REST: the list, a change of notify, and unwatching.
    let r = alice.rest("GET", "/watchlist", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let titles: Vec<&str> = r.body["watches"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|w| w["title"].as_str())
        .collect();
    assert!(
        titles.contains(&"Library Base:Scope") && titles.contains(&"Library Base:Later"),
        "{}",
        r.body
    );
    let r = alice
        .rest(
            "PUT",
            &format!("/watchlist/page/{page}"),
            Some(json!({"notify": false, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["notify"], false, "{}", r.body);
    bob.api_post(&format!(
        "action=edit&title=Project:Scope&text=Narrower.&token={}",
        enc(&bt)
    ))
    .await;
    assert_eq!(reasons(&mut alice).await.len(), 3, "notify is off");
    let r = alice
        .api_post(&format!(
            "action=watch&titles=Project:Scope&unwatch=1&token={}",
            enc(&awt)
        ))
        .await;
    assert_eq!(r["watch"][0]["unwatched"], true, "{r}");
    let info = alice
        .api("action=query&prop=info&titles=Project:Scope&inprop=watched")
        .await;
    assert_eq!(info["query"]["pages"][0]["watched"], false, "{info}");
    let r = alice.rest("GET", "/watchlist", None).await;
    assert_eq!(r.body["watches"].as_array().unwrap().len(), 1, "{}", r.body);

    // Not logged in: no watchlist.
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    let r = anon.api("action=query&list=watchlist").await;
    assert_eq!(r["error"]["code"], "notloggedin", "{r}");
}
