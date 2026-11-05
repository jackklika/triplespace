//! The skins' parity check (0097 §16, A12; 19 §5.10): every case of the corpus
//! (`tests/corpus/cases.json`, and `cases.recorded.json` from a seeded instance) is
//! rendered with Worksheet, the default skin, and with Plain, and the two are compared as
//! a browser reads them (`triplespace_ui::parity`) once Worksheet's stylesheet links are
//! taken out: Worksheet is Plain with the site's stylesheets, and adds nothing else.
//!
//! Until 2026-11-03 this harness compared the built-in renderer with the skin, group by
//! group, and timed both (0097 §16, A5); the built-in renderer was removed when the last
//! group switched (A12). The time each skin took is still measured and reported, as a
//! watch on page render time.
//!
//! `cargo xtask skin-parity` runs this with a report (`SKIN_PARITY_REPORT`, a TSV path)
//! and timing rounds (`SKIN_PARITY_ROUNDS`, default 1; the skins alternate within a
//! round, after one untimed warm-up each).

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as IoWrite;
use std::time::Instant;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::fixture_api::FixtureApi;
use http_body_util::BodyExt as _;
use serde::Deserialize;
use tower::ServiceExt as _;
use triplespace_client::Client;
use triplespace_client::replay::{Exchange, Replay};
use triplespace_ui::parity::{Kind, compare};

/// One case of the corpus.
#[derive(Debug, Clone, Deserialize)]
struct Case {
    name: String,
    set: String,
    path: String,
    #[serde(default = "anonymous")]
    viewer: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    group: u8,
    /// The status the site answers with; `200` unless the case says otherwise.
    #[serde(default = "ok")]
    status: u16,
}

fn ok() -> u16 {
    200
}

fn anonymous() -> String {
    "anonymous".into()
}

#[derive(Debug, Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

use triplespace_ui::Site;

