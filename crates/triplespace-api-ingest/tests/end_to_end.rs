//! The whole write side over Postgres: adoption, a bulk batch, a mirror sync and an
//! interactive edit go through `scatter-ingest` on `PgIngest`, and the `view` tables come
//! out as the projections describe them. Named by `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use ed25519_dalek::SigningKey;
use scatter_actors::issuer::IssuerRegistry;
use scatter_ingest::adopt::{Adopted, Adoption, Author, Floors, run_adoption};
use scatter_ingest::{
    Attestation, IngestError, Request, SyncItem, run_batch, run_sync, write_operation,
};
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
use scatter_projection::{Backend, Pipeline};
use scatter_providers::Registry;
use scatter_wikibase_changeset::{JobHeader, Mode, Operation, ProviderOrder, Upstream};
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use serde_json::json;
use tokio_postgres::{Client, Config, NoTls};
use triplespace_api_ingest::PgIngest;
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
const LOCAL: u64 = 0x3001;
const PAGES: u64 = 0x3002;
const TENANT_LOG: u64 = 0x3003;
const ACTORS: u64 = 0x3004;
const INSTANCE_LOG: u64 = 0x3011;
const MIRROR: u64 = 0x3012;
const NOW: u64 = 1_790_000_000_000_000;

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
        "tsingest_test_{}_{}",
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

