//! Reports end to end (0047 §4; 21 §2): Librarybase's pages adopted, the reports built on
//! first read, then kept by the `report` projection's queue as pages are created, edited,
//! moved, deleted and restricted, and served by `list=querypage` and `GET /reports`.
//!
//! The central check: after each round of edits, the entries the queue kept are exactly
//! the entries a rebuild from `view` makes. The per-key recompute and the set-based build
//! are two implementations of each report, so each checks the other. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::Value;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{adopt, instance};
use triplespace_projections::reports;

const DB: &str = "tsreports_test";
const TENANT: &str = "librarybase";
const HOST: &str = "librarybase.org";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {DB} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {DB}"))
        .await
        .expect("create database");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{DB}"))
        .unwrap()
}

async fn setup() -> Option<(String, PathBuf)> {
    admin_config()?;
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsreports-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();
    let key_file = dir.join("triplespace.key");
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec![],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(1),
        owner_name: "Librarybase".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    adopt::run(adopt::Adopt {
        database: database.clone(),
        tenant: TENANT.into(),
        dump: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/samples/librarybase/dump/librarybase-wikitext.xml.gz"),
        source: format!("https://{HOST}/"),
        version: Some("LibrarybaseWikitext-20261010030044".into()),
        frozen: true,
        current_only: false,
        counters: Some("item=0,property=0".into()),
        log_floor: 0,
        entity_sources: vec![],
        subsidiary: None,
        batch: 50,
        skip_talk: false,
        no_accounts: false,
    })
    .await
    .expect("adopt");
    Some((database, key_file))
}

struct Client {
    router: Router,
    cookie: Option<String>,
}

