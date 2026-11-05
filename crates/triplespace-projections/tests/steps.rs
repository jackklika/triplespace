//! The step 2–5 projections against a real database named by
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it. A tenant adopts an entity and
//! edits it, a key-mapped provider mirrors a Domain the tenant also asserts about, jobs
//! start and finish, and the rows come out as 0013 §5 describes them.

use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::issuer::IssuerRegistry;
use scatter_log::body::Body;
use scatter_log::cbor::Value;
use scatter_log::genesis::Genesis;
use scatter_log::header::CONFIG_PARTITION;
use scatter_log::registry::{
    Export, GraphRegistry, History, Integrity, KeyEntry, PAYLOAD_CONFIG, Scope,
};
use scatter_log::store::{Draft, LogStore};
use scatter_log::tree::Segments;
use scatter_log_postgres::{PartitionInfo, PgLog, log};
use scatter_projection::Pipeline;
use serde_json::json;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_projections::common::{
    PAYLOAD_CHANGESET, PAYLOAD_KEYED_SURROGATE, PAYLOAD_LOGEVENT,
};
use triplespace_projections::{Farm, PgBackend, milestone_pipeline};

static COUNTER: AtomicU32 = AtomicU32::new(0);
thread_local! {
    /// The scratch databases this test made. Per thread, not per process: the tests run
    /// in parallel, each on its own thread, and `cleanup` must drop only its own
    /// databases — `DROP DATABASE … WITH (FORCE)` on another test's would terminate
    /// that test's connections mid-run (SQLSTATE 57P01).
    static CREATED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

const TENANT: &str = "librarybase";
const LOCAL: u64 = 0x2001;
const MIRROR: u64 = 0x2002;
const INSTANCE_LOG: u64 = 0x2003;
const TENANT_LOG: u64 = 0x2004;
const ALL: [u64; 5] = [CONFIG_PARTITION, INSTANCE_LOG, MIRROR, LOCAL, TENANT_LOG];

const DOMAIN: &str = "domain:example.org";
const SURROGATE: &str = "domain#1";
const M_P1: &str = "domain:example.org$00000000-0000-0000-0000-0000000000A1";
const M_P5: &str = "domain:example.org$00000000-0000-0000-0000-0000000000A5";
const L_P1: &str = "domain:example.org$00000000-0000-0000-0000-0000000000B1";
const L_P3: &str = "domain:example.org$00000000-0000-0000-0000-0000000000B3";
const Q6_P2: &str = "Q6$00000000-0000-0000-0000-000000000062";
const Q6_P4: &str = "Q6$00000000-0000-0000-0000-000000000064";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn connect(config: &Config) -> Client {
    let (client, connection) = config.connect(NoTls).await.expect("connect");
    tokio::spawn(async move {
        let _ = connection.await;
    });
    client
}

async fn fresh() -> (Client, PgBackend) {
    let admin = admin_config().expect("checked by the caller");
    let name = format!(
        "tssteps_test_{}_{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    connect(&admin)
        .await
        .batch_execute(&format!("CREATE DATABASE {name}"))
        .await
        .expect("create database");
    CREATED.with_borrow_mut(|c| c.push(name.clone()));
    let mut config = admin;
    config.dbname(&name);
    let mut client = connect(&config).await;
    triplespace_db::migrator()
        .apply(&mut client)
        .await
        .expect("migrate");
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").unwrap();
    let url = url
        .rsplit_once('/')
        .map(|(head, _)| format!("{head}/{name}"))
        .unwrap();
    let pool = triplespace_db::pool::pool(&url, 4).unwrap();
    (client, PgBackend::new(pool))
}

async fn cleanup() {
    let Some(admin) = admin_config() else { return };
    let client = connect(&admin).await;
    let names: Vec<String> = CREATED.with_borrow_mut(std::mem::take);
    for n in names {
        let _ = client
            .batch_execute(&format!("DROP DATABASE IF EXISTS {n} WITH (FORCE)"))
            .await;
    }
}

fn skipped() -> bool {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return true;
    }
    false
}

fn pipeline() -> Pipeline<PgBackend> {
    milestone_pipeline(
        IssuerRegistry::default_registry().clone(),
        Farm {
            slug: "scatter".into(),
            code: "scatter".into(),
            base: "https://scatter.example".into(),
        },
    )
    .unwrap()
}

/// A record: content from JSON, a comment, and an attestation with an actor and a job.
fn draft(
    payload_type: &str,
    key: &str,
    content: &serde_json::Value,
    job: Option<u64>,
    page_id: Option<u64>,
) -> Draft {
    let mut attestation = vec![(Value::text("actor"), Value::text("local:1"))];
    if let Some(j) = job {
        attestation.push((Value::text("job"), Value::Int(i128::from(j))));
        attestation.push((Value::text("tags"), Value::Array(vec![Value::text("bulk")])));
    }
    Draft {
        appended_at: 1_790_000_000_000_000 + u64::from(COUNTER.load(Ordering::Relaxed)),
        payload_type: payload_type.into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id,
        body: Body::core(
            &Value::from_json(content),
            &Value::text("test edit"),
            &Value::map(attestation),
        )
        .unwrap(),
    }
}

fn string_statement(p: &str, v: &str, id: &str, datatype: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": datatype},
        "type": "statement", "rank": "normal", "id": id})
}