/// The corpus: `cases.json`, written by hand, and `cases.recorded.json`, which
/// `tools/skin_corpus.sh` writes from a seeded instance.
fn corpus() -> Corpus {
    let mut cases = Vec::new();
    for file in ["cases.json", "cases.recorded.json"] {
        let path = format!("{}/tests/corpus/{file}", env!("CARGO_MANIFEST_DIR"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let c: Corpus = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
        cases.extend(c.cases);
    }
    Corpus { cases }
}

/// The API a set's cases are rendered against.
fn client(set: &str) -> Client {
    match set {
        "fixture" => Client::new(FixtureApi::new()),
        "wikidata" => Client::new(FixtureApi::wikidata()),
        name => {
            let path = format!(
                "{}/tests/corpus/exchanges/{name}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("the recording {path}: {e}"));
            let exchanges: Vec<Exchange> = serde_json::from_str(&text).expect("recording parses");
            Client::new(Replay::new(exchanges))
        }
    }
}

/// A rendering: what the comparison reads of the response.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rendering {
    status: StatusCode,
    content_type: Option<String>,
    location: Option<String>,
    body: String,
}

/// The skins a case is rendered with: Worksheet, the default, and Plain, asked for with
/// `useskin=`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum With {
    Worksheet,
    Plain,
}

async fn render(client: &Client, case: &Case, with: With) -> Rendering {
    let path = match with {
        With::Worksheet => case.path.clone(),
        With::Plain => {
            let sep = if case.path.contains('?') { '&' } else { '?' };
            format!("{}{sep}useskin=plain", case.path)
        }
    };
    let mut req = Request::get(&path).header(header::HOST, "librarybase.org");
    if case.viewer == "user" {
        req = req.header(header::COOKIE, "triplespace_session=alice");
    }
    for (k, v) in &case.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let r = triplespace_ui::router_for(Site::new(client.clone()))
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let header = |h: &HeaderMap, n: header::HeaderName| {
        h.get(n).and_then(|v| v.to_str().ok()).map(str::to_string)
    };
    let content_type = header(r.headers(), header::CONTENT_TYPE);
    let location = header(r.headers(), header::LOCATION);
    let body = r.into_body().collect().await.unwrap().to_bytes();
    Rendering {
        status,
        content_type,
        location,
        body: String::from_utf8_lossy(&body).into_owned(),
    }
}

/// Why Worksheet's rendering differs from Plain's once its stylesheet links are taken
/// out, in one line for the report.
fn difference(a: &Rendering, b: &Rendering) -> Option<String> {
    if a.status != b.status {
        return Some(format!("status {} vs {}", a.status, b.status));
    }
    if a.content_type != b.content_type {
        return Some(format!(
            "content-type {:?} vs {:?}",
            a.content_type, b.content_type
        ));
    }
    // A redirect keeps the query it was asked with, Plain's `useskin=` among it.
    let plain_location = b.location.as_deref().map(|l| {
        l.replace("?useskin=plain&", "?")
            .replace("&useskin=plain", "")
            .replace("?useskin=plain", "")
    });
    if a.location != plain_location {
        return Some(format!("location {:?} vs {:?}", a.location, plain_location));
    }
    let html = a
        .content_type
        .as_deref()
        .is_some_and(|c| c.starts_with("text/html"));
    if !html {
        return (a.body != b.body).then(|| "body differs (not HTML)".to_string());
    }
    let without = stylesheet_links(&a.body)
        .iter()
        .fold(a.body.clone(), |body, l| body.replace(l, ""));
    let kind = Kind::of(&without);
    compare(&without, &b.body, kind).map(|d| d.to_string().replace('\n', " "))
}

/// The median and the 95th percentile of some durations, in microseconds.
fn stats(mut us: Vec<u128>) -> (u128, u128) {
    us.sort_unstable();
    // The nearest-rank percentile, in integers: the index of q% of the way through.
    let at = |per_cent: usize| {
        let i = ((us.len() - 1) * per_cent).div_ceil(100).min(us.len() - 1);
        us[i]
    };
    (at(50), at(95))
}

#[derive(Debug)]
struct Row {
    case: Case,
    status: u16,
    equal: bool,
    difference: Option<String>,
    worksheet_us: (u128, u128),
    plain_us: (u128, u128),
}

#[tokio::test(flavor = "multi_thread")]
async fn worksheet_is_plain_with_its_stylesheets() {
    let rounds: usize = std::env::var("SKIN_PARITY_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(1);
    let corpus = corpus();
    let mut rows = Vec::new();
    let mut clients: BTreeMap<String, Client> = BTreeMap::new();
    let skins = [With::Worksheet, With::Plain];
    for case in corpus.cases {
        let client = clients
            .entry(case.set.clone())
            .or_insert_with(|| client(&case.set))
            .clone();
        let mut timings = [Vec::new(), Vec::new()];
        let mut first = [None, None];
        // A warm-up render each, untimed, so that lazily built state (the message
        // bundle, the manifest) is charged to neither skin.
        if rounds > 1 {
            for with in skins {
                render(&client, &case, with).await;
            }
        }
        for _ in 0..rounds {
            for (i, with) in skins.into_iter().enumerate() {
                let t = Instant::now();
                let r = render(&client, &case, with).await;
                timings[i].push(t.elapsed().as_micros());
                first[i].get_or_insert(r);
            }
        }
        let [Some(a), Some(b)] = first else {
            unreachable!()
        };
        let difference = difference(&a, &b);
        rows.push(Row {
            status: a.status.as_u16(),
            equal: difference.is_none(),
            difference,
            worksheet_us: stats(timings[0].clone()),
            plain_us: stats(timings[1].clone()),
            case,
        });
    }
    // The report: one row per case.
    let mut out = String::from(
        "case\tset\tgroup\tviewer\tpath\tstatus\tequal\tworksheet_median_us\tworksheet_p95_us\tplain_median_us\tplain_p95_us\tdifference\n",
    );
    for r in &rows {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.case.name,
            r.case.set,
            r.case.group,
            r.case.viewer,
            r.case.path,
            r.status,
            if r.equal { "equal" } else { "differs" },
            r.worksheet_us.0,
            r.worksheet_us.1,
            r.plain_us.0,
            r.plain_us.1,
            r.difference.as_deref().unwrap_or_default(),
        );
    }
    if let Ok(path) = std::env::var("SKIN_PARITY_REPORT") {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(dir).expect("report directory");
        }
        std::fs::File::create(&path)
            .and_then(|mut f| IoWrite::write_all(&mut f, out.as_bytes()))
            .expect("write the report");
    }
    let equal = rows.iter().filter(|r| r.equal).count();
    eprintln!(
        "skin-parity: {equal} of {} cases equal; {rounds} round(s)",
        rows.len()
    );
    let failing: Vec<String> = rows
        .iter()
        .filter(|r| !r.equal)
        .map(|r| {
            format!(
                "{} ({}): {}",
                r.case.name,
                r.case.path,
                r.difference.as_deref().unwrap_or_default()
            )
        })
        .collect();
    assert!(
        failing.is_empty(),
        "Worksheet adds more than its stylesheets to Plain:\n{}",
        failing.join("\n")
    );
}