impl Client {
    async fn call(&mut self, method: &str, uri: &str, form: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match form {
            Some(f) => req
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(f.to_string()))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        if let Some(sc) = response.headers().get(header::SET_COOKIE) {
            let value = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap_or_default()
                .to_string();
            self.cookie = (!value.is_empty()).then_some(value);
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&mut self, query: &str) -> Value {
        self.call(
            "GET",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            None,
        )
        .await
        .1
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        self.call(
            "POST",
            &format!("/w/api.php?{query}&format=json&formatversion=2"),
            Some(form),
        )
        .await
        .1
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Every entry of the tenant's reports, in order.
async fn entries(database: &str) -> Vec<(String, String, Option<i64>)> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client
        .query(
            "SELECT report, key, sort_value FROM view.report_entry
             WHERE tenant = $1 ORDER BY report, key",
            &[&TENANT],
        )
        .await
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect()
}

async fn queued(database: &str) -> i64 {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client
        .query_one(
            "SELECT count(*) FROM ops.projection_work WHERE projection = 'report' AND tenant = $1",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0)
}

/// The queue's entries, once the worker has applied it, are the entries a rebuild makes.
async fn incremental_matches_rebuild(app: &App, database: &str, after: &str) {
    app.work_until_idle(100_000).await.unwrap();
    let kept = entries(database).await;
    reports::rebuild(app.pool(), TENANT).await.unwrap();
    let built = entries(database).await;
    let only_kept: Vec<_> = kept.iter().filter(|e| !built.contains(e)).collect();
    let only_built: Vec<_> = built.iter().filter(|e| !kept.contains(e)).collect();
    assert!(
        only_kept.is_empty() && only_built.is_empty(),
        "after {after}: kept but not built {only_kept:?}; built but not kept {only_built:?}"
    );
}

fn listed(entries: &[(String, String, Option<i64>)], report: &str, key: &str) -> bool {
    entries.iter().any(|(r, k, _)| r == report && k == key)
}

fn count(entries: &[(String, String, Option<i64>)], report: &str, key: &str) -> Option<i64> {
    entries
        .iter()
        .find(|(r, k, _)| r == report && k == key)
        .and_then(|(_, _, v)| *v)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn reports_are_built_kept_and_served() {
    let Some((database, key_file)) = setup().await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(&key_file).unwrap();
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&signing.to_bytes()),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec![],
            dev_tenant: None,
            farm,
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let mut api = Client {
        router: router_with(app.clone(), triplespace_api_rest::routes()),
        cookie: None,
    };

    // Off until built: the adoption queued nothing.
    assert_eq!(queued(&database).await, 0);
    assert!(entries(&database).await.is_empty());

    // The first read builds the tenant's reports; Librarybase's pages link to a few
    // titles it never wrote.
    let wanted = api
        .get("action=query&list=querypage&qppage=Wantedpages&qplimit=max")
        .await;
    let qp = &wanted["query"]["querypage"];
    assert_eq!(qp["name"], "Wantedpages", "{wanted}");
    assert_eq!(qp["ts-state"]["backing"], "projection");
    assert!(qp["ts-state"]["computed_at"].is_string(), "{wanted}");
    assert!(qp["results"].as_array().is_some(), "{wanted}");
    incremental_matches_rebuild(&app, &database, "the first build").await;

    // The owner logs in.
    let tokens = api.get("action=query&meta=tokens&type=login").await;
    let login = tokens["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = api
        .post(
            "action=clientlogin",
            &format!(
                "username=Librarybase&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
                enc(&login)
            ),
        )
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let csrf = api.get("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    let edit = |title: &str, text: &str| {
        format!(
            "title={}&text={}&token={}",
            enc(title),
            enc(text),
            enc(&csrf)
        )
    };

    // Round 1: main-namespace pages, links to titles no page has, categories.
    for (title, text) in [
        ("A lone page", "Nothing links here and it links nowhere."),
        (
            "A linker",
            "[[A nowhere]] and [[A nowhere 2]] and [[A lone page]]. [[Category:Testing]] [[Category:Other]]",
        ),
        ("A second linker", "[[A nowhere]] again, and [[A linker]]."),
    ] {
        let r = api.post("action=edit", &edit(title, text)).await;
        assert_eq!(r["edit"]["result"], "Success", "{r}");
    }
    // The writes queued their keys; a read applies none of them (03 §6.1), it reports
    // what is waiting, and the worker catches up behind it.
    let waiting = queued(&database).await;
    assert!(waiting > 0);
    let wanted = api
        .get("action=query&list=querypage&qppage=Wantedpages&qplimit=max")
        .await;
    assert_eq!(
        wanted["query"]["querypage"]["ts-state"]["waiting"], waiting,
        "{wanted}"
    );
    assert_eq!(queued(&database).await, waiting, "a read does no work");
    let ran = app.work_until_idle(100_000).await.unwrap();
    assert!(ran.failures.is_empty(), "{ran:?}");
    assert_eq!(queued(&database).await, 0);
    let wanted = api
        .get("action=query&list=querypage&qppage=Wantedpages&qplimit=max")
        .await;
    let results = wanted["query"]["querypage"]["results"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        results
            .iter()
            .any(|r| r["title"] == "A nowhere" && r["value"] == "2"),
        "{wanted}"
    );
    assert_eq!(wanted["query"]["querypage"]["ts-state"]["waiting"], 0);
    // The lag is in GET /version too, for this tenant.
    let (status, version) = api
        .call("GET", "/w/rest.php/triplespace/v0/version", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{version}");
    let queues = version["queues"].as_array().unwrap();
    assert!(
        queues
            .iter()
            .all(|q| q["tenant"] == TENANT || q["tenant"] == ""),
        "{queues:?}"
    );
    let e = entries(&database).await;
    assert_eq!(count(&e, "WantedPages", "0:A nowhere"), Some(2));
    assert_eq!(count(&e, "MostLinkedPages", "0:A nowhere"), Some(2));
    assert_eq!(count(&e, "WantedPages", "0:A nowhere 2"), Some(1));
    assert!(listed(&e, "DeadendPages", "0:A lone page"));
    assert!(listed(&e, "UncategorizedPages", "0:A lone page"));
    assert!(
        !listed(&e, "LonelyPages", "0:A lone page"),
        "A linker links to it"
    );
    assert!(listed(&e, "LonelyPages", "0:A second linker"));
    assert_eq!(count(&e, "MostCategories", "0:A linker"), Some(2));
    assert_eq!(count(&e, "WantedCategories", "14:Testing"), Some(1));
    incremental_matches_rebuild(&app, &database, "round 1").await;

    // Round 2: a link removed, a wanted title created, category pages created.
    for (title, text) in [
        (
            "A linker",
            "[[A nowhere]] and [[A lone page]]. [[Category:Testing]] [[Category:Other]]",
        ),
        ("A nowhere", "Now it exists. [[Category:Testing]]"),
        ("Category:Testing", "The testing category."),
        ("Category:Empty", "No page is in it."),
    ] {
        let r = api.post("action=edit", &edit(title, text)).await;
        assert_eq!(r["edit"]["result"], "Success", "{r}");
    }
    app.work_until_idle(100_000).await.unwrap();
    let e = entries(&database).await;
    assert!(
        !listed(&e, "WantedPages", "0:A nowhere 2"),
        "its only link was removed"
    );
    assert!(!listed(&e, "WantedPages", "0:A nowhere"), "it exists now");
    assert_eq!(count(&e, "MostLinkedPages", "0:A nowhere"), Some(2));
    assert!(!listed(&e, "WantedCategories", "14:Testing"));
    assert_eq!(count(&e, "MostLinkedCategories", "14:Testing"), Some(2));
    assert!(listed(&e, "UnusedCategories", "14:Empty"));
    assert!(listed(&e, "UncategorizedCategories", "14:Empty"));
    incremental_matches_rebuild(&app, &database, "round 2").await;

    // Round 3: a move, a deletion and a read restriction.
    let r = api
        .post(
            "action=move",
            &format!(
                "from=A%20second%20linker&to=A%20third%20linker&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert!(r["move"].is_object(), "{r}");
    app.work_until_idle(100_000).await.unwrap();
    let e = entries(&database).await;
    assert!(
        !listed(&e, "LonelyPages", "0:A second linker"),
        "moved away"
    );
    incremental_matches_rebuild(&app, &database, "the move").await;

    let r = api
        .post(
            "action=delete",
            &format!("title=A%20linker&reason=test&token={}", enc(&csrf)),
        )
        .await;
    assert!(r["delete"].is_object(), "{r}");
    app.work_until_idle(100_000).await.unwrap();
    let e = entries(&database).await;
    // A linker's links and memberships no longer count.
    assert!(!listed(&e, "MostCategories", "0:A linker"));
    assert!(
        listed(&e, "LonelyPages", "0:A lone page"),
        "its one link is deleted"
    );
    assert_eq!(count(&e, "MostLinkedCategories", "14:Testing"), Some(1));
    incremental_matches_rebuild(&app, &database, "the deletion").await;

    let r = api
        .post(
            "action=protect",
            &format!(
                "title=A%20nowhere&protections=read%3Downer&expiry=infinite&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert!(r["protect"].is_object(), "{r}");
    app.work_until_idle(100_000).await.unwrap();
    let e = entries(&database).await;
    // A confidential page is not listed and is not wanted: it exists, for some.
    assert!(!listed(&e, "MostLinkedPages", "0:A nowhere"));
    assert!(!listed(&e, "WantedPages", "0:A nowhere"));
    assert!(
        e.iter().all(|(_, k, _)| k != "0:A nowhere"),
        "no report lists a page an anonymous reader cannot see: {e:?}"
    );
    incremental_matches_rebuild(&app, &database, "the read restriction").await;

    // Index-backed reports read live: the adopted redirects, the new pages by size.
    let redirects = api
        .get("action=query&list=querypage&qppage=Listredirects&qplimit=max")
        .await;
    let rows = redirects["query"]["querypage"]["results"]
        .as_array()
        .unwrap();
    assert!(
        rows.iter()
            .any(|r| r["title"].as_str().unwrap().ends_with(":Notability")
                && r["ts-target"].is_string()),
        "{redirects}"
    );
    assert_eq!(
        redirects["query"]["querypage"]["ts-state"]["backing"],
        "index"
    );
    let short = api
        .get("action=query&list=querypage&qppage=ShortPages&qplimit=1")
        .await;
    assert_eq!(
        short["query"]["querypage"]["results"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(short["continue"]["qpoffset"], 1, "{short}");
    let props = api
        .get("action=query&list=querypage&qppage=ListProperties")
        .await;
    assert!(props["query"]["querypage"]["results"].is_array(), "{props}");
    let none = api
        .get("action=query&list=querypage&qppage=WantedTemplates")
        .await;
    assert_eq!(none["error"]["code"], "ts-report-unavailable", "{none}");

    // The REST routes.
    let (status, all) = api
        .call("GET", "/w/rest.php/triplespace/v0/reports", None)
        .await;
    assert_eq!(status, StatusCode::OK, "{all}");
    let list = all["reports"].as_array().unwrap();
    assert_eq!(list.len(), reports::REPORTS.len());
    assert!(
        list.iter()
            .any(|r| r["name"] == "WantedPages" && r["mode"] == "projection" && r["waiting"] == 0)
    );
    let (status, one) = api
        .call(
            "GET",
            "/w/rest.php/triplespace/v0/reports/DeadendPages?limit=1",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{one}");
    assert_eq!(one["report"]["name"], "DeadendPages");
    // By now Category:Testing's members are deleted or restricted: nothing counts.
    let at_end = entries(&database).await;
    assert!(!listed(&at_end, "MostLinkedCategories", "14:Testing"));
    assert_eq!(one["rows"].as_array().unwrap().len(), 1, "{one}");
    let (status, _) = api
        .call(
            "GET",
            "/w/rest.php/triplespace/v0/reports/NoSuchReport",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