fn label(lang: &str, v: &str) -> serde_json::Value {
    json!({"language": lang, "value": v})
}

/// Genesis, the tenant, four partitions, and the records of the scenario.
#[allow(clippy::too_many_lines)]
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[23; 32]);
    let mut store = PgLog::new(client);
    Genesis {
        partition: CONFIG_PARTITION,
        scope: Scope::Instance,
        key: KeyEntry::ed25519(key.verifying_key().to_bytes()),
        segment_exponent: 16,
        appended_at: 1,
        founding: Value::map(Vec::new()),
        attestation: Value::map(vec![(
            Value::text("actor"),
            Value::text("instance:scatter"),
        )]),
        comment: None,
    }
    .append(&mut store, &GraphRegistry::embedded())
    .await
    .unwrap();
    store
        .append(
            0,
            draft(
                PAYLOAD_CONFIG,
                &format!("tenant:{TENANT}"),
                &json!({"kind": "tenant", "slug": TENANT, "base": "https://librarybase.org", "issuer": TENANT}),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    let client = store.into_inner();
    for (partition, tenant, name) in [
        (LOCAL, Some(TENANT), "local"),
        (TENANT_LOG, Some(TENANT), "log"),
        (MIRROR, None, "mirror/internetdomains"),
        (INSTANCE_LOG, None, "log"),
    ] {
        log::create_partition(
            &client,
            partition,
            Segments::new(8).unwrap(),
            &PartitionInfo {
                tenant: tenant.map(str::to_owned),
                name: name.into(),
                graph_iri: format!("https://scatter.example/graph/{name}"),
                history: History::Full,
                integrity: Integrity::Logged,
                export: Export::Public,
                hash: "sha-256".into(),
            },
        )
        .await
        .unwrap();
    }
    let mut store = PgLog::new(client);

    // The instance log: the Domain's surrogate, then the mirror sync job.
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_KEYED_SURROGATE,
                SURROGATE,
                &json!({"keyed_type": "domain", "surrogate": 1, "key": "example.org"}),
                None,
                None,
            ),
        )
        .await
        .unwrap();
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_LOGEVENT,
                "7",
                &json!({"type": "job", "action": "start", "time": 1_790_000_000_000_000u64,
                    "target": {"kind": "job", "id": 7},
                    "params": {"source": "internetdomains", "version": "2026-10-01", "mode": "upsert",
                               "graph": "mirror/internetdomains", "adapter": "scatter-adapter-internetdomains 0.0.1",
                               "args": {"subset": "all", "follow": true}}}),
                None,
                None,
            ),
        )
        .await
        .unwrap();

    // The mirror: the Domain, mapped from the provider's Q9, and an ordinary item.
    let domain_entity = json!({"type": "domain", "id": DOMAIN,
        "labels": {"en": label("en", "example.org"), "de": label("de", "example.org (de)")},
        "claims": {"P1": [string_statement("P1", "a", M_P1, "string")],
                   "P5": [string_statement("P5", "10.1/x", M_P5, "external-id")]}});
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                SURROGATE,
                &json!({"op": "put", "id": DOMAIN, "entity": domain_entity, "upstream": {"revid": 55, "time": "2026-10-01T00:00:00Z"},
                    "size": 400, "prev_size": 380, "upstream_id": "Q9",
                    "changes": {"labels": {"de": {"added": 1, "removed": 0, "changed": 0}}}}),
                Some(7),
                Some(100),
            ),
        )
        .await
        .unwrap();
    let item = json!({"type": "item", "id": "XDQ10", "labels": {"en": label("en", "Registrar")}, "claims": {}});
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                "XDQ10",
                &json!({"op": "put", "id": "XDQ10", "entity": item, "upstream": {"revid": 56}, "size": 90}),
                Some(7),
                Some(101),
            ),
        )
        .await
        .unwrap();
    store
        .append(
            INSTANCE_LOG,
            draft(
                PAYLOAD_LOGEVENT,
                "7",
                &json!({"type": "job", "action": "finish", "time": 1_790_000_001_000_000u64,
                    "target": {"kind": "job", "id": 7},
                    "params": {"counts": {"created": 2, "merged": 0, "unchanged": 0, "rejected": 1},
                               "hash_mismatches": {"time": 2},
                               "checkpoint": "scatter.example/log/mirror/internetdomains\n2\nROOT\n\n— sig\n",
                               "rejects": [{"line": 4, "match": {"P1": "not a domain"}, "reason": "invalid key"}]}}),
                None,
                None,
            ),
        )
        .await
        .unwrap();

    // The tenant: adopt Q6, edit it, assert about the Domain, correct the mirror.
    let q6 = json!({"type": "item", "id": "Q6",
        "labels": {"en": label("en", "Six")},
        "claims": {"P2": [string_statement("P2", "abc", Q6_P2, "external-id")]}});
    let mut adopt = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "adopt", "id": "Q6", "entity": q6, "source_revid": 41877, "source_time": "2026-09-20T14:02:11Z", "source_pageid": 6}),
        Some(8),
        Some(6),
    );
    adopt.revid = Some(900);
    store.append(LOCAL, adopt).await.unwrap();
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "labels": {"fr": "Six"}, "claims": {"P4": [string_statement("P4", "x", Q6_P4, "string")]}}),
        None,
        Some(6),
    );
    edit.revid = Some(901);
    store.append(LOCAL, edit).await.unwrap();
    for op in [
        json!({"op": "add", "id": DOMAIN, "labels": {"en": "Example"},
            "claims": {"P1": [string_statement("P1", "a", L_P1, "string")], "P3": [string_statement("P3", "c", L_P3, "string")]}}),
        json!({"op": "override", "id": DOMAIN, "statement": M_P5, "rank": "deprecated"}),
        json!({"op": "retain", "id": DOMAIN, "policy": "retain"}),
    ] {
        store
            .append(
                LOCAL,
                draft(PAYLOAD_CHANGESET, SURROGATE, &op, None, Some(100)),
            )
            .await
            .unwrap();
    }
    // A local job that failed.
    for ev in [
        json!({"type": "job", "action": "start", "time": 1_790_000_002_000_000u64, "target": {"kind": "job", "id": 8},
            "params": {"source": "https://librarybase.org/", "version": "librarybase-20260928.json.gz", "mode": "adopt", "graph": "local"}}),
        json!({"type": "job", "action": "fail", "time": 1_790_000_003_000_000u64, "target": {"kind": "job", "id": 8},
            "params": {"error": "ts-adopt-not-empty"}}),
    ] {
        store
            .append(TENANT_LOG, draft(PAYLOAD_LOGEVENT, "8", &ev, None, None))
            .await
            .unwrap();
    }
    store.into_inner()
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect()
}

