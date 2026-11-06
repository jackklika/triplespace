//! The site both ways (0057 §1.4, §2): the web tier over HTTP to the API on a real TCP
//! listener, and the server's embedded site in process, serve byte-identical pages; and
//! the embedded site never answers for the API's paths. Needs
//! `TRIPLESPACE_TEST_DATABASE_URL`; vacuous without it.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use http_body_util::BodyExt as _;
use tokio_postgres::{Config, NoTls};
use tower::ServiceExt as _;
use triplespace_accounts::Secret;
use triplespace_api_action::{App, Config as ServerConfig, Mode, router_with};
use triplespace_cli::{adopt, instance, sync};
use triplespace_client::{Client, HttpTransport, ServiceTransport};

const DB: &str = "tssite_test";
const HOST: &str = "librarybase.org";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

async fn fresh_database() -> String {
    let (client, connection) = admin_config()
        .unwrap()
        .connect(NoTls)
        .await
        .expect("connect");
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

/// What the browser gets.
struct Got {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn get(router: &Router, path: &str, cookie: Option<&str>) -> Got {
    let browser: SocketAddr = "198.51.100.7:50000".parse().unwrap();
    let mut req = Request::get(path).header(header::HOST, HOST);
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, c);
    }
    let mut req = req.body(Body::empty()).unwrap();
    req.extensions_mut().insert(ConnectInfo(browser));
    let r = router.clone().oneshot(req).await.unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    Got {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn the_web_tier_and_the_embedded_site_serve_the_same_pages() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tssite-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    instance::run(instance::Create {
        database: database.clone(),
        key_file: dir.join("triplespace.key"),
        tenant: "librarybase".into(),
        base: format!("https://{HOST}"),
        name: None,
        farm_slug: "testfarm".into(),
        tenancy: "isolated".into(),
        farm_base: None,
        providers: vec!["internetdomains".into()],
        adopt: Some(format!("https://{HOST}/")),
        owner: Some(7),
        owner_name: "Alice".into(),
        owner_password_file: None,
    })
    .await
    .expect("instance create");
    // The entities of the site's fixtures: every data type, ranks, qualifiers and a
    // reference, terms in three languages, and a Domain from the mirror.
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    // The fixture's revisions are dated a day ago, so that they stay inside recent
    // changes' 30-day window however long after the fixture was written the test runs.
    let dated = std::env::temp_dir().join(format!("site-entities-{}.xml", std::process::id()));
    let yesterday = triplespace_api_action::entity_json::iso8601(
        u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_micros(),
        )
        .unwrap()
            - 86_400_000_000,
    );
    std::fs::write(
        &dated,
        std::fs::read_to_string(fixtures.join("site-entities.xml"))
            .unwrap()
            .replace("2026-09-20T14:02:11Z", &yesterday),
    )
    .unwrap();
    adopt::run(adopt::Adopt {
        database: database.clone(),
        tenant: "librarybase".into(),
        dump: dated.clone(),
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
    let _ = std::fs::remove_file(&dated);
    sync::run(sync::Sync {
        database: database.clone(),
        provider: Some("internetdomains".into()),
        tenant: None,
        source: None,
        dump: fixtures.join("internetdomains.json"),
        version: Some("20261001".into()),
        snapshot: false,
        threshold: 1000,
        batch: 200,
        rejects: None,
        no_bootstrap: false,
    })
    .await
    .expect("sync");
    // The load wrote no activity rows of its own (03 §4.6; 0013 §5.5); the follower then brings one
    // upstream edit of wikipedia.org (0092 §5), whose record is a `sync` row.
    follow_one_edit(&database, &fixtures.join("internetdomains.json")).await;

    // The API, trusting the web tier on loopback by address (0057 §10).
    let app = App::new(
        triplespace_db::pool::pool(&database, 4).unwrap(),
        Secret::derive(&[7u8; 32]),
        ServerConfig {
            mode: Mode::Production,
            trusted_proxies: vec!["127.0.0.1".into()],
            dev_tenant: None,
            farm: triplespace_cli::common::farm_at(&database).await.unwrap(),
            auth_base: None,
            generator: "Triplespace test".into(),
            version: None,
        },
    )
    .unwrap();
    let api = router_with(app, triplespace_api_rest::routes());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let served = api.clone();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            served.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    // The web tier, over HTTP; the server, with the site embedded.
    let web = triplespace_ui::router(Client::new(
        HttpTransport::new(&format!("http://127.0.0.1:{port}"), None)
            .await
            .unwrap(),
    ));
    let embedded = api
        .clone()
        .fallback_service(triplespace_ui::router(Client::new(ServiceTransport::new(
            api.clone(),
        ))));

    for path in [
        "/wiki/Project:Home",
        "/wiki/Special:SpecialPages",
        "/wiki/Project:About",
        "/wiki/Item:Q6",
        "/wiki/Item:Q6?tab=identifiers",
        "/wiki/Item:Q6?tab=sitelinks",
        "/wiki/Item:Q6?tab=labels",
        "/wiki/Item:Q6?uselang=ar",
        "/wiki/Item:Q6?action=render&region=statements/P3",
        "/wiki/Property:P3",
        "/wiki/Domain:wikipedia.org",
        "/wiki/Item:Q404",
        "/w/index.php?title=Item:Q6&action=history",
        "/w/index.php?title=Special:Search&search=six&fulltext=1",
        "/wiki/Special:RecentChanges",
        "/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&days=90",
        "/wiki/Special:Contributions/Alice",
        "/wiki/Domain:wikipedia.org?action=history&source=local",
        "/nothing/here",
    ] {
        let a = get(&web, path, None).await;
        let b = get(&embedded, path, None).await;
        assert!(
            a.status == StatusCode::OK || a.status == StatusCode::NOT_FOUND,
            "{path}: {} {}",
            a.status,
            a.body
        );
        assert_eq!(a.status, b.status, "{path}");
        assert_eq!(a.body, b.body, "{path}: the HTML differs");
        for h in [header::ETAG, header::CACHE_CONTROL, header::VARY] {
            assert_eq!(a.headers.get(&h), b.headers.get(&h), "{path}: {h}");
        }
        assert!(
            a.headers[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public"),
            "{path}: an anonymous page is public"
        );
    }
    // Recent changes (0020 §4): the adoption's rows, made here, and with the switch on
    // the mirror's syncs too.
    let rc = get(&web, "/wiki/Special:RecentChanges", None).await;
    assert_eq!(rc.status, StatusCode::OK);
    assert!(
        rc.body
            .contains("<title>Recent changes – librarybase</title>")
    );
    assert!(rc.body.contains("ts-change--edit"), "{}", rc.body);
    assert!(rc.body.contains("href=\"/wiki/Item:Q6\""), "{}", rc.body);
    assert!(!rc.body.contains("ts-change--sync"));
    assert!(rc.body.contains("Mirror syncs are not shown."));
    let with = get(
        &web,
        "/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&days=90",
        None,
    )
    .await;
    assert!(with.body.contains("ts-change--sync"), "{}", with.body);
    assert!(with.body.contains("Domain:wikipedia.org"));
    // The History tab (0012 A71; 19 §3.3): a mirrored domain's syncs and local edits,
    // the source switch, and the note that the mirror keeps only its latest state.
    let hist = get(
        &web,
        "/w/index.php?title=Domain:wikipedia.org&action=history",
        None,
    )
    .await;
    assert_eq!(hist.status, StatusCode::OK);
    assert!(
        hist.body.contains("aria-current=\"page\">History</a>"),
        "{}",
        hist.body
    );
    assert!(hist.body.contains("ts-change--sync"), "{}", hist.body);
    assert!(
        !hist.body.contains("ts-history__switch"),
        "the mirror alone contributes here: no switch"
    );
    assert!(hist.body.contains("Only the latest state from"));
    let local = get(
        &web,
        "/wiki/Domain:wikipedia.org?action=history&source=local",
        None,
    )
    .await;
    assert!(local.body.contains("No history is held for this entity."));
    // A key-mapped provider's own item ID is an alias of the Domain it was written under
    // (05 §4.1): the site sends it to the Domain page, with the query kept, and the API
    // answers with the Domain entity and Wikibase's `redirects`.
    let alias = get(&web, "/wiki/Item:XDQ11?tab=sitelinks", None).await;
    assert_eq!(alias.status, StatusCode::FOUND, "{}", alias.body);
    assert_eq!(
        alias.headers.get(header::LOCATION).unwrap(),
        "/wiki/Domain:wikipedia.org?tab=sitelinks"
    );
    let unmapped = get(&web, "/wiki/Item:XDQ999", None).await;
    assert_eq!(unmapped.status, StatusCode::NOT_FOUND);
    let api = get(
        &embedded,
        "/w/api.php?action=wbgetentities&ids=XDQ11&props=info&format=json",
        None,
    )
    .await;
    let v: serde_json::Value = serde_json::from_str(&api.body).unwrap();
    assert_eq!(
        v["entities"]["domain:wikipedia.org"]["redirects"],
        serde_json::json!({"from": "XDQ11", "to": "domain:wikipedia.org"}),
        "{}",
        api.body
    );
    let data = get(&embedded, "/wiki/Special:EntityData/XDQ11.json", None).await;
    assert_eq!(data.status, StatusCode::OK, "{}", data.body);
    assert!(data.body.contains("\"entities\":{\"domain:wikipedia.org\""));
    let prov = get(
        &embedded,
        "/rest.php/triplespace/v0/entity/XDQ11/provenance",
        None,
    )
    .await;
    assert_eq!(prov.status, StatusCode::OK, "{}", prov.body);
    let q6 = get(&web, "/wiki/Item:Q6?action=history", None).await;
    assert!(
        !q6.body.contains("ts-history__switch"),
        "one source: no switch"
    );
    // The adopted revision shows the source's own summary (none in the fixture), with no
    // `adopt` before it (0035 A12).
    assert!(
        q6.body.contains("ts-change--edit") && !q6.body.contains(">adopt<"),
        "{}",
        q6.body
    );

    // action=compare of an entity's revisions (18 §2.3): the local graph's assertions as
    // sorted JSON, one value to a line, without the fields every revision changes.
    let api = |q: &str| format!("/w/api.php?{q}&format=json&formatversion=2");
    let revs: serde_json::Value = serde_json::from_str(
        &get(
            &embedded,
            &api("action=query&prop=revisions&titles=Item:Q6&rvprop=ids&rvlimit=50"),
            None,
        )
        .await
        .body,
    )
    .unwrap();
    let revs = revs["query"]["pages"][0]["revisions"]
        .as_array()
        .unwrap()
        .clone();
    assert!(!revs.is_empty(), "Q6 has local revisions");
    let latest = revs[0]["revid"].as_u64().unwrap();
    let cmp: serde_json::Value = serde_json::from_str(
        &get(
            &embedded,
            &api("action=compare&fromtitle=Item:Q6&torelative=prev&prop=diff|ids|title"),
            None,
        )
        .await
        .body,
    )
    .unwrap();
    let compared = &cmp["compare"];
    assert_eq!(compared["totitle"], "Item:Q6", "{cmp}");
    assert_eq!(compared["torevid"].as_u64(), Some(latest));
    let body = compared["body"].as_str().unwrap_or_else(|| panic!("{cmp}"));
    assert!(
        body.contains("diff-addedline") || body.contains("diff-deletedline"),
        "{body}"
    );
    assert!(
        !body.contains("lastrevid") && !body.contains("modified"),
        "{body}"
    );
    if revs.len() > 1 {
        assert_eq!(compared["fromrevid"], revs[1]["revid"], "{cmp}");
    } else {
        assert!(
            body.contains("&quot;claims&quot;"),
            "a creation shows the whole entity: {body}"
        );
    }
    // By revision ID, against itself: no difference; and an entity with no local
    // revision says so.
    let same: serde_json::Value = serde_json::from_str(
        &get(
            &embedded,
            &api(&format!("action=compare&fromrev={latest}&torev={latest}")),
            None,
        )
        .await
        .body,
    )
    .unwrap();
    assert_eq!(same["compare"]["body"], "", "{same}");
    let mirrored: serde_json::Value = serde_json::from_str(
        &get(
            &embedded,
            &api("action=compare&fromtitle=Domain:wikipedia.org&torelative=prev"),
            None,
        )
        .await
        .body,
    )
    .unwrap();
    assert!(
        mirrored["compare"].is_object() || mirrored["error"]["code"] == "ts-no-local-revision",
        "{mirrored}"
    );

    // The diff page and an old revision (19 §3.8), from the history's own links; both
    // tiers draw them alike.
    let diff_href = q6
        .body
        .split("href=\"")
        .filter_map(|s| s.split('"').next())
        .find(|h| h.contains("?diff="))
        .expect("a history row links to its diff")
        .replace("&amp;", "&");
    for path in [diff_href.clone(), diff_href.replace("?diff=", "?oldid=")] {
        let a = get(&web, &path, None).await;
        let b = get(&embedded, &path, None).await;
        assert_eq!(a.status, StatusCode::OK, "{path}: {}", a.body);
        assert_eq!(a.body, b.body, "{path}: the HTML differs");
    }
    let diff = get(&web, &diff_href, None).await;
    assert!(diff.body.contains("About this edit"), "{}", diff.body);
    assert!(diff.body.contains("ts-diff__table"), "{}", diff.body);
    let old = get(&web, &diff_href.replace("?diff=", "?oldid="), None).await;
    assert!(old.body.contains("This is revision "), "{}", old.body);
    assert!(old.body.contains("Current version"));
    let page = get(&web, "/wiki/Project:Home", None).await;
    assert!(
        page.body
            .contains("<title>Project:Home – librarybase</title>"),
        "{}",
        page.body
    );
    assert!(page.body.contains("This page is generated"));
    let main = get(&web, "/wiki/Main_Page", None).await;
    assert_eq!(
        main.status,
        StatusCode::FOUND,
        "the old main page redirects"
    );

    // A session cookie the API does not know: the site asks who the viewer is, and the
    // API answers with the anonymous, public form, so the page is the anonymous page.
    let a = get(
        &web,
        "/wiki/Project:Home",
        Some("triplespace_session=nonsense"),
    )
    .await;
    let b = get(
        &embedded,
        "/wiki/Project:Home",
        Some("triplespace_session=nonsense"),
    )
    .await;
    assert_eq!(a.body, b.body);
    assert_eq!(a.body, page.body);
    assert_eq!(
        a.headers.get(header::CACHE_CONTROL),
        b.headers.get(header::CACHE_CONTROL)
    );

    // The theme the pages link to.
    let href = page
        .body
        .split("href=\"/ui/theme/")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap();
    let css = get(&embedded, &format!("/ui/theme/{href}"), None).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(
        css.headers[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .contains("immutable")
    );

    // The embedded site leaves the API's paths to the API.
    let api_call = get(
        &embedded,
        "/w/api.php?action=query&meta=siteinfo&format=json&formatversion=2",
        None,
    )
    .await;
    assert_eq!(api_call.status, StatusCode::OK);
    assert!(api_call.body.contains("\"sitename\""));
    for path in [
        "/w/rest.php/triplespace/v0/nothing",
        "/wiki/Special:EntityData/",
        "/oauth/x",
    ] {
        let r = get(&embedded, path, None).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert!(!r.body.contains("<html"), "{path} reached the site");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A change source with one batch, for [`follow_one_edit`].
struct OneBatch(Option<scatter_ingest::ChangeBatch>);

impl scatter_ingest::ChangeSource for OneBatch {
    async fn next(&mut self) -> Result<scatter_ingest::ChangeBatch, scatter_ingest::IngestError> {
        Ok(self.0.take().unwrap_or(scatter_ingest::ChangeBatch {
            caught_up: true,
            ..Default::default()
        }))
    }

    async fn save(&mut self) -> Result<(), scatter_ingest::IngestError> {
        Ok(())
    }
}

/// Runs the follower once over the fixture with wikipedia.org (`Q11`) edited upstream:
/// revision 57, a new English label. The other entities are unchanged, so only Q11 is
/// written, under a `follow` job.
async fn follow_one_edit(database: &str, fixture: &std::path::Path) {
    let text = std::fs::read_to_string(fixture).unwrap();
    let mut lines = Vec::new();
    for line in text.lines() {
        let entity = line.trim().trim_end_matches(',');
        if entity.is_empty() || entity == "[" || entity == "]" {
            continue;
        }
        let mut e: serde_json::Value = serde_json::from_str(entity).unwrap();
        if e["id"] == "Q11" {
            e["lastrevid"] = serde_json::json!(57);
            e["modified"] = serde_json::json!("2026-10-02T00:00:00Z");
            e["labels"]["en"]["value"] = serde_json::json!("Wikipedia, the free encyclopedia");
        }
        lines.push(e.to_string());
    }
    let dump = std::env::temp_dir().join(format!("ts-site-follow-{}.json", std::process::id()));
    std::fs::write(&dump, format!("[\n{}\n]\n", lines.join(",\n"))).unwrap();
    let registry = scatter_providers::Registry::default_registry();
    let provider = registry.by_slug("internetdomains").unwrap();
    let items: Vec<_> = sync::prepare_wikibase(provider, registry, &dump)
        .unwrap()
        .collect();
    std::fs::remove_file(&dump).unwrap();
    let client = triplespace_cli::common::connect(database).await.unwrap();
    let farm = triplespace_cli::common::farm_of(&client).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(database, &farm).unwrap();
    let mut source = OneBatch(Some(scatter_ingest::ChangeBatch {
        items,
        rows: 1,
        caught_up: true,
        ..Default::default()
    }));
    let out = scatter_ingest::run_follow(
        &store,
        &pipeline,
        "",
        "internetdomains",
        scatter_ingest::write::Attestation::by(&farm.operator().to_string()),
        &mut source,
        registry,
        triplespace_cli::common::now,
        200,
        true,
    )
    .await
    .unwrap();
    assert!(out.job_id > 0, "the follower wrote a job");
    // The load's four entities wrote no `sync` rows (03 §4.6); the follower's one did.
    let rows = client
        .query("SELECT job_id FROM view.activity WHERE kind = 'sync'", &[])
        .await
        .unwrap();
    let jobs: Vec<Option<i64>> = rows.iter().map(|r| r.get(0)).collect();
    assert_eq!(
        jobs,
        [Some(i64::try_from(out.job_id).unwrap())],
        "one sync row, the follower's"
    );
}
