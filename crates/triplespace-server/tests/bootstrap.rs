//! Bootstrap mode over Postgres (0013 §9, A70; 05 §3.8): `triplespace sync` into an empty
//! instance mirror with no key map loads in blocks by binary `COPY`, from records prepared
//! on worker threads, and writes what the ordinary sync writes. The same JSON dump goes
//! into two fresh instances, once each way, and the mirror's records, its tree and the
//! `view` rows a mirror fills are compared; then the bootstrapped instance is rebuilt from
//! its log, which reads every `COPY`-written record back from its bytes.

use std::path::PathBuf;

use tokio_postgres::{Config, NoTls};
use triplespace_cli::{instance, status, sync};

const HOST: &str = "reader.example";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database(name: &str) -> String {
    let admin = admin_config().expect("checked by the caller");
    let (client, connection) = admin.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    let _ = client
        .batch_execute(&format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"))
        .await;
    client
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    url.rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap()
}

/// A query's rows, each as its columns joined by `|`.
async fn rows(database: &str, sql: &str) -> Vec<String> {
    let config: Config = database.parse().unwrap();
    let (client, connection) = config.connect(NoTls).await.unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .simple_query(sql)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|m| match m {
            tokio_postgres::SimpleQueryMessage::Row(r) => Some(
                (0..r.len())
                    .map(|i| r.get(i).unwrap_or("∅").to_string())
                    .collect::<Vec<_>>()
                    .join("|"),
            ),
            _ => None,
        })
        .collect()
}

/// An instance mirroring Librarybase, loaded from the internetdomains JSON fixture (read as
/// Librarybase's, which has no key map): bootstrapped, or through the ordinary sync.
async fn loaded(name: &str, no_bootstrap: bool) -> String {
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("tsbootstrap-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "reader".into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec!["librarybase".into()],
        adopt: None,
        owner: Some(1),
        owner_name: "Owner".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");
    sync::run(sync::Sync {
        database: database.clone(),
        provider: Some("librarybase".into()),
        tenant: None,
        source: None,
        dump: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/internetdomains.json"),
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 2,
        rejects: None,
        no_bootstrap,
    })
    .await
    .expect("sync");
    database
}

const MIRROR: &str = "(SELECT partition FROM log.partition WHERE name = 'mirror/librarybase')";

/// What both loads must agree on: the mirror's records, its size and tree shape, and the
/// view rows a mirror fills.
fn compared() -> Vec<(&'static str, String)> {
    vec![
        ("records", format!("SELECT \"offset\", key, revid, page_id, payload_type FROM log.record WHERE partition = {MIRROR} ORDER BY \"offset\"")),
        ("size", "SELECT next_offset FROM log.partition WHERE name = 'mirror/librarybase'".into()),
        ("tree", format!("SELECT level, count(*) FROM log.merkle_node WHERE partition = {MIRROR} GROUP BY level ORDER BY level")),
        ("entity", "SELECT tenant, id, type, page_id, resolved_kind, resolved_size, deleted FROM view.entity ORDER BY tenant, id".into()),
        ("term", "SELECT tenant, entity_id, kind, lang, ordinal, text FROM view.term ORDER BY 1, 2, 3, 4, 5".into()),
        ("identifier", "SELECT tenant, property, value_key, entity_id, graph, statement_id FROM view.identifier ORDER BY 1, 2, 3, 4, 5".into()),
        ("entity_source", "SELECT tenant, entity_id, graph, \"offset\", upstream_version, prev_upstream, size FROM view.entity_source ORDER BY 1, 2, 3".into()),
        ("job counts", "SELECT counts::text FROM view.job ORDER BY job_id".into()),
    ]
}

#[tokio::test]
async fn a_bootstrap_load_writes_what_a_sync_writes_over_postgres() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let ordinary = loaded("tsbootstrap_ordinary", true).await;
    let bootstrapped = loaded("tsbootstrap_loaded", false).await;
    // The load took bootstrap mode, and the other did not.
    let job = "SELECT coalesce(params->>'bootstrap', '-') FROM view.job";
    assert_eq!(rows(&bootstrapped, job).await, ["true"]);
    assert_eq!(rows(&ordinary, job).await, ["-"]);
    for (what, sql) in compared() {
        let a = rows(&ordinary, &sql).await;
        // The fixture has no external identifiers; every other table has rows.
        assert!(what == "identifier" || !a.is_empty(), "{what}: no rows");
        assert_eq!(rows(&bootstrapped, &sql).await, a, "{what}");
    }
    // Every stored leaf is its record's, and the tree folds over them.
    let leaves = format!(
        "SELECT count(*) FROM log.record r JOIN log.merkle_node m ON m.partition = r.partition
           AND m.level = 0 AND m.index = r.\"offset\" AND m.hash = r.leaf WHERE r.partition = {MIRROR}"
    );
    let records = format!("SELECT count(*) FROM log.record WHERE partition = {MIRROR}");
    assert_eq!(
        rows(&bootstrapped, &leaves).await,
        rows(&bootstrapped, &records).await
    );
    // A rebuild reads every record back from its bytes and gives the same views.
    let views: Vec<String> = compared().into_iter().skip(3).map(|(_, sql)| sql).collect();
    let mut before = Vec::new();
    for sql in &views {
        before.push(rows(&bootstrapped, sql).await);
    }
    status::rebuild(status::Target {
        database: bootstrapped.clone(),
    })
    .await
    .expect("rebuild");
    for (sql, rows_before) in views.iter().zip(before) {
        assert_eq!(
            rows(&bootstrapped, sql).await,
            rows_before,
            "after the rebuild: {sql}"
        );
    }
}