/// Rows sorted bytewise, independent of the database's collation.
async fn sorted(client: &Client, sql: &str) -> Vec<String> {
    let mut out = rows(client, sql).await;
    out.sort();
    out
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn steps_two_to_five_project_and_rebuild() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    for partition in ALL {
        let progress = p.catch_up(&backend, &store, partition, 2).await.unwrap();
        assert_eq!(progress.records, progress.head, "partition {partition:#x}");
    }
    let client = store.into_inner();

    // Step 1: the surrogate.
    let key: Option<String> = client
        .query_one("SELECT key FROM view.keyed_surrogate WHERE tenant = '' AND keyed_type = 'domain' AND surrogate = 1", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(key.as_deref(), Some("example.org"));

    // Step 2: cursors, the keyed map, jobs.
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || entity_id || '|' || graph || '|' || coalesce(upstream_version, '-') || '|' || coalesce(prev_upstream, '-') || '|' || (size > 0)::text || '|' || coalesce(job_id::text, '-') FROM view.entity_source").await,
        vec![
            "librarybase|Q6|local|-|-|true|-",
            "librarybase|domain:example.org|local|-|-|true|-",
            "|XDQ10|mirror/internetdomains|56|-|true|7",
            "|domain:example.org|mirror/internetdomains|55|-|true|7",
        ]
    );
    // The local cursor is the newest local record: Q6's edit, not its adoption.
    let q6_offset: i64 = client
        .query_one("SELECT \"offset\" FROM view.entity_source WHERE tenant = 'librarybase' AND entity_id = 'Q6'", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(q6_offset, 1);
    assert_eq!(
        sorted(&client, "SELECT provider || '|' || upstream_id || '|' || keyed_type || '|' || key FROM view.keyed_map WHERE tenant = ''").await,
        vec!["internetdomains|Q9|domain|example.org"]
    );
    // The upstream ID is an alias of the key (05 §4.1): `XDQ9` resolves to the Domain,
    // an unmapped or unknown one to nothing.
    let registry = scatter_providers::Registry::default_registry();
    let alias = triplespace_projections::read::mapped_alias(
        &client,
        registry,
        TENANT,
        &"XDQ9".parse().unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        alias.map(|a| a.as_str().to_string()),
        Some(DOMAIN.to_string())
    );
    for id in ["XDQ999", "WDQ9", "Q9", DOMAIN] {
        assert!(
            triplespace_projections::read::mapped_alias(
                &client,
                registry,
                TENANT,
                &id.parse().unwrap()
            )
            .await
            .unwrap()
            .is_none(),
            "{id}"
        );
    }
    let job7 = client
        .query_one("SELECT status, source, source_version, mode, graph, adapter_version, counts::text, checkpoint_size, start_offset, finish_offset, actor_key, params::text FROM view.job WHERE tenant = '' AND job_id = 7", &[])
        .await
        .unwrap();
    assert_eq!(job7.get::<_, String>(0), "finished");
    assert_eq!(job7.get::<_, String>(1), "internetdomains");
    assert_eq!(
        job7.get::<_, Option<String>>(2).as_deref(),
        Some("2026-10-01")
    );
    assert_eq!(job7.get::<_, Option<String>>(3).as_deref(), Some("upsert"));
    assert_eq!(job7.get::<_, String>(4), "mirror/internetdomains");
    assert_eq!(
        job7.get::<_, Option<String>>(5).as_deref(),
        Some("scatter-adapter-internetdomains 0.0.1")
    );
    let counts: serde_json::Value = serde_json::from_str(&job7.get::<_, String>(6)).unwrap();
    assert_eq!(counts["created"], 2);
    assert_eq!(counts["hash_mismatches"]["time"], 2);
    assert_eq!(job7.get::<_, Option<i64>>(7), Some(2));
    assert_eq!(
        (job7.get::<_, i64>(8), job7.get::<_, Option<i64>>(9)),
        (1, Some(2))
    );
    assert_eq!(job7.get::<_, String>(10), "local:1");
    assert_eq!(
        job7.get::<_, String>(11),
        r#"{"follow": true, "subset": "all"}"#
    );
    assert_eq!(
        sorted(&client, "SELECT line::text || '|' || coalesce(match_key, '') || '|' || reason FROM view.job_reject WHERE job_id = 7").await,
        vec![r#"4|{"P1":"not a domain"}|invalid key"#]
    );
    let job8 = client
        .query_one("SELECT status, counts::text, mode FROM view.job WHERE tenant = 'librarybase' AND job_id = 8", &[])
        .await
        .unwrap();
    assert_eq!(job8.get::<_, String>(0), "failed");
    assert!(job8.get::<_, String>(1).contains("ts-adopt-not-empty"));
    assert_eq!(job8.get::<_, Option<String>>(2).as_deref(), Some("adopt"));

    // Step 4: entities, terms, identifiers.
    let entities = sorted(&client, "SELECT tenant || '|' || id || '|' || type || '|' || page_id::text || '|' || coalesce(surrogate::text, '-') || '|' || coalesce(retention, '-') || '|' || resolved_kind || '|' || resolved_version::text || '|' || coalesce(local_revid::text, '-') FROM view.entity").await;
    assert_eq!(
        entities,
        vec![
            "librarybase|Q6|item|6|-|-|materialized|2|901",
            // Resolved three times: the add, the override and the retain each re-resolve.
            "librarybase|domain:example.org|domain|100|1|retain|materialized|3|-",
            "|XDQ10|item|101|-|-|source|1|-",
            "|domain:example.org|domain|100|1|-|source|1|-",
        ]
    );
    let shared: Option<Vec<u8>> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = '' AND id = $1",
            &[&DOMAIN],
        )
        .await
        .unwrap()
        .get(0);
    assert!(
        shared.is_none(),
        "a single mirror source is not stored twice"
    );
    let overlay: Vec<u8> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = 'librarybase' AND id = $1",
            &[&DOMAIN],
        )
        .await
        .unwrap()
        .get::<_, Option<Vec<u8>>>(0)
        .unwrap();
    let overlay: serde_json::Value = serde_json::from_slice(&overlay).unwrap();
    assert_eq!(
        overlay["labels"]["en"]["value"], "Example",
        "local wins the label"
    );
    assert_eq!(
        overlay["labels"]["de"]["value"], "example.org (de)",
        "the mirror keeps its own"
    );
    assert_eq!(
        overlay["claims"]["P1"].as_array().unwrap().len(),
        1,
        "P1 fused across graphs"
    );
    assert_eq!(
        overlay["claims"]["P1"][0]["id"], L_P1,
        "the local member's ID, local being first"
    );
    assert_eq!(
        overlay["claims"]["P5"][0]["rank"], "deprecated",
        "the override applied"
    );
    assert_eq!(overlay["claims"]["P3"][0]["id"], L_P3);
    let q6: Vec<u8> = client
        .query_one(
            "SELECT resolved FROM view.entity WHERE tenant = 'librarybase' AND id = 'Q6'",
            &[],
        )
        .await
        .unwrap()
        .get::<_, Option<Vec<u8>>>(0)
        .unwrap();
    let q6: serde_json::Value = serde_json::from_slice(&q6).unwrap();
    assert_eq!(q6["labels"]["fr"]["value"], "Six");
    assert_eq!(q6["claims"].as_object().unwrap().len(), 2);
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || entity_id || '|' || kind::text || '|' || lang || '|' || text FROM view.term").await,
        vec![
            "librarybase|Q6|1|en|Six",
            "librarybase|Q6|1|fr|Six",
            "librarybase|domain:example.org|1|de|example.org (de)",
            "librarybase|domain:example.org|1|en|Example",
            "|XDQ10|1|en|Registrar",
            // A mirrored entity's shared rows hold the instance's term languages only
            // (05 §5.8; 0082 §6): `mul`, `en` and the tenants' content languages, here
            // `en`. Its German label stays in the record; the tenant's rows keep it.
            "|domain:example.org|1|en|example.org",
        ]
    );
    assert_eq!(
        sorted(&client, "SELECT tenant || '|' || property || '|' || entity_id || '|' || graph || '|' || statement_id FROM view.identifier").await,
        vec![
            format!("librarybase|P2|Q6|local|{Q6_P2}"),
            format!("librarybase|P5|domain:example.org|mirror/internetdomains|{M_P5}"),
            format!("|P5|domain:example.org|mirror/internetdomains|{M_P5}"),
        ]
    );

    // Step 5: activity, in partition then offset order: local, mirror, instance log, tenant log.
    assert_eq!(
        rows(&client, "SELECT tenant || '|' || kind || '|' || source || '|' || coalesce(summary_op, log_type || '/' || log_action) || '|' || coalesce(target_id, '-') || '|' || coalesce(actor_key, '-') || '|' || coalesce(job_id::text, '-') || '|' || coalesce(new::text, '-') FROM view.activity ORDER BY partition, \"offset\"").await,
        vec![
            "librarybase|edit|local|adopt|Q6|local:1|8|true",
            "librarybase|edit|local|add|Q6|local:1|-|false",
            "librarybase|edit|local|add|domain:example.org|local:1|-|false",
            "librarybase|edit|local|override|domain:example.org|local:1|-|false",
            "librarybase|edit|local|retain|domain:example.org|local:1|-|false",
            // Job 7 is the follower's (`follow`), so its mirror records are rows; a load's
            // are not (03 §4.6; 0013 §5.5), as `end_to_end.rs` and the site test check.
            "|sync|mirror/internetdomains|put|domain:example.org|local:1|7|false",
            "|sync|mirror/internetdomains|put|XDQ10|local:1|7|false",
            "|job|log|job/start|7|local:1|-|-",
            "|job|log|job/finish|7|local:1|-|-",
            "librarybase|job|log|job/start|8|local:1|-|-",
            "librarybase|job|log|job/fail|8|local:1|-|-",
        ]
    );
    let put_row = client
        .query_one("SELECT revid, size, delta, changes::text, tags, comment, summary_args FROM view.activity WHERE source = 'mirror/internetdomains' AND target_id = $1", &[&DOMAIN])
        .await
        .unwrap();
    assert_eq!(put_row.get::<_, Option<i64>>(0), None);
    assert_eq!(put_row.get::<_, Option<i32>>(1), Some(400));
    assert_eq!(put_row.get::<_, Option<i32>>(2), Some(20));
    assert!(
        put_row
            .get::<_, Option<String>>(3)
            .unwrap()
            .contains("\"de\"")
    );
    assert_eq!(put_row.get::<_, Vec<String>>(4), vec!["bulk"]);
    assert_eq!(
        put_row.get::<_, Option<String>>(5).as_deref(),
        Some("test edit")
    );
    assert_eq!(put_row.get::<_, Option<String>>(6), None);
    let add_row = client
        .query_one("SELECT revid, summary_args FROM view.activity WHERE source = 'local' AND summary_op = 'add' AND target_id = 'Q6'", &[])
        .await
        .unwrap();
    assert_eq!(add_row.get::<_, Option<i64>>(0), Some(901));
    assert_eq!(
        add_row.get::<_, Option<String>>(1).as_deref(),
        Some("claims:P4 labels:fr")
    );

    // The feeds over those rows (0020 §1–2): recent changes, contributions, a history.
    feeds(&client).await;

    // The tier-1 state tables (0083 §4): one row per local revision, one per key.
    Box::pin(revision_state(&client)).await;

    // A tombstone retracts the mirror's contribution and the rows that hung off it.
    let mut store = PgLog::new(client);
    store
        .append(
            MIRROR,
            draft(
                PAYLOAD_CHANGESET,
                "XDQ10",
                &json!({"op": "tombstone", "id": "XDQ10", "upstream": {"revid": 57}}),
                Some(7),
                Some(101),
            ),
        )
        .await
        .unwrap();
    p.catch_up(&backend, &store, MIRROR, 10).await.unwrap();
    let client = store.into_inner();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.entity_source WHERE entity_id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.entity WHERE id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.term WHERE entity_id = 'XDQ10'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.activity WHERE summary_op = 'tombstone'"
        )
        .await,
        1
    );

    // A rebuild from offset 0, with the partitions in dependency order, reproduces the rows.
    let before = snapshot(&client).await;
    let store = PgLog::new(client);
    let progress = p.rebuild(&backend, &store, &ALL, 3).await.unwrap();
    assert_eq!(progress.len(), ALL.len());
    assert_eq!(snapshot(store.client()).await, before);
    // The lag is zero everywhere.
    for partition in ALL {
        for lag in p.lags(&backend, &store, partition).await.unwrap() {
            assert_eq!(
                lag.applied, lag.head,
                "{} on {partition:#x}",
                lag.projection
            );
        }
    }
    // Without the state rows, the readers fold (0083 A1).
    revision_state_fallback(store.client()).await;

    // A key with no graph_state row is seeded from its earlier records on its next edit,
    // so the state is whole without a rebuild (0083 A1).
    let mut store = store;
    let mut edit = draft(
        PAYLOAD_CHANGESET,
        "Q6",
        &json!({"op": "add", "id": "Q6", "labels": {"de": "Sechs"}}),
        None,
        Some(6),
    );
    edit.revid = Some(902);
    store.append(LOCAL, edit).await.unwrap();
    p.catch_up(&backend, &store, LOCAL, 10).await.unwrap();
    let (state, offset, version) =
        triplespace_projections::revision_state::graph_state(store.client(), TENANT, "Q6", "local")
            .await
            .unwrap()
            .unwrap();
    assert_eq!((offset, version), (5, 1), "a fresh row at the new edit");
    assert_eq!(
        state.offsets,
        vec![0, 1, 5],
        "seeded from the records before it"
    );
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 3, "en, fr, de");
    let (state, revid) =
        triplespace_projections::revision_state::state_at_record(store.client(), LOCAL, 5)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(revid, 902);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 3);
    cleanup().await;
}

