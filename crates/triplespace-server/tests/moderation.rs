//! The moderation modules end to end (0023 §8; 09 §6): `action=protect`, `action=delete`,
//! `action=undelete` and `action=revisiondelete` on document pages and threads, what they
//! do to the views (deleted pages leave the title index, hidden parts set the visibility
//! bits) and the log events they project as; protection enforced on `action=edit` and on
//! thread writes; reserved titles (09 §4.8). Needs `TRIPLESPACE_TEST_DATABASE_URL`;
//! vacuous without it.

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

const DB: &str = "tsmoderation_test";
const HOST: &str = "librarybase.org";
const TENANT: &str = "lbwiki";
const REST: &str = "/w/rest.php/triplespace/v0";

fn admin_config() -> Option<Config> {
    let url = std::env::var("TRIPLESPACE_TEST_DATABASE_URL").ok()?;
    Some(url.parse().expect("TRIPLESPACE_TEST_DATABASE_URL parses"))
}

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

/// A fresh instance (database `{DB}_{name}`) with Alice, its owner, logged in.
async fn instance(name: &str) -> (String, App, Client) {
    let database = fresh_database(name).await;
    let dir = std::env::temp_dir().join(format!("tsmoderation-{}-{name}", std::process::id()));
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
        router: router_with(app.clone(), triplespace_api_rest::routes()),
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
    (database, app, c)
}

/// Registers an account, solving the challenge when one is offered, and logs it in.
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