/// Every case is a request the site answers with the status the case expects: not a
/// `503` from an answer the fake or the recording lacks, nor a `404` for a page meant to
/// exist, either of which would compare equal and prove nothing.
#[tokio::test(flavor = "multi_thread")]
async fn every_case_is_answered() {
    let corpus = corpus();
    let mut clients: BTreeMap<String, Client> = BTreeMap::new();
    for case in &corpus.cases {
        let client = clients
            .entry(case.set.clone())
            .or_insert_with(|| client(&case.set))
            .clone();
        let r = render(&client, case, With::Worksheet).await;
        assert_eq!(
            r.status.as_u16(),
            case.status,
            "{}: {} {}",
            case.name,
            r.status,
            r.body.chars().take(300).collect::<String>()
        );
    }
}

/// Plain links no site stylesheet, Worksheet links it where the build has one
/// (`ui/dist`), and the theme's stylesheets are linked by both.
#[tokio::test(flavor = "multi_thread")]
async fn the_skins_link_their_stylesheets() {
    let client = client("fixture");
    let case = Case {
        name: "x".into(),
        set: "fixture".into(),
        path: "/wiki/Item:Q6".into(),
        viewer: anonymous(),
        headers: BTreeMap::new(),
        group: 1,
        status: 200,
    };
    let worksheet = render(&client, &case, With::Worksheet).await;
    let plain = render(&client, &case, With::Plain).await;
    assert!(worksheet.body.starts_with("<!DOCTYPE html>\n<html "));
    assert!(plain.body.starts_with("<!DOCTYPE html>\n<html "));
    assert!(worksheet.body.contains("id=\"ts-palette\""));
    assert!(plain.body.contains("id=\"ts-palette\""));
    assert!(!plain.body.contains("/ui/assets/site"));
    assert_eq!(difference(&worksheet, &plain), None);
}

/// The `<link rel="stylesheet" href="/ui/assets/…">` lines of a page, each with its
/// leading newline.
fn stylesheet_links(html: &str) -> Vec<String> {
    html.split('\n')
        .filter(|l| l.starts_with("<link rel=\"stylesheet\" href=\"/ui/assets/"))
        .map(|l| format!("\n{l}"))
        .collect()
}