/// `view.graph_state` and `view.entity_revision` (0083 §4) after the catch-up, and the
/// readers over them: a past state is one row; where the row is missing (a database
/// migrated but not rebuilt, 0083 A1) the fold answers the same; a key whose row is
/// missing is seeded whole on its next edit.
#[allow(clippy::too_many_lines)]
async fn revision_state(client: &Client) {
    use scatter_wikibase_model::id::EntityId;
    use triplespace_projections::revision_state::{graph_state, revision_state, state_at_record};
    use triplespace_projections::revisions::{
        LocalRevision, change_at, fold_to, local_change_at, local_revisions, local_state_at,
    };
    let q6: EntityId = "Q6".parse().unwrap();
    let domain: EntityId = DOMAIN.parse().unwrap();

    // One graph_state row per locally edited key, at the key's latest local offset; one
    // entity_revision row per local revision that has a revision ID (the Domain's
    // records are surrogate-keyed assertions without one, 01 §2.5).
    assert_eq!(
        sorted(
            client,
            "SELECT tenant || '|' || entity_id || '|' || graph || '|' || \"offset\" || '|' || version FROM view.graph_state"
        )
        .await,
        vec![
            "librarybase|Q6|local|1|2",
            "librarybase|domain:example.org|local|4|3",
        ]
    );
    assert_eq!(
        sorted(
            client,
            "SELECT tenant || '|' || revid || '|' || entity_id || '|' || \"offset\" FROM view.entity_revision"
        )
        .await,
        vec!["librarybase|900|Q6|0", "librarybase|901|Q6|1"]
    );

    // The stored states are the fold's.
    let revs = local_revisions(client, TENANT, &q6).await.unwrap();
    assert_eq!(revs.len(), 2);
    let (state, offset, version) = graph_state(client, TENANT, "Q6", "local")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((offset, version), (1, 2));
    assert_eq!(state, fold_to(&revs, 1).unwrap().1);
    let (state, partition, offset) = revision_state(client, TENANT, 900).await.unwrap().unwrap();
    assert_eq!((partition, offset), (LOCAL, 0));
    assert_eq!(state, fold_to(&revs, 0).unwrap().1);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 1, "en only");
    let (state, revid) = state_at_record(client, LOCAL, 1).await.unwrap().unwrap();
    assert_eq!(revid, 901);
    assert_eq!(state.entity.as_ref().unwrap().labels.len(), 2, "en and fr");
    assert!(revision_state(client, TENANT, 902).await.unwrap().is_none());
    let domain_revs = local_revisions(client, TENANT, &domain).await.unwrap();
    assert_eq!(domain_revs.len(), 3);
    let (state, offset, _) = graph_state(client, TENANT, DOMAIN, "local")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(offset, 4);
    assert_eq!(state, fold_to(&domain_revs, 4).unwrap().1);
    assert_eq!(
        state.retention,
        Some(scatter_wikibase_changeset::Retention::Retain)
    );

    // The readers: via the table and via the fold, the same answer.
    let via_table = local_state_at(client, TENANT, &q6, 0)
        .await
        .unwrap()
        .unwrap();
    let via_fold = fold_to(&revs, 0).unwrap();
    assert_eq!(via_table.0.revid(), via_fold.0.revid());
    assert_eq!(via_table.1, via_fold.1);
    let change = local_change_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    let folded = change_at(&revs, 1).unwrap();
    assert_eq!(change.before, folded.before);
    assert_eq!(change.after, folded.after);
    assert_eq!(change.previous.as_ref().map(LocalRevision::offset), Some(0));
    assert_eq!(change.revision.revid(), Some(901));
    let first = local_change_at(client, TENANT, &q6, 0)
        .await
        .unwrap()
        .unwrap();
    assert!(first.before.entity.is_none());
    assert!(first.previous.is_none());
    assert!(
        local_state_at(client, TENANT, &q6, 3)
            .await
            .unwrap()
            .is_none(),
        "the Domain's record, not Q6's"
    );
    assert!(
        local_state_at(client, TENANT, &q6, 9)
            .await
            .unwrap()
            .is_none()
    );
}

