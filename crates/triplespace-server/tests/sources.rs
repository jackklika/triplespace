//! Entity sources end to end (0078 §1–§4, §10): a tenant declares a source, loads it from a
//! Wikibase dump through `sync --tenant --source`, and reads its entities by their
//! colon-form IDs through the Action API and the REST API, on that tenant only. Needs
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
use triplespace_cli::{instance, source, sync};

const DB: &str = "tssources_test";
const TENANT: &str = "reader";
const HOST: &str = "reader.example";

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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/librarybase/dump/librarybase-sample.xml.gz")
}

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let config: Config = database.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client.query(sql, &[]).await.unwrap()
}

struct Client {
    router: Router,
}

impl Client {
    async fn raw(&self, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .header(header::HOST, HOST)
            .body(Body::empty())
            .unwrap();
        let response = self.router.clone().oneshot(req).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&self, q: &str) -> Value {
        let (status, v) = self
            .raw(&format!("/w/api.php?{q}&format=json&formatversion=2"))
            .await;
        assert_eq!(status, StatusCode::OK, "{q}: {v}");
        v
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_source_is_declared_loaded_and_read_on_its_tenant() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssources-{}", std::process::id()));
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
        adopt: None,
        owner: Some(1),
        owner_name: "Reader".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");

    // 1. The declaration: numbered, with its partition (0078 §1, §6.4).
    let at = || source::At {
        database: database.clone(),
        tenant: TENANT.into(),
    };
    source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "lb".into(),
            reason: "the test".into(),
        },
        label: "Librarybase as a source".into(),
        types: vec!["Q=item".into(), "P=property".into()],
        iri: "https://librarybase.org/entity/{upstream_id}".into(),
        api: Some("https://librarybase.org/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
    }))
    .await
    .expect("source add");
    let declared = query(
        &database,
        "SELECT config->>'number' FROM view.registry WHERE tenant = 'reader' AND kind = 'entity-source' AND code = 'lb'",
    )
    .await;
    assert_eq!(declared[0].get::<_, String>(0), "4096");
    assert_eq!(
        query(
            &database,
            "SELECT count(*)::int FROM log.partition WHERE tenant = 'reader' AND name = 'source/lb'"
        )
        .await[0]
            .get::<_, i32>(0),
        1
    );
    // A registry name is refused (0078 §3 rule 1).
    let taken = source::run(source::SourceCommand::Add(source::Add {
        which: source::Which {
            at: at(),
            name: "librarybase".into(),
            reason: String::new(),
        },
        label: "x".into(),
        types: vec!["Q=item".into()],
        iri: "https://x.example/{upstream_id}".into(),
        api: None,
        entity_data: None,
        revision_ids: false,
        licence: None,
    }))
    .await;
    assert!(
        taken
            .as_ref()
            .is_err_and(|e| e.to_string().contains("ts-source-name-taken")),
        "{taken:?}"
    );

    // 2. The load from the dump (0092 §4), recording where it ended (0092 §5).
    sync::run(sync::Sync {
        database: database.clone(),
        provider: None,
        tenant: Some(TENANT.into()),
        source: Some("lb".into()),
        dump: fixture(),
        version: Some("sample".into()),
        snapshot: false,
        threshold: 100,
        batch: 50,
        rejects: None,
    })
    .await
    .expect("sync into the source");
    let loaded = query(
        &database,
        "SELECT entity_id FROM view.entity_source WHERE tenant = 'reader' AND graph = 'source/lb' ORDER BY entity_id",
    )
    .await;
    assert_eq!(
        loaded.len(),
        254,
        "every entity of the sample, as tenant rows"
    );
    assert!(
        loaded
            .iter()
            .all(|r| r.get::<_, String>(0).starts_with("lb:"))
    );
    assert_eq!(
        query(
            &database,
            "SELECT count(*)::int FROM view.entity_source WHERE tenant = '' AND graph = 'source/lb'"
        )
        .await[0]
            .get::<_, i32>(0),
        0,
        "no shared row (0078 §4)"
    );
    let cursor = query(
        &database,
        "SELECT kind, name FROM ops.repo_cursor WHERE tenant = 'reader' AND loaded_to IS NOT NULL",
    )
    .await;
    assert_eq!(
        (cursor[0].get::<_, String>(0), cursor[0].get::<_, String>(1)),
        ("source".to_string(), "lb".to_string())
    );

    // 3. Reads on the tenant.
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
    let c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
    };
    let v = c
        .get("action=wbgetentities&ids=LB:q1&props=info|labels|claims")
        .await;
    let e = &v["entities"]["lb:Q1"];
    assert_eq!(
        e["id"], "lb:Q1",
        "canonicalized from `LB:q1` (0078 §2): {v}"
    );
    assert_eq!(e["type"], "item");
    assert_eq!(e["title"], "Item:lb:Q1");
    assert_eq!(e["ns"], 120);
    assert!(e["labels"]["en"]["value"].is_string());
    // Statements and values are in the source's form too.
    let claims = e["claims"].as_object().unwrap();
    assert!(claims.keys().all(|p| p.starts_with("lb:P")), "{claims:?}");
    let v = c
        .get("action=wbgetentities&ids=lb:Q999999&props=info")
        .await;
    assert!(v["entities"]["lb:Q999999"]["missing"].is_string(), "{v}");
    let v = c.get("action=wbgetentities&ids=lb:X1").await;
    assert_eq!(v["error"]["code"], "invalid-entity-id", "{v}");
    let v = c.get("action=wbgetentities&ids=nosuch:Q1").await;
    assert_eq!(v["error"]["code"], "invalid-entity-id", "{v}");
    let v = c
        .get("action=wbsearchentities&search=Librarybase&language=en&limit=3")
        .await;
    let hit = v["search"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|r| r["id"].as_str().is_some_and(|s| s.starts_with("lb:")))
        })
        .cloned();
    assert!(hit.is_some(), "the source's entities are searchable: {v}");
    let (status, p) = c
        .raw("/w/rest.php/triplespace/v0/entity/lb:Q1/provenance")
        .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["minted_by"]["kind"], "source", "{p}");
    assert_eq!(p["minted_by"]["upstream_id"], "Q1");
    assert_eq!(p["dominant"], "source/lb");
    let (status, _) = c
        .raw("/w/rest.php/triplespace/v0/entity/lb:Q999999/provenance")
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