fn pipeline() -> Pipeline<PgIngest> {
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

/// Genesis, the tenant's registry entry, the partitions and the sequences.
async fn seed(client: Client) -> Client {
    let key = SigningKey::from_bytes(&[29; 32]);
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
            Draft {
                appended_at: 2,
                payload_type: PAYLOAD_CONFIG.into(),
                key: Some(format!("tenant:{TENANT}")),
                revid: None,
                logid: None,
                page_id: None,
                body: Body::core(
                    &Value::from_json(&json!({"kind": "tenant", "slug": TENANT, "base": "https://librarybase.org", "issuer": TENANT})),
                    &Value::Null,
                    &Value::map(vec![(Value::text("actor"), Value::text("instance:scatter"))]),
                )
                .unwrap(),
            },
        )
        .await
        .unwrap();
    let client = store.into_inner();
    for (partition, tenant, name) in [
        (LOCAL, Some(TENANT), "local"),
        (PAGES, Some(TENANT), "pages"),
        (TENANT_LOG, Some(TENANT), "log"),
        (ACTORS, Some(TENANT), "actors"),
        (INSTANCE_LOG, None, "log"),
        (MIRROR, None, "mirror/internetdomains"),
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
    PgIngest::create_sequences(&client, TENANT, &["item", "property"], &["domain"])
        .await
        .unwrap();
    client
}

fn entity(v: serde_json::Value) -> Entity {
    Entity::from_value(v).unwrap().entity
}

fn string_statement(p: &str, v: &str, datatype: &str, id: &str) -> serde_json::Value {
    json!({"mainsnak": {"snaktype": "value", "property": p, "datavalue": {"value": v, "type": "string"}, "datatype": datatype},
        "type": "statement", "rank": "normal", "id": id})
}

fn item(id: &str, label: &str, claims: &serde_json::Value) -> serde_json::Value {
    json!({"type": "item", "id": id, "labels": {"en": {"language": "en", "value": label}}, "claims": claims})
}

fn account(key: &str, name: &str) -> Draft {
    Draft {
        appended_at: NOW,
        payload_type: "scatter:v0/actor".into(),
        key: Some(key.into()),
        revid: None,
        logid: None,
        page_id: None,
        body: Body::core(
            &Value::from_json(&json!({"kind": "registered", "name": name, "status": "active"})),
            &Value::Null,
            &Value::map(vec![(Value::text("actor"), Value::text("librarybase:2"))]),
        )
        .unwrap(),
    }
}

async fn rows(client: &Client, sql: &str) -> Vec<String> {
    let mut out: Vec<String> = client
        .query(sql, &[])
        .await
        .unwrap()
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();
    out.sort();
    out
}

async fn count(client: &Client, sql: &str) -> i64 {
    client.query_one(sql, &[]).await.unwrap().get(0)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn adoption_batch_sync_and_edit_end_to_end() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    let registry = Registry::default_registry();
    // The config partition's projections (the tenant entry) first.
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();

    // 1. Adoption: two items and a property, with an account and the floors.
    let adoption = Adoption {
        issuer: TENANT.into(),
        source: "https://librarybase.org/".into(),
        version: "librarybase-20260928.json.gz".into(),
        adapter_version: Some("scatter-adapter-wikibase 0.0.1".into()),
        frozen: true,
        history: false,
        current_state_dump: false,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000), ("property".to_string(), 50)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![account("librarybase:7", "Editor")],
        property_types: BTreeMap::new(),
    };
    let items = vec![
        Adopted::current(
            entity(item(
                "Q6",
                "Six",
                &json!({"P356": [string_statement("P356", "10.1/known", "external-id", "Q6$00000000-0000-0000-0000-000000000001")]}),
            )),
            41_877,
            "2026-09-20T14:02:11Z",
            12,
        ),
        Adopted::current(
            entity(item("Q8", "Eight", &json!({}))),
            42_000,
            "2026-09-21T00:00:00Z",
            13,
        ),
        Adopted::current(
            entity(
                json!({"type": "property", "id": "P356", "datatype": "external-id",
                "labels": {"en": {"language": "en", "value": "DOI"}}, "claims": {}}),
            ),
            903,
            "2024-01-03T09:15:40Z",
            40,
        ),
    ];
    let out = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption,
        items,
        registry,
        NOW,
        2,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.adopted, 3);
    assert_eq!(out.accounts, 1);
    assert_eq!(
        rows(&client, "SELECT id || '|' || type || '|' || page_id::text || '|' || resolved_kind || '|' || local_revid::text FROM view.entity WHERE tenant = 'librarybase'").await,
        vec!["P356|property|40|source|50003", "Q6|item|12|source|50001", "Q8|item|13|source|50002"]
    );
    assert_eq!(
        rows(&client, "SELECT status || '|' || mode || '|' || source || '|' || (params->'floors'->>'page') || '|' || (counts->>'adopted') FROM view.job WHERE tenant = 'librarybase'").await,
        vec!["finished|adopt|https://librarybase.org/|2000|3"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.actor WHERE tenant = 'librarybase'"
        )
        .await,
        1
    );
    assert_eq!(count(&client, "SELECT count(*) FROM view.activity WHERE tenant = 'librarybase' AND kind = 'edit' AND new").await, 3);
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.identifier WHERE entity_id = 'Q6'"
        )
        .await,
        1
    );
    // Idempotent: the same dump again changes nothing.
    let again = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        Adoption {
            issuer: TENANT.into(),
            source: "https://librarybase.org/".into(),
            version: "librarybase-20260928.json.gz".into(),
            adapter_version: None,
            frozen: true,
            history: false,
            current_state_dump: false,
            floors: Floors::default(),
            accounts: vec![],
            property_types: BTreeMap::new(),
        },
        vec![Adopted::current(
            entity(item("Q8", "Eight", &json!({}))),
            42_000,
            "2026-09-21T00:00:00Z",
            13,
        )],
        registry,
        NOW + 1,
        2,
    )
    .await
    .unwrap();
    assert_eq!((again.counts.adopted, again.counts.unchanged), (0, 1));
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.record WHERE partition = 12289"
        )
        .await,
        3
    );

    // 2. A streamed batch: a new item with a DOI, a create-or-add that matches Q6's DOI,
    //    an add that points at the new item, and one bad line.
    let batch = r#"{"job":{"source":"citation-batch","graph":"local"}}
{"op":"create","ref":"$w1","entity":{"type":"item","id":"$w1","labels":{"en":{"language":"en","value":"A paper"}},"claims":{"P356":[{"mainsnak":{"snaktype":"value","property":"P356","datavalue":{"value":"10.1/new","type":"string"},"datatype":"external-id"},"type":"statement","rank":"normal"}]}}}
{"op":"create-or-add","ref":"$w2","match":{"P356":"10.1/known"},"entity":{"type":"item","id":"$w2","labels":{"fr":{"language":"fr","value":"Six"}},"claims":{}}}
{"op":"add","id":"$w2","claims":{"P31":[{"mainsnak":{"snaktype":"value","property":"P31","datavalue":{"value":{"entity-type":"item","id":"$w1"},"type":"wikibase-entityid"},"datatype":"wikibase-item"},"type":"statement","rank":"normal"}]}}
{"op":"add","id":"Q8","page":3,"labels":{"en":"both"}}
"#;
    let out = run_batch(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:7"),
        batch,
        &ProviderOrder::default(),
        registry,
        NOW + 2,
    )
    .await
    .unwrap();
    assert_eq!(
        out.ids.get("$w1").map(EntityId::as_str),
        Some("Q1001"),
        "above the item floor"
    );
    assert_eq!(
        out.ids.get("$w2").map(EntityId::as_str),
        Some("Q6"),
        "matched through view.identifier"
    );
    assert_eq!(
        (out.counts.created, out.counts.merged, out.counts.rejected),
        (1, 2, 1)
    );
    assert_eq!(
        rows(&client, "SELECT entity_id || '|' || kind::text || '|' || lang || '|' || text FROM view.term WHERE tenant = 'librarybase' AND entity_id IN ('Q6', 'Q1001')").await,
        vec!["Q1001|1|en|A paper", "Q6|1|en|Six", "Q6|1|fr|Six"]
    );
    assert_eq!(
        rows(&client, "SELECT resolved_kind || '|' || resolved_version::text FROM view.entity WHERE tenant = 'librarybase' AND id = 'Q6'").await,
        vec!["materialized|3"],
        "two adds re-resolved the adopted entity"
    );
    assert_eq!(
        rows(&client, "SELECT status || '|' || (counts->>'rejected') || '|' || (counts->>'created') FROM view.job WHERE tenant = 'librarybase' AND source = 'citation-batch'").await,
        vec!["finished|1|1"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.job_reject WHERE tenant = 'librarybase'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.identifier WHERE entity_id = 'Q1001' AND graph = 'local'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.activity WHERE tenant = 'librarybase' AND job_id = {}",
                out.job_id
            )
        )
        .await,
        3,
        "the create and two adds name the job"
    );

    // 3. A mirror sync of a key-mapped Domain and an ordinary item: an instance job.
    let domain = entity(json!({"type": "domain", "id": "domain:example.org",
        "labels": {"en": {"language": "en", "value": "example.org"}},
        "claims": {"P1": [string_statement("P1", "a", "string", "domain:example.org$00000000-0000-0000-0000-0000000000A1")]}}));
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-01", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            SyncItem::State {
                entity: Box::new(domain),
                upstream: Upstream::revision(55, None),
                upstream_id: Some("Q9".into()),
                upstream_aliases: Vec::new(),
            },
            SyncItem::State {
                entity: Box::new(entity(item("XDQ10", "Registrar", &json!({})))),
                upstream: Upstream::revision(56, None),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 3,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2);
    assert_eq!(
        rows(
            &client,
            "SELECT keyed_type || '|' || surrogate::text || '|' || key FROM view.keyed_surrogate"
        )
        .await,
        vec!["domain|1|example.org"]
    );
    assert_eq!(
        rows(
            &client,
            "SELECT provider || '|' || upstream_id || '|' || key FROM view.keyed_map"
        )
        .await,
        vec!["internetdomains|Q9|example.org"]
    );
    assert_eq!(
        rows(&client, "SELECT id || '|' || resolved_kind || '|' || coalesce(surrogate::text, '-') || '|' || page_id::text FROM view.entity WHERE tenant = ''").await,
        vec!["XDQ10|source|-|2", "domain:example.org|source|1|1"],
        "mirror page IDs come from the instance sequence (0013 A29)"
    );
    assert_eq!(
        rows(&client, "SELECT key FROM log.record WHERE partition = 12306 AND payload_type = 'scatter:v0/changeset'").await,
        vec!["XDQ10", "domain#1"]
    );
    assert_eq!(count(&client, "SELECT count(*) FROM view.job WHERE tenant = '' AND status = 'finished' AND graph = 'mirror/internetdomains'").await, 1);
    let xd = u64::from(registry.by_slug("internetdomains").unwrap().number);
    assert_eq!(
        count(&client, &format!("SELECT count(*) FROM log.record WHERE payload_type = 'scatter:v0/changeset' AND revid = {}", (xd << 40) | 0x37)).await,
        1,
        "the mirror record carries the upstream revision ID"
    );
    // The sync is a load: its records are no activity rows (03 §4.6; 0013 §5.5).
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.activity WHERE source = 'mirror/internetdomains' AND kind = 'sync'").await,
        0
    );

    // 4. An interactive edit asserts about the Domain in the tenant's local graph: a
    //    tenant overlay row appears, materialized, with the local label winning.
    let request = Request::new(TENANT, Attestation::by("librarybase:7"), NOW + 4);
    let mut cx = store.begin().await.unwrap();
    let add: Operation = serde_json::from_value(
        json!({"op": "add", "id": "domain:example.org", "labels": {"en": "Example"}}),
    )
    .unwrap();
    let w = write_operation(&store, &p, &mut cx, &request, &add, registry)
        .await
        .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(
        w.record.header().key.as_deref(),
        Some("domain#1"),
        "the surrogate the sync minted"
    );
    assert_eq!(
        w.record.header().page_id,
        Some(1),
        "the key's page ID, across partitions"
    );
    assert!(w.inline.rows > 0);
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || resolved_kind FROM view.entity WHERE id = 'domain:example.org'"
        )
        .await,
        vec!["librarybase|materialized", "|source"]
    );
    assert_eq!(
        rows(&client, "SELECT tenant || '|' || text FROM view.term WHERE entity_id = 'domain:example.org' AND lang = 'en'").await,
        vec!["librarybase|Example", "|example.org"]
    );
    // A stale base offset is refused.
    let mut stale = request.clone();
    stale.base_offset = Some(0);
    let mut cx = store.begin().await.unwrap();
    let err = write_operation(&store, &p, &mut cx, &stale, &add, registry)
        .await
        .unwrap_err();
    store.rollback(cx).await.unwrap();
    assert!(matches!(err, IngestError::Conflict { .. }), "{err}");

    // 5. Keys move (0009 §9, A13): Q20 owns b.example with Q21 merged into it, beside a
    //    foreign XDQ22; then Q20's value moves to c.example and Q22 becomes d.example.
    //    b.example is cleared, XDQ22 retired, and keyed_map follows the newest puts.
    let domain_of = |key: &str, owner: &str, aliases: &[&str], revid: u64| {
        let mut e = entity(json!({"type": "domain", "id": format!("domain:{key}"),
            "labels": {"en": {"language": "en", "value": key}}}));
        e.id = scatter_wikibase_model::id::EntityId::parse(&format!("domain:{key}")).unwrap();
        SyncItem::State {
            entity: Box::new(e),
            upstream: Upstream::revision(revid, None),
            upstream_id: Some(owner.into()),
            upstream_aliases: aliases.iter().map(|a| (*a).to_string()).collect(),
        }
    };
    let keyed_map = "SELECT upstream_id || '|' || key || '|' || alias::text FROM view.keyed_map WHERE provider = 'internetdomains' AND upstream_id <> 'Q9' ORDER BY upstream_id";
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-02", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            domain_of("b.example", "Q20", &["Q21"], 60),
            SyncItem::State {
                entity: Box::new(entity(item("XDQ22", "Not yet a domain", &json!({})))),
                upstream: Upstream::revision(61, None),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 6,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2);
    assert_eq!(
        rows(&client, keyed_map).await,
        vec!["Q20|b.example|false", "Q21|b.example|true"]
    );
    let out = run_sync(
        &store,
        &p,
        JobHeader::mirror("internetdomains", "2026-10-03", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            domain_of("c.example", "Q20", &["Q21"], 62),
            domain_of("d.example", "Q22", &[], 63),
        ],
        100,
        registry,
        NOW + 7,
        10,
    )
    .await
    .unwrap();
    assert_eq!(
        (
            out.counts.created,
            out.counts.cleared,
            out.counts.tombstoned
        ),
        (2, 1, 1)
    );
    assert_eq!(
        rows(&client, keyed_map).await,
        vec![
            "Q20|c.example|false",
            "Q21|c.example|true",
            "Q22|d.example|false"
        ]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.entity_source WHERE graph = 'mirror/internetdomains' AND entity_id = 'XDQ22'").await,
        0,
        "the foreign form of a newly mapped item is retired"
    );

    // Everything is consistent with a rebuild from the log.
    let keyed_before = rows(&client, keyed_map).await;
    let before = rows(&client, "SELECT (tenant, id, type, page_id, surrogate, resolved_kind, convert_from(resolved, 'UTF8'))::text FROM view.entity").await;
    let log_store = PgLog::new(client);
    p.rebuild(
        &store,
        &log_store,
        &[
            CONFIG_PARTITION,
            INSTANCE_LOG,
            MIRROR,
            ACTORS,
            LOCAL,
            PAGES,
            TENANT_LOG,
        ],
        5,
    )
    .await
    .unwrap();
    let client = log_store.into_inner();
    let after = rows(&client, "SELECT (tenant, id, type, page_id, surrogate, resolved_kind, convert_from(resolved, 'UTF8'))::text FROM view.entity").await;
    assert_eq!(before, after);
    assert_eq!(keyed_before, rows(&client, keyed_map).await);
    cleanup().await;
}