/// A database migrated to the tables but not yet rebuilt (0083 A1) has no rows for its
/// earlier revisions: the fold answers the same, and the latest state folds too.
async fn revision_state_fallback(client: &Client) {
    use scatter_wikibase_model::id::EntityId;
    use triplespace_projections::revisions::{
        LocalRevision, change_at, fold_to, local_change_at, local_revisions, local_state_at,
    };
    let q6: EntityId = "Q6".parse().unwrap();
    let revs = local_revisions(client, TENANT, &q6).await.unwrap();
    let folded = change_at(&revs, 1).unwrap();
    client
        .execute("DELETE FROM view.entity_revision WHERE revid = 901", &[])
        .await
        .unwrap();
    let fallback = local_state_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fallback.1, fold_to(&revs, 1).unwrap().1);
    let fallback = local_change_at(client, TENANT, &q6, 1)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fallback.before, folded.before);
    assert_eq!(fallback.after, folded.after);
    assert_eq!(
        fallback.previous.as_ref().map(LocalRevision::offset),
        Some(0)
    );
    client
        .execute("DELETE FROM view.graph_state WHERE entity_id = 'Q6'", &[])
        .await
        .unwrap();
    let order = triplespace_projections::EntityProjection::default();
    let current = triplespace_projections::read::current(client, &order, TENANT, &q6)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        current.local().unwrap().labels.len(),
        2,
        "the latest state folds without a graph_state row"
    );
    assert_eq!(current.local_revid, Some(901));
}

