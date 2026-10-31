//! The parity harness of the skin migration (0097 §16; 19 §5.10): every case of the
//! corpus (`tests/corpus/cases.json`) is rendered by both renderers and compared as a
//! browser reads it (`triplespace_ui::parity`), and the time each renderer took is
//! measured.
//!
//! The built-in renderer against itself was the baseline (2026-10-30, 56 of 56 equal):
//! a page rendered twice is equal, so every difference reported is a real one. The skin
//! renderer (`Renderer::Skin`, Worksheet) is compared against the built-in one; since
//! group 1 switched the frame is the skin's under both, and the comparison is of the
//! page bodies as each group gets its templates.
//!
//! `cargo xtask skin-parity` runs this with a report (`SKIN_PARITY_REPORT`, a TSV path)
//! and timing rounds (`SKIN_PARITY_ROUNDS`, default 1; the renderers alternate within a
//! round, after one untimed warm-up each). A case whose group has switched to the skin
//! must be equal; the rest are reported, not failed. The baseline run of 2026-10-30
//! (built-in against itself, release build, 100 rounds) put the noise of a group's
//! summed median under 1% and of its 95th percentile at several percent, so the budget
//! of 0097 §16 is judged on at least 100 rounds, on an idle machine, and a p95 verdict
//! within the noise is run again before it counts.

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

/// The migration groups that have switched to the skin (19 §5.10): a case in one of
/// them must be equal; none yet.
const MIGRATED: &[u8] = &[];

use triplespace_ui::{Renderer, Site};

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

async fn render(client: &Client, case: &Case, renderer: Renderer) -> Rendering {
    let mut req = Request::get(&case.path).header(header::HOST, "librarybase.org");
    if case.viewer == "user" {
        req = req.header(header::COOKIE, "triplespace_session=alice");
    }
    for (k, v) in &case.headers {
        req = req.header(k.as_str(), v.as_str());
    }
    let r = triplespace_ui::router_for(Site::new(client.clone()).with_renderer(renderer))
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

/// Why two renderings differ, in one line for the report.
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
    if a.location != b.location {
        return Some(format!("location {:?} vs {:?}", a.location, b.location));
    }
    let html = a
        .content_type
        .as_deref()
        .is_some_and(|c| c.starts_with("text/html"));
    if !html {
        return (a.body != b.body).then(|| "body differs (not HTML)".to_string());
    }
    let kind = Kind::of(&a.body);
    compare(&a.body, &b.body, kind).map(|d| d.to_string().replace('\n', " "))
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
    builtin_us: (u128, u128),
    skin_us: (u128, u128),
}

