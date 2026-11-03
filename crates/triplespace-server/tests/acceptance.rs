//! The milestone-1 acceptance test (`claude/milestone-1-critical-path.md`): a bot client
//! logs in with a bot password, reads `Item:Q6` and `Domain:wikipedia.org`, and saves an
//! edit — against the router the server runs, over a database the CLI set up from a
//! Librarybase-shaped XML dump and an internetdomains-shaped JSON dump. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::path::PathBuf;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt as _;
use serde_json::{Value, json};
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::{Secret, keys};
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::forwarder::{Create, Forwarder, Revoke, Target};
use triplespace_cli::{accounts, adopt, instance, sync};

const DB: &str = "tsaccept_test";
const TENANT: &str = "librarybase";
const HOST: &str = "librarybase.org";

const DUMP_XML: &str = r#"<mediawiki xmlns="http://www.mediawiki.org/xml/export-0.11/" version="0.11" xml:lang="en">
  <siteinfo><sitename>Librarybase</sitename><namespaces>
    <namespace key="120" case="first-letter">Item</namespace>
    <namespace key="122" case="first-letter">Property</namespace>
  </namespaces></siteinfo>
  <page><title>Property:P12</title><ns>122</ns><id>40</id>
    <revision><id>903</id><timestamp>2024-01-03T09:15:40Z</timestamp>
      <contributor><username>Alice</username><id>7</id></contributor>
      <model>wikibase-property</model><format>application/json</format>
      <text xml:space="preserve">{"type":"property","id":"P12","datatype":"string","labels":{"en":{"language":"en","value":"note"}},"claims":{}}</text>
    </revision></page>
  <page><title>Item:Q6</title><ns>120</ns><id>12</id>
    <revision><id>41877</id><timestamp>2026-09-20T14:02:11Z</timestamp>
      <contributor><username>Bob</username><id>9</id></contributor>
      <comment>/* wbeditentity-create-item:0| */ Six</comment>
      <model>wikibase-item</model><format>application/json</format>
      <text xml:space="preserve">{"type":"item","id":"Q6","labels":{"en":{"language":"en","value":"Six &amp; more"}},"descriptions":{"en":{"language":"en","value":"a number"}},"claims":{"P12":[{"mainsnak":{"snaktype":"value","property":"P12","datatype":"string","datavalue":{"value":"six","type":"string"}},"type":"statement","rank":"normal","id":"Q6$11111111-1111-1111-1111-111111111111"}]}}</text>
    </revision></page>
</mediawiki>
"#;