/// Adds an account to a group with a membership record, as `userrights` will.
async fn grant(app: &App, name: &str, group: &str) {
    use scatter_ingest::store::{IngestStore as _, Sequence};
    use scatter_projection::Backend as _;
    let store = app.store();
    let mut cx = store.begin().await.unwrap();
    let key: String = cx
        .conn()
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = $2",
            &[&TENANT, &name],
        )
        .await
        .unwrap()
        .get(0);
    let actors = store
        .partition(&mut cx, TENANT, "actors")
        .await
        .unwrap()
        .unwrap();
    let mut d = triplespace_cli::common::draft(
        "scatter:v0/membership",
        &key,
        &serde_json::to_value(scatter_actors::membership::Membership::add(group, None)).unwrap(),
        &key,
        Some("test"),
        triplespace_api_action::edit::now_micros(),
    )
    .unwrap();
    d.logid = Some(
        store
            .next_id(&mut cx, TENANT, &Sequence::Log)
            .await
            .unwrap(),
    );
    scatter_ingest::write::append_projected(
        store,
        app.pipeline(),
        &mut cx,
        actors,
        d,
        scatter_projection::Budget::NONE,
    )
    .await
    .unwrap();
    store.commit(cx).await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn protect_delete_undelete_and_hide() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut c) = instance("main").await;
    let token = c.csrf().await;
    let t = enc(&token);
    let mut anon = Client {
        router: c.router.clone(),
        cookie: None,
    };

    // A page with two revisions.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=First.&token={t}"
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().unwrap();
    c.api_post(&format!(
        "action=edit&title=Project:Scope&text=Second.&token={t}"
    ))
    .await;

    // The modules need a token and the right; an anonymous caller has neither.
    let r = anon
        .send(
            "POST",
            "/w/api.php",
            Some("application/x-www-form-urlencoded"),
            Some(
                "action=delete&title=Project:Scope&format=json&formatversion=2&token=%2B%5C".into(),
            ),
            &[],
        )
        .await;
    assert!(r.body["error"]["code"].is_string(), "{}", r.body);

    // Protect, then change the protection.
    let p = c
        .api_post(&format!("action=protect&title=Project:Scope&protections=edit%3Dsysop%7Cmove%3Dsysop&expiry=infinite&reason=Vandalism&token={t}"))
        .await;
    assert_eq!(p["protect"]["protections"][0]["edit"], "sysop", "{p}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dautoconfirmed&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dsysop&cascade=1&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-no-cascade");

    // Delete: the page is missing for everyone, its title is free, and the protection
    // stays beside the deletion.
    let d = c
        .api_post(&format!(
            "action=delete&title=Project:Scope&reason=Spam&token={t}"
        ))
        .await;
    assert_eq!(d["delete"]["title"], "Library Base:Scope", "{d}");
    assert!(d["delete"]["logid"].is_u64());
    let q = anon
        .api("action=query&prop=info&titles=Project:Scope")
        .await;
    assert_eq!(q["query"]["pages"][0]["missing"], true, "{q}");
    let r = c
        .api_post(&format!("action=delete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");

    // A new page takes the title; undeleting into it is refused.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Fresh.&createonly=1&token={t}"
        ))
        .await;
    let fresh = e["edit"]["pageid"].as_u64().unwrap();
    assert_ne!(fresh, page);
    let r = c
        .api_post(&format!("action=undelete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-title-taken", "{r}");

    // Delete the new one: two deleted pages had the title, so undelete needs `pageid`.
    c.api_post(&format!("action=delete&title=Project:Scope&token={t}"))
        .await;
    let r = c
        .api_post(&format!("action=undelete&title=Project:Scope&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-ambiguous-undelete", "{r}");
    assert_eq!(r["error"]["pageids"], json!([fresh, page]), "{r}");
    let u = c
        .api_post(&format!(
            "action=undelete&title=Project:Scope&pageid={fresh}&reason=Mistake&token={t}"
        ))
        .await;
    assert_eq!(u["undelete"]["title"], "Library Base:Scope", "{u}");
    assert_eq!(u["undelete"]["pageid"], fresh, "{u}");
    let q = anon
        .api("action=query&prop=info&titles=Project:Scope")
        .await;
    assert_eq!(q["query"]["pages"][0]["pageid"], fresh, "{q}");

    // Protection is enforced: a group Alice is not in may edit, she may not.
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Scope&protections=edit%3Dbot&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Blocked.&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "protectedpage", "{r}");
    c.api_post(&format!(
        "action=protect&title=Project:Scope&protections=edit%3Dall&token={t}"
    ))
    .await;
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Open.&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");

    // Create-protection reserves the title under a page ID; a create takes it once the
    // creator is admitted (Alice is in `owner`, not `bot`).
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Dbot&token={t}"
        ))
        .await;
    let reserved = p["protect"]["pageid"].as_u64().expect("a reserved ID");
    let q = anon
        .api("action=query&prop=info&titles=Project:Reserved")
        .await;
    assert_eq!(q["query"]["pages"][0]["missing"], true, "{q}");
    let r = c
        .api_post(&format!(
            "action=edit&title=Project:Reserved&text=Mine.&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle-createonly", "{r}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Downer&token={t}"
        ))
        .await;
    assert_eq!(p["protect"]["pageid"], reserved, "{p}");
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Reserved&text=Mine.&token={t}"
        ))
        .await;
    assert_eq!(e["edit"]["pageid"], reserved, "{e}");
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Downer&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "create-titleexists", "{r}");

    // Read protection before a page exists: a confidential reservation, which may name
    // only a group the protector is in.
    let r = c
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Dbot&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-locked-out", "{r}");
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Downer&token={t}"
        ))
        .await;
    let secret = p["protect"]["pageid"].as_u64().expect("a reserved ID");
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Secret&text=Hush.&token={t}"
        ))
        .await;
    assert_eq!(e["edit"]["pageid"], secret, "{e}");

    // The confidential page is absent for a principal outside the group, on every read
    // that looks it up, lists it, links to it or transcludes it (0056 §5; 09 §5.3), and
    // readable for the group.
    let i = anon
        .api("action=query&prop=info&titles=Project:Secret")
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let i = anon
        .api(&format!(
            "action=query&prop=revisions&pageids={secret}&rvprop=content"
        ))
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let i = c
        .api(&format!(
            "action=query&prop=revisions&pageids={secret}&rvprop=content"
        ))
        .await;
    assert_eq!(i["query"]["pages"][0]["pageid"], secret, "{i}");
    let p = anon.api("action=parse&page=Project:Secret").await;
    assert_eq!(p["error"]["code"], "missingtitle", "{p}");
    let p = c.api("action=parse&page=Project:Secret&prop=text").await;
    assert!(
        p["parse"]["text"].as_str().unwrap().contains("Hush."),
        "{p}"
    );
    let titles = |r: &Value, list: &str| -> Vec<String> {
        r["query"][list]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["title"].as_str().map(str::to_owned))
            .collect()
    };
    let a = anon.api("action=query&list=allpages&apnamespace=4").await;
    assert!(
        !titles(&a, "allpages").iter().any(|t| t.ends_with("Secret")),
        "{a}"
    );
    let a = c.api("action=query&list=allpages&apnamespace=4").await;
    assert!(
        titles(&a, "allpages").iter().any(|t| t.ends_with("Secret")),
        "{a}"
    );
    let rc = anon
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=500")
        .await;
    assert!(
        !titles(&rc, "recentchanges")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{rc}"
    );
    let rc = c
        .api("action=query&list=recentchanges&rcprop=title%7Cids&rclimit=500")
        .await;
    assert!(
        titles(&rc, "recentchanges")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{rc}"
    );
    let l = anon.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        !titles(&l, "logevents")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{l}"
    );
    let l = c.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        titles(&l, "logevents")
            .iter()
            .any(|t| t.ends_with("Secret")),
        "{l}"
    );
    let r = anon.rest("GET", "/activity?limit=500", None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!r.body.to_string().contains("Secret"), "{}", r.body);
    // A public page that links to it and transcludes it: a red link and nothing of its
    // text for the outsider.
    c.api_post(&format!(
        "action=edit&title=Project:Index&text={}&token={t}",
        enc("See [[Project:Secret]]. {{Project:Secret}}")
    ))
    .await;
    let p = anon.api("action=parse&page=Project:Index&prop=text").await;
    let html = p["parse"]["text"].as_str().unwrap();
    assert!(!html.contains("Hush."), "{html}");
    assert!(html.contains("redlink=1"), "{html}");
    let p = c.api("action=parse&page=Project:Index&prop=text").await;
    let html = p["parse"]["text"].as_str().unwrap();
    assert!(html.contains("Hush."), "{html}");
    assert!(!html.contains("redlink=1"), "{html}");

    // A confidential `namespace` or `tenant` restriction hides every page under it, even
    // from the owner when the group is the suppression group, which `owner` does not
    // read as (0016 A37). The rows go into view.acl directly.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    for target in [
        "acl:namespace:4".to_string(),
        format!("acl:tenant:{TENANT}"),
    ] {
        db.execute(
            "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
             VALUES ($1, $2, $3, 'confidential', NULL, 0)",
            &[&TENANT, &target, &json!({"read": {"group": "suppress"}})],
        )
        .await
        .unwrap();
        for who in [&mut anon, &mut c] {
            let i = who.api("action=query&prop=info&titles=Project:Index").await;
            assert_eq!(i["query"]["pages"][0]["missing"], true, "{target}: {i}");
            let a = who.api("action=query&list=allpages&apnamespace=4").await;
            assert!(titles(&a, "allpages").is_empty(), "{target}: {a}");
            let rc = who
                .api("action=query&list=recentchanges&rcprop=title&rclimit=500")
                .await;
            assert!(
                !titles(&rc, "recentchanges")
                    .iter()
                    .any(|t| t.ends_with("Index")),
                "{target}: {rc}"
            );
        }
        db.execute(
            "DELETE FROM view.acl WHERE tenant = $1 AND target = $2",
            &[&TENANT, &target],
        )
        .await
        .unwrap();
    }
    let i = anon
        .api("action=query&prop=info&titles=Project:Index")
        .await;
    assert!(i["query"]["pages"][0].get("missing").is_none(), "{i}");

    // Lifting a reservation before anyone creates the page frees the title.
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Free&protections=create%3Dbot&token={t}"
        ))
        .await;
    let held = p["protect"]["pageid"].as_u64().unwrap();
    c.api_post(&format!(
        "action=protect&title=Project:Free&protections=create%3Dall&token={t}"
    ))
    .await;
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Free&text=Open.&token={t}"
        ))
        .await;
    assert_ne!(e["edit"]["pageid"], held, "{e}");

    // A deleted page takes no read protection; its title can be reserved, and the
    // reservation then keeps the page from being undeleted into it.
    c.api_post(&format!(
        "action=edit&title=Project:Gone&text=Bye.&token={t}"
    ))
    .await;
    c.api_post(&format!("action=delete&title=Project:Gone&token={t}"))
        .await;
    let p = c
        .api_post(&format!(
            "action=protect&title=Project:Gone&protections=read%3Downer&token={t}"
        ))
        .await;
    assert!(p["protect"]["pageid"].is_u64(), "{p}");
    let r = c
        .api_post(&format!("action=undelete&title=Project:Gone&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "ts-title-taken", "{r}");

    // The log events.
    let l = anon.api("action=query&list=logevents&lelimit=50").await;
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
    for want in [
        "protect/protect",
        "protect/modify",
        "delete/delete",
        "delete/restore",
    ] {
        assert!(events.iter().any(|e| e == want), "{want} in {events:?}");
    }

    // Hide a revision's text and summary: `texthidden` and `commenthidden` in the page's
    // history, for everyone.
    let e = c
        .api_post(&format!(
            "action=edit&title=Project:Scope&text=Libel.&token={t}"
        ))
        .await;
    let libel = e["edit"]["newrevid"].as_u64().unwrap();
    let h = c
        .api_post(&format!("action=revisiondelete&type=revision&ids={libel}&hide=content%7Ccomment&reason=Libel&token={t}"))
        .await;
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true, "{h}");
    assert_eq!(h["revisiondelete"]["items"][0]["commenthidden"], true);
    let r = anon
        .api(&format!(
            "action=query&prop=revisions&revids={libel}&rvprop=content%7Ccomment%7Cids"
        ))
        .await;
    let rev = &r["query"]["pages"][0]["revisions"][0];
    assert_eq!(rev["texthidden"], true, "{r}");
    assert_eq!(rev["commenthidden"], true);
    assert!(
        rev.get("content").is_none() && rev.get("slots").is_none(),
        "{rev}"
    );
    // Showing the comment again leaves the text hidden.
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={libel}&show=comment&token={t}"
        ))
        .await;
    assert!(
        h["revisiondelete"]["items"][0]
            .get("commenthidden")
            .is_none(),
        "{h}"
    );
    assert_eq!(h["revisiondelete"]["items"][0]["texthidden"], true);
    let l = anon
        .api("action=query&list=logevents&letype=delete&lelimit=50")
        .await;
    assert!(
        l["query"]["logevents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "revision"),
        "{l}"
    );

    // Suppressing: the writer marks the record, so the bit and the `suppress/revision`
    // event follow the tenant's suppression group without the projections reading it.
    let h = c
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={libel}&hide=user&suppress=yes&token={t}"
        ))
        .await;
    assert!(h.get("error").is_none(), "{h}");
    let r = c
        .api(&format!(
            "action=query&prop=revisions&revids={libel}&rvprop=user%7Cids"
        ))
        .await;
    assert_eq!(
        r["query"]["pages"][0]["revisions"][0]["suppressed"], true,
        "{r}"
    );
    // The suppression log is the suppression group's (09 §6.5); the owner is not in it.
    let l = c
        .api("action=query&list=logevents&letype=suppress&lelimit=50")
        .await;
    assert_eq!(l["query"]["logevents"], json!([]), "{l}");
    grant(&app, "Alice", "suppress").await;
    let l = c
        .api("action=query&list=logevents&letype=suppress&lelimit=50")
        .await;
    assert!(
        l["query"]["logevents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["action"] == "revision"),
        "{l}"
    );

    // A thread deleted through action=delete answers 410 on the thread route.
    let r = c
        .rest("POST", "/thread", Some(json!({"subject": "Q", "target": {"kind": "page", "id": fresh}, "text": "Hi", "token": token})))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let title = r.body["title"].as_str().unwrap().to_string();
    let base = r.body["base"].as_u64().unwrap();
    let read = c.rest("GET", &format!("/thread/{thread}"), None).await;
    let first = read.body["posts"][0]["id"].as_u64().unwrap();

    // Protecting the home talk page stops posting there.
    let home = read.body["home"]["talk_page_id"]
        .as_u64()
        .unwrap_or_else(|| panic!("a home talk page in {}", read.body));
    let p = c
        .api_post(&format!(
            "action=protect&pageid={home}&protections=edit%3Dbot&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/post"),
            Some(json!({"inReplyTo": first, "text": "Me too", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    assert_eq!(r.body["code"], "protectedpage", "{}", r.body);
    c.api_post(&format!(
        "action=protect&pageid={home}&protections=edit%3Dall&token={t}"
    ))
    .await;
    let r = c
        .rest(
            "POST",
            &format!("/thread/{thread}/post"),
            Some(json!({"inReplyTo": first, "text": "Me too", "base": base, "token": token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let d = c
        .api_post(&format!(
            "action=delete&title={}&token={t}",
            enc(&format!("Thread:{title}"))
        ))
        .await;
    assert!(d.get("error").is_none(), "{d}");
    assert_eq!(
        anon.rest("GET", &format!("/thread/{thread}"), None)
            .await
            .status,
        StatusCode::GONE
    );
}

/// Confidentiality holds against administrators outside the group (0056 A18; 09 §4.5,
/// §5.2, §5.3): a sysop outside `board` can neither find, delete, unprotect nor
/// revision-delete a board page; deleting it keeps `board` beside the deletion group, so
/// the deletion is invisible outside `board` and undeleting restores the restriction; and
/// `security.restrictions = none` refuses a new confidential restriction.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn confidentiality_holds_against_administrators_outside_the_group() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("outsiders").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let bt = enc(&bob.csrf().await);

    let e = alice
        .api_post(&format!(
            "action=edit&title=Project:Minutes&text=Secret+minutes.&token={t}"
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().unwrap();
    let revid = e["edit"]["newrevid"].as_u64().unwrap();
    let p = alice
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=read%3Dboard&token={t}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let missing = |r: &Value| r["query"]["pages"][0]["missing"] == true;
    assert!(missing(
        &bob.api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // Bob holds delete, protect and deleterevision, and none of them reaches the page.
    let r = bob
        .api_post(&format!("action=delete&title=Project:Minutes&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");
    let r = bob
        .api_post(&format!("action=delete&pageid={page}&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "nosuchpageid", "{r}");
    let r = bob
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=read%3Dall&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    let r = bob
        .api_post(&format!(
            "action=protect&pageid={page}&protections=read%3Dall&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "nosuchpageid", "{r}");
    let r = bob
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={revid}&hide=content&token={bt}"
        ))
        .await;
    assert!(r.to_string().contains("nosuchrevid"), "{r}");
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // Alice deletes it. The deletion is not shown outside board, Bob cannot undelete
    // what he cannot see, and undeleting brings the restriction back.
    let d = alice
        .api_post(&format!("action=delete&title=Project:Minutes&token={t}"))
        .await;
    assert!(d.get("error").is_none(), "{d}");
    let logs = |r: &Value| r["query"]["logevents"].to_string();
    for who in [&mut anon, &mut bob] {
        let l = who.api("action=query&list=logevents&lelimit=500").await;
        assert!(!logs(&l).contains("Minutes"), "{l}");
    }
    let l = carol.api("action=query&list=logevents&lelimit=500").await;
    assert!(logs(&l).contains("Minutes"), "{l}");
    let r = bob
        .api_post(&format!("action=undelete&title=Project:Minutes&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "cantundelete", "{r}");
    let u = alice
        .api_post(&format!("action=undelete&title=Project:Minutes&token={t}"))
        .await;
    assert!(u.get("error").is_none(), "{u}");
    for who in [&mut anon, &mut bob] {
        assert!(missing(
            &who.api("action=query&prop=info&titles=Project:Minutes")
                .await
        ));
    }
    assert!(!missing(
        &carol
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));

    // security.restrictions = none: no new confidential restriction, but protecting
    // `edit` on a page that already has one keeps it and is allowed.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let set = |v: &'static str| {
        let db = &db;
        async move {
            db.execute(
                "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
                 VALUES ('', 'tenancy', 'security.restrictions', $1, 0)
                 ON CONFLICT (tenant, kind, code) DO UPDATE SET config = EXCLUDED.config",
                &[&json!({"kind": "tenancy", "value": v})],
            )
            .await
            .unwrap();
        }
    };
    set("none").await;
    alice
        .api_post(&format!(
            "action=edit&title=Project:Agenda&text=Next+time.&token={t}"
        ))
        .await;
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Agenda&protections=read%3Dboard&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-policy", "{r}");
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    assert!(missing(
        &anon
            .api("action=query&prop=info&titles=Project:Minutes")
            .await
    ));
    set("any").await;
}

/// Moderation records stay with what they are about (09 §6.3, §6.5): a suppression's log
/// event is for the suppression group alone; a revision-deletion event on a confidential
/// page is absent with the page; and a hidden username is attributed to nobody outside the
/// group, in every feed and in lookups by name.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn moderation_records_stay_with_their_targets() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("records").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let events = |r: &Value| -> Vec<(String, u64)> {
        r["query"]["logevents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                (
                    format!(
                        "{}/{}",
                        e["type"].as_str().unwrap_or("?"),
                        e["action"].as_str().unwrap_or("?")
                    ),
                    e["logid"].as_u64().unwrap_or(0),
                )
            })
            .collect()
    };

    // Suppressing a revision's comment: `suppress/revision`, for the suppression group.
    let e = alice
        .api_post(&format!(
            "action=edit&title=Project:Open&text=Hello.&summary=Abuse&token={t}"
        ))
        .await;
    let open = e["edit"]["newrevid"].as_u64().unwrap();
    let h = alice
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={open}&hide=comment&suppress=yes&token={t}"
        ))
        .await;
    assert!(h.get("error").is_none(), "{h}");
    for who in [&mut anon, &mut bob, &mut alice] {
        let l = who.api("action=query&list=logevents&lelimit=500").await;
        assert!(
            !events(&l).iter().any(|(k, _)| k.starts_with("suppress/")),
            "{l}"
        );
    }
    grant(&app, "Alice", "suppress").await;
    let l = alice.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        events(&l).iter().any(|(k, _)| k == "suppress/revision"),
        "{l}"
    );

    // Hiding a revision of a confidential page: the event goes with the page.
    let e = alice
        .api_post(&format!(
            "action=edit&title=Project:Minutes&text=Secret.&token={t}"
        ))
        .await;
    let minutes = e["edit"]["newrevid"].as_u64().unwrap();
    alice
        .api_post(&format!(
            "action=protect&title=Project:Minutes&protections=read%3Dboard&token={t}"
        ))
        .await;
    let h = alice
        .api_post(&format!(
            "action=revisiondelete&type=revision&ids={minutes}&hide=comment&token={t}"
        ))
        .await;
    assert!(h.get("error").is_none(), "{h}");
    let l = carol.api("action=query&list=logevents&lelimit=500").await;
    let revdel: Vec<u64> = events(&l)
        .into_iter()
        .filter(|(k, _)| k == "delete/revision")
        .map(|(_, id)| id)
        .collect();
    assert_eq!(revdel.len(), 1, "{l}");
    for who in [&mut anon, &mut bob] {
        let l = who.api("action=query&list=logevents&lelimit=500").await;
        assert!(!events(&l).iter().any(|(_, id)| *id == revdel[0]), "{l}");
    }

    // Hiding Carol's name, as `suppress/hide-user` will: nothing writes actor ACLs yet, so
    // the row goes into view.acl directly.
    let ct = enc(&carol.csrf().await);
    carol
        .api_post(&format!(
            "action=edit&title=Project:Carol&text=Mine.&token={ct}"
        ))
        .await;
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    let key: String = db
        .query_one(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = 'Carol'",
            &[&TENANT],
        )
        .await
        .unwrap()
        .get(0);
    db.execute(
        "INSERT INTO view.acl (tenant, target, restrictions, read_kind, extra, \"offset\")
         VALUES ($1, $2, $3, 'moderation', NULL, 0)",
        &[
            &TENANT,
            &format!("acl:actor:{key}"),
            &json!({"read": {"group": "suppress"}}),
        ],
    )
    .await
    .unwrap();
    let rc = "action=query&list=recentchanges&rcprop=title%7Cuser&rclimit=500";
    for who in [&mut anon, &mut bob] {
        let r = who.api(rc).await;
        assert!(!r.to_string().contains("\"user\":\"Carol\""), "{r}");
        assert!(r.to_string().contains("\"userhidden\":true"), "{r}");
        let u = who
            .api("action=query&list=usercontribs&ucuser=Carol&uclimit=50")
            .await;
        assert_eq!(u["query"]["usercontribs"], json!([]), "{u}");
    }
    // Alice is in `suppress` now: the name is hers to see.
    let r = alice.api(rc).await;
    assert!(r.to_string().contains("\"user\":\"Carol\""), "{r}");
    let u = alice
        .api("action=query&list=usercontribs&ucuser=Carol&uclimit=50")
        .await;
    assert!(
        !u["query"]["usercontribs"].as_array().unwrap().is_empty(),
        "{u}"
    );
}

/// `/acl/{kind}/{id}` (0016 §7; 0056 §13; 18 §3.2): a namespace and a page ACL set, read
/// and retired over REST; a namespace confidential to `board` hides its pages from
/// everyone outside it, the owner among them unless it has joined; an outsider cannot see,
/// change or retire the ACL; the refusals.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn acl_routes_set_read_and_retire() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("aclroutes").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let at = alice.csrf().await;
    let bt = bob.csrf().await;
    let e = alice
        .api_post(&format!(
            "action=edit&title=Project:Index&text=Hello.&token={t}"
        ))
        .await;
    let page = e["edit"]["pageid"].as_u64().unwrap();
    let missing = |r: &Value| r["query"]["pages"][0]["missing"] == true;
    let info = "action=query&prop=info&titles=Project:Index";

    // A namespace confidential to board.
    let r = alice
        .rest(
            "PUT",
            "/acl/namespace/4",
            Some(json!({"restrictions": {"read": {"group": "board"}}, "reason": "Board only", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(
        r.body["visibility"]["groups"],
        json!(["board"]),
        "{}",
        r.body
    );
    assert!(r.body["logid"].is_u64(), "{}", r.body);
    for who in [&mut anon, &mut bob] {
        assert!(missing(&who.api(info).await));
        let g = who.rest("GET", "/acl/namespace/4", None).await;
        assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
        assert_eq!(g.body["code"], "ts-no-target");
    }
    assert!(!missing(&carol.api(info).await));
    let g = carol.rest("GET", "/acl/namespace/4", None).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(g.body["acl"]["read_kind"], "confidential", "{}", g.body);

    // Bob holds protect and still cannot touch it: it does not exist for him.
    let r = bob
        .rest("DELETE", "/acl/namespace/4", Some(json!({"token": bt})))
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let r = bob
        .rest(
            "PUT",
            "/acl/namespace/4",
            Some(json!({"restrictions": {"edit": {"group": "sysop"}}, "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);

    // Alice retires it.
    let r = alice
        .rest(
            "DELETE",
            "/acl/namespace/4",
            Some(json!({"reason": "Open again", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["acl"], Value::Null, "{}", r.body);
    assert!(!missing(&anon.api(info).await));
    let r = alice
        .rest("DELETE", "/acl/namespace/4", Some(json!({"token": at})))
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.body["code"], "ts-no-acl");

    // A page protected over REST, as action=protect would.
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"edit": {"group": "sysop", "expiry": "infinite"}}, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let ct = enc(&carol.csrf().await);
    let e = carol
        .api_post(&format!(
            "action=edit&title=Project:Index&text=Mine.&token={ct}"
        ))
        .await;
    assert_eq!(e["error"]["code"], "protectedpage", "{e}");
    let g = anon.rest("GET", &format!("/acl/page/{page}"), None).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(g.body["acl"]["restrictions"]["edit"]["group"], "sysop");
    assert_eq!(g.body["visibility"]["public"], true);

    // Refusals.
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"edit": {"group": "nosuchgroup"}}, "token": at})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-unknown-group", "{}", r.body);
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"read": {"group": "suppress"}}, "token": at})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-locked-out", "{}", r.body);
    let r = alice.rest("GET", "/acl/entity/Q1", None).await;
    assert_eq!(r.body["code"], "ts-no-target", "no Q1 here: {}", r.body);
    let r = alice.rest("GET", "/acl/entity/WDQ42", None).await;
    assert_eq!(r.body["code"], "ts-not-supported", "mirrored: {}", r.body);
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"edit": {"group": "sysop"}}})),
        )
        .await;
    assert_eq!(r.body["code"], "badtoken", "{}", r.body);
    alice
        .api_post(&format!("action=delete&title=Project:Index&token={t}"))
        .await;
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"edit": {"group": "sysop"}}, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.body);
    assert_eq!(r.body["code"], "ts-deleted");
}

/// Subpages and sets (09 §4.4, §4.7): a confidential page hides its subpages, and a create
/// under it is a restricted title; a confidential set hides its members and their
/// subpages, its own log events, and itself; members come and go with the record.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn subpages_and_sets_enclose() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("sets").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let at = alice.csrf().await;
    let bt = enc(&bob.csrf().await);
    let mut ids = std::collections::HashMap::new();
    for title in ["Board", "Board/Minutes", "Other", "Other/Sub", "Public"] {
        let e = alice
            .api_post(&format!(
                "action=edit&title={}&text=Text.&token={t}",
                enc(&format!("Project:{title}"))
            ))
            .await;
        ids.insert(title, e["edit"]["pageid"].as_u64().unwrap());
    }
    let missing = |r: &Value| r["query"]["pages"][0]["missing"] == true;
    let info = |title: &str| {
        format!(
            "action=query&prop=info&titles={}",
            enc(&format!("Project:{title}"))
        )
    };

    // A confidential page encloses its subpages.
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{}", ids["Board"]),
            Some(json!({"restrictions": {"read": {"group": "board"}}, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for who in [&mut anon, &mut bob] {
        assert!(missing(&who.api(&info("Board/Minutes")).await));
    }
    assert!(!missing(&carol.api(&info("Board/Minutes")).await));
    let e = bob
        .api_post(&format!(
            "action=edit&title={}&text=Peek.&token={bt}",
            enc("Project:Board/New")
        ))
        .await;
    assert_eq!(e["error"]["code"], "ts-restricted-title", "{e}");
    let r = alice
        .rest(
            "DELETE",
            &format!("/acl/page/{}", ids["Board"]),
            Some(json!({"token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!missing(&anon.api(&info("Board/Minutes")).await));

    // A confidential set.
    let r = alice
        .rest(
            "POST",
            "/acl/set",
            Some(json!({
                "name": "Board papers",
                "restrictions": {"read": {"group": "board"}},
                "members": [{"kind": "page", "id": ids["Other"]}],
                "reason": "Papers",
                "token": at,
            })),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let set = r.body["id"].as_u64().expect("a set ID");
    let created = r.body["logid"].as_u64().unwrap();
    for who in [&mut anon, &mut bob] {
        assert!(missing(&who.api(&info("Other")).await));
        assert!(missing(&who.api(&info("Other/Sub")).await));
        assert!(!missing(&who.api(&info("Public")).await));
        let g = who.rest("GET", &format!("/acl/set/{set}"), None).await;
        assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
        let l = who.api("action=query&list=logevents&lelimit=500").await;
        assert!(
            !l.to_string().contains(&format!("\"logid\":{created}")),
            "{l}"
        );
    }
    assert!(!missing(&carol.api(&info("Other/Sub")).await));
    let g = carol.rest("GET", &format!("/acl/set/{set}"), None).await;
    assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    assert_eq!(g.body["acl"]["name"], "Board papers");
    let l = carol.api("action=query&list=logevents&lelimit=500").await;
    assert!(
        l.to_string().contains(&format!("\"logid\":{created}")),
        "{l}"
    );

    // Members come and go; Bob cannot touch a set he cannot see.
    let r = alice
        .rest(
            "POST",
            &format!("/acl/set/{set}/members"),
            Some(json!({"kind": "page", "id": ids["Public"], "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(missing(&anon.api(&info("Public")).await));
    let r = alice
        .rest(
            "POST",
            &format!("/acl/set/{set}/members"),
            Some(json!({"kind": "page", "id": ids["Public"], "token": at})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-already-member", "{}", r.body);
    let r = alice
        .rest(
            "POST",
            &format!("/acl/set/{set}/members"),
            Some(json!({"kind": "entity", "id": "Q1", "token": at})),
        )
        .await;
    assert_eq!(r.body["code"], "ts-no-target", "no Q1 here: {}", r.body);
    let bob_token = bob.csrf().await;
    let r = bob
        .rest(
            "POST",
            &format!("/acl/set/{set}/members"),
            Some(json!({"kind": "page", "id": ids["Board"], "token": bob_token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let r = alice
        .rest(
            "DELETE",
            &format!("/acl/set/{set}/members/page/{}", ids["Public"]),
            Some(json!({"token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!missing(&anon.api(&info("Public")).await));
    assert!(missing(&anon.api(&info("Other")).await));

    // Retiring the set frees its members.
    let r = alice
        .rest(
            "DELETE",
            &format!("/acl/set/{set}"),
            Some(json!({"token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert!(!missing(&anon.api(&info("Other/Sub")).await));
    let g = alice.rest("GET", &format!("/acl/set/{set}"), None).await;
    assert_eq!(g.status, StatusCode::NOT_FOUND, "{}", g.body);
}

/// A private tenant (09 §4.7; 18 §2.2): with its `tenant` ACL restricted to a group,
/// everything of it, pages, feeds, entities and search, is absent for those outside the
/// group, who may not write to it either; `siteinfo` tells them its name and that it is
/// private, and nothing else; its ACL is public; `security.restrictions = tenant` allows
/// it but not a confidential page.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn a_private_tenant_is_absent_for_outsiders() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("private").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    grant(&app, "Carol", "board").await;
    let t = enc(&alice.csrf().await);
    let at = alice.csrf().await;
    alice
        .api_post(&format!(
            "action=edit&title=Project:Index&text=Hello.&token={t}"
        ))
        .await;
    let item = alice
        .api_post(&format!(
            "action=wbeditentity&new=item&data={}&token={t}",
            enc(r#"{"labels":{"en":{"language":"en","value":"Thingamajig"}}}"#)
        ))
        .await;
    let q = item["entity"]["id"].as_str().expect("an item").to_string();

    // security.restrictions = tenant: the tenant may be private, a page may not be.
    let (db, conn) = database
        .parse::<Config>()
        .unwrap()
        .connect(NoTls)
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.await;
    });
    db.execute(
        "INSERT INTO view.registry (tenant, kind, code, config, \"offset\")
         VALUES ('', 'tenancy', 'security.restrictions', $1, 0)
         ON CONFLICT (tenant, kind, code) DO UPDATE SET config = EXCLUDED.config",
        &[&json!({"kind": "tenancy", "value": "tenant"})],
    )
    .await
    .unwrap();
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Index&protections=read%3Dboard&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-policy", "{r}");
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/tenant/{TENANT}"),
            Some(json!({"restrictions": {"read": {"group": "board"}}, "reason": "Private", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let bob_token = bob.csrf().await;
    let r = bob
        .rest(
            "DELETE",
            &format!("/acl/tenant/{TENANT}"),
            Some(json!({"token": bob_token})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);

    let get_q = format!("action=wbgetentities&ids={q}");
    let search = "action=wbsearchentities&search=Thingamajig&language=en";
    let si = "action=query&meta=siteinfo&siprop=general%7Ctriplespace%7Cnamespaces";
    for who in [&mut anon, &mut bob] {
        let i = who.api("action=query&prop=info&titles=Project:Index").await;
        assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
        let rc = who.api("action=query&list=recentchanges&rclimit=50").await;
        assert_eq!(rc["query"]["recentchanges"], json!([]), "{rc}");
        let e = who.api(&get_q).await;
        assert!(e["entities"][&q].get("missing").is_some(), "{e}");
        let s = who.api(search).await;
        assert_eq!(s["search"], json!([]), "{s}");
        let h = who.rest("GET", &format!("/entity/{q}/history"), None).await;
        assert_eq!(h.status, StatusCode::NOT_FOUND, "{}", h.body);
        let s = who.api(si).await;
        assert_eq!(s["query"]["general"]["private"], true, "{s}");
        assert!(s["query"]["general"]["sitename"].is_string(), "{s}");
        assert!(s["query"]["general"].get("generator").is_none(), "{s}");
        assert!(s["query"].get("namespaces").is_none(), "{s}");
        assert_eq!(s["query"]["triplespace"]["visibility"], "private", "{s}");
        assert!(
            s["query"]["triplespace"].get("capabilities").is_none(),
            "{s}"
        );
        let g = who
            .rest("GET", &format!("/acl/tenant/{TENANT}"), None)
            .await;
        assert_eq!(g.status, StatusCode::OK, "{}", g.body);
    }
    let bt = enc(&bob.csrf().await);
    let w = bob
        .api_post(&format!(
            "action=wbeditentity&new=item&data={}&token={bt}",
            enc(r#"{"labels":{"en":{"language":"en","value":"Intruder"}}}"#)
        ))
        .await;
    assert_eq!(w["error"]["code"], "permissiondenied", "{w}");
    let w = bob
        .api_post(&format!(
            "action=edit&title=Project:Index&text=Mine.&token={bt}"
        ))
        .await;
    assert_eq!(w["error"]["code"], "ts-restricted-title", "{w}");

    // Carol, a reader, sees all of it, and is told the wiki is private.
    let e = carol.api(&get_q).await;
    assert!(e["entities"][&q].get("missing").is_none(), "{e}");
    let s = carol.api(search).await;
    assert_eq!(s["search"][0]["id"], q.as_str(), "{s}");
    let s = carol.api(si).await;
    assert!(s["query"].get("namespaces").is_some(), "{s}");
    assert_eq!(s["query"]["triplespace"]["visibility"], "private", "{s}");

    // Retiring the restriction makes it public again.
    let r = alice
        .rest(
            "DELETE",
            &format!("/acl/tenant/{TENANT}"),
            Some(json!({"token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let e = anon.api(&get_q).await;
    assert!(e["entities"][&q].get("missing").is_none(), "{e}");
    let s = anon.api(si).await;
    assert_eq!(s["query"]["triplespace"]["visibility"], "public", "{s}");
    db.execute(
        "UPDATE view.registry SET config = $1 WHERE tenant = '' AND kind = 'tenancy' AND code = 'security.restrictions'",
        &[&json!({"kind": "tenancy", "value": "any"})],
    )
    .await
    .unwrap();
}

/// Rebuilding one partition replays only the ACLs it holds: a tenant ACL (config) survives
/// a rebuild of the tenant log, and a page ACL (log) survives one of the config partition.
#[tokio::test(flavor = "multi_thread")]
async fn a_partition_rebuild_keeps_the_other_partitions_acls() {
    use scatter_ingest::store::IngestStore as _;
    use scatter_projection::Backend as _;
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("rebuild").await;
    grant(&app, "Alice", "board").await;
    let t = enc(&alice.csrf().await);
    let at = alice.csrf().await;
    alice
        .api_post(&format!(
            "action=edit&title=Project:Index&text=Hello.&token={t}"
        ))
        .await;
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Index&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/tenant/{TENANT}"),
            Some(json!({"restrictions": {"read": {"group": "board"}}, "reason": "Private", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);

    let connect = || async {
        let (c, conn) = database
            .parse::<Config>()
            .unwrap()
            .connect(NoTls)
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.await;
        });
        c
    };
    let db = connect().await;
    let targets = || async {
        let mut v: Vec<String> = db
            .query("SELECT target FROM view.acl WHERE tenant = $1", &[&TENANT])
            .await
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect();
        v.sort();
        v
    };
    let before = targets().await;
    assert_eq!(before.len(), 2, "{before:?}");
    let (log, config) = {
        let store = app.store();
        let mut cx = store.begin().await.unwrap();
        let log = store
            .partition(&mut cx, TENANT, "log")
            .await
            .unwrap()
            .unwrap();
        let config = store
            .partition(&mut cx, TENANT, "config")
            .await
            .unwrap()
            .unwrap();
        store.rollback(cx).await.unwrap();
        (log, config)
    };
    for partition in [log, config] {
        let log_store = scatter_log_postgres::PgLog::new(connect().await);
        app.pipeline()
            .rebuild(app.store(), &log_store, &[partition], 500)
            .await
            .unwrap();
        assert_eq!(targets().await, before, "after rebuilding {partition:#x}");
    }
}

impl Client {
    async fn userrights(&mut self, form: &str) -> Value {
        let t = self
            .api("action=query&meta=tokens&type=userrights")
            .await["query"]["tokens"]["userrightstoken"]
            .as_str()
            .unwrap()
            .to_string();
        self.api_post(&format!("action=userrights&{form}&token={}", enc(&t)))
            .await
    }
}

/// Delegated administration (09 §3.2–3.6; 0016 A35–A38): `action=userrights` under
/// `userrights` and under a group's delegation lists, group definitions over `/group`,
/// a scoped group's rights holding only within its scope, `read-all` and `owner` reading
/// everything but the suppression group's, the last owner kept, and the CLI appointing one.
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn delegated_administration() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (database, app, mut alice) = instance("delegation").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut dave = account(&alice.router, "Dave", "dave-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };

    // The owner holds userrights: Bob becomes a bureaucrat, publicly.
    let r = alice
        .userrights("user=Bob&add=bureaucrat&reason=Trusted")
        .await;
    assert_eq!(r["userrights"]["added"], json!(["bureaucrat"]), "{r}");
    assert_eq!(r["userrights"]["user"], "Bob");
    let r = alice
        .api_post("action=userrights&user=Bob&add=sysop&token=x")
        .await;
    assert_eq!(
        r["error"]["code"], "badtoken",
        "the userrights token, not csrf: {r}"
    );
    let r = carol.userrights("user=Dave&add=sysop").await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = bob.userrights("user=Dave&add=owner").await;
    assert_eq!(
        r["error"]["code"], "permissiondenied",
        "only owners make owners: {r}"
    );
    let r = bob.userrights("user=Dave&add=user").await;
    assert_eq!(r["error"]["code"], "permissiondenied", "implicit: {r}");
    let r = bob.userrights("user=Dave&add=nonesuch").await;
    assert_eq!(r["error"]["code"], "badvalue", "{r}");
    // A span from now is an expiry too (MediaWiki's forms take them), and `list=users`
    // reports the memberships with their expiries: what `Special:UserRights` shows.
    let r = bob
        .userrights("user=Dave&add=sysop&expiry=1%20week&reason=Trial")
        .await;
    assert_eq!(r["userrights"]["added"], json!(["sysop"]), "{r}");
    let u = anon
        .api("action=query&list=users&ususers=Dave|Nobody&usprop=groups|groupmemberships|blockinfo")
        .await;
    let users = u["query"]["users"].as_array().unwrap();
    assert_eq!(users[0]["name"], "Dave", "{u}");
    assert!(
        users[0]["groups"]
            .as_array()
            .unwrap()
            .contains(&json!("sysop")),
        "{u}"
    );
    let m = &users[0]["groupmemberships"][0];
    assert_eq!(m["group"], "sysop", "{u}");
    assert!(
        m["expiry"].as_str().unwrap().starts_with("20") && m["expiry"] != "infinity",
        "a week from now: {u}"
    );
    assert!(users[0].get("blockexpiry").is_none(), "{u}");
    assert_eq!(users[1]["missing"], true, "{u}");
    let r = bob.userrights("user=Dave&add=board&expiry=soon").await;
    assert_eq!(r["error"]["code"], "invalidexpiry", "{r}");
    let r = bob.userrights("user=Dave&remove=sysop").await;
    assert_eq!(r["userrights"]["removed"], json!(["sysop"]), "{r}");
    let g = anon
        .api("action=query&meta=siteinfo&siprop=usergroups")
        .await;
    let groups = g["query"]["usergroups"].as_array().unwrap();
    assert!(
        groups
            .iter()
            .any(|g| g["name"] == "user" && g.get("implicit").is_some()),
        "implicit groups are marked: {g}"
    );
    assert!(
        groups
            .iter()
            .any(|g| g["name"] == "sysop" && g.get("implicit").is_none()),
        "{g}"
    );
    let l = anon.api("action=query&list=logevents&letype=rights").await;
    let bobs = l["query"]["logevents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["params"]["group"] == "bureaucrat")
        .cloned()
        .unwrap_or_default();
    assert_eq!(bobs["title"], "User:Bob", "titled by name, not key: {l}");

    // Bob, a bureaucrat, defines a department: hr-admins run hr's membership, and may
    // delete and protect in the Project namespace only.
    let bt = bob.csrf().await;
    let r = bob
        .rest(
            "PUT",
            "/group/hr",
            Some(json!({"permissions": [], "reason": "HR", "token": bt})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["source"], "tenant");
    let def = json!({"permissions": ["delete", "protect"], "within": ["namespace:4"], "adds": ["hr"], "removes": ["hr"], "token": bt});
    let r = bob.rest("PUT", "/group/hr-admins", Some(def.clone())).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["group"]["within"], json!(["namespace:4"]));
    for bad in [
        json!({"permissions": ["block"], "within": ["namespace:4"], "token": bt}),
        json!({"permissions": [], "adds": ["bureaucrat"], "token": bt}),
    ] {
        let r = bob.rest("PUT", "/group/badly", Some(bad)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.body);
        assert_eq!(r.body["code"], "ts-invalid-group");
    }
    let ct = carol.csrf().await;
    let r = carol
        .rest(
            "PUT",
            "/group/hr",
            Some(json!({"permissions": ["delete"], "token": ct})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
    let s = anon
        .api("action=query&meta=siteinfo&siprop=usergroups")
        .await;
    let hra = s["query"]["usergroups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["name"] == "hr-admins")
        .cloned()
        .unwrap();
    assert_eq!(hra["add"], json!(["hr"]), "{hra}");

    // Carol, an hr-admin, adds others to hr, and to nothing else, and not herself.
    let r = bob.userrights("user=Carol&add=hr-admins").await;
    assert_eq!(r["userrights"]["added"], json!(["hr-admins"]), "{r}");
    let r = carol.userrights("user=Dave&add=hr").await;
    assert_eq!(r["userrights"]["added"], json!(["hr"]), "{r}");
    for form in ["user=Dave&add=sysop", "user=Carol&add=hr"] {
        let r = carol.userrights(form).await;
        assert_eq!(r["error"]["code"], "permissiondenied", "{form}: {r}");
    }
    let r = carol.userrights("user=Dave&remove=hr").await;
    assert_eq!(r["userrights"]["removed"], json!(["hr"]), "{r}");
    let u = carol
        .api("action=query&meta=userinfo&uiprop=rights|changeablegroups")
        .await;
    let rights = &u["query"]["userinfo"]["rights"];
    assert!(
        !rights.as_array().unwrap().contains(&json!("delete")),
        "{u}"
    );
    assert_eq!(
        u["query"]["userinfo"]["ts-scopedrights"]["delete"],
        json!(["namespace:4"]),
        "{u}"
    );
    assert_eq!(
        u["query"]["userinfo"]["changeablegroups"]["add"],
        json!(["hr"]),
        "{u}"
    );

    // Her rights hold in the Project namespace and nowhere else.
    let at = enc(&alice.csrf().await);
    for title in ["Project:Handbook", "User:Alice", "Project:Other"] {
        let e = alice
            .api_post(&format!("action=edit&title={title}&text=Text.&token={at}"))
            .await;
        assert_eq!(e["edit"]["result"], "Success", "{title}: {e}");
    }
    let ct = enc(&ct);
    let r = carol
        .api_post(&format!("action=delete&title=Project:Handbook&token={ct}"))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = carol
        .api_post(&format!("action=delete&title=User:Alice&token={ct}"))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = carol
        .api_post(&format!(
            "action=protect&title=Project:Other&protections=edit%3Dsysop&token={ct}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = carol
        .api_post(&format!(
            "action=protect&title=User:Alice&protections=edit%3Dsysop&token={ct}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");

    // read-all and owner read every confidential target but the suppression group's.
    let r = bob.userrights("user=Dave&add=read-all").await;
    assert_eq!(r["userrights"]["added"], json!(["read-all"]), "{r}");
    grant(&app, "Alice", "board").await;
    alice
        .api_post(&format!(
            "action=protect&title=Project:Other&protections=read%3Dboard&token={at}"
        ))
        .await;
    let info = "action=query&prop=info&titles=Project:Other";
    assert_eq!(anon.api(info).await["query"]["pages"][0]["missing"], true);
    assert_eq!(carol.api(info).await["query"]["pages"][0]["missing"], true);
    assert!(
        dave.api(info).await["query"]["pages"][0]
            .get("missing")
            .is_none()
    );
    let dt = enc(&dave.csrf().await);
    let r = dave
        .api_post(&format!(
            "action=edit&title=Project:Other&text=Mine.&token={dt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "protectedpage", "reading only: {r}");

    // The last owner may not leave while a right is held by no other group with a member.
    let r = alice.userrights("user=Alice&remove=owner").await;
    assert_eq!(r["error"]["code"], "ts-last-owner", "{r}");
    // The command line appoints an owner, and the second may let the first go... while
    // something still needs owner, neither may be the last to leave.
    triplespace_cli::rights::run_member(triplespace_cli::rights::Member::Add(
        triplespace_cli::rights::MemberArgs {
            at: triplespace_cli::rights::Tenant {
                database: database.clone(),
                tenant: TENANT.into(),
                reason: "Second owner".into(),
            },
            user: "Bob".into(),
            group: "owner".into(),
            expiry: None,
        },
    ))
    .await
    .unwrap();
    let r = bob.userrights("user=Alice&remove=owner").await;
    assert_eq!(r["userrights"]["removed"], json!(["owner"]), "{r}");
    let r = bob.userrights("user=Bob&remove=owner").await;
    assert_eq!(r["error"]["code"], "ts-last-owner", "{r}");
    let _ = &mut dave;
}

impl Client {
    async fn notifications(&mut self, extra: &str) -> Value {
        self.api(&format!("action=query&meta=notifications{extra}"))
            .await
    }
}

/// Accountability (09 §3.3, §4.5, §6.3; 16 §5.2; 0021 A17, A19): `rights` and
/// `group-join` notifications through Echo's modules, `action=block` and `unblock` with
/// their log events and `hidename`, and the empty-group report.
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn accountability() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, _app, mut alice) = instance("accountability").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut dave = account(&alice.router, "Dave", "dave-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    let at = alice.csrf().await;

    // A group that announces its joins; Carol is in it before Bob joins.
    let r = alice
        .rest(
            "PUT",
            "/group/board",
            Some(json!({"permissions": [], "notify_joins": true, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    for form in [
        "user=Carol&add=board",
        "user=Carol&add=sysop",
        "user=Bob&add=board&reason=Welcome",
        "user=Dave&add=sysop",
    ] {
        let r = alice.userrights(form).await;
        assert!(r.get("error").is_none(), "{form}: {r}");
    }

    let n = bob.notifications("&notprop=list|count").await;
    let list = n["query"]["notifications"]["list"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(list.len(), 1, "{n}");
    assert_eq!(list[0]["type"], "user-rights");
    assert_eq!(list[0]["agent"]["name"], "Alice");
    assert_eq!(list[0]["title"]["text"], "Bob");
    assert_eq!(list[0]["ts-params"]["group"], "board");
    assert_eq!(n["query"]["notifications"]["rawcount"], 1, "{n}");
    let n = carol.notifications("").await;
    let types: Vec<Value> = n["query"]["notifications"]["list"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["type"].clone())
        .collect();
    // Her own two rights changes, and Bob's join of a group that announces joins; nothing
    // for Dave's join of sysop, which does not, though she is in it.
    assert_eq!(
        types,
        vec![
            json!("ts-group-join"),
            json!("user-rights"),
            json!("user-rights")
        ],
        "{n}"
    );
    // Nothing of her own acts: only the instance making her owner, which names the
    // operator.
    let n = alice.notifications("").await;
    let list = n["query"]["notifications"]["list"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(list.len(), 1, "{n}");
    assert_eq!(list[0]["ts-params"]["group"], "owner");
    assert_eq!(list[0]["agent"]["ts-instance"], true);
    let r = anon.api("action=query&meta=notifications").await;
    assert_eq!(r["error"]["code"], "login-required", "{r}");
    let ct = enc(&carol.csrf().await);
    let r = carol
        .api_post(&format!("action=echomarkread&all=1&token={ct}"))
        .await;
    assert_eq!(r["echomarkread"]["rawcount"], 0, "{r}");
    let n = carol.notifications("&notfilter=!read").await;
    assert_eq!(n["query"]["notifications"]["list"], json!([]), "{n}");
    let r = carol
        .api_post(&format!("action=echomarkseen&type=all&token={ct}"))
        .await;
    assert_eq!(r["query"]["echomarkseen"]["result"], "success", "{r}");

    // Blocks: logged, enforced, and notified.
    let dt = enc(&dave.csrf().await);
    dave.api_post(&format!(
        "action=edit&title=Project:Dave&text=Mine.&token={dt}"
    ))
    .await;
    let t = enc(&at);
    let bt = enc(&bob.csrf().await);
    let r = bob
        .api_post(&format!("action=block&user=Dave&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = alice
        .api_post(&format!("action=block&user=Dave&reason=Spam&token={t}"))
        .await;
    assert_eq!(r["block"]["user"], "Dave", "{r}");
    let r = dave
        .api_post(&format!(
            "action=edit&title=Project:Dave&text=More.&token={dt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "blocked: {r}");
    let r = alice
        .api_post(&format!("action=block&user=Dave&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "alreadyblocked", "{r}");
    let r = alice
        .api_post(&format!(
            "action=block&user=Dave&reblock=1&hidename=1&expiry=2099-01-01T00:00:00Z&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ipb_expiry_temp", "{r}");
    let r = alice
        .api_post(&format!(
            "action=block&user=Dave&reblock=1&hidename=1&token={t}"
        ))
        .await;
    assert_eq!(r["block"]["hidename"], true, "{r}");
    let block_id = r["block"]["id"].as_u64().unwrap();
    let l = bob.api("action=query&list=logevents&letype=block").await;
    let actions: Vec<Value> = l["query"]["logevents"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].clone())
        .collect();
    assert_eq!(actions, vec![json!("reblock"), json!("block")], "{l}");
    let u = anon
        .api("action=query&list=usercontribs&ucuser=Dave&uclimit=50")
        .await;
    assert_eq!(
        u["query"]["usercontribs"],
        json!([]),
        "the name is hidden: {u}"
    );
    let l = anon.api("action=query&list=logevents&letype=block").await;
    assert!(
        !l.to_string().contains("Dave"),
        "the block log hides the name: {l}"
    );
    let l = anon.api("action=query&list=logevents&letype=rights").await;
    assert!(
        !l.to_string().contains("Dave"),
        "the rights log hides the name: {l}"
    );
    // `list=blocks` lists the block in force, with the name hidden from outsiders.
    let b = anon.api("action=query&list=blocks").await;
    let blocks = b["query"]["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1, "{b}");
    assert_eq!(blocks[0]["userhidden"], true, "{b}");
    assert!(blocks[0].get("user").is_none(), "{b}");
    assert_eq!(blocks[0]["expiry"], "infinite", "{b}");
    assert_eq!(blocks[0]["by"], "Alice", "{b}");
    let b = alice.api("action=query&list=blocks&bkusers=Dave").await;
    assert_eq!(
        b["query"]["blocks"],
        json!([]),
        "a hidden name finds nothing: {b}"
    );
    // The name is hidden from Alice too, who is not in `suppress`: she unblocks by the
    // block's ID.
    let r = alice
        .api_post(&format!("action=unblock&user=Dave&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "nosuchuser", "{r}");
    let r = alice
        .api_post(&format!(
            "action=unblock&id={block_id}&reason=Appeal&token={t}"
        ))
        .await;
    assert!(r["unblock"]["id"].is_u64(), "{r}");
    assert!(
        !r.to_string().contains("Dave"),
        "the answer hides the name: {r}"
    );
    // Alice holds `hideuser`, so her unblock also lifted the hide: the name is back.
    let u = anon
        .api("action=query&list=usercontribs&ucuser=Dave&uclimit=50")
        .await;
    assert!(
        u.to_string().contains("Project:Dave"),
        "the name is back after the unblock: {u}"
    );
    let r = alice
        .api_post(&format!("action=unblock&id={block_id}&token={t}"))
        .await;
    assert_eq!(r["error"]["code"], "cantunblock", "{r}");
    let r = dave
        .api_post(&format!(
            "action=edit&title=Project:Dave&text=Again.&token={dt}"
        ))
        .await;
    assert_eq!(r["edit"]["result"], "Success", "unblocked: {r}");
    let n = dave.notifications("").await;
    assert_eq!(
        n["query"]["notifications"]["list"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "sysop, block, reblock, unblock: {n}"
    );

    // The empty-group report: a confidential restriction whose group has emptied.
    let r = alice
        .rest(
            "PUT",
            "/group/vault",
            Some(json!({"permissions": [], "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    alice.userrights("user=Alice&add=vault").await;
    alice
        .api_post(&format!(
            "action=edit&title=Project:Vault&text=Gold.&token={t}"
        ))
        .await;
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Vault&protections=read%3Dvault&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = alice.rest("GET", "/acl/empty-groups", None).await;
    assert_eq!(r.body["groups"], json!([]), "{}", r.body);
    alice.userrights("user=Alice&remove=vault").await;
    let r = alice.rest("GET", "/acl/empty-groups", None).await;
    assert_eq!(
        r.body["groups"],
        json!([{"group": "vault", "restrictions": 1}]),
        "{}",
        r.body
    );
    let r = carol.rest("GET", "/acl/empty-groups", None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{}", r.body);
}

/// Entities under ACLs (09 §4.2, §5.3, §6.2; 0023 §4): a confidential item is absent on
/// every entity route for outsiders and served to the group; a deleted item is missing
/// outside the deletion group, undeletable by it; a set holds entities; `/acl/entity`;
/// writes are refused as a protected page's are.
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn entities_under_acls() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("entities").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut carol = account(&alice.router, "Carol", "carol-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Carol", "board").await;
    grant(&app, "Alice", "board").await;
    let at = alice.csrf().await;
    let t = enc(&at);
    let mut ids = Vec::new();
    for label in ["Secretive", "Goner", "Membership"] {
        let data = enc(&format!(
            r#"{{"labels":{{"en":{{"language":"en","value":"{label}"}}}}}}"#
        ));
        let r = alice
            .api_post(&format!(
                "action=wbeditentity&new=item&data={data}&token={t}"
            ))
            .await;
        ids.push(r["entity"]["id"].as_str().expect("an item").to_string());
    }
    let (secret, gone, member) = (ids[0].clone(), ids[1].clone(), ids[2].clone());

    // Confidential to board, through the entity's own ACL.
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/entity/{secret}"),
            Some(json!({"restrictions": {"read": {"group": "board"}}, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let get = |id: &str| format!("action=wbgetentities&ids={id}");
    let search = "action=wbsearchentities&search=Secret&language=en";
    let bt = enc(&bob.csrf().await);
    for who in [&mut anon, &mut bob] {
        let e = who.api(&get(&secret)).await;
        assert!(e["entities"][&secret].get("missing").is_some(), "{e}");
        let s = who.api(search).await;
        assert_eq!(s["search"], json!([]), "{s}");
        let h = who
            .rest("GET", &format!("/entity/{secret}/history"), None)
            .await;
        assert_eq!(h.status, StatusCode::NOT_FOUND, "{}", h.body);
        let d = who
            .rest("GET", &format!("/entity/{secret}/provenance"), None)
            .await;
        assert_eq!(d.status, StatusCode::NOT_FOUND, "{}", d.body);
        let i = who
            .api(&format!("action=query&prop=info&titles=Item:{secret}"))
            .await;
        assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
        let rc = who
            .api("action=query&list=recentchanges&rcprop=title&rclimit=50")
            .await;
        assert!(!rc.to_string().contains(&secret), "{rc}");
        let a = who
            .rest("GET", &format!("/acl/entity/{secret}"), None)
            .await;
        assert_eq!(a.status, StatusCode::NOT_FOUND, "{}", a.body);
    }
    // An outsider's write is a write to a missing entity; a sysop's too.
    let w = bob
        .api_post(&format!(
            "action=wbsetlabel&id={secret}&language=en&value=Pwned&token={bt}"
        ))
        .await;
    assert_eq!(w["error"]["code"], "no-such-entity", "{w}");
    let w = bob
        .api_post(&format!("action=delete&title=Item:{secret}&token={bt}"))
        .await;
    assert_eq!(w["error"]["code"], "missingtitle", "{w}");
    // The group reads and writes it.
    let e = carol.api(&get(&secret)).await;
    assert_eq!(
        e["entities"][&secret]["labels"]["en"]["value"], "Secretive",
        "{e}"
    );
    let s = carol.api(search).await;
    assert_eq!(s["search"][0]["id"], secret.as_str(), "{s}");
    let ct = enc(&carol.csrf().await);
    let w = carol
        .api_post(&format!(
            "action=wbsetlabel&id={secret}&language=en&value=Still+secret&token={ct}"
        ))
        .await;
    assert_eq!(w["success"], 1, "{w}");

    // Protection on an entity: edit restricted to sysop refuses an ordinary user.
    let r = alice
        .api_post(&format!(
            "action=protect&title=Item:{member}&protections=edit%3Dsysop&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let w = carol
        .api_post(&format!(
            "action=wbsetlabel&id={member}&language=en&value=Nope&token={ct}"
        ))
        .await;
    assert_eq!(w["error"]["code"], "protectedpage", "{w}");
    let w = bob
        .api_post(&format!(
            "action=wbsetlabel&id={member}&language=en&value=Sysop+may&token={bt}"
        ))
        .await;
    assert_eq!(w["success"], 1, "{w}");

    // A set with an entity member hides it from outsiders.
    let r = alice
        .rest(
            "POST",
            "/acl/set",
            Some(
                json!({"name": "Vault", "restrictions": {"read": {"group": "board"}},
                        "members": [{"kind": "entity", "id": member}], "token": at}),
            ),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let e = anon.api(&get(&member)).await;
    assert!(e["entities"][&member].get("missing").is_some(), "{e}");
    let e = carol.api(&get(&member)).await;
    assert!(e["entities"][&member].get("missing").is_none(), "{e}");

    // Deletion: missing outside sysop, seen as deleted within, undeleted by it.
    let r = bob
        .api_post(&format!(
            "action=delete&title=Item:{gone}&reason=Spam&token={bt}"
        ))
        .await;
    assert!(r["delete"]["logid"].is_u64(), "{r}");
    let e = anon.api(&get(&gone)).await;
    assert!(e["entities"][&gone].get("missing").is_some(), "{e}");
    let s = anon
        .api("action=wbsearchentities&search=Goner&language=en")
        .await;
    assert_eq!(s["search"], json!([]), "{s}");
    let w = carol
        .api_post(&format!(
            "action=wbsetlabel&id={gone}&language=en&value=Back&token={ct}"
        ))
        .await;
    assert_eq!(w["error"]["code"], "no-such-entity", "{w}");
    let l = anon.api("action=query&list=logevents&letype=delete").await;
    assert!(l.to_string().contains(&gone), "a deletion is public: {l}");
    let r = carol
        .api_post(&format!("action=undelete&title=Item:{gone}&token={ct}"))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = bob
        .api_post(&format!("action=undelete&title=Item:{gone}&token={bt}"))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let e = anon.api(&get(&gone)).await;
    assert_eq!(
        e["entities"][&gone]["labels"]["en"]["value"], "Goner",
        "{e}"
    );
    let r = bob
        .api_post(&format!("action=undelete&title=Item:{gone}&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "cantundelete", "{r}");
    // Found on a live instance: deleting an entity no graph holds wrote a deletion ACL.
    let r = bob
        .api_post(&format!("action=delete&title=Item:Q999999&token={bt}"))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");
    let r = bob
        .api_post(&format!(
            "action=protect&title=Item:Q999999&protections=edit%3Dsysop&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
}

/// A confidential restriction may take the talk page and its threads (09 §4.4; 0056
/// A24): `tstalk` on `action=protect`, `talk` on `/acl`; kept through delete and undelete.
#[tokio::test(flavor = "multi_thread")]
async fn a_confidential_restriction_may_take_the_talk_page() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("talk").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Alice", "board").await;
    let at = alice.csrf().await;
    let t = enc(&at);
    alice
        .api_post(&format!(
            "action=edit&title=Project:Secret&text=Hush.&token={t}"
        ))
        .await;
    let page = alice
        .api("action=query&prop=info&titles=Project:Secret")
        .await["query"]["pages"][0]["pageid"]
        .as_u64()
        .unwrap();
    let r = alice
        .rest(
            "POST",
            "/thread",
            Some(json!({"target": {"kind": "page", "id": page}, "subject": "Secret talk", "text": "4711", "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.body);
    let thread = r.body["thread"].as_u64().unwrap();
    let talk = alice
        .rest("GET", &format!("/thread/{thread}"), None)
        .await
        .body["home"]["talk_page_id"]
        .as_u64()
        .unwrap();

    // Without the flag the talk page stays public, as 09 §4.4 has it.
    alice
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Dboard&token={t}"
        ))
        .await;
    let r = anon.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    // With it, the talk page and the thread go under the restriction.
    let r = alice
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Dboard&tstalk=1&token={t}"
        ))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = anon.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let r = anon
        .rest("GET", &format!("/page/{talk}/threads"), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.body);
    let i = anon
        .api("action=query&prop=info&titles=Project_talk:Secret")
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let rc = anon
        .api("action=query&list=recentchanges&rcprop=title&rclimit=50")
        .await;
    assert!(!rc.to_string().contains("Secret"), "{rc}");
    let r = alice.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "the group reads: {}", r.body);
    // Through delete and undelete.
    alice
        .api_post(&format!("action=delete&title=Project:Secret&token={t}"))
        .await;
    let r = alice
        .api_post(&format!("action=undelete&title=Project:Secret&token={t}"))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let r = anon.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "kept: {}", r.body);
    // Over REST, and off again.
    let r = alice
        .rest(
            "PUT",
            &format!("/acl/page/{page}"),
            Some(json!({"restrictions": {"read": {"group": "board"}}, "talk": false, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    assert_eq!(r.body["acl"]["talk"], Value::Null, "{}", r.body);
    let r = anon.rest("GET", &format!("/thread/{thread}"), None).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
}

/// Moderation completeness (0023 §8; 18 §2.4): `inprop=protection` lists the restrictions
/// along the chain; `prop=deletedrevisions` serves the deletion group; `list=protectedpages`,
/// `protectedtitles` and `protectedsets` list what the caller may read; a hidden user's own
/// pages are absent for those outside the suppression group (09 §6.3).
#[tokio::test(flavor = "multi_thread")]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn moderation_completeness() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("completeness").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut mallory = account(&alice.router, "Mallory", "mallory-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Alice", "board").await;
    grant(&app, "Alice", "suppress").await;
    grant(&app, "Bob", "sysop").await;
    let at = alice.csrf().await;
    let t = enc(&at);
    for (title, text) in [("Project:Index", "Index."), ("Project:Gone", "Bye.")] {
        alice
            .api_post(&format!("action=edit&title={title}&text={text}&token={t}"))
            .await;
    }
    let mt = enc(&mallory.csrf().await);
    mallory
        .api_post(&format!(
            "action=edit&title=User:Mallory&text=Me.&token={mt}"
        ))
        .await;
    mallory
        .api_post(&format!(
            "action=edit&title=User:Mallory/Notes&text=Notes.&token={mt}"
        ))
        .await;

    // inprop=protection: the page's own, and an enclosing namespace's with its source.
    alice
        .api_post(&format!(
            "action=protect&title=Project:Index&protections=edit%3Dsysop%7Cread%3Dboard&expiry=infinite%7C2099-01-01T00:00:00Z&token={t}"
        ))
        .await;
    let r = alice
        .rest(
            "PUT",
            "/acl/namespace/4",
            Some(json!({"restrictions": {"move": {"group": "sysop"}}, "token": at})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let i = alice
        .api("action=query&prop=info&inprop=protection&titles=Project:Index")
        .await;
    let prot = i["query"]["pages"][0]["protection"]
        .as_array()
        .unwrap()
        .clone();
    let find = |kind: &str| {
        prot.iter()
            .find(|p| p["type"] == kind)
            .cloned()
            .unwrap_or_default()
    };
    assert_eq!(find("edit")["level"], "sysop", "{i}");
    assert_eq!(find("edit")["expiry"], "infinity");
    assert_eq!(find("read")["level"], "board");
    assert!(
        find("read")["expiry"].as_str().unwrap().starts_with("2099"),
        "{i}"
    );
    assert_eq!(find("move")["source"], "acl:namespace:4", "{i}");
    assert!(find("move").get("source").is_some());

    // list=protectedpages: the confidential page is listed to the group, not to others.
    let l = alice
        .api("action=query&list=protectedpages&prlimit=50")
        .await;
    assert!(l.to_string().contains("Index"), "{l}");
    let l = bob.api("action=query&list=protectedpages&prlimit=50").await;
    assert!(!l.to_string().contains("Index"), "{l}");
    let l = alice
        .api("action=query&list=protectedpages&prtype=read&prlevel=board")
        .await;
    assert_eq!(
        l["query"]["protectedpages"].as_array().unwrap().len(),
        1,
        "{l}"
    );
    // list=protectedtitles: a reservation.
    alice
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Dsysop&token={t}"
        ))
        .await;
    let l = anon.api("action=query&list=protectedtitles").await;
    assert_eq!(l["query"]["protectedtitles"][0]["level"], "sysop", "{l}");
    assert!(l.to_string().contains("Reserved"), "{l}");
    // inprop=protection on a title with no page: what it may be restricted on, and the
    // reservation's restriction, for the Protect form.
    let i = anon
        .api("action=query&prop=info&inprop=protection&titles=Project:Reserved|Project:Free")
        .await;
    let pages = i["query"]["pages"].as_array().unwrap();
    let reserved = pages
        .iter()
        .find(|p| p["title"].as_str().unwrap().ends_with("Reserved"))
        .unwrap();
    assert_eq!(reserved["missing"], true, "{i}");
    assert_eq!(
        reserved["restrictiontypes"],
        json!(["create", "read"]),
        "{i}"
    );
    assert_eq!(reserved["protection"][0]["type"], "create", "{i}");
    assert_eq!(reserved["protection"][0]["level"], "sysop", "{i}");
    let free = pages
        .iter()
        .find(|p| p["title"].as_str().unwrap().ends_with("Free"))
        .unwrap();
    assert_eq!(free["protection"], json!([]), "{i}");
    // list=protectedsets: a set and its readable members.
    let gone_id = alice
        .api("action=query&prop=info&titles=Project:Gone")
        .await["query"]["pages"][0]["pageid"]
        .clone();
    let r = alice
        .rest(
            "POST",
            "/acl/set",
            Some(
                json!({"name": "Board set", "restrictions": {"read": {"group": "board"}},
                        "members": [{"kind": "page", "id": gone_id}], "token": at}),
            ),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.body);
    let l = alice.api("action=query&list=protectedsets").await;
    assert_eq!(l["query"]["protectedsets"][0]["name"], "Board set", "{l}");
    assert!(l.to_string().contains("Gone"), "{l}");
    let l = bob.api("action=query&list=protectedsets").await;
    assert_eq!(l["query"]["protectedsets"], json!([]), "{l}");

    // prop=deletedrevisions: the deletion group reads a deleted page's revisions.
    let bt = enc(&bob.csrf().await);
    alice
        .rest(
            "DELETE",
            &format!(
                "/acl/set/{}",
                r.body["target"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("acl:set:")
            ),
            Some(json!({"token": at})),
        )
        .await;
    let r = bob
        .api_post(&format!("action=delete&title=Project:Gone&token={bt}"))
        .await;
    assert!(r.get("error").is_none(), "{r}");
    let d = bob
        .api("action=query&prop=deletedrevisions&drvprop=content%7Cuser&titles=Project:Gone")
        .await;
    assert_eq!(
        d["query"]["pages"][0]["deletedrevisions"][0]["content"], "Bye.",
        "{d}"
    );
    let d = mallory
        .api("action=query&prop=deletedrevisions&drvprop=content&titles=Project:Gone")
        .await;
    assert_eq!(d["error"]["code"], "permissiondenied", "{d}");
    let d = anon
        .api("action=query&prop=revisions&rvprop=content&titles=Project:Gone")
        .await;
    assert_eq!(d["query"]["pages"][0]["missing"], true, "{d}");
    // letitle: a deleted page's events are found by its title, by everyone for a public
    // deletion; an unknown title has none.
    let l = anon
        .api("action=query&list=logevents&letitle=Project:Gone&letype=delete")
        .await;
    assert_eq!(l["query"]["logevents"][0]["action"], "delete", "{l}");
    assert_eq!(l["query"]["logevents"][0]["pageid"], gone_id, "{l}");
    let l = anon
        .api("action=query&list=logevents&letitle=Project:Nonesuch")
        .await;
    assert_eq!(l["query"]["logevents"], json!([]), "{l}");

    // A hidden user's own pages are absent for outsiders, read by the suppression group.
    let r = alice
        .api_post(&format!("action=block&user=Mallory&hidename=1&token={t}"))
        .await;
    assert_eq!(r["block"]["hidename"], true, "{r}");
    for title in ["User:Mallory", "User:Mallory/Notes"] {
        let i = anon
            .api(&format!("action=query&prop=info&titles={title}"))
            .await;
        assert_eq!(i["query"]["pages"][0]["missing"], true, "{title}: {i}");
        let i = bob
            .api(&format!("action=query&prop=info&titles={title}"))
            .await;
        assert_eq!(i["query"]["pages"][0]["missing"], true, "{title}: {i}");
        let i = alice
            .api(&format!(
                "action=query&prop=revisions&rvprop=content&titles={title}"
            ))
            .await;
        assert!(
            i["query"]["pages"][0].get("missing").is_none(),
            "{title}: {i}"
        );
    }
    let a = anon.api("action=query&list=allpages&apnamespace=2").await;
    assert!(!a.to_string().contains("Mallory"), "{a}");
}

/// `action=move` (10 §2.5): one `move` record and a redirect at the old title; the
/// `move/move` event; moving over a one-revision redirect; subpages; `noredirect`; and
/// the ACL checks at both ends.
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn moves() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("moves").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut mallory = account(&alice.router, "Mallory", "mallory-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    grant(&app, "Alice", "board").await;
    let at = alice.csrf().await;
    let t = enc(&at);
    let mt = enc(&mallory.csrf().await);
    let bt = enc(&bob.csrf().await);
    for (title, text) in [
        ("Project:Old", "The page."),
        ("Project:Old/Sub", "Its subpage."),
        ("Project:Locked", "Locked."),
        ("Project:Secret", "Hidden."),
    ] {
        alice
            .api_post(&format!("action=edit&title={title}&text={text}&token={t}"))
            .await;
    }
    mallory
        .api_post(&format!(
            "action=edit&title=User:Mallory/Draft&text=Draft.&token={mt}"
        ))
        .await;

    // The move: a redirect is left, the page keeps its history, the event is titled by
    // the old title and names the new one and the redirect.
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Old&to=Project:New&reason=renamed&token={t}"
        ))
        .await;
    assert_eq!(r["move"]["from"], "Library Base:Old", "{r}");
    assert_eq!(r["move"]["to"], "Library Base:New", "{r}");
    assert_eq!(r["move"]["redirectcreated"], true, "{r}");
    let i = anon
        .api("action=query&prop=info&titles=Project:New|Project:Old")
        .await;
    let pages = i["query"]["pages"].as_array().unwrap();
    let find = |title: &str| {
        pages
            .iter()
            .find(|p| p["title"] == title)
            .cloned()
            .unwrap_or_default()
    };
    assert!(find("Library Base:New").get("missing").is_none(), "{i}");
    assert_eq!(find("Library Base:Old")["redirect"], true, "{i}");
    let redirect_id = find("Library Base:Old")["pageid"].as_u64().unwrap();
    let page_id = find("Library Base:New")["pageid"].as_u64().unwrap();
    let h = anon
        .api("action=query&prop=revisions&rvprop=ids|comment&rvlimit=10&titles=Project:New")
        .await;
    assert_eq!(
        h["query"]["pages"][0]["revisions"]
            .as_array()
            .unwrap()
            .len(),
        2,
        "{h}"
    );
    let rd = anon
        .api("action=query&prop=revisions&rvprop=content&rvslots=main&titles=Project:Old")
        .await;
    assert!(
        rd.to_string().contains("#REDIRECT [[Library Base:New]]"),
        "{rd}"
    );
    let l = anon.api("action=query&list=logevents&letype=move").await;
    let e = &l["query"]["logevents"][0];
    assert_eq!(e["action"], "move", "{l}");
    assert_eq!(e["title"], "Project:Old", "{l}");
    assert_eq!(e["pageid"], page_id, "{l}");
    assert_eq!(e["params"]["target_title"], "Project:New", "{l}");
    assert_eq!(e["params"]["ts-redirect"], redirect_id, "{l}");
    assert!(e["comment"].as_str().unwrap().ends_with("renamed"), "{l}");
    let l = anon
        .api("action=query&list=logevents&letitle=Project:New")
        .await;
    assert_eq!(l["query"]["logevents"][0]["action"], "move", "{l}");

    // Moving back over the one-revision redirect displaces it: the redirect is deleted
    // with the move as its reason, and the title is the page's again.
    let r = bob
        .api_post(&format!(
            "action=move&from=Project:New&to=Project:Old&token={bt}"
        ))
        .await;
    assert_eq!(r["move"]["moveoverredirect"], true, "{r}");
    assert_eq!(r["move"]["redirectcreated"], true, "{r}");
    let i = anon
        .api(&format!("action=query&prop=info&pageids={page_id}"))
        .await;
    assert_eq!(i["query"]["pages"][0]["title"], "Library Base:Old", "{i}");
    let l = anon.api("action=query&list=logevents&letype=delete").await;
    let e = &l["query"]["logevents"][0];
    assert_eq!(e["pageid"], redirect_id, "{l}");
    assert!(
        e["comment"]
            .as_str()
            .unwrap()
            .contains("Moved over redirect"),
        "{l}"
    );
    // Project:New is now a redirect to Project:Old, with one revision: a further move
    // onto it is allowed; onto a page with history it is not.
    alice
        .api_post(&format!(
            "action=edit&title=Project:New&text=Not%20a%20redirect%20any%20more.&token={t}"
        ))
        .await;
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Old&to=Project:New&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "articleexists", "{r}");

    // Subpages follow with `movesubpages`; without it they stay.
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Old&to=Project:Renamed&movesubpages=1&token={t}"
        ))
        .await;
    assert_eq!(
        r["move"]["subpages"][0]["from"], "Library Base:Old/Sub",
        "{r}"
    );
    assert_eq!(
        r["move"]["subpages"][0]["to"], "Library Base:Renamed/Sub",
        "{r}"
    );
    let i = anon
        .api("action=query&prop=info&titles=Project:Renamed/Sub|Project:Old/Sub")
        .await;
    assert_eq!(i["query"]["pages"].as_array().unwrap().len(), 2, "{i}");
    assert!(
        i["query"]["pages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p.get("missing").is_none()),
        "{i}"
    );

    // `noredirect` needs `suppressredirect`: a plain user is refused, a sysop leaves none.
    let r = mallory
        .api_post(&format!(
            "action=move&from=User:Mallory/Draft&to=User:Mallory/Draft2&noredirect=1&token={mt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = mallory
        .api_post(&format!(
            "action=move&from=User:Mallory/Draft&to=User:Mallory/Draft2&token={mt}"
        ))
        .await;
    assert_eq!(r["move"]["redirectcreated"], true, "{r}");
    let r = bob
        .api_post(&format!(
            "action=move&from=User:Mallory/Draft2&to=User:Mallory/Draft3&noredirect=1&token={bt}"
        ))
        .await;
    assert!(r["move"].get("redirectcreated").is_none(), "{r}");
    let i = anon
        .api("action=query&prop=info&titles=User:Mallory/Draft2")
        .await;
    assert_eq!(i["query"]["pages"][0]["missing"], true, "{i}");
    let l = anon
        .api("action=query&list=logevents&letype=move&lelimit=1")
        .await;
    assert_eq!(
        l["query"]["logevents"][0]["params"]["suppressredirect"], true,
        "{l}"
    );

    // Entities, talk pages and threads do not move; a page cannot move onto itself.
    for from in ["Item:Q1", "Project talk:Old", "Property:P1"] {
        let r = alice
            .api_post(&format!("action=move&from={from}&to=Project:X&token={t}"))
            .await;
        assert!(
            ["immobilenamespace", "missingtitle"]
                .contains(&r["error"]["code"].as_str().unwrap_or("")),
            "{from}: {r}"
        );
    }
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Renamed&to=Item:Q99&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "immobilenamespace", "{r}");
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Renamed&to=Project:Renamed&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "selfmove", "{r}");

    // The ACLs at both ends: a `move` restriction on the source, a reservation at the
    // destination, and a confidential page there, which is a title taken.
    alice
        .api_post(&format!(
            "action=protect&title=Project:Locked&protections=move%3Dsysop&token={t}"
        ))
        .await;
    let r = mallory
        .api_post(&format!(
            "action=move&from=Project:Locked&to=Project:Unlocked&token={mt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "protectedpage", "{r}");
    let r = bob
        .api_post(&format!(
            "action=move&from=Project:Locked&to=Project:Unlocked&token={bt}"
        ))
        .await;
    assert_eq!(r["move"]["to"], "Library Base:Unlocked", "{r}");
    alice
        .api_post(&format!(
            "action=protect&title=Project:Reserved&protections=create%3Dsysop&token={t}"
        ))
        .await;
    let r = mallory
        .api_post(&format!(
            "action=move&from=User:Mallory/Draft3&to=Project:Reserved&token={mt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    let r = bob
        .api_post(&format!(
            "action=move&from=Project:Unlocked&to=Project:Reserved&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "ts-restricted-title", "{r}");
    alice
        .api_post(&format!(
            "action=protect&title=Project:Secret&protections=read%3Dboard&token={t}"
        ))
        .await;
    let r = mallory
        .api_post(&format!(
            "action=move&from=User:Mallory/Draft3&to=Project:Secret&token={mt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "articleexists", "{r}");
    // A confidential source is missing for those outside its group, even sysops.
    let r = bob
        .api_post(&format!(
            "action=move&from=Project:Secret&to=Project:Public&token={bt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "missingtitle", "{r}");
    // Its group moves it, and the redirect left behind is a public page naming the new
    // title: the group is told so by the response, which a form may warn on.
    let r = alice
        .api_post(&format!(
            "action=move&from=Project:Secret&to=Project:Secret2&noredirect=1&token={t}"
        ))
        .await;
    assert_eq!(r["move"]["to"], "Library Base:Secret2", "{r}");
    let i = bob
        .api("action=query&prop=info&titles=Project:Secret2|Project:Secret")
        .await;
    assert!(
        i["query"]["pages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["missing"] == true),
        "{i}"
    );
    let _ = &app;
}

/// Partial undeletion (09 §4.3) and hiding log events (`type=logging`).
#[tokio::test]
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
async fn partial_undelete_and_hidden_events() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("partial").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let mut mallory = account(&alice.router, "Mallory", "mallory-secret-1234").await;
    let mut anon = Client {
        router: alice.router.clone(),
        cookie: None,
    };
    grant(&app, "Bob", "sysop").await;
    let at = alice.csrf().await;
    let t = enc(&at);
    let bt = enc(&bob.csrf().await);
    let mt = enc(&mallory.csrf().await);
    let mut revids = Vec::new();
    for text in ["one", "two", "three"] {
        let r = alice
            .api_post(&format!(
                "action=edit&title=Project:Partial&text={text}&token={t}"
            ))
            .await;
        revids.push(r["edit"]["newrevid"].as_u64().unwrap());
    }
    let page_id = alice
        .api("action=query&prop=info&titles=Project:Partial")
        .await["query"]["pages"][0]["pageid"]
        .as_u64()
        .unwrap();
    alice
        .api_post(&format!("action=delete&title=Project:Partial&token={t}"))
        .await;

    // Restore the latest revision only: the page is back, the two older revisions stay
    // hidden, and the restore event lists them.
    let r = alice
        .api_post(&format!(
            "action=undelete&title=Project:Partial&revisions={}&token={t}",
            revids[2]
        ))
        .await;
    assert_eq!(r["undelete"]["revisions"], 1, "{r}");
    assert_eq!(
        r["undelete"]["ts-kept"],
        json!([revids[0], revids[1]]),
        "{r}"
    );
    let i = anon
        .api("action=query&prop=revisions&rvprop=ids|content&rvslots=main&rvlimit=10&titles=Project:Partial")
        .await;
    let revs = i["query"]["pages"][0]["revisions"].as_array().unwrap();
    assert_eq!(revs.len(), 3, "{i}");
    let find = |id: u64| revs.iter().find(|r| r["revid"] == id).cloned().unwrap();
    assert_eq!(find(revids[0])["texthidden"], true, "{i}");
    assert_eq!(find(revids[1])["texthidden"], true, "{i}");
    assert!(find(revids[2]).get("texthidden").is_none(), "{i}");
    assert!(i.to_string().contains("three"), "{i}");
    assert!(!i.to_string().contains("\"one\""), "{i}");
    let l = anon
        .api("action=query&list=logevents&letype=delete&lelimit=5")
        .await;
    let e = &l["query"]["logevents"][0];
    assert_eq!(e["action"], "restore", "{l}");
    assert_eq!(e["pageid"], page_id, "{l}");
    assert_eq!(e["params"]["kept"], json!([revids[0], revids[1]]), "{l}");
    // The two revision-deletion events sit behind it.
    assert_eq!(l["query"]["logevents"][1]["action"], "revision", "{l}");
    // Naming nothing that is deleted is refused; naming a second of history works.
    let r = alice
        .api_post(&format!(
            "action=undelete&title=Project:Partial&revisions=999999&token={t}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "cantundelete", "{r}");

    // A log event is hidden by its log ID: the block's comment and action go.
    let b = bob
        .api_post(&format!(
            "action=block&user=Mallory&reason=naughty&token={bt}"
        ))
        .await;
    let logid = b["block"]["id"].as_u64().unwrap();
    let r = mallory
        .api_post(&format!(
            "action=revisiondelete&type=logging&ids={logid}&hide=comment&token={mt}"
        ))
        .await;
    assert_eq!(r["error"]["code"], "permissiondenied", "{r}");
    let r = alice
        .api_post(&format!(
            "action=revisiondelete&type=logging&ids={logid}|999999&hide=comment%7Ccontent&token={t}"
        ))
        .await;
    let items = r["revisiondelete"]["items"].as_array().unwrap();
    assert_eq!(items[0]["status"], "Success", "{r}");
    assert_eq!(items[0]["commenthidden"], true, "{r}");
    assert_eq!(items[1]["errors"][0]["code"], "nosuchlogid", "{r}");
    let l = anon
        .api("action=query&list=logevents&letype=block&lelimit=5")
        .await;
    let e = &l["query"]["logevents"][0];
    assert_eq!(e["actionhidden"], true, "{l}");
    assert_eq!(e["commenthidden"], true, "{l}");
    assert!(e.get("params").is_none(), "{l}");
    assert!(e.get("title").is_none(), "{l}");
    assert!(!l.to_string().contains("naughty"), "{l}");
    let l = anon
        .api("action=query&list=logevents&letype=delete&lelimit=1")
        .await;
    assert_eq!(l["query"]["logevents"][0]["action"], "event", "{l}");
    // Shown again.
    let r = alice
        .api_post(&format!(
            "action=revisiondelete&type=logging&ids={logid}&show=comment%7Ccontent&token={t}"
        ))
        .await;
    assert_eq!(r["revisiondelete"]["items"][0]["status"], "Success", "{r}");
    let l = anon
        .api("action=query&list=logevents&letype=block&lelimit=1")
        .await;
    assert!(l.to_string().contains("naughty"), "{l}");
    assert!(
        l["query"]["logevents"][0].get("actionhidden").is_none(),
        "{l}"
    );
}

/// Interface pages need `editinterface` (0095 §4), which `sysop` holds and a user does not;
/// and one under a read restriction is no override (§5).
#[tokio::test]
async fn interface_pages_need_editinterface() {
    if admin_config().is_none() {
        eprintln!("TRIPLESPACE_TEST_DATABASE_URL is not set; skipping");
        return;
    }
    let (_database, app, mut alice) = instance("interface").await;
    let mut bob = account(&alice.router, "Bob", "bob-secret-1234").await;
    let bt = enc(&bob.csrf().await);
    let refused = bob
        .api_post(&format!(
            "action=edit&title=Interface:Ts-new&text=Mine&token={bt}"
        ))
        .await;
    assert_eq!(refused["error"]["code"], "permissiondenied", "{refused}");
    grant(&app, "Bob", "sysop").await;
    let made = bob
        .api_post(&format!(
            "action=edit&title=Interface:Ts-new&text=Create&token={bt}"
        ))
        .await;
    assert_eq!(made["edit"]["result"], "Success", "{made}");
    // Restricted to a group, the page is no longer an override.
    let p = bob
        .api_post(&format!(
            "action=protect&title=Interface:Ts-new&protections=read%3Dsysop&token={bt}"
        ))
        .await;
    assert!(p.get("error").is_none(), "{p}");
    let si = alice
        .api_post("action=query&meta=siteinfo&siprop=triplespace")
        .await;
    assert!(
        si["query"]["triplespace"]["interface"]["messages"]
            .get("ts-new")
            .is_none(),
        "{si}"
    );
}