#[tokio::test(flavor = "multi_thread")]
async fn every_case_renders_equal_through_both_renderers() {
    let rounds: usize = std::env::var("SKIN_PARITY_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(1);
    let corpus = corpus();
    let mut rows = Vec::new();
    let mut clients: BTreeMap<String, Client> = BTreeMap::new();
    for case in corpus.cases {
        let client = clients
            .entry(case.set.clone())
            .or_insert_with(|| client(&case.set))
            .clone();
        let mut timings = [Vec::new(), Vec::new()];
        let mut first = [None, None];
        // A warm-up render each, untimed, so that lazily built state (the message
        // bundle, the manifest) is charged to neither renderer.
        if rounds > 1 {
            for renderer in [Renderer::Builtin, Renderer::Skin] {
                render(&client, &case, renderer).await;
            }
        }
        for _ in 0..rounds {
            for (i, renderer) in [Renderer::Builtin, Renderer::Skin].into_iter().enumerate() {
                let t = Instant::now();
                let r = render(&client, &case, renderer).await;
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
            builtin_us: stats(timings[0].clone()),
            skin_us: stats(timings[1].clone()),
            case,
        });
    }
    // The report: one row per case.
    let mut out = String::from(
        "case\tset\tgroup\tviewer\tpath\tstatus\tequal\tbuiltin_median_us\tbuiltin_p95_us\tskin_median_us\tskin_p95_us\tdifference\n",
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
            r.builtin_us.0,
            r.builtin_us.1,
            r.skin_us.0,
            r.skin_us.1,
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
    for r in rows.iter().filter(|r| !r.equal) {
        eprintln!(
            "  {} ({}, group {}): {}",
            r.case.name,
            r.case.path,
            r.case.group,
            r.difference.as_deref().unwrap_or_default()
        );
    }
    // Until the engine exists both renderers are the built-in one, and the baseline is
    // the gate: a page rendered twice is equal. Once groups switch, only theirs fail.
    let must_be_equal = |r: &Row| MIGRATED.is_empty() || MIGRATED.contains(&r.case.group);
    let failing: Vec<&str> = rows
        .iter()
        .filter(|r| !r.equal && must_be_equal(r))
        .map(|r| r.case.name.as_str())
        .collect();
    assert!(
        failing.is_empty(),
        "cases that must be equal differ: {failing:?}"
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
        let r = render(&client, case, Renderer::Builtin).await;
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

/// The frame is the skin's since group 1 switched: Plain links no site stylesheet,
/// Worksheet links it, and the theme's stylesheets are linked by both.
#[tokio::test(flavor = "multi_thread")]
async fn the_frame_is_the_skins() {
    let client = client("fixture");
    let case = |path: &str| Case {
        name: "x".into(),
        set: "fixture".into(),
        path: path.into(),
        viewer: anonymous(),
        headers: BTreeMap::new(),
        group: 1,
        status: 200,
    };
    let worksheet = render(&client, &case("/wiki/Item:Q6"), Renderer::Builtin).await;
    let plain = render(
        &client,
        &case("/wiki/Item:Q6?useskin=plain"),
        Renderer::Builtin,
    )
    .await;
    assert!(worksheet.body.starts_with("<!DOCTYPE html>\n<html "));
    assert!(plain.body.starts_with("<!DOCTYPE html>\n<html "));
    assert!(worksheet.body.contains("id=\"ts-palette\""));
    assert!(plain.body.contains("id=\"ts-palette\""));
    // Plain links no site stylesheet; Worksheet links it where the build has one
    // (`ui/dist`), and is Plain's page otherwise, by construction (19 §5.10).
    assert!(!plain.body.contains("/ui/assets/site"));
    let site_css = regex_free_stylesheet_links(&worksheet.body);
    let without: String = site_css
        .iter()
        .fold(worksheet.body.clone(), |b, l| b.replace(l, ""));
    assert_eq!(compare(&without, &plain.body, Kind::Document), None);
}

/// The `<link rel="stylesheet" href="/ui/assets/…">` lines of a page, each with its
/// leading newline.
fn regex_free_stylesheet_links(html: &str) -> Vec<String> {
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
    for f in ["frame.html", "codex.html", "watch.html", "entity.html"] {
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
    let site = Site::new(client)
        .with_skins_dir(Some(&dir))
        .with_renderer(Renderer::Skin);
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
    for renderer in [Renderer::Builtin, Renderer::Skin] {
        let mut us = Vec::new();
        for _ in 0..30 {
            let t = Instant::now();
            render(&client, &case, renderer).await;
            us.push(t.elapsed().as_micros());
        }
        eprintln!("{renderer:?}: whole page median {} us", stats(us).0);
    }
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

/// `Context::draw_or`, the mechanism a group not yet switched renders through: the
/// built-in builder, the skin, or both with the built-in result served.
#[tokio::test(flavor = "multi_thread")]
async fn draw_or_follows_the_renderer() {
    use triplespace_ui::pages;
    for renderer in [Renderer::Builtin, Renderer::Skin, Renderer::Compare] {
        let site = Site::new(client("fixture")).with_renderer(renderer);
        let headers = HeaderMap::new();
        let incoming = pages::incoming(&headers, None);
        let Ok(cx) = pages::context(&site, &incoming, &BTreeMap::new()).await else {
            panic!("a context");
        };
        let drawn = cx.draw_or("x", serde_json::json!({}), "the test", || {
            "<p>builtin</p>".to_string()
        });
        // An unknown template is an empty part through the skin, and the built-in result
        // under `compare`; a known one draws.
        match renderer {
            Renderer::Builtin | Renderer::Compare => assert_eq!(drawn, "<p>builtin</p>"),
            Renderer::Skin => assert_eq!(drawn, ""),
        }
        let drawn = cx.draw(
            "entity/terms.html",
            serde_json::json!({"terms": {
            "description": null, "no_description": "none", "aliases": null, "edit": null}}),
        );
        assert!(drawn.contains("ts-terms__description--none"), "{drawn}");
    }
}