/// One revision of a history adoption (0035 A12).
fn revision(id: &str, revid: u64, parent: Option<u64>, time: &str, author: Author) -> Adopted {
    Adopted {
        id: EntityId::parse(id).unwrap(),
        entity: Some(entity(item(id, &format!("{id} at {revid}"), &json!({})))),
        redirect: None,
        source_revid: revid,
        source_parentid: parent,
        source_time: time.into(),
        source_pageid: if id == "Q6" { 12 } else { 13 },
        minor: false,
        withheld: Vec::new(),
        comment: Some(format!("edit {revid}")),
        author,
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_history_adoption_over_postgres() {
    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    let store = PgIngest::new(backend);
    let p = pipeline();
    let registry = Registry::default_registry();
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();

    let ip = || Author::Surrogate {
        kind: "anonymous".into(),
        raw: "192.0.2.1".into(),
    };
    let mut hidden = revision("Q6", 41_500, Some(41_000), "2026-09-10T00:00:00Z", ip());
    hidden.comment = None;
    hidden.withheld = vec!["comment".into()];
    hidden.minor = true;
    let mut merged = revision(
        "Q7",
        44_000,
        Some(43_000),
        "2026-09-21T00:00:00Z",
        Author::Hidden,
    );
    merged.entity = None;
    merged.redirect = Some(EntityId::parse("Q6").unwrap());
    merged.withheld = vec!["attestation".into()];
    let history = vec![
        revision(
            "Q6",
            41_000,
            None,
            "2026-09-01T00:00:00Z",
            Author::Actor("librarybase:7".into()),
        ),
        hidden,
        revision("Q6", 41_877, Some(41_500), "2026-09-20T14:02:11Z", ip()),
        revision(
            "Q7",
            43_000,
            None,
            "2026-09-02T00:00:00Z",
            Author::Actor("librarybase:7".into()),
        ),
        merged,
    ];
    let adoption = Adoption {
        issuer: TENANT.into(),
        source: "https://librarybase.org/".into(),
        version: "librarybase-20261019-history.xml.gz".into(),
        adapter_version: None,
        frozen: true,
        history: true,
        current_state_dump: false,
        floors: Floors {
            entity: BTreeMap::from([("item".to_string(), 1000)]),
            page: 2000,
            revision: 50_000,
            log: 700,
            user: 40,
        },
        accounts: vec![account("librarybase:7", "Editor")],
        property_types: BTreeMap::new(),
    };
    let out = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption.clone(),
        history.clone(),
        registry,
        NOW,
        2,
    )
    .await
    .unwrap();
    assert_eq!(
        (out.counts.adopted, out.counts.rejected),
        (5, 0),
        "{:?}",
        out.rejects
    );
    // The current state is the source's newest revision, under its own ID; the merged
    // item has no state of its own.
    assert_eq!(
        rows(&client, "SELECT id || '|' || page_id::text || '|' || local_revid::text FROM view.entity WHERE tenant = 'librarybase'").await,
        vec!["Q6|12|41877"]
    );
    // Each revision is an activity row at its source time, under its author.
    assert_eq!(
        rows(&client, "SELECT revid::text || '|' || to_char(time AT TIME ZONE 'UTC', 'YYYY-MM-DD') || '|' || coalesce(actor_key, '-') || '|' || coalesce(comment, '-') || '|' || new::text || '|' || minor::text || '|' || bot::text || '|' || visibility::text FROM view.activity WHERE tenant = 'librarybase' AND kind = 'edit' ORDER BY revid").await,
        vec![
            "41000|2026-09-01|librarybase:7|edit 41000|true|false|true|0",
            "41500|2026-09-10|actor:1|-|false|true|true|10",
            "41877|2026-09-20|actor:1|edit 41877|false|false|true|0",
            "43000|2026-09-02|librarybase:7|edit 43000|true|false|true|0",
            "44000|2026-09-21|-|edit 44000|false|false|true|12",
        ]
    );
    // A `record` ACL's bits join the ones the source withheld, and retiring it leaves
    // the source's (0023 §5; 0035 A14).
    let row = client
        .query_one(
            "SELECT partition, \"offset\" FROM view.activity WHERE tenant = 'librarybase' AND revid = 41500",
            &[],
        )
        .await
        .unwrap();
    let target = scatter_actors::Target::Record {
        partition: scatter_log_postgres::ids::partition_from_db(row.get(0)),
        offset: u64::try_from(row.get::<_, i64>(1)).unwrap(),
    };
    let who = scatter_actors::evaluate::evaluate(
        &scatter_actors::evaluate::Principal::registered(["sysop".to_string()]),
        scatter_actors::GroupRegistry::default_registry(),
        scatter_actors::GrantRegistry::default_registry(),
    );
    let mut hide = scatter_actors::Acl::read(
        target.clone(),
        scatter_actors::ReadKind::Moderation,
        "sysop",
        None,
    );
    hide.parts = vec!["content".into()];
    for (acl, want) in [(Some(hide), "11"), (None, "10")] {
        let mut cx = store.begin().await.unwrap();
        triplespace_api_ingest::acl::write(
            &store,
            &p,
            &mut cx,
            TENANT,
            "librarybase:7",
            &who,
            triplespace_api_ingest::acl::AclWrite {
                target: target.clone(),
                acl,
                reason: "test".into(),
                suppress_group: "suppress".into(),
                reserve: None,
                enclosing: Vec::new(),
            },
            NOW + 2,
        )
        .await
        .unwrap();
        store.commit(cx).await.unwrap();
        assert_eq!(
            rows(&client, "SELECT visibility::text FROM view.activity WHERE tenant = 'librarybase' AND revid = 41500").await,
            vec![want]
        );
    }
    // The IP is one surrogate, `actor:1`, with its IRI under the tenant's base.
    assert_eq!(
        rows(&client, "SELECT actor_key || '|' || kind || '|' || raw || '|' || iri FROM view.actor WHERE tenant = 'librarybase' AND issuer = 'actor'").await,
        vec!["actor:1|anonymous|192.0.2.1|https://librarybase.org/actor/1"]
    );
    // A re-run changes nothing.
    let again = run_adoption(
        &store,
        &p,
        TENANT,
        Attestation::by("librarybase:2"),
        adoption,
        history,
        registry,
        NOW + 1,
        10,
    )
    .await
    .unwrap();
    assert_eq!((again.counts.adopted, again.counts.unchanged), (0, 5));
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.record WHERE partition = 12289"
        )
        .await,
        5
    );
    cleanup().await;
}