/// Every derived row, as text. `resolved_version` and `first_seen` may legitimately differ
/// after a rebuild that interleaves partitions differently; everything else must not.
/// Recent changes, contributions and a history are one query with three target sets.
#[allow(clippy::too_many_lines)]
async fn feeds(client: &Client) {
    use triplespace_projections::feed::{
        self, Cursor, Direction, Feed, Filters, Flag, TargetSet, Viewer, target_kind,
    };
    let viewer = Viewer::default();
    let brief = |p: &feed::Page| -> Vec<String> {
        p.rows
            .iter()
            .map(|r| {
                format!(
                    "{}|{}|{}",
                    r.kind,
                    r.summary_op
                        .clone()
                        .or_else(|| r.log_action.clone())
                        .unwrap_or_default(),
                    r.target.id.clone().unwrap_or_default()
                )
            })
            .collect()
    };

    // Recent changes: everything but syncs, newest first; the mirror job's own rows
    // from the instance log are in it.
    let mut rc = Feed::everything(50);
    rc.filters.kinds = vec!["edit".into(), "log".into(), "job".into()];
    let page = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(page.rows.len(), 9, "{:?}", brief(&page));
    assert!(page.next.is_none());
    assert!(page.rows.iter().all(|r| r.kind != "sync"));
    assert!(
        page.rows
            .windows(2)
            .all(|w| w[0].cursor().time >= w[1].cursor().time),
        "newest first"
    );
    let adopt = page
        .rows
        .iter()
        .find(|r| r.summary_op.as_deref() == Some("adopt"))
        .unwrap();
    assert!(
        adopt.new,
        "an adopted entity's first record is new (16 §3.1)"
    );
    assert_eq!(adopt.target.entity_type.as_deref(), Some("item"));
    assert_eq!(
        adopt.target.label,
        Some(("Six".to_string(), "en".to_string()))
    );
    assert_eq!(adopt.actor.key.as_deref(), Some("local:1"));

    // Paging: two at a time, then from the cursor, covers the set once in order.
    rc.limit = 2;
    let first = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(first.rows.len(), 2);
    let mut all = brief(&first);
    let mut next = first.next;
    while let Some(c) = next {
        rc.cursor = Some(c);
        let p = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
        all.extend(brief(&p));
        next = p.next;
    }
    assert_eq!(
        all,
        brief(&page),
        "paging walks the same rows in the same order"
    );
    rc.cursor = None;
    rc.limit = 50;

    // Oldest first, and a window.
    rc.direction = Direction::Newer;
    let asc = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    let mut expected = brief(&page);
    expected.reverse();
    assert_eq!(brief(&asc), expected);
    let since = asc.rows[2].time;
    rc.filters.since = Some(since);
    let windowed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(
        windowed.rows.len(),
        asc.rows.iter().filter(|r| r.time >= since).count()
    );
    assert!(windowed.rows.iter().all(|r| r.time >= since));
    rc.filters.since = None;
    rc.filters.until = Some(since);
    let windowed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(windowed.rows.iter().all(|r| r.time <= since));
    rc.filters.until = None;

    // Flags and tags narrow the set.
    rc.filters.new = Flag::Only;
    let new = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(brief(&new), vec!["edit|adopt|Q6"]);
    rc.filters.new = Flag::Not;
    rc.filters.tag = Some("bot".into());
    let bot = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(bot.rows.is_empty());
    rc.filters = Filters::default();
    rc.filters.entity_types = vec!["domain".into()];
    let domains = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert!(
        domains
            .rows
            .iter()
            .all(|r| r.target.id.as_deref() == Some(DOMAIN))
    );
    assert_eq!(domains.rows.len(), 4, "{:?}", brief(&domains));
    rc.filters = Filters::default();
    rc.filters.log_type = Some("job".into());
    rc.filters.log_action = Some("fail".into());
    let failed = feed::query(client, TENANT, &rc, &viewer).await.unwrap();
    assert_eq!(brief(&failed), vec!["job|fail|8"]);

    // Contributions: the actor set.
    let contribs = Feed {
        set: TargetSet::Actor("local:1".into()),
        filters: Filters {
            kinds: vec!["edit".into()],
            ..Filters::default()
        },
        direction: Direction::Older,
        cursor: None,
        limit: 50,
        languages: vec!["en".into()],
    };
    let page = feed::query(client, TENANT, &contribs, &viewer)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 5);
    assert!(
        page.rows
            .iter()
            .all(|r| r.actor.key.as_deref() == Some("local:1"))
    );

    // A history: the one-target set keeps syncs, and reaches the shared mirror partition.
    let history = Feed {
        set: TargetSet::Target {
            kind: target_kind::ENTITY,
            id: DOMAIN.into(),
        },
        filters: Filters::default(),
        direction: Direction::Newer,
        cursor: None,
        limit: 50,
        languages: vec!["en".into()],
    };
    let page = feed::query(client, TENANT, &history, &viewer)
        .await
        .unwrap();
    assert_eq!(
        brief(&page),
        vec![
            "edit|add|domain:example.org",
            "edit|override|domain:example.org",
            "edit|retain|domain:example.org",
            "sync|put|domain:example.org",
        ],
        "in time order, the mirror's sync after the local edits that preceded it"
    );
    let sync = &page.rows[3];
    assert_eq!(sync.source, "mirror/internetdomains");
    assert_eq!(sync.size, Some(400));
    assert_eq!(sync.delta, Some(20));
    assert!(sync.changes.is_some());
    assert_eq!(sync.tags, vec!["bulk"]);

    // The source switch narrows a history to one graph.
    let mut made_here = history.clone();
    made_here.filters.sources = vec!["local".into()];
    let page = feed::query(client, TENANT, &made_here, &viewer)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 3);
    assert!(page.rows.iter().all(|r| r.source == "local"));
    made_here.filters.sources = vec!["mirror/internetdomains".into()];
    let page = feed::query(client, TENANT, &made_here, &viewer)
        .await
        .unwrap();
    assert_eq!(brief(&page), vec!["sync|put|domain:example.org"]);

    // A cursor is a token a client hands back.
    let c = Cursor {
        time: 1,
        partition: 2,
        offset: 3,
    };
    assert_eq!(Cursor::parse(&c.token()), Some(c));
}