/// The entity parts go through the skin's templates, not the built-in builders: checked
/// by rendering with a skin whose `entity/statements.html` draws a marker instead.
#[tokio::test(flavor = "multi_thread")]
async fn entity_parts_are_drawn_by_the_skin() {
    let dir = std::env::temp_dir().join(format!("ts-skin-{}", std::process::id()));
    let broken = dir.join("brokenplain");
    std::fs::create_dir_all(broken.join("templates/entity")).unwrap();
    std::fs::write(
        broken.join("skin.toml"),
        "name = \"brokenplain\"\nversion = \"0.1.0\"\n[requires]\ntemplate_contract = \"0.1\"\ncodex_tokens = 2\n",
    )
    .unwrap();
    // Plain's templates, with the Statements tab drawing a marker instead.
    for f in [
        "frame.html",
        "codex.html",
        "inline.html",
        "watch.html",
        "appearance.html",
        "entity.html",
    ] {
        std::fs::copy(
            format!(
                "{}/../../skins/plain/templates/{f}",
                env!("CARGO_MANIFEST_DIR")
            ),
            broken.join("templates").join(f),
        )
        .unwrap();
    }
    for f in [
        "group.html",
        "identifiers.html",
        "terms.html",
        "sitelinks.html",
        "labels.html",
        "identity.html",
    ] {
        std::fs::copy(
            format!(
                "{}/../../skins/plain/templates/entity/{f}",
                env!("CARGO_MANIFEST_DIR")
            ),
            broken.join("templates/entity").join(f),
        )
        .unwrap();
    }
    std::fs::write(
        broken.join("templates/entity/statements.html"),
        "<!--MARKER-->",
    )
    .unwrap();
    let client = client("fixture");
    let site = Site::new(client).with_skins_dir(Some(&dir));
    let req = Request::get("/wiki/Item:Q6?useskin=brokenplain")
        .header(header::HOST, "librarybase.org")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router_for(site).oneshot(req).await.unwrap();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("<!--MARKER-->"),
        "the tab is the skin's: {html}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Where the skin's time goes on the largest page: the model, its conversion to a
/// template value, and the render, timed apart. Run with `--nocapture --ignored`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a profile, run by hand with --ignored --nocapture"]
async fn profile_entity_rendering() {
    use scatter_wikibase_model::entity::Entity;
    use triplespace_ui::entity::{Endnotes, Lookup, Render, site_roles};
    use triplespace_ui::skin::Skins;
    let skins = Skins::load(None);
    let plain = skins.get("plain").unwrap();
    let api = FixtureApi::wikidata();
    let client = Client::new(api.clone());
    let case = Case {
        name: "x".into(),
        set: "wikidata".into(),
        path: "/wiki/Item:WDQ65".into(),
        viewer: anonymous(),
        headers: BTreeMap::new(),
        group: 3,
        status: 200,
    };
    let mut us = Vec::new();
    for _ in 0..30 {
        let t = Instant::now();
        render(&client, &case, With::Worksheet).await;
        us.push(t.elapsed().as_micros());
    }
    eprintln!("whole page median {} us", stats(us).0);
    let entity = Entity::from_value(api.entities["WDQ65"].clone())
        .unwrap()
        .entity;
    let m = triplespace_ui::i18n::Messages::for_language("en");
    let site = triplespace_client::SiteInfo {
        providers: vec![triplespace_client::ProviderInfo {
            code: "WD".into(),
            slug: "wikidata".into(),
            name: "Wikidata".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let lookup = Lookup::from_entities(&api.entities, &m.term_chain());
    let roles = site_roles(&site);
    let r = Render {
        m: &m,
        site: &site,
        lookup: &lookup,
        entity: &entity,
        provenance: None,
        roles: &roles,
        editable: false,
        notes: Endnotes::default(),
    };
    triplespace_ui::entity::profile(&r, plain, 30);
}

/// `Context::draw`: a template the skin lacks is an empty part, a known one draws.
#[tokio::test(flavor = "multi_thread")]
async fn a_part_is_drawn_by_the_skin() {
    use triplespace_ui::pages;
    let site = Site::new(client("fixture"));
    let headers = HeaderMap::new();
    let incoming = pages::incoming(&headers, None);
    let Ok(cx) = pages::context(&site, &incoming, &BTreeMap::new()).await else {
        panic!("a context");
    };
    assert_eq!(cx.draw("x", serde_json::json!({})), "");
    let drawn = cx.draw(
        "entity/terms.html",
        serde_json::json!({"terms": {
        "description": null, "no_description": "none", "aliases": null, "edit": null}}),
    );
    assert!(drawn.contains("ts-terms__description--none"), "{drawn}");
}

/// One case's whole-page render time, for finding where a group's time goes: `CASE`
/// names it (`CASE=lb-special-recentchanges cargo test --release -p triplespace-ui
/// --test parity -- profile_one_case --ignored --nocapture`).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "a profile, run by hand with --ignored --nocapture"]
async fn profile_one_case() {
    let name = std::env::var("CASE").unwrap_or_else(|_| "lb-special-recentchanges-limit-3".into());
    let corpus = corpus();
    let case = corpus
        .cases
        .iter()
        .find(|c| c.name == name)
        .expect("a case");
    let client = client(&case.set);
    for _ in 0..20 {
        render(&client, case, With::Worksheet).await;
    }
    let mut us = Vec::new();
    for _ in 0..300 {
        let t = Instant::now();
        render(&client, case, With::Worksheet).await;
        us.push(t.elapsed().as_micros());
    }
    let (med, p95) = stats(us);
    eprintln!("{name}: median {med} us, p95 {p95} us");
}