/// A tenant declares an entity source (0078 §1), its partition opens, a sync loads it
/// under the tenant with IDs in the colon form (0078 §2, §6.4), and a deletion tombstones.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn a_tenant_declares_and_loads_an_entity_source() {
    use scatter_actors::evaluate::Effective;
    use scatter_ingest::sync::run_sync_in;
    use scatter_providers::{EntitySource, SourceType};
    use triplespace_api_ingest::sources::{SourceWriteError, write as write_source};
    use triplespace_projections::entity_sources;

    if skipped() {
        return;
    }
    let (client, backend) = fresh().await;
    let client = seed(client).await;
    // The tenant's `config` partition, where its declarations go.
    log::create_partition(
        &client,
        0x3005,
        Segments::new(8).unwrap(),
        &PartitionInfo {
            tenant: Some(TENANT.into()),
            name: "config".into(),
            graph_iri: "https://librarybase.org/graph/config".into(),
            history: History::Full,
            integrity: Integrity::Logged,
            export: Export::Public,
            hash: "sha-256".into(),
        },
    )
    .await
    .unwrap();
    let store = PgIngest::new(backend);
    let p = pipeline();
    let log_store = PgLog::new(client);
    p.catch_up(&store, &log_store, CONFIG_PARTITION, 10)
        .await
        .unwrap();
    let client = log_store.into_inner();
    let who = Effective {
        permissions: scatter_actors::group::GroupRegistry::default_registry()
            .permissions()
            .clone(),
        ..Effective::default()
    };
    let source = |name: &str| EntitySource {
        name: name.into(),
        label: "A source".into(),
        types: vec![
            SourceType {
                upstream_prefix: "Q".into(),
                entity_type: "item".into(),
                id_grammar: "digits".into(),
                iri: "https://src.example/entity/{upstream_id}".into(),
            },
            SourceType {
                upstream_prefix: "P".into(),
                entity_type: "property".into(),
                id_grammar: "digits".into(),
                iri: "https://src.example/entity/{upstream_id}".into(),
            },
        ],
        api: Some("https://src.example/w/api.php".into()),
        entity_data: None,
        revision_ids: true,
        licence: Some("CC0-1.0".into()),
        number: None,
        promoted_to: None,
    };

    // 1. A registry name is refused (0078 §3 rule 1); a fresh name is numbered and opens
    //    its partition.
    for taken in ["wikidata", "domain"] {
        let mut cx = store.begin().await.unwrap();
        let err = write_source(
            &store,
            &p,
            &mut cx,
            TENANT,
            "https://librarybase.org",
            "instance:scatter",
            &who,
            taken,
            Some(source(taken)),
            "",
            NOW,
        )
        .await
        .unwrap_err();
        store.rollback(cx).await.unwrap();
        assert!(
            matches!(
                err,
                SourceWriteError::NameTaken(_)
                    | SourceWriteError::Invalid(scatter_providers::SourceError::NameTaken(_))
            ),
            "{taken}: {err}"
        );
    }
    let mut cx = store.begin().await.unwrap();
    let written = write_source(
        &store,
        &p,
        &mut cx,
        TENANT,
        "https://librarybase.org",
        "instance:scatter",
        &who,
        "src",
        Some(source("src")),
        "declared",
        NOW,
    )
    .await
    .unwrap()
    .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(written.number, Some(4096));
    assert_eq!(
        rows(
            &client,
            "SELECT kind || ':' || code FROM view.registry WHERE tenant = 'librarybase' AND (kind = 'entity-source' OR code = 'source/src') ORDER BY 1"
        )
        .await,
        ["entity-source:src", "graph:source/src"]
    );
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM log.partition WHERE tenant = 'librarybase' AND name = 'source/src'"
        )
        .await,
        1
    );
    let registry = entity_sources::registry_for(&client, TENANT).await.unwrap();
    let src = registry.by_slug("src").unwrap();
    assert!(src.source);
    assert_eq!(
        registry.parse_foreign_id("SRC:q7").unwrap().to_string(),
        "src:Q7"
    );
    let registry: &'static Registry = Box::leak(Box::new(registry));

    // 2. A sync into the source: tenant rows, colon-form IDs, no surrogates.
    let q7 = entity(json!({"type": "item", "id": "src:Q7",
        "labels": {"en": {"language": "en", "value": "Seven"}},
        "claims": {"src:P1": [string_statement("src:P1", "a", "string", "src:Q7$00000000-0000-0000-0000-0000000000B1")]}}));
    let q8 = entity(json!({"type": "item", "id": "src:Q8",
        "labels": {"en": {"language": "en", "value": "Eight"}}, "claims": {}}));
    let out = run_sync_in(
        &store,
        &p,
        TENANT,
        JobHeader::source("src", "dump-1", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![
            SyncItem::State {
                entity: Box::new(q7),
                upstream: Upstream::revision(5, Some("2026-10-01T00:00:00Z")),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
            SyncItem::State {
                entity: Box::new(q8),
                upstream: Upstream::revision(6, Some("2026-10-01T00:00:00Z")),
                upstream_id: None,
                upstream_aliases: Vec::new(),
            },
        ],
        100,
        registry,
        NOW + 1,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.created, 2, "{out:?}");
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || graph || '|' || entity_id || '|' || upstream_version FROM view.entity_source WHERE graph = 'source/src' ORDER BY entity_id"
        )
        .await,
        [
            "librarybase|source/src|src:Q7|5",
            "librarybase|source/src|src:Q8|6"
        ]
    );
    assert_eq!(
        rows(
            &client,
            "SELECT tenant || '|' || id || '|' || type || '|' || coalesce(surrogate::text, '-') FROM view.entity WHERE id LIKE 'src:%' ORDER BY id"
        )
        .await,
        ["librarybase|src:Q7|item|-", "librarybase|src:Q8|item|-"]
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.keyed_surrogate").await,
        0,
        "a source's entity needs no surrogate (0078 §2)"
    );
    assert_eq!(
        count(&client, "SELECT count(*) FROM view.keyed_map").await,
        0
    );
    // The job is the tenant's (0078 §4).
    assert_eq!(
        count(
            &client,
            &format!(
                "SELECT count(*) FROM view.job WHERE tenant = 'librarybase' AND job_id = {}",
                out.job_id
            )
        )
        .await,
        1
    );

    // 3. An upstream deletion tombstones (a source's entity is foreign, not keyed).
    let out = run_sync_in(
        &store,
        &p,
        TENANT,
        JobHeader::source("src", "follow-1", Mode::Upsert),
        Attestation::by("instance:scatter"),
        vec![SyncItem::Deleted {
            id: EntityId::parse("src:Q8").unwrap(),
            upstream: Upstream::revision(7, None),
        }],
        100,
        registry,
        NOW + 2,
        10,
    )
    .await
    .unwrap();
    assert_eq!(out.counts.tombstoned, 1, "{out:?}");
    assert_eq!(
        rows(
            &client,
            "SELECT entity_id FROM view.entity_source WHERE graph = 'source/src' ORDER BY 1"
        )
        .await,
        ["src:Q7"]
    );

    // 4. The instance cannot write a source graph, and the record retires.
    let err = scatter_ingest::run_sync(
        &store,
        &p,
        JobHeader::source("src", "x", Mode::Upsert),
        Attestation::by("instance:scatter"),
        Vec::<SyncItem>::new(),
        100,
        registry,
        NOW + 3,
        10,
    )
    .await
    .unwrap_err();
    assert!(matches!(err, IngestError::Adapter(_)), "{err}");
    let mut cx = store.begin().await.unwrap();
    write_source(
        &store,
        &p,
        &mut cx,
        TENANT,
        "https://librarybase.org",
        "instance:scatter",
        &who,
        "src",
        None,
        "retired",
        NOW + 4,
    )
    .await
    .unwrap();
    store.commit(cx).await.unwrap();
    assert_eq!(
        count(
            &client,
            "SELECT count(*) FROM view.registry WHERE tenant = 'librarybase' AND kind = 'entity-source'"
        )
        .await,
        0
    );
    cleanup().await;
}