fn dump_json() -> String {
    let st = |p: &str, v: Value, guid: &str, dt: &str| {
        let dv = if dt == "string" {
            json!({"value": v, "type": "string"})
        } else {
            json!({"value": {"entity-type": "item", "id": v}, "type": "wikibase-entityid"})
        };
        json!({"mainsnak": {"snaktype": "value", "property": p, "datatype": dt, "datavalue": dv},
            "type": "statement", "rank": "normal", "id": guid})
    };
    let items = [
        json!({"type": "item", "id": "Q9", "labels": {"en": {"language": "en", "value": "example.org"}}, "descriptions": {}, "aliases": {},
            "claims": {"P1": [st("P1", json!("example.org"), "Q9$00000000-0000-0000-0000-0000000000A1", "string")]},
            "sitelinks": {}, "lastrevid": 55, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "item", "id": "Q11", "labels": {"en": {"language": "en", "value": "Wikipedia"}},
            "descriptions": {"en": {"language": "en", "value": "the free encyclopedia"}}, "aliases": {},
            "claims": {"P1": [st("P1", json!("wikipedia.org"), "Q11$00000000-0000-0000-0000-0000000000B1", "string")],
                       "P8": [st("P8", json!("Q9"), "Q11$00000000-0000-0000-0000-0000000000B2", "wikibase-item")]},
            "sitelinks": {}, "lastrevid": 56, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "property", "id": "P1", "datatype": "string", "labels": {"en": {"language": "en", "value": "domain name"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "lastrevid": 3, "modified": "2026-10-01T00:00:00Z"}),
        json!({"type": "property", "id": "P8", "datatype": "wikibase-item", "labels": {"en": {"language": "en", "value": "related domain"}},
            "descriptions": {}, "aliases": {}, "claims": {}, "lastrevid": 4, "modified": "2026-10-01T00:00:00Z"}),
    ];
    let lines: Vec<String> = items.iter().map(ToString::to_string).collect();
    format!("[\n{}\n]\n", lines.join(",\n"))
}

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

/// A client with a cookie jar of one cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
    bearer: Option<String>,
}

impl Client {
    async fn call(&mut self, method: &str, query: &str, form: Option<&str>) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/w/api.php?{query}"))
            .header(header::HOST, HOST);
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        if let Some(b) = &self.bearer {
            req = req.header(header::AUTHORIZATION, format!("Bearer {b}"));
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
            let text = sc.to_str().unwrap();
            let value = text
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
                .unwrap()
                .to_string();
            self.cookie = if value.is_empty() { None } else { Some(value) };
        }
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let body: Value = serde_json::from_slice(&bytes)
            .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body)
    }

    async fn get(&mut self, query: &str) -> Value {
        let (status, body) = self.call("GET", query, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        let (status, body) = self.call("POST", query, Some(form)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn a_bot_logs_in_reads_and_edits() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tsaccept-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let key_file: PathBuf = dir.join("triplespace.key");
    let xml = dir.join("librarybase.xml");
    std::fs::write(&xml, DUMP_XML).unwrap();
    let xd = dir.join("internetdomains.json");
    std::fs::write(&xd, dump_json()).unwrap();
    let pw = dir.join("owner.pw");
    std::fs::write(&pw, "owner-secret-1234\n").unwrap();

    // 1. The instance, the adoption, the mirror sync: what the operator runs.
    instance::run(instance::Create {
        database: database.clone(),
        key_file: key_file.clone(),
        tenant: TENANT.into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: Some(pw),
    })
    .await
    .expect("instance create");
    adopt::run(adopt::Adopt {
        database: database.clone(),
        tenant: TENANT.into(),
        dump: xml,
        source: format!("https://{HOST}/"),
        version: Some("test".into()),
        frozen: true,
        current_only: false,
        counters: Some("item=6,property=12".into()),
        log_floor: 0,
        entity_sources: vec![],
        subsidiary: None,
        batch: 200,
        skip_talk: false,
        no_accounts: false,
    })
    .await
    .expect("adopt");
    sync::run(sync::Sync {
        database: database.clone(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        dump: xd,
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        rejects: None,
    })
    .await
    .expect("sync");

    // 2. A subsidiary with a key.
    let farm = triplespace_cli::common::farm_at(&database).await.unwrap();
    // The tenant's issuer code, derived from its founding record (0079), which its actor
    // keys carry.
    let lb = triplespace_cli::common::issuer_at(&database, TENANT)
        .await
        .unwrap();
    assert!(scatter_actors::issuer::is_derived_code(&lb), "{lb}");
    let alice = format!("{lb}:7");
    let (store, pipeline) = triplespace_cli::common::store(&database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let bot_key = accounts::create_subsidiary(
        &store,
        &pipeline,
        &mut cx,
        TENANT,
        "Alice bot",
        &alice,
        &["bot".into()],
        &alice,
        triplespace_cli::common::now(),
    )
    .await
    .unwrap();
    scatter_projection::Backend::commit(&store, cx)
        .await
        .unwrap();
    assert_eq!(
        bot_key,
        format!("{lb}:10"),
        "after the adopted users 7 and 9"
    );
    let conn = triplespace_cli::common::connect(&database).await.unwrap();
    let issued = keys::issue(
        &conn,
        &bot_key,
        "laptop",
        &["editentity".into(), "highvolume".into()],
        None,
    )
    .await
    .unwrap();
    let (_, secret) = issued.bearer.split_once('.').unwrap();

    // A forwarder key (0057 §10): the hash in `private`, the act in the instance config,
    // and its revocation a null record that retires the registry row.
    let target = || Target {
        database: database.clone(),
    };
    triplespace_cli::forwarder::run(Forwarder::Create(Create {
        target: target(),
        label: "edge".into(),
    }))
    .await
    .unwrap();
    let key_id: String = conn
        .query_one(
            "SELECT key_id FROM private.forwarder_key WHERE label = 'edge'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    let row = conn
        .query_one(
            "SELECT config FROM view.registry WHERE tenant = '' AND kind = 'forwarder' AND code = $1",
            &[&key_id],
        )
        .await
        .unwrap();
    let entry: Value = row.get(0);
    assert_eq!(entry["label"], "edge");
    assert!(entry.get("hash").is_none() && entry.get("secret").is_none());
    triplespace_cli::forwarder::run(Forwarder::Revoke(Revoke {
        target: target(),
        key_id: key_id.clone(),
    }))
    .await
    .unwrap();
    assert!(
        conn.query_opt(
            "SELECT 1 FROM view.registry WHERE kind = 'forwarder' AND code = $1",
            &[&key_id],
        )
        .await
        .unwrap()
        .is_none()
    );

    // 3. The server.
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
    let mut c = Client {
        router: router_with(app, triplespace_api_rest::routes()),
        cookie: None,
        bearer: None,
    };

    // An unregistered host is 421.
    let req = Request::builder()
        .uri("/w/api.php?action=query&meta=siteinfo&format=json")
        .header(header::HOST, "evil.example")
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        c.router.clone().oneshot(req).await.unwrap().status(),
        StatusCode::MISDIRECTED_REQUEST
    );

    // siteinfo, as a client's first call.
    let si = c
        .get("action=query&meta=siteinfo&siprop=general|namespaces|triplespace&format=json")
        .await;
    assert_eq!(
        si["query"]["general"]["wikibase-conceptbaseuri"],
        json!(format!("https://{HOST}/entity/"))
    );
    assert_eq!(
        si["query"]["namespaces"]["120"]["defaultcontentmodel"],
        "wikibase-item"
    );
    assert_eq!(si["query"]["namespaces"]["210"]["name"], "Domain");
    assert!(si["query"]["triplespace"].get("insecure").is_none());

    // Anonymous read of both entities.
    let r = c
        .get("action=wbgetentities&ids=Q6|Domain:wikipedia.org&format=json")
        .await;
    assert_eq!(r["success"], 1);
    let q6 = &r["entities"]["Q6"];
    assert_eq!(q6["labels"]["en"]["value"], "Six & more");
    assert_eq!(q6["title"], "Item:Q6");
    assert_eq!(q6["pageid"], 12);
    let first_revid = q6["lastrevid"].as_u64().unwrap();
    assert_eq!(
        first_revid, 41_877,
        "the source's own revision ID (0035 A12)"
    );
    let wp = &r["entities"]["domain:wikipedia.org"];
    assert_eq!(wp["type"], "domain");
    assert_eq!(wp["title"], "Domain:wikipedia.org");
    assert_eq!(wp["labels"]["en"]["value"], "Wikipedia");
    assert_eq!(
        wp["labels"]["mul"]["value"], "wikipedia.org",
        "the derived label (0009 §5)"
    );
    assert_eq!(
        wp["claims"]["XDP8"][0]["mainsnak"]["datavalue"]["value"]["id"], "domain:example.org",
        "a value pointing at a mapped item is a domain value (0009 §9)"
    );
    assert!(
        wp["claims"].get("XDP1").is_none(),
        "the identity property is dropped"
    );
    let empty = c
        .get("action=wbgetentities&ids=domain:nothing-here.example&format=json")
        .await;
    assert_eq!(
        empty["entities"]["domain:nothing-here.example"]["labels"]["mul"]["value"],
        "nothing-here.example"
    );
    assert!(
        empty["entities"]["domain:nothing-here.example"]
            .get("missing")
            .is_none(),
        "a Domain is never missing (0009 §4)"
    );
    let missing = c.get("action=wbgetentities&ids=Q999&format=json").await;
    assert_eq!(missing["entities"]["Q999"]["missing"], "");

    // An anonymous write is refused.
    let (_, denied) = c
        .call(
            "POST",
            "action=wbsetlabel&format=json",
            Some(&format!("id=Q6&language=fr&value=Six&token={}", enc(r"+\"))),
        )
        .await;
    assert_eq!(denied["error"]["code"], "permissiondenied", "{denied}");

    // 4. Bot-password login, as WikibaseIntegrator does it.
    let t = c
        .get("action=query&meta=tokens&type=login&format=json")
        .await;
    let login_token = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(c.cookie.is_some(), "an anonymous session for the token");
    let bad = c
        .post(
            "action=login&format=json",
            &format!(
                "lgname={}&lgpassword=wrong&lgtoken={}",
                enc("Alice bot@laptop"),
                enc(&login_token)
            ),
        )
        .await;
    assert_eq!(bad["login"]["result"], "Failed");
    let primary = c
        .post(
            "action=login&format=json",
            &format!("lgname=Alice&lgpassword=x&lgtoken={}", enc(&login_token)),
        )
        .await;
    assert_eq!(primary["login"]["code"], "ts-use-oauth", "{primary}");
    let ok = c
        .post(
            "action=login&format=json",
            &format!(
                "lgname={}&lgpassword={}&lgtoken={}",
                enc("Alice bot@laptop"),
                enc(secret),
                enc(&login_token)
            ),
        )
        .await;
    assert_eq!(ok["login"]["result"], "Success", "{ok}");
    assert_eq!(ok["login"]["lgusername"], "Alice bot");
    let ui = c
        .get("action=query&meta=userinfo&uiprop=groups|rights&format=json")
        .await;
    assert_eq!(ui["query"]["userinfo"]["name"], "Alice bot");
    assert!(
        ui["query"]["userinfo"]["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("bot"))
    );
    assert!(
        ui["query"]["userinfo"]["rights"]
            .as_array()
            .unwrap()
            .contains(&json!("edit"))
    );
    assert_eq!(ui["query"]["userinfo"]["operator"], alice.as_str());
    let csrf = c.get("action=query&meta=tokens&format=json").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(csrf.ends_with(r"+\"));

    // 5. Edits: a label on Q6 through wbeditentity, a statement on the Domain.
    let (_, wrong) = c
        .call(
            "POST",
            "action=wbeditentity&format=json",
            Some(&format!(
                "id=Q6&data={}&token={}",
                enc(r#"{"labels":{"fr":{"language":"fr","value":"Six"}}}"#),
                enc(r"+\")
            )),
        )
        .await;
    assert_eq!(wrong["error"]["code"], "badtoken");
    let edited = c
        .post(
            "action=wbeditentity&format=json",
            &format!(
                "id=Q6&data={}&summary=fr+label&token={}",
                enc(r#"{"labels":{"fr":{"language":"fr","value":"Six"}}}"#),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(edited["success"], 1, "{edited}");
    assert_eq!(edited["entity"]["labels"]["fr"]["value"], "Six");
    assert_eq!(
        edited["entity"]["labels"]["en"]["value"], "Six & more",
        "untouched terms stay"
    );
    let second_revid = edited["entity"]["lastrevid"].as_u64().unwrap();
    assert!(second_revid > first_revid);

    let created = c
        .post(
            "action=wbcreateclaim&format=json",
            &format!(
                "entity={}&property=P12&snaktype=value&value={}&token={}",
                enc("domain:wikipedia.org"),
                enc("\"an assertion of ours\""),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(created["success"], 1, "{created}");
    let guid = created["claim"]["id"].as_str().unwrap().to_string();
    assert!(guid.starts_with("domain:wikipedia.org$"), "{guid}");
    let r = c
        .get("action=wbgetentities&ids=domain:wikipedia.org&format=json")
        .await;
    let wp = &r["entities"]["domain:wikipedia.org"];
    assert_eq!(
        wp["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"],
        "an assertion of ours"
    );
    assert_eq!(
        wp["claims"]["XDP8"][0]["mainsnak"]["datavalue"]["value"]["id"], "domain:example.org",
        "the mirrored statement is still there"
    );
    assert_eq!(wp["labels"]["en"]["value"], "Wikipedia");

    // A stale base revision is an edit conflict (0006 §8).
    let (_, conflict) = c
        .call(
            "POST",
            "action=wbsetdescription&format=json",
            Some(&format!(
                "id=Q6&language=fr&value=nombre&baserevid={first_revid}&token={}",
                enc(&csrf)
            )),
        )
        .await;
    assert_eq!(conflict["error"]["code"], "editconflict", "{conflict}");
    let fine = c
        .post(
            "action=wbsetdescription&format=json",
            &format!(
                "id=Q6&language=fr&value=nombre&baserevid={second_revid}&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(fine["success"], 1, "{fine}");
    let third_revid = fine["entity"]["lastrevid"].as_u64().unwrap();

    // Changing and removing a statement: remove + add under the same GUID, then remove.
    let changed = c.post("action=wbsetclaim&format=json", &format!("claim={}&token={}", enc(r#"{"id":"Q6$11111111-1111-1111-1111-111111111111","mainsnak":{"snaktype":"value","property":"P12","datavalue":{"value":"SIX","type":"string"}},"type":"statement","rank":"preferred"}"#), enc(&csrf))).await;
    assert_eq!(changed["success"], 1, "{changed}");
    assert_eq!(changed["claim"]["rank"], "preferred");
    assert_eq!(changed["claim"]["mainsnak"]["datavalue"]["value"], "SIX");
    let r = c.get("action=wbgetentities&ids=Q6&format=json").await;
    assert_eq!(
        r["entities"]["Q6"]["claims"]["P12"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "replaced, not duplicated: {}",
        r["entities"]["Q6"]["claims"]
    );
    let removed = c
        .post(
            "action=wbremoveclaims&format=json",
            &format!(
                "claim={}&token={}",
                enc("Q6$11111111-1111-1111-1111-111111111111"),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(removed["success"], 1, "{removed}");
    let r = c.get("action=wbgetentities&ids=Q6&format=json").await;
    assert!(
        r["entities"]["Q6"]["claims"].get("P12").is_none(),
        "{}",
        r["entities"]["Q6"]
    );
    assert!(r["entities"]["Q6"]["lastrevid"].as_u64().unwrap() > third_revid);

    // A new item.
    let new = c.post("action=wbeditentity&format=json", &format!("new=item&data={}&token={}", enc(r#"{"labels":{"en":{"language":"en","value":"Seven"}},"claims":{"P12":[{"mainsnak":{"snaktype":"value","property":"P12","datavalue":{"value":"seven","type":"string"}},"type":"statement","rank":"normal"}]}}"#), enc(&csrf))).await;
    assert_eq!(new["success"], 1, "{new}");
    assert_eq!(new["entity"]["id"], "Q7", "above the item floor");
    assert!(
        new["entity"]["claims"]["P12"][0]["id"]
            .as_str()
            .unwrap()
            .starts_with("Q7$")
    );
    let s = c
        .get("action=wbsearchentities&search=sev&language=en&format=json")
        .await;
    assert_eq!(s["search"][0]["id"], "Q7", "{s}");
    assert_eq!(s["search"][0]["label"], "Seven");
    // A new property needs `property-create`, which every registered account holds by
    // default, as in a stock Wikibase (0016 A32).
    let property = c
        .post(
            "action=wbeditentity&format=json",
            &format!(
                "new=property&data={}&token={}",
                enc(
                    r#"{"labels":{"en":{"language":"en","value":"ISBN"}},"datatype":"external-id"}"#
                ),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(property["success"], 1, "{property}");
    assert_eq!(property["entity"]["type"], "property");
    assert_eq!(property["entity"]["datatype"], "external-id");

    // 6. The same key as a bearer credential, stateless.
    let mut b = Client {
        router: c.router.clone(),
        cookie: None,
        bearer: Some(issued.bearer.clone()),
    };
    let ui = b
        .get("action=query&meta=userinfo&uiprop=groups&format=json")
        .await;
    assert_eq!(ui["query"]["userinfo"]["name"], "Alice bot");
    assert!(b.cookie.is_none(), "no session for a bearer request");
    let btoken =
        b.get("action=query&meta=tokens&format=json").await["query"]["tokens"]["csrftoken"]
            .as_str()
            .unwrap()
            .to_string();
    let aliased = b
        .post(
            "action=wbsetaliases&format=json",
            &format!("id=Q6&language=en&add=VI&token={}", enc(&btoken)),
        )
        .await;
    assert_eq!(aliased["success"], 1, "{aliased}");
    assert_eq!(aliased["entity"]["aliases"]["en"][0]["value"], "VI");

    // 7. The data document and the concept URI.
    let req = Request::builder()
        .uri("/wiki/Special:EntityData/Q6.json")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let resp = c.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["entities"]["Q6"]["labels"]["fr"]["value"], "Six");
    let req = Request::builder()
        .uri("/entity/Q6")
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let resp = c.router.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        resp.headers()[header::LOCATION],
        "/wiki/Special:EntityData/Q6.json"
    );

    // 7b. The feeds (0020 §1–2, §5): the bot's edits are recent changes and its
    // contributions, the adoption and the mirror sync are jobs, and the REST feed
    // carries the same rows in the shape of 16 §3.1.
    let rc = c
        .get("action=query&list=recentchanges&rcprop=title|ids|user|comment|flags|tags|timestamp|sizes&rclimit=50&format=json&formatversion=2")
        .await;
    let rows = rc["query"]["recentchanges"].as_array().unwrap();
    assert!(rows.len() >= 6, "{rc}");
    let alias_edit = rows
        .iter()
        .find(|r| {
            r["comment"]
                .as_str()
                .is_some_and(|c| c.contains("aliases:en"))
        })
        .unwrap_or_else(|| panic!("the alias edit is in recent changes: {rc}"));
    assert_eq!(alias_edit["title"], "Item:Q6");
    assert_eq!(alias_edit["ns"], 120);
    assert_eq!(alias_edit["user"], "Alice bot");
    assert_eq!(alias_edit["type"], "edit");
    assert_eq!(alias_edit["bot"], false, "no bot=1 was sent");
    assert!(alias_edit["revid"].as_u64().unwrap() > 0);
    assert!(alias_edit["timestamp"].as_str().unwrap().ends_with('Z'));
    assert!(
        rows.iter().all(|r| r["type"] != "sync"),
        "mirror syncs are not recent changes"
    );
    let jobs: Vec<&Value> = rows.iter().filter(|r| r["type"] == "log").collect();
    assert!(!jobs.is_empty(), "each job run is a log row: {rc}");
    let newest = rows[0]["timestamp"].as_str().unwrap();
    let oldest = rows[rows.len() - 1]["timestamp"].as_str().unwrap();
    assert!(newest >= oldest, "newest first");

    // Paging through `continue`, in MediaWiki's protocol.
    let page = c
        .get("action=query&list=recentchanges&rclimit=2&format=json&formatversion=2")
        .await;
    assert_eq!(page["query"]["recentchanges"].as_array().unwrap().len(), 2);
    let token = page["continue"]["rccontinue"].as_str().unwrap().to_string();
    assert_eq!(page["continue"]["continue"], "-||");
    let page2 = c
        .get(&format!(
            "action=query&list=recentchanges&rclimit=2&rccontinue={}&format=json&formatversion=2",
            enc(&token)
        ))
        .await;
    assert_ne!(
        page2["query"]["recentchanges"][0]["rcid"],
        page["query"]["recentchanges"][0]["rcid"]
    );

    // Filters: only new rows (the adopted entities), and a window that excludes everything.
    let new = c
        .get("action=query&list=recentchanges&rcshow=new&rcprop=title|flags&rclimit=50&format=json&formatversion=2")
        .await;
    assert!(
        new["query"]["recentchanges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["new"] == true),
        "{new}"
    );
    let none = c
        .get("action=query&list=recentchanges&rcend=2000-01-01T00:00:00Z&rcstart=2000-01-02T00:00:00Z&format=json&formatversion=2")
        .await;
    assert_eq!(none["query"]["recentchanges"], json!([]));
    let (status, err) = c
        .call(
            "GET",
            "action=query&list=recentchanges&rcshow=bot|!bot&format=json",
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(err["error"]["code"], "show", "{err}");

    // Contributions.
    let uc = c
        .get(&format!(
            "action=query&list=usercontribs&ucuser={}&uclimit=50&format=json&formatversion=2",
            enc("Alice bot")
        ))
        .await;
    let contribs = uc["query"]["usercontribs"].as_array().unwrap();
    assert!(contribs.len() >= 5, "{uc}");
    assert!(
        contribs
            .iter()
            .all(|r| r["user"] == "Alice bot" && r["userid"] == 10)
    );
    assert!(contribs.iter().any(|r| r["title"] == "Item:Q6"));
    let nobody = c
        .get("action=query&list=usercontribs&ucuser=Nobody&format=json&formatversion=2")
        .await;
    assert_eq!(nobody["query"]["usercontribs"], json!([]));

    // The log.
    let le = c
        .get("action=query&list=logevents&letype=job&lelimit=50&format=json&formatversion=2")
        .await;
    let events = le["query"]["logevents"].as_array().unwrap();
    assert!(events.iter().any(|e| e["action"] == "finish"), "{le}");
    assert!(events.iter().all(|e| e["type"] == "job"));

    // The REST feed: the same rows, Triplespace's shape, and a target's history.
    let (status, h, feed) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/activity?limit=100",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{feed}");
    assert!(h["cache-control"].to_str().unwrap().starts_with("public"));
    assert_eq!(h["cache-tag"], "activity");
    let feed_rows = feed["rows"].as_array().unwrap();
    assert_eq!(feed_rows.len(), rows.len(), "{feed}");
    let r = feed_rows
        .iter()
        .find(|r| {
            r["summary"]["args"]
                .as_str()
                .is_some_and(|a| a.contains("aliases:en"))
        })
        .unwrap();
    assert_eq!(r["kind"], "edit");
    assert_eq!(r["source"], "local");
    assert_eq!(r["target"]["title"], "Item:Q6");
    assert_eq!(
        r["target"]["label"]["value"], "Six & more",
        "the current label"
    );
    assert_eq!(r["actor"]["name"], "Alice bot");
    assert_eq!(r["actor"]["kind"], "bot");
    assert_eq!(r["visibility"], json!([]));
    assert!(r["links"]["diff"].as_str().unwrap().contains("diff="));
    let (status, _, history) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/activity?target=entity:domain:wikipedia.org&dir=newer",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    let hist = history["rows"].as_array().unwrap();
    assert!(
        hist.iter().any(|r| r["kind"] == "sync") && hist.iter().any(|r| r["kind"] == "edit"),
        "a mirrored entity's history has its sync and the local edit: {history}"
    );
    assert!(
        hist.iter()
            .all(|r| r["target"]["id"] == "domain:wikipedia.org")
    );
    // The history route: the same rows under the entity's own path, the source switch,
    // and the answers for an ID that is not there.
    let (status, h, history) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/domain:wikipedia.org/history?dir=newer",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{history}");
    assert_eq!(h["cache-tag"], "entity:domain:wikipedia.org");
    assert_eq!(
        history["set"],
        json!({"kind": "entity", "id": "domain:wikipedia.org"})
    );
    assert_eq!(history["rows"], json!(hist));
    let (_, _, local_only) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/Domain:wikipedia.org/history?source=local",
        &[],
    )
    .await;
    let rows = local_only["rows"].as_array().unwrap();
    assert!(
        !rows.is_empty() && rows.iter().all(|r| r["source"] == "local"),
        "{local_only}"
    );
    assert_eq!(local_only["sources"], json!(["local"]));
    let (_, _, q6) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/Q6/history",
        &[],
    )
    .await;
    assert!(
        q6["rows"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["target"]["id"] == "Q6"),
        "{q6}"
    );
    assert!(
        q6["rows"].as_array().unwrap().len() >= 5,
        "the bot's edits and the adoption"
    );
    let (status, _, none) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/Q404/history",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(none["code"], "entity-not-found");
    let (status, _, bad_id) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/nope!/history",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bad_id["code"], "invalid-entity-id");
    // `prop=revisions` (18 §2.3): Q6's local revisions, newest first, and the content at
    // an old one is the state as it then was (the fold of 0083 §4's stepping stone).
    let rv = c
        .get("action=query&prop=revisions&titles=Item:Q6&rvprop=ids|timestamp|user|comment|size|tags&rvlimit=50&format=json&formatversion=2")
        .await;
    let page = &rv["query"]["pages"][0];
    assert_eq!(page["title"], "Item:Q6", "{rv}");
    let revs = page["revisions"].as_array().unwrap();
    assert!(revs.len() >= 5, "{rv}");
    assert!(
        revs.windows(2)
            .all(|w| w[0]["revid"].as_u64() > w[1]["revid"].as_u64()),
        "newest first"
    );
    assert!(revs.iter().any(|r| r["user"] == "Alice bot"));
    let oldest = revs.last().unwrap();
    // An adopted revision shows the source's own summary, with no `adopt` before it
    // (0035 A12).
    assert_eq!(
        oldest["comment"], "/* wbeditentity-create-item:0| */ Six",
        "{oldest}"
    );
    let old = c
        .get(&format!(
            "action=query&prop=revisions&revids={}&rvprop=ids|content&rvslots=main&format=json&formatversion=2",
            oldest["revid"]
        ))
        .await;
    let slot = &old["query"]["pages"][0]["revisions"][0]["slots"]["main"];
    assert_eq!(slot["contentmodel"], "wikibase-item", "{old}");
    let then: Value = serde_json::from_str(slot["content"].as_str().unwrap()).unwrap();
    assert_eq!(then["id"], "Q6");
    assert!(
        then["labels"].get("fr").is_none(),
        "the French label came later: {then}"
    );
    assert_eq!(then["lastrevid"], oldest["revid"]);
    let now = c
        .get("action=query&prop=revisions&titles=Item:Q6&rvprop=ids|content&rvslots=main&rvlimit=1&format=json&formatversion=2")
        .await;
    let latest: Value = serde_json::from_str(
        now["query"]["pages"][0]["revisions"][0]["slots"]["main"]["content"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(latest["labels"]["fr"]["value"], "Six", "{latest}");
    assert_eq!(latest["aliases"]["en"][0]["value"], "VI");
    let rv_page_a = c
        .get("action=query&prop=revisions&titles=Item:Q6&rvlimit=2&format=json&formatversion=2")
        .await;
    assert_eq!(
        rv_page_a["query"]["pages"][0]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let token = rv_page_a["continue"]["rvcontinue"]
        .as_str()
        .unwrap()
        .to_string();
    let rv_page_b = c
        .get(&format!("action=query&prop=revisions&titles=Item:Q6&rvlimit=2&rvcontinue={}&format=json&formatversion=2", enc(&token)))
        .await;
    assert_ne!(
        rv_page_b["query"]["pages"][0]["revisions"][0]["revid"],
        rv_page_a["query"]["pages"][0]["revisions"][0]["revid"]
    );
    let gone = c
        .get("action=query&prop=revisions&titles=Item:Q404&format=json&formatversion=2")
        .await;
    assert_eq!(gone["query"]["pages"][0]["missing"], true, "{gone}");

    // One change's structured diff (18 §5.1): the alias edit added `VI` in English, and
    // nothing else; a comparison across the bot's edits; the mirror's `put`.
    let alias_rev = q6["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| {
            r["summary"]["args"]
                .as_str()
                .is_some_and(|a| a.contains("aliases:en"))
        })
        .unwrap();
    let (p, o) = (
        alias_rev["record"]["partition"].as_u64().unwrap(),
        alias_rev["record"]["offset"].as_u64().unwrap(),
    );
    let (status, h, change) = send(
        &c.router,
        &format!("/w/rest.php/triplespace/v0/entity/Q6/changes/{p}/{o}"),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{change}");
    assert_eq!(h["cache-tag"], "entity:Q6");
    assert_eq!(change["source"], "local");
    assert_eq!(change["revision"]["revid"], alias_rev["revid"]);
    assert_eq!(change["revision"]["actor"], format!("{lb}:10"));
    assert_eq!(change["operation"]["op"], "add");
    assert_eq!(change["diff"]["aliases"][0]["language"], "en");
    assert_eq!(change["diff"]["aliases"][0]["added"], json!(["VI"]));
    assert_eq!(change["diff"]["labels"], json!([]));
    assert_eq!(change["diff"]["claims"], json!([]));
    assert!(change["previous"]["revid"].as_u64().unwrap() < alias_rev["revid"].as_u64().unwrap());
    assert!(change["effect"].is_null());
    let first_rev = q6["rows"].as_array().unwrap().last().unwrap()["revid"]
        .as_u64()
        .unwrap();
    let (status, _, cmp) = send(
        &c.router,
        &format!(
            "/w/rest.php/triplespace/v0/entity/Q6/compare?from={first_rev}&to={}",
            alias_rev["revid"]
        ),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cmp}");
    assert_eq!(cmp["from"]["op"], "adopt");
    assert!(
        cmp["diff"]["labels"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["language"] == "fr"),
        "the French label the bot set: {cmp}"
    );
    assert!(
        cmp["diff"]["claims"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["property"] == "P12"),
        "the claim it changed: {cmp}"
    );
    let (status, _, err) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/Q6/compare?source=mirror/internetdomains&from=1&to=2",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(err["code"], "ts-compare-source");
    let (status, _, err) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/entity/Q6/compare?from=1&to=999999",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(err["code"], "revision-not-found");
    let sync = hist.iter().find(|r| r["kind"] == "sync").unwrap();
    let (status, _, put) = send(
        &c.router,
        &format!(
            "/w/rest.php/triplespace/v0/entity/domain:wikipedia.org/changes/{}/{}",
            sync["record"]["partition"], sync["record"]["offset"]
        ),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{put}");
    assert_eq!(put["source"], "mirror/internetdomains");
    assert_eq!(put["revision"]["op"], "put");
    assert!(put["diff"].is_null());
    assert_eq!(put["after"]["id"], "domain:wikipedia.org");
    let (status, _, err) = send(
        &c.router,
        &format!("/w/rest.php/triplespace/v0/entity/Q7/changes/{p}/{o}"),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "another entity's record");
    assert_eq!(err["code"], "change-not-found");
    let (status, _, bad) = send(
        &c.router,
        "/w/rest.php/triplespace/v0/activity?target=nope",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(bad["code"], "bad-parameter");

    // 8. Logout ends the session.
    let out = c
        .post(
            "action=logout&format=json",
            &format!("token={}", enc(&csrf)),
        )
        .await;
    assert!(out.get("logout").is_some(), "{out}");
    let ui = c.get("action=query&meta=userinfo&format=json").await;
    assert_eq!(ui["query"]["userinfo"]["anon"], "");

    // 9. The owner's password works for clientlogin.
    let t = c
        .get("action=query&meta=tokens&type=login&format=json")
        .await;
    let login_token = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c.post("action=clientlogin&format=json", &format!("username=Alice&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/", enc(&login_token))).await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let ui = c
        .get("action=query&meta=userinfo&uiprop=groups&format=json")
        .await;
    assert!(
        ui["query"]["userinfo"]["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("owner")),
        "{ui}"
    );

    // What the site reads in its first phase (0057 §5–6, §9; 0012 §5).
    what_the_site_reads(&c.router, &database).await;

    // 10. Everything the API wrote replays from the log.
    let status = triplespace_cli::status::Target {
        database: database.clone(),
    };
    triplespace_cli::status::rebuild(status)
        .await
        .expect("rebuild");
    let r = c
        .get("action=wbgetentities&ids=Q6|Q7|domain:wikipedia.org&format=json")
        .await;
    assert_eq!(r["entities"]["Q6"]["aliases"]["en"][0]["value"], "VI");
    assert_eq!(r["entities"]["Q7"]["labels"]["en"]["value"], "Seven");
    assert_eq!(
        r["entities"]["domain:wikipedia.org"]["claims"]["P12"][0]["mainsnak"]["datavalue"]["value"],
        "an assertion of ours"
    );
}

/// Sends a request with no credentials, or with the headers given, and returns the
/// status, the headers and the body.
async fn send(
    router: &Router,
    uri: &str,
    extra: &[(&str, String)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut req = Request::builder().uri(uri).header(header::HOST, HOST);
    for (k, v) in extra {
        req = req.header(*k, v.as_str());
    }
    let response = router
        .clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

#[allow(clippy::too_many_lines)]
async fn what_the_site_reads(router: &Router, database: &str) {
    // siteinfo: the API level, and the shipped theme until one is set (0086 §5).
    let siteinfo = "/w/api.php?action=query&meta=siteinfo&siprop=triplespace&format=json";
    let (status, h, body) = send(router, siteinfo, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["query"]["triplespace"]["api_version"],
        json!(triplespace_api_action::API_VERSION)
    );
    let theme = &body["query"]["triplespace"]["theme"];
    assert_eq!(theme["palette"], "parchment");
    assert_eq!(theme["typeset"], "default26");
    assert_eq!(theme["choice"], "any");
    // The catalogue's order is the registry's (`encyclopedia` first since its palette
    // arrived); find each palette by name.
    let palette = |name: &str| {
        theme["palettes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("{name} is offered"))
            .clone()
    };
    assert_eq!(
        palette("parchment")["light"]["color-progressive"],
        "#2B559E"
    );
    // The colour-vision palettes are parchment's alternates (0094), and their old names
    // are no palettes.
    assert_eq!(
        palette("parchment")["alternate"]["rg"]["light"]["color-error"],
        "#A84300"
    );
    assert!(
        !theme["palettes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "parchment-by")
    );
    assert!(palette("encyclopedia")["light"].is_object());
    let typeset = |name: &str| {
        theme["typesets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is offered"))
            .clone()
    };
    assert_eq!(typeset("hyperlegible")["accessibility"], true);
    assert_ne!(typeset("basic")["accessibility"], true);
    assert!(
        h["cache-control"]
            .to_str()
            .unwrap()
            .starts_with("public, max-age=60"),
        "siteinfo alone is stable: {h:?}"
    );

    // The wiki's palette, set in its config, is what siteinfo reports.
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let config = scatter_ingest::store::IngestStore::partition(&store, &mut cx, TENANT, "config")
        .await
        .unwrap()
        .expect("the tenant has a config partition");
    let d = triplespace_cli::common::draft(
        scatter_log::registry::PAYLOAD_CONFIG,
        "site:theme.palette",
        &json!({"kind": "site", "name": "theme.palette", "value": "encyclopedia"}),
        &format!(
            "{}:7",
            triplespace_cli::common::issuer_at(database, TENANT)
                .await
                .unwrap()
        ),
        None,
        triplespace_cli::common::now(),
    )
    .unwrap();
    scatter_ingest::write::append_projected(
        &store,
        &pipeline,
        &mut cx,
        config,
        d,
        scatter_projection::Budget::NONE,
    )
    .await
    .unwrap();
    scatter_projection::Backend::commit(&store, cx)
        .await
        .unwrap();
    let (_, _, body) = send(router, siteinfo, &[]).await;
    assert_eq!(
        body["query"]["triplespace"]["theme"]["palette"],
        "encyclopedia"
    );

    // An anonymous entity read is public, tagged and revalidates to 304.
    let get = "/w/api.php?action=wbgetentities&ids=Q6|domain:wikipedia.org&format=json";
    let (status, h, _) = send(router, get, &[]).await;
    assert_eq!(status, StatusCode::OK);
    let cc = h["cache-control"].to_str().unwrap();
    assert!(cc.starts_with("public, max-age=0, s-maxage=60"), "{cc}");
    assert_eq!(h["cache-tag"], "entity:Q6, entity:domain:wikipedia.org");
    let etag = h["etag"].to_str().unwrap().to_string();
    let (status, h2, _) = send(router, get, &[("if-none-match", etag.clone())]).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);
    assert_eq!(h2["etag"].to_str().unwrap(), etag);

    // With credentials it is private, and carries no tags.
    let (_, h, _) = send(
        router,
        get,
        &[("authorization", "Bearer nonsense.key".into())],
    )
    .await;
    assert_eq!(h["cache-control"], "private, no-cache");
    assert!(h.get("cache-tag").is_none());

    // The data document caches the same way.
    let doc = "/wiki/Special:EntityData/Q6.json";
    let (status, h, _) = send(router, doc, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(h["cache-control"].to_str().unwrap().contains("s-maxage=60"));
    let etag = h["etag"].to_str().unwrap().to_string();
    let (status, _, _) = send(router, doc, &[("if-none-match", etag)]).await;
    assert_eq!(status, StatusCode::NOT_MODIFIED);

    // Provenance: a local item adopted from Librarybase.
    let (status, h, p) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/Q6/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["id"], "Q6");
    assert_eq!(p["type"], "item");
    assert_eq!(p["minted_by"]["kind"], "tenant");
    assert_eq!(p["minted_by"]["adopted_from"], "https://librarybase.org/");
    assert_eq!(p["graphs"][0]["graph"], "local");
    assert_eq!(p["graphs"][0]["history"], "full");
    // The test removed Q6's one statement earlier, so no graph dominates.
    assert_eq!(p["graphs"][0]["statements"], 0);
    assert!(p.get("dominant").is_none());
    assert_eq!(h["cache-tag"], "entity:Q6");

    // A Domain: the mirror's record and the tenant's assertion, side by side.
    let (status, _, p) = send(
        router,
        "/rest.php/triplespace/v0/entity/domain:wikipedia.org/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{p}");
    assert_eq!(p["minted_by"], json!({"kind": "keyed", "type": "domain"}));
    let graphs: Vec<&str> = p["graphs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["graph"].as_str().unwrap())
        .collect();
    assert_eq!(graphs, ["local", "mirror/internetdomains"]);
    let mirror = &p["graphs"][1];
    assert!(mirror.get("upstream_version").is_some(), "{mirror}");
    // The upstream is a MediaWiki that publishes revision IDs: its revision has a page.
    assert_eq!(
        mirror["upstream_url"],
        format!(
            "https://internetdomains.wiki/w/index.php?oldid={}",
            mirror["upstream_version"].as_str().unwrap()
        ),
        "{mirror}"
    );
    assert_eq!(mirror["history"], "latest");
    assert!(mirror.get("job").is_some(), "the sync job: {mirror}");
    // One statement each; a tie goes to the earlier graph, local first.
    assert_eq!(p["dominant"], "local");
    assert!(
        p["statements"]
            .as_object()
            .unwrap()
            .values()
            .any(|s| s["graphs"] == json!(["local"])),
        "the tenant's own statement on the Domain: {p}"
    );

    // Missing and malformed IDs.
    let (status, _, e) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/Q999999/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(e["code"], "entity-not-found");
    let (status, _, e) = send(
        router,
        "/w/rest.php/triplespace/v0/entity/not-an-id/provenance",
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(e["code"], "invalid-entity-id");
}