async fn snapshot(client: &Client) -> Vec<String> {
    let mut all = Vec::new();
    all.extend(
        sorted(
            client,
            "SELECT (tenant, id, type, page_id, surrogate, canonical_id, retention, local_offset, local_revid, resolved_kind,
                     convert_from(resolved, 'UTF8'), resolved_size, deleted)::text FROM view.entity",
        )
        .await,
    );
    for table in [
        "view.entity_source",
        "view.term",
        "view.identifier",
        "view.keyed_map",
        "view.job",
        "view.job_reject",
        "view.activity",
        "view.graph_state",
        "view.entity_revision",
    ] {
        all.extend(sorted(client, &format!("SELECT t::text FROM {table} t")).await);
    }
    all
}

/// The instance's term languages (05 §5.8; 0082 §6, A2): by default `mul`, `en` and the
/// instance's content languages, every tenant's `site.language` and the instance's default
/// for it; an instance `entities.term_languages` ceiling replaces the default.
#[tokio::test]
async fn the_instance_term_languages() {
    if skipped() {
        return;
    }
    let (client, _) = fresh().await;
    let languages = || async {
        rows(
            &client,
            &format!(
                "WITH {} SELECT lang FROM shared_term_languages ORDER BY lang",
                triplespace_projections::entities::SHARED_TERM_LANGUAGES
            ),
        )
        .await
    };
    assert_eq!(languages().await, ["en", "mul"], "no configuration");
    let set = |tenant: &'static str,
               kind: &'static str,
               code: &'static str,
               config: serde_json::Value| {
        let client = &client;
        async move {
            client
                .execute(
                    "INSERT INTO view.registry (tenant, kind, code, config, \"offset\") VALUES ($1, $2, $3, $4, 1)
                     ON CONFLICT (tenant, kind, code) DO UPDATE SET config = EXCLUDED.config",
                    &[&tenant, &kind, &code, &config],
                )
                .await
                .unwrap();
        }
    };
    set(
        "librarybase",
        "site",
        "site.language",
        json!({"kind": "site", "name": "site.language", "value": "de"}),
    )
    .await;
    set(
        "other",
        "site",
        "site.language",
        json!({"kind": "site", "name": "site.language", "value": "en"}),
    )
    .await;
    set("", "template", "site:site.language", json!({"kind": "template", "record": {"kind": "site", "name": "site.language", "value": "fr"}})).await;
    assert_eq!(
        languages().await,
        ["de", "en", "fr", "mul"],
        "each tenant's content language and the instance's default for it"
    );
    set(
        "",
        "tenancy",
        "entities.term_languages",
        json!({"kind": "tenancy", "value": ["nl", "en"]}),
    )
    .await;
    assert_eq!(
        languages().await,
        ["en", "nl"],
        "the instance ceiling replaces the default"
    );
    cleanup().await;
}

#[tokio::test]
async fn a_rebuild_in_the_wrong_order_fails_loudly() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let p = pipeline();
    let store = PgLog::new(client);
    // The local partition before the instance log: the Domain's surrogate is unknown when
    // its local records are replayed.
    p.catch_up(&backend, &store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let err = p.catch_up(&backend, &store, LOCAL, 10).await.unwrap_err();
    assert!(err.to_string().contains("no surrogate"), "{err}");
    // Once the instance log is in, the same catch-up succeeds.
    p.catch_up(&backend, &store, INSTANCE_LOG, 10)
        .await
        .unwrap();
    p.catch_up(&backend, &store, LOCAL, 10).await.unwrap();
    cleanup().await;
}
