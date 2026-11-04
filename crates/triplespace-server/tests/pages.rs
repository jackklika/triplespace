//! Document pages end to end: Librarybase's wikitext pages, adopted with their history from
//! its own export (`docs/samples/librarybase/dump/librarybase-wikitext.xml.gz`, 39 pages, 147 revisions),
//! projected, and served through the Action API. Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

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

const DB: &str = "tspages_test";
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

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/samples/librarybase/dump/librarybase-wikitext.xml.gz")
}

async fn setup() -> Option<(String, PathBuf)> {
    admin_config()?;
    let database = fresh_database().await;
    let dir = std::env::temp_dir().join(format!("tspages-{}", std::process::id()));
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
        dump: fixture(),
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

async fn query(database: &str, sql: &str) -> Vec<tokio_postgres::Row> {
    let client = triplespace_cli::common::connect(database).await.unwrap();
    client.query(sql, &[]).await.unwrap()
}

/// Harej's talk page, a welcome he posted to himself on 2025-08-31: one archived thread
/// whose single post is his, dated from its signature, with its DiscussionTools name; and
/// the thread's `create` names the page it was imported from, so a second adoption
/// does not write it again (checked where the test adopts twice).
async fn talk_pages_became_threads(database: &str) {
    let threads = query(
        database,
        "SELECT t.subject, t.status, t.post_count, t.target_kind, t.target_id, p.title
         FROM view.thread t JOIN view.page p ON p.tenant = t.tenant AND p.page_id = t.page_id
         WHERE t.tenant = 'librarybase'",
    )
    .await;
    assert_eq!(threads.len(), 1);
    let t = &threads[0];
    assert_eq!(t.get::<_, String>(0), "Welcome to Librarybase!");
    assert_eq!(t.get::<_, String>(1), "archived");
    assert_eq!(t.get::<_, i32>(2), 1);
    assert_eq!(t.get::<_, i16>(3), 2, "an actor target: a user talk page");
    assert!(t.get::<_, String>(4).ends_with(":3"), "Harej is user 3");
    assert_eq!(t.get::<_, String>(5), "2025-08-31/Welcome to Librarybase!");
    let posts = query(
        database,
        "SELECT actor, dt_name, original::text FROM view.post WHERE tenant = 'librarybase'",
    )
    .await;
    assert_eq!(posts.len(), 1);
    assert!(posts[0].get::<_, String>(0).ends_with(":3"));
    assert_eq!(posts[0].get::<_, String>(1), "c-Harej-20250831155600");
    assert!(posts[0].get::<_, String>(2).starts_with("2025-08-31 15:56"));
}

/// Adopting the same dump again writes no revision and converts no talk page twice.
async fn adopting_again_changes_nothing(database: &str) {
    // Adopting again changes nothing.
    adopt::run(adopt::Adopt {
        database: database.to_string(),
        tenant: TENANT.into(),
        dump: fixture(),
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
    .expect("adopt again");
    let again = query(
        database,
        "SELECT count(*)::bigint FROM view.activity WHERE tenant = 'librarybase' AND target_kind = 2",
    )
    .await;
    assert_eq!(
        again[0].get::<_, i64>(0),
        141,
        "140 page revisions and the thread's create"
    );
    let threads = query(
        database,
        "SELECT count(*)::bigint FROM view.thread WHERE tenant = 'librarybase'",
    )
    .await;
    assert_eq!(
        threads[0].get::<_, i64>(0),
        1,
        "the talk page is converted once"
    );
}

#[tokio::test]
async fn librarybase_pages_are_adopted_with_their_history() {
    let Some((database, key_file)) = setup().await else {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    };

    // 27 Project, 4 User and 2 Template pages. Of the 6 User talk pages, one converts to
    // a thread (0090 §10): two are redirects and three belong to users the dump never
    // names as accounts, so they have no home here.
    let by_ns = query(
        &database,
        "SELECT ns, count(*)::bigint FROM view.page WHERE tenant = 'librarybase' GROUP BY ns ORDER BY ns",
    )
    .await;
    let by_ns: Vec<(i32, i64)> = by_ns.iter().map(|r| (r.get(0), r.get(1))).collect();
    // The thread's home mints its talk page (ns 3) as a page too (14 §1.5).
    assert_eq!(by_ns, [(2, 4), (3, 1), (4, 27), (10, 2), (214, 1)]);
    talk_pages_became_threads(&database).await;

    // Pages keep their source page IDs, and their latest revision the source's ID.
    let home = query(
        &database,
        "SELECT page_id, latest_revid, revisions, is_redirect, len FROM view.page
         WHERE tenant = 'librarybase' AND ns = 4 AND title = 'Home'",
    )
    .await;
    assert_eq!(home[0].get::<_, i64>(0), 489);
    assert_eq!(home[0].get::<_, i32>(2), 18);
    assert!(!home[0].get::<_, bool>(3));

    // Every revision of an adopted page has its text, under its source revision ID.
    let revisions = query(
        &database,
        "SELECT count(*)::bigint FROM view.entity_revision r JOIN view.page p
           ON p.tenant = r.tenant AND r.entity_id = p.page_id::text
         WHERE p.tenant = 'librarybase'",
    )
    .await;
    assert_eq!(revisions[0].get::<_, i64>(0), 140);

    // Redirects resolve to their targets.
    let redirects = query(
        &database,
        "SELECT p.title, r.target_ns, r.target_title, r.target_page_id FROM view.redirect r
         JOIN view.page p ON p.tenant = r.tenant AND p.page_id = r.page_id
         WHERE r.tenant = 'librarybase' ORDER BY p.title",
    )
    .await;
    let notability = redirects
        .iter()
        .find(|r| r.get::<_, String>(0) == "Notability")
        .expect("Project:Notability is a redirect");
    assert_eq!(notability.get::<_, Option<i32>>(1), Some(4));
    assert_eq!(notability.get::<_, String>(2), "Scope");
    assert_eq!(notability.get::<_, Option<i64>>(3), Some(10));

    // Links are indexed from the text: Project:Home links to Project:Principles and
    // to Item:Q2.
    let links = query(
        &database,
        "SELECT target_kind, target_id FROM view.page_link WHERE tenant = 'librarybase' AND from_page = 489
         ORDER BY target_kind, target_id",
    )
    .await;
    let links: Vec<(i16, String)> = links.iter().map(|r| (r.get(0), r.get(1))).collect();
    assert!(links.contains(&(6, "4:Principles".into())), "{links:?}");
    assert!(links.contains(&(1, "Q2".into())), "{links:?}");

    // History is activity: one edit row per revision, at the source's time, by the
    // source's author.
    let history = query(
        &database,
        "SELECT revid, time, actor_key, new, bot FROM view.activity
         WHERE tenant = 'librarybase' AND target_kind = 2 AND target_id = '489' ORDER BY revid",
    )
    .await;
    assert_eq!(history.len(), 18);
    assert!(history[0].get::<_, Option<bool>>(3).unwrap_or(false));
    assert_eq!(history[0].get::<_, Option<bool>>(4), Some(true));
    assert!(
        history[0]
            .get::<_, Option<String>>(2)
            .is_some_and(|a| a.ends_with(":3")),
        "the source's user 3, Harej"
    );

    adopting_again_changes_nothing(&database).await;
    api(&database, &key_file).await;
}

/// A client with one cookie.
struct Client {
    router: Router,
    cookie: Option<String>,
}

impl Client {
    async fn call(&mut self, method: &str, query: &str, form: Option<&str>) -> Value {
        let mut req = Request::builder()
            .method(method)
            .uri(format!("/w/api.php?{query}&format=json&formatversion=2"))
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
        assert_eq!(response.status(), StatusCode::OK);
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
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    }

    async fn get(&mut self, query: &str) -> Value {
        self.call("GET", query, None).await
    }

    async fn post(&mut self, query: &str, form: &str) -> Value {
        self.call("POST", query, Some(form)).await
    }
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[allow(clippy::too_many_lines)]
async fn api(database: &str, key_file: &std::path::Path) {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let signing = triplespace_cli::common::signing_key(key_file).unwrap();
    let app = App::new(
        triplespace_db::pool::pool(database, 4).unwrap(),
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
    let api = router_with(app, triplespace_api_rest::routes());
    let site =
        api.clone()
            .fallback_service(triplespace_ui::router(triplespace_client::Client::new(
                triplespace_client::ServiceTransport::new(api.clone()),
            )));
    let mut c = Client {
        router: api,
        cookie: None,
    };

    // The Template namespace is listed: templates are part of wikitext (0042 A19).
    let si = c.get("action=query&meta=siteinfo&siprop=namespaces").await;
    assert!(si["query"]["namespaces"]["10"].is_object(), "{si}");
    let project = si["query"]["namespaces"]["4"]["name"]
        .as_str()
        .unwrap()
        .to_string();

    // Project:Home renders with its footer expanded and its links resolved.
    let home = c.get("action=parse&page=Project:Home").await;
    let p = &home["parse"];
    assert_eq!(p["pageid"], 489, "{home}");
    let html = p["text"].as_str().unwrap();
    assert!(html.contains("Cool Wikibase Network"), "{html}");
    let principles = format!("{project}:Principles");
    assert!(
        p["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["title"] == principles.as_str() && l["exists"] == true),
        "{p}"
    );
    assert!(
        p["templates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["title"] == "Template:Cwbn" && t["exists"] == true),
        "{p}"
    );

    // A revision's content is its stored text.
    let revs = c
        .get(
            "action=query&prop=revisions&titles=Project:Scope&rvprop=ids|content|size&rvslots=main",
        )
        .await;
    let page = &revs["query"]["pages"][0];
    assert_eq!(page["pageid"], 10, "{revs}");
    let content = page["revisions"][0]["slots"]["main"]["content"]
        .as_str()
        .unwrap();
    assert!(content.starts_with("Entities are eligible"), "{content}");
    assert_eq!(
        page["revisions"][0]["slots"]["main"]["contentmodel"],
        "wikitext"
    );
    assert_eq!(
        revs["query"]["normalized"][0]["to"],
        format!("{project}:Scope")
    );

    // A redirect is followed, and prop=info says what the page is.
    let info = c
        .get("action=query&prop=info&titles=Project:Notability&redirects=1")
        .await;
    assert_eq!(
        info["query"]["redirects"][0]["to"],
        format!("{project}:Scope"),
        "{info}"
    );
    assert_eq!(info["query"]["pages"][0]["pageid"], 10);
    assert_eq!(info["query"]["pages"][0]["contentmodel"], "wikitext");

    // prop=templates: what a page transcludes, beside prop=info's fields.
    let tl = c
        .get("action=query&prop=info|templates&titles=Project:Home|Project:Scope")
        .await;
    let pages = tl["query"]["pages"].as_array().unwrap();
    let home = pages.iter().find(|p| p["pageid"] == 489).unwrap();
    assert_eq!(home["templates"][0]["title"], "Template:Cwbn", "{tl}");
    assert_eq!(home["contentmodel"], "wikitext");
    let scope = pages.iter().find(|p| p["pageid"] == 10).unwrap();
    assert!(scope.get("templates").is_none(), "{tl}");

    // allpages lists Project pages in title order, with continuation.
    let all = c
        .get("action=query&list=allpages&apnamespace=4&aplimit=5")
        .await;
    assert_eq!(
        all["query"]["allpages"].as_array().unwrap().len(),
        5,
        "{all}"
    );
    assert_eq!(
        all["query"]["allpages"][0]["title"],
        format!("{project}:About")
    );
    let next = all["continue"]["apcontinue"].as_str().unwrap().to_string();
    let rest = c
        .get(&format!(
            "action=query&list=allpages&apnamespace=4&aplimit=max&apcontinue={}",
            enc(&next)
        ))
        .await;
    assert_eq!(
        rest["query"]["allpages"].as_array().unwrap().len(),
        22,
        "{rest}"
    );

    // The owner logs in and edits.
    let t = c.get("action=query&meta=tokens&type=login").await;
    let login = t["query"]["tokens"]["logintoken"]
        .as_str()
        .unwrap()
        .to_string();
    let cl = c
        .post(
            "action=clientlogin",
            &format!(
                "username=Librarybase&password=owner-secret-1234&logintoken={}&loginreturnurl=https://{HOST}/",
                enc(&login)
            ),
        )
        .await;
    assert_eq!(cl["clientlogin"]["status"], "PASS", "{cl}");
    let csrf = c.get("action=query&meta=tokens").await["query"]["tokens"]["csrftoken"]
        .as_str()
        .unwrap()
        .to_string();

    let created = c
        .post(
            "action=edit",
            &format!(
                "title=Project:Sandbox&text={}&summary=hello&token={}",
                enc("Hello ~~~~"),
                enc(&csrf)
            ),
        )
        .await;
    let e = &created["edit"];
    assert_eq!(e["result"], "Success", "{created}");
    assert_eq!(e["new"], true);
    let first = e["newrevid"].as_u64().unwrap();
    let sandbox = c
        .get("action=parse&page=Project:Sandbox&prop=text|wikitext")
        .await;
    let wikitext = sandbox["parse"]["wikitext"].as_str().unwrap();
    assert!(
        wikitext.starts_with("Hello [[User:Librarybase|Librarybase]]"),
        "{wikitext}"
    );
    assert!(!wikitext.contains("~~~~"));

    // A second edit without a base edits the latest; one from the first revision conflicts.
    let second = c
        .post(
            "action=edit",
            &format!("title=Project:Sandbox&text=Two&token={}", enc(&csrf)),
        )
        .await;
    assert_eq!(second["edit"]["result"], "Success", "{second}");
    assert_eq!(second["edit"]["oldrevid"], first);
    let stale = c
        .post(
            "action=edit",
            &format!(
                "title=Project:Sandbox&text=Three&baserevid={first}&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(stale["error"]["code"], "editconflict", "{stale}");
    let same = c
        .post(
            "action=edit",
            &format!("title=Project:Sandbox&text=Two&token={}", enc(&csrf)),
        )
        .await;
    assert_eq!(same["edit"]["nochange"], true, "{same}");
    // The Interface namespace (0095 §1): MediaWiki's number and canonical name, shown as
    // Interface; `MediaWiki:` titles reach it. Its pages override messages and name IDs,
    // which siteinfo reports (§5).
    let ns = c.get("action=query&meta=siteinfo&siprop=namespaces").await;
    assert_eq!(ns["query"]["namespaces"]["8"]["name"], "Interface", "{ns}");
    assert_eq!(ns["query"]["namespaces"]["8"]["canonical"], "MediaWiki");
    assert_eq!(ns["query"]["namespaces"]["9"]["name"], "Interface talk");
    for (title, text) in [
        ("MediaWiki:Ts-new", "Create"),
        ("Interface:Ts-new/de", "Anlegen"),
        ("Interface:Name/instance:far", "Far Farm"),
    ] {
        let e = c
            .post(
                "action=edit",
                &format!(
                    "title={}&text={}&token={}",
                    enc(title),
                    enc(text),
                    enc(&csrf)
                ),
            )
            .await;
        assert_eq!(e["edit"]["result"], "Success", "{e}");
    }
    let info = c
        .get("action=query&prop=info&titles=MediaWiki:Ts-new")
        .await;
    let page = &info["query"]["pages"][0];
    assert_eq!(page["title"], "Interface:Ts-new", "{info}");
    assert_eq!(page["contentmodel"], "text");
    let si = c.get("action=query&meta=siteinfo&siprop=triplespace").await;
    let messages = &si["query"]["triplespace"]["interface"]["messages"];
    assert_eq!(messages["ts-new"][""], "Create", "{si}");
    assert_eq!(messages["ts-new"]["de"], "Anlegen");
    assert_eq!(messages["name/instance:far"][""], "Far Farm");
    // `{{int:}}` reads the wiki's overrides; `meta=allmessages` lists them over the
    // shipped messages (0095 §2, §8).
    let e = c
        .post(
            "action=edit",
            &format!(
                "title=Interface:Greeting&text={}&token={}",
                enc("Hello, $1!"),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(e["edit"]["result"], "Success", "{e}");
    let parsed = c
        .post(
            "action=parse",
            &format!(
                "contentmodel=wikitext&prop=text&title=Project:Sandbox&text={}",
                enc("{{int:greeting|Bob}} and {{int:no-such-message}}")
            ),
        )
        .await;
    let html = parsed["parse"]["text"].as_str().unwrap_or_default();
    assert!(html.contains("Hello, Bob!"), "{parsed}");
    let all = c
        .get("action=query&meta=allmessages&ammessages=ts-new|greeting|ts-login|no-such-message")
        .await;
    let all = all["query"]["allmessages"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(all[0]["content"], "Create", "{all:?}");
    assert_eq!(all[0]["customised"], true);
    assert_eq!(all[1]["content"], "Hello, $1!");
    assert_eq!(all[2]["content"], "Log in");
    assert!(all[2].get("customised").is_none());
    assert_eq!(all[3]["missing"], true);
    // `amprop=default`: the shipped text beside the override, as Special:AllMessages
    // shows them; `defaultmissing` for a message only the wiki has.
    let with_default = c
        .get("action=query&meta=allmessages&ammessages=ts-new|greeting&amprop=default")
        .await;
    let rows = &with_default["query"]["allmessages"];
    assert_eq!(rows[0]["content"], "Create", "{with_default}");
    assert!(
        rows[0]["default"]
            .as_str()
            .is_some_and(|d| !d.is_empty() && d != "Create"),
        "{with_default}"
    );
    assert_eq!(rows[1]["defaultmissing"], true, "{with_default}");
    // A namespace alias the tenant gives (0008 A37; 10 §3.5): siteinfo lists it, and a
    // link through it renders and is recorded as a Project link.
    namespace_record(
        database,
        4,
        &serde_json::json!({"kind": "namespace", "id": 4, "canonical": "Project", "aliases": ["LB"]}),
    )
    .await;
    let aliases = c
        .get("action=query&meta=siteinfo&siprop=namespacealiases")
        .await;
    assert!(
        aliases["query"]["namespacealiases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["id"] == 4 && a["alias"] == "LB"),
        "{aliases}"
    );
    let through = c
        .post(
            "action=edit",
            &format!(
                "title=Project:Aliased&text={}&createonly=1&token={}",
                enc("[[LB:Home]] and [[Librarybase talk:Home]]"),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(through["edit"]["result"], "Success", "{through}");
    let parsed = c.get("action=parse&page=Project:Aliased&prop=links").await;
    let links = parsed["parse"]["links"].as_array().unwrap();
    assert!(
        links.iter().any(|l| l["ns"] == 4 && l["exists"] == true),
        "{parsed}"
    );
    assert!(links.iter().any(|l| l["ns"] == 5), "{parsed}");
    let (db, conn) = tokio_postgres::connect(database, NoTls).await.unwrap();
    tokio::spawn(conn);
    let rows = db
        .query(
            "SELECT l.target_id FROM view.page_link l JOIN view.page p
               ON p.tenant = l.tenant AND p.page_id = l.from_page
             WHERE l.tenant = $1 AND p.ns = 4 AND p.title = 'Aliased' ORDER BY 1",
            &[&TENANT],
        )
        .await
        .unwrap();
    let targets: Vec<String> = rows.iter().map(|r| r.get(0)).collect();
    assert_eq!(targets, ["4:Home", "5:Home"]);

    // A base by time: the latest revision's own timestamp, shown to the second, names it
    // and not the revision before it, however far into the second it was made. The
    // revision is made in a second of its own, after the earlier ones.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let own = c
        .post(
            "action=edit",
            &format!("title=Project:Sandbox&text=Two.&token={}", enc(&csrf)),
        )
        .await;
    assert_eq!(own["edit"]["result"], "Success", "{own}");
    let latest = c
        .get("action=query&prop=revisions&titles=Project:Sandbox&rvprop=ids|timestamp")
        .await;
    let ts = latest["query"]["pages"][0]["revisions"][0]["timestamp"]
        .as_str()
        .unwrap()
        .to_string();
    let by_time = c
        .post(
            "action=edit",
            &format!(
                "title=Project:Sandbox&text=Two!&basetimestamp={}&token={}",
                enc(&ts),
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(by_time["edit"]["result"], "Success", "{by_time}");
    let back = c
        .post(
            "action=edit",
            &format!("title=Project:Sandbox&text=Two&token={}", enc(&csrf)),
        )
        .await;
    assert_eq!(back["edit"]["result"], "Success", "{back}");
    let exists = c
        .post(
            "action=edit",
            &format!(
                "title=Project:Sandbox&text=x&createonly=1&token={}",
                enc(&csrf)
            ),
        )
        .await;
    assert_eq!(exists["error"]["code"], "articleexists", "{exists}");
    let entity = c
        .post(
            "action=edit",
            &format!("title=Item:Q1&text=x&token={}", enc(&csrf)),
        )
        .await;
    assert_eq!(entity["error"]["code"], "no-direct-editing", "{entity}");
    // Namespace 0 holds articles (10 §3.3; 0038 A17): a bare title is an article whatever
    // its shape, an entity ID's (`Q1`, whose item's page is `Item:Q1`) or a foreign ID's
    // (`Citing sources` parses as `CITing sources`).
    for title in ["Citing_sources", "Stub", "Q1"] {
        let article = c
            .post(
                "action=edit",
                &format!("title={title}&text=Short.&token={}", enc(&csrf)),
            )
            .await;
        assert_eq!(article["edit"]["result"], "Success", "{title}: {article}");
    }

    let history = c
        .get(
            "action=query&prop=revisions&titles=Project:Sandbox&rvprop=ids|comment|user&rvlimit=10",
        )
        .await;
    let revisions = history["query"]["pages"][0]["revisions"]
        .as_array()
        .unwrap();
    assert_eq!(revisions.len(), 5, "{history}");
    assert_eq!(revisions[4]["comment"], "hello");
    assert_eq!(revisions[0]["user"], "Librarybase");
    // parentid chains the revisions, the first's being 0, also at the end of a short page.
    let ids: Vec<u64> = revisions
        .iter()
        .map(|r| r["revid"].as_u64().unwrap())
        .collect();
    for (i, r) in revisions.iter().enumerate() {
        let parent = ids.get(i + 1).copied().unwrap_or(0);
        assert_eq!(r["parentid"].as_u64(), Some(parent), "{history}");
    }
    let first_page = c
        .get("action=query&prop=revisions&titles=Project:Sandbox&rvprop=ids&rvlimit=2")
        .await;
    assert_eq!(
        first_page["query"]["pages"][0]["revisions"][1]["parentid"].as_u64(),
        Some(ids[2]),
        "{first_page}"
    );

    // action=compare: two revisions, MediaWiki's table rows.
    let (oldest, newest) = (ids[4], ids[0]);
    let cmp = c
        .get(&format!(
            "action=compare&fromrev={oldest}&torev={}&prop=diff|ids|title|user|comment|size|rel",
            ids[3]
        ))
        .await;
    let body = cmp["compare"]["body"]
        .as_str()
        .unwrap_or_else(|| panic!("{cmp}"));
    assert!(body.contains("class=\"diff-lineno\">Line 1:"), "{body}");
    assert!(
        body.contains("diff-deletedline") && body.contains("diff-addedline"),
        "{body}"
    );
    assert_eq!(cmp["compare"]["fromrevid"].as_u64(), Some(oldest));
    assert_eq!(cmp["compare"]["fromtitle"], format!("{project}:Sandbox"));
    assert_eq!(cmp["compare"]["fromcomment"], "hello");
    assert_eq!(cmp["compare"]["touser"], "Librarybase");
    assert_eq!(cmp["compare"]["next"].as_u64(), Some(ids[2]));
    assert!(cmp["compare"].get("prev").is_none(), "the first has none");
    // difftype=inline: the same diff in one column, no table rows.
    let inline = c
        .get(&format!(
            "action=compare&fromrev={oldest}&torev={}&difftype=inline",
            ids[3]
        ))
        .await;
    let ib = inline["compare"]["body"]
        .as_str()
        .unwrap_or_else(|| panic!("{inline}"));
    assert!(ib.contains("mw-diff-inline-"), "{ib}");
    assert!(!ib.contains("<tr"), "{ib}");
    let unified = c
        .get(&format!(
            "action=compare&fromrev={oldest}&torev={}&difftype=unified",
            ids[3]
        ))
        .await;
    assert_eq!(unified["error"]["code"], "badvalue", "{unified}");
    // torelative=prev diffs the previous revision against `from`; the first is a creation.
    let prev = c
        .get(&format!(
            "action=compare&fromrev={}&torelative=prev",
            ids[3]
        ))
        .await;
    assert_eq!(
        prev["compare"]["fromrevid"].as_u64(),
        Some(oldest),
        "{prev}"
    );
    assert_eq!(prev["compare"]["torevid"].as_u64(), Some(ids[3]));
    let created = c
        .get(&format!("action=compare&fromrev={oldest}&torelative=prev"))
        .await;
    assert!(created["compare"].get("fromrevid").is_none(), "{created}");
    assert!(
        created["compare"]["body"]
            .as_str()
            .unwrap()
            .contains("diff-addedline")
    );
    assert!(
        !created["compare"]["body"]
            .as_str()
            .unwrap()
            .contains("diff-deletedline")
    );
    assert!(
        created["warnings"]
            .to_string()
            .contains("no previous revision"),
        "{created}"
    );
    // A title names its latest revision; next of the latest is itself.
    let latest = c
        .get("action=compare&fromtitle=Project:Sandbox&torelative=next")
        .await;
    assert_eq!(
        latest["compare"]["fromrevid"].as_u64(),
        Some(newest),
        "{latest}"
    );
    assert_eq!(latest["compare"]["body"], "");
    assert!(latest["warnings"].to_string().contains("no next revision"));
    // Errors.
    let none = c
        .get("action=compare&fromrev=99999999&torelative=prev")
        .await;
    assert_eq!(none["error"]["code"], "nosuchrevid", "{none}");
    let half = c.get(&format!("action=compare&fromrev={oldest}")).await;
    assert_eq!(half["error"]["code"], "missingparam", "{half}");

    pages_on_the_site(site, &project).await;
}

/// Writes a tenant `namespace` record, as the namespace settings will.
async fn namespace_record(database: &str, id: i32, content: &Value) {
    let farm = triplespace_cli::common::farm_at(database).await.unwrap();
    let (store, pipeline) = triplespace_cli::common::store(database, &farm).unwrap();
    let mut cx = scatter_projection::Backend::begin(&store).await.unwrap();
    let config = scatter_ingest::store::IngestStore::partition(&store, &mut cx, TENANT, "config")
        .await
        .unwrap()
        .expect("the tenant has a config partition");
    let d = triplespace_cli::common::draft(
        scatter_log::registry::PAYLOAD_CONFIG,
        &format!("namespace:{id}"),
        content,
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
}

/// What a browser got.
struct Got {
    status: StatusCode,
    location: String,
    cache_control: String,
    body: String,
}

impl Got {
    /// The value of the first input of a name.
    fn field(&self, name: &str) -> String {
        let marker = format!("name=\"{name}\" value=\"");
        let at = self
            .body
            .find(&marker)
            .unwrap_or_else(|| panic!("no {name} field in {}", self.body));
        let rest = &self.body[at + marker.len()..];
        rest[..rest.find('"').unwrap()]
            .replace("&#43;", "+")
            .replace("&#92;", "\\")
            .replace("&quot;", "\"")
            .replace("&amp;", "&")
    }
}

/// A browser on the site: one cookie.
struct Browser {
    router: Router,
    cookie: Option<String>,
}

impl Browser {
    async fn send(&mut self, method: &str, path: &str, form: Option<&[(&str, &str)]>) -> Got {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, format!("https://{HOST}"));
        if let Some(c) = &self.cookie {
            req = req.header(header::COOKIE, format!("triplespace_session={c}"));
        }
        let req = match form {
            Some(pairs) => req
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(
                    pairs
                        .iter()
                        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
                        .collect::<Vec<_>>()
                        .join("&"),
                ))
                .unwrap(),
            None => req.body(Body::empty()).unwrap(),
        };
        let r = self.router.clone().oneshot(req).await.unwrap();
        let status = r.status();
        let header = |h: header::HeaderName| {
            r.headers()
                .get(h)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let location = header(header::LOCATION);
        let cache_control = header(header::CACHE_CONTROL);
        for sc in r.headers().get_all(header::SET_COOKIE) {
            if let Some(v) = sc
                .to_str()
                .unwrap()
                .strip_prefix("triplespace_session=")
                .and_then(|r| r.split(';').next())
            {
                self.cookie = (!v.is_empty()).then(|| v.to_string());
            }
        }
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        Got {
            status,
            location,
            cache_control,
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }
    }

    async fn get(&mut self, path: &str) -> Got {
        self.send("GET", path, None).await
    }

    async fn post(&mut self, path: &str, form: &[(&str, &str)]) -> Got {
        self.send("POST", path, Some(form)).await
    }
}

/// Reading, editing and the history of document pages through the site (19 §3).
#[allow(clippy::too_many_lines)]
async fn pages_on_the_site(site: Router, project: &str) {
    let mut reader = Browser {
        router: site.clone(),
        cookie: None,
    };
    // The main page is the adopted one, not the generated one, with its tabs.
    let home = reader.get("/wiki/Project:Home").await;
    assert_eq!(home.status, StatusCode::OK, "{}", home.body);
    assert!(
        home.cache_control.starts_with("public"),
        "{}",
        home.cache_control
    );
    assert!(home.body.contains("Cool Wikibase Network"), "{}", home.body);
    assert!(!home.body.contains("This page is generated"));
    assert!(home.body.contains("ts-document__content"));
    assert!(
        home.body
            .contains(&format!("href=\"/wiki/{project}:Home?action=history\""))
    );
    assert!(
        home.body
            .contains(&format!("href=\"/wiki/{project}:Home?action=edit\""))
    );
    assert!(
        home.body.contains(&format!("<title>{project}:Home – ")),
        "{}",
        home.body
    );

    // A redirect is followed and said, and can be read itself.
    let scope = reader.get("/wiki/Project:Notability").await;
    assert!(scope.body.contains("(Redirected from"), "{}", scope.body);
    assert!(scope.body.contains("Entities are eligible"));
    assert!(
        scope
            .body
            .contains(&format!("href=\"/wiki/{project}:Notability?redirect=no\""))
    );
    let itself = reader.get("/wiki/Project:Notability?redirect=no").await;
    assert!(!itself.body.contains("(Redirected from"));
    assert!(itself.body.contains("redirect"), "{}", itself.body);

    // `action=render`: the content alone.
    let r = reader.get("/wiki/Project:Home?action=render").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.body.starts_with("<div class=\"ts-document__content\">"),
        "{}",
        r.body
    );

    // The history: the adoption's rows, by the page's own editors.
    let history = reader.get("/wiki/Project:Home?action=history").await;
    assert_eq!(history.status, StatusCode::OK, "{}", history.body);
    assert_eq!(
        history.body.matches("class=\"ts-change ").count(),
        18,
        "{}",
        history.body
    );
    assert!(history.body.contains("aria-current=\"page\">History</a>"));

    // Each history row links to its diff (19 §3.8); the diff walks along the history.
    let marker = "href=\"/wiki/Project:Home?diff=";
    let diff_links: Vec<String> = history
        .body
        .match_indices(marker)
        .map(|(i, _)| {
            let rest = &history.body[i + "href=\"".len()..];
            rest[..rest.find('"').unwrap()].to_string()
        })
        .collect();
    assert_eq!(diff_links.len(), 18, "{}", history.body);
    let newest = reader.get(&diff_links[0]).await;
    assert_eq!(newest.status, StatusCode::OK, "{}", newest.body);
    assert!(newest.body.contains("ts-page-diff__table"));
    assert!(
        newest
            .body
            .contains("class=\"ts-page-diff__inline mw-diff-inline\""),
        "the one-column layout too"
    );
    assert!(
        newest.body.contains("Latest revision as of"),
        "{}",
        newest.body
    );
    assert!(newest.body.contains("← Older edit"));
    assert!(!newest.body.contains("Newer edit →"));
    assert!(
        newest.body.contains("diff-addedline") || newest.body.contains("diff-deletedline"),
        "{}",
        newest.body
    );
    assert!(
        newest.body.contains("ts-document__content"),
        "the revision below"
    );
    assert!(
        newest.cache_control.starts_with("public"),
        "{}",
        newest.cache_control
    );
    let first = reader.get(&diff_links[17]).await;
    assert!(first.body.contains("(Page created)"), "{}", first.body);
    assert!(first.body.contains("Newer edit →"));
    assert!(!first.body.contains("diff-deletedline"));
    let only = reader.get(&format!("{}&diffonly=1", diff_links[0])).await;
    assert!(!only.body.contains("ts-document__content"));
    let bad = reader
        .get(&format!("/wiki/{project}:Home?diff=nonsense"))
        .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    let gone = reader
        .get(&format!("/wiki/{project}:Home?diff=99999999"))
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND, "{}", gone.body);

    // The redirecting special pages find a revision's page through the API (21 §6.1).
    let revid: String = diff_links[17]
        .split(['=', '&'])
        .find(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        .expect("a revision ID in the first diff link")
        .to_string();
    let permanent = reader
        .get(&format!("/wiki/Special:PermanentLink/{revid}"))
        .await;
    assert_eq!(permanent.status, StatusCode::FOUND, "{}", permanent.body);
    assert_eq!(
        permanent.location,
        format!("/wiki/{project}:Home?oldid={revid}")
    );
    assert_eq!(reader.get(&permanent.location).await.status, StatusCode::OK);
    let diff = reader.get(&format!("/wiki/Special:Diff/{revid}")).await;
    assert_eq!(diff.location, format!("/wiki/{project}:Home?diff={revid}"));
    let by_type = reader
        .get(&format!(
            "/w/index.php?title=Special:Redirect&wptype=revision&wpvalue={revid}"
        ))
        .await;
    assert_eq!(by_type.location, permanent.location);
    let nowhere = reader.get("/wiki/Special:PermanentLink/99999999").await;
    assert_eq!(nowhere.status, StatusCode::NOT_FOUND, "{}", nowhere.body);

    // AllPages and PrefixIndex over the adopted pages: titles without the namespace, the
    // redirect in italics (a second, standard `list=allpages` call finds it).
    let all = reader.get("/wiki/Special:AllPages?namespace=4").await;
    assert_eq!(all.status, StatusCode::OK, "{}", all.body);
    assert!(
        all.body.contains(&format!(
            "<li><a href=\"/wiki/{project}:Home\">Home</a></li>"
        )),
        "{}",
        all.body
    );
    assert!(
        all.body.contains(&format!(
            "<li class=\"ts-allpages__redirect\"><a href=\"/wiki/{project}:Notability\">Notability</a></li>"
        )),
        "{}",
        all.body
    );
    let hidden = reader
        .get("/wiki/Special:AllPages?namespace=4&hideredirects=1")
        .await;
    assert!(!hidden.body.contains("Notability</a></li>"));
    assert!(hidden.body.contains("Home</a></li>"));
    let prefixed = reader.get("/wiki/Special:PrefixIndex/Project:No").await;
    assert_eq!(prefixed.status, StatusCode::OK, "{}", prefixed.body);
    assert!(
        prefixed.body.contains("Notability</a></li>"),
        "{}",
        prefixed.body
    );
    assert!(!prefixed.body.contains(">Home</a></li>"));
    let stripped = reader
        .get("/w/index.php?title=Special:PrefixIndex&prefix=No&namespace=4&stripprefix=1")
        .await;
    assert!(stripped.body.contains(">tability</a>"), "{}", stripped.body);

    // The read-only pages about the wiki, from what the API reports (21 §6.1).
    let compared = reader
        .get(&format!(
            "/w/index.php?title=Special:ComparePages&page1=Project:Notability&rev2={revid}"
        ))
        .await;
    assert_eq!(compared.status, StatusCode::OK, "{}", compared.body);
    assert!(
        compared.body.contains("ts-page-diff__table"),
        "{}",
        compared.body
    );
    assert!(compared.body.contains("diff-deletedline") || compared.body.contains("diff-addedline"));
    let namespaces = reader.get("/wiki/Special:NamespaceInfo").await;
    assert_eq!(namespaces.status, StatusCode::OK, "{}", namespaces.body);
    assert!(namespaces.body.contains("<td>4</td>"));
    assert!(namespaces.body.contains("<td>120</td><td>Item</td>"));
    let groups = reader.get("/wiki/Special:ListGroupRights").await;
    assert_eq!(groups.status, StatusCode::OK);
    assert!(
        groups.body.contains("<code>sysop</code>"),
        "{}",
        groups.body
    );
    let types = reader.get("/wiki/Special:ListDatatypes").await;
    assert!(
        types.body.contains("<code>wikibase-item</code>"),
        "{}",
        types.body
    );
    let chain = reader
        .get("/wiki/Special:MyLanguageFallbackChain?uselang=pt-br")
        .await;
    assert!(chain.body.contains("<code lang=\"pt\">pt</code>"));
    let titles = reader.get("/wiki/Special:ProtectedTitles").await;
    assert_eq!(titles.status, StatusCode::OK, "{}", titles.body);
    let messages = reader
        .get("/w/index.php?title=Special:AllMessages&prefix=ts-allmessages-")
        .await;
    assert_eq!(messages.status, StatusCode::OK, "{}", messages.body);
    assert!(
        messages.body.contains("<td>The interface&#39;s messages."),
        "{}",
        messages.body
    );
    assert!(
        messages
            .body
            .contains("href=\"/wiki/Interface:Ts-allmessages-title\"")
    );

    // A title with no page: the notice, and Create for a signed-in reader only.
    let none = reader.get("/wiki/Project:Nothing_here").await;
    assert_eq!(none.status, StatusCode::NOT_FOUND);
    assert!(
        none.body
            .contains("There is currently no text in this page.")
    );
    assert!(!none.body.contains("Create this page"));
    let anon_edit = reader.get("/wiki/Project:Sandbox?action=edit").await;
    assert!(
        anon_edit.body.contains("Log in to edit pages."),
        "{}",
        anon_edit.body
    );
    assert!(!anon_edit.body.contains("wpTextbox1"));
    // A title outside the page namespaces is the site's 404, and Main Page still redirects.
    assert_eq!(
        reader.get("/wiki/Main_Page").await.status,
        StatusCode::FOUND
    );

    // The owner logs in through the site.
    let mut owner = Browser {
        router: site.clone(),
        cookie: None,
    };
    let login = owner.get("/wiki/Special:UserLogin").await;
    let r = owner
        .post(
            "/w/index.php?title=Special:UserLogin",
            &[
                ("logintoken", &login.field("logintoken")),
                ("returnto", "Project:Sandbox"),
                ("username", "Librarybase"),
                ("password", "owner-secret-1234"),
            ],
        )
        .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{}", r.body);

    // The edit form: the source, the base, and the token.
    let form = owner.get("/wiki/Project:Sandbox?action=edit").await;
    assert_eq!(form.status, StatusCode::OK, "{}", form.body);
    assert!(form.cache_control.starts_with("private"));
    assert!(form.body.contains(">\nTwo</textarea>"), "{}", form.body);
    assert!(form.body.contains(&format!("Editing {project}:Sandbox")));
    let token = form.field("wpEditToken");
    let base = form.field("baseRevId");
    let edittime = form.field("wpEdittime");
    let path = format!("/w/index.php?title={project}:Sandbox&action=submit");

    // A preview renders, with the signature expanded, and saves nothing.
    let preview = owner
        .post(
            &path,
            &[
                ("wpEditToken", &token),
                ("baseRevId", &base),
                ("wpEdittime", &edittime),
                ("wpTextbox1", "'''Three''' ~~~"),
                ("wpSummary", "third"),
                ("wpPreview", "1"),
            ],
        )
        .await;
    assert_eq!(preview.status, StatusCode::OK, "{}", preview.body);
    assert!(preview.body.contains("<b>Three</b>"), "{}", preview.body);
    assert!(preview.body.contains("User:Librarybase"));
    assert!(preview.body.contains("This is only a preview"));
    assert!(preview.body.contains("value=\"third\""));

    // A save goes on to the page.
    let saved = owner
        .post(
            &path,
            &[
                ("wpEditToken", &token),
                ("baseRevId", &base),
                ("wpEdittime", &edittime),
                ("wpTextbox1", "'''Three'''\r\nlines"),
                ("wpSummary", "third"),
                ("wpSave", "1"),
            ],
        )
        .await;
    assert_eq!(saved.status, StatusCode::SEE_OTHER, "{}", saved.body);
    assert_eq!(saved.location, format!("/wiki/{project}:Sandbox"));
    let after = owner.get(&saved.location).await;
    assert!(after.body.contains("<b>Three</b>"), "{}", after.body);

    // A second save from the same base conflicts: the text is kept, the current shown,
    // and the base moved on, so saving again succeeds.
    let conflict = owner
        .post(
            &path,
            &[
                ("wpEditToken", &token),
                ("baseRevId", &base),
                ("wpEdittime", &edittime),
                ("wpTextbox1", "Four"),
                ("wpSummary", "fourth"),
                ("wpSave", "1"),
            ],
        )
        .await;
    assert_eq!(conflict.status, StatusCode::OK, "{}", conflict.body);
    assert!(
        conflict.body.contains("Someone else changed this page"),
        "{}",
        conflict.body
    );
    assert!(conflict.body.contains(">\nFour</textarea>"));
    assert!(
        conflict
            .body
            .contains("&#39;&#39;&#39;Three&#39;&#39;&#39;\nlines</textarea>"),
        "{}",
        conflict.body
    );
    let moved = conflict.field("baseRevId");
    assert_ne!(moved, base);
    let again = owner
        .post(
            &path,
            &[
                ("wpEditToken", &token),
                ("baseRevId", &moved),
                ("wpEdittime", &conflict.field("wpEdittime")),
                ("wpTextbox1", "Four"),
                ("wpSummary", "fourth"),
                ("wpSave", "1"),
            ],
        )
        .await;
    assert_eq!(again.status, StatusCode::SEE_OTHER, "{}", again.body);

    // An old revision says so; one that does not exist is a 404.
    let old = owner
        .get(&format!("/wiki/{project}:Sandbox?oldid={base}"))
        .await;
    assert_eq!(old.status, StatusCode::OK, "{}", old.body);
    assert!(old.body.contains("This is an old revision of this page."));
    assert!(
        old.body.contains("<p>Two\n</p>") || old.body.contains("<p>Two</p>"),
        "{}",
        old.body
    );
    let gone = owner
        .get(&format!("/wiki/{project}:Sandbox?oldid=99999999"))
        .await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND, "{}", gone.body);

    // A new page: the New page form, then the edit form that creates it.
    let new = owner.get("/w/index.php?action=edit").await;
    assert_eq!(new.status, StatusCode::OK, "{}", new.body);
    assert!(new.body.contains("name=\"title\""));
    let create = owner
        .get("/w/index.php?title=Project:Fresh&action=edit")
        .await;
    assert!(
        create.body.contains(&format!("Creating {project}:Fresh")),
        "{}",
        create.body
    );
    assert!(create.body.contains("name=\"wpCreateonly\""));
    let made = owner
        .post(
            &format!("/w/index.php?title={project}:Fresh&action=submit"),
            &[
                ("wpEditToken", &create.field("wpEditToken")),
                ("wpCreateonly", "1"),
                ("wpTextbox1", "Fresh [[Project:Home]]"),
                ("wpSummary", "new"),
                ("wpSave", "1"),
            ],
        )
        .await;
    assert_eq!(made.status, StatusCode::SEE_OTHER, "{}", made.body);
    let fresh = owner.get(&made.location).await;
    assert!(
        fresh
            .body
            .contains(&format!("href=\"/wiki/{project}:Home\"")),
        "{}",
        fresh.body
    );

    // An expired token keeps the text and asks again.
    let stale = owner
        .post(
            &path,
            &[
                ("wpEditToken", "nonsense+\\"),
                ("baseRevId", &moved),
                ("wpTextbox1", "Five"),
                ("wpSave", "1"),
            ],
        )
        .await;
    assert_eq!(stale.status, StatusCode::OK, "{}", stale.body);
    assert!(
        stale.body.contains("Your session expired"),
        "{}",
        stale.body
    );
    assert!(stale.body.contains(">\nFive</textarea>"));
}
