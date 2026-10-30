//! Entity pages against the fake API of `tests/common/fixture_api.rs`: snapshots of each
//! page kind and region, and their caching.
//!
//! Update the snapshots with `INSTA_UPDATE=always cargo test -p triplespace-ui --test entity`
//! and review the diff.

mod common;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use common::fixture_api::FixtureApi;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;
use triplespace_client::Client;

async fn get(uri: &str) -> (StatusCode, HeaderMap, String) {
    get_from(FixtureApi::new(), uri).await
}

async fn get_from(api: FixtureApi, uri: &str) -> (StatusCode, HeaderMap, String) {
    let req = Request::get(uri)
        .header(header::HOST, "librarybase.org")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router(Client::new(api))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

/// The page's `<main>`, which is what these snapshots are about; the frame has its own
/// tests.
fn main_of(html: &str) -> String {
    let start = html.find("<main").expect("a <main>");
    let end = html.find("</main>").expect("a </main>") + "</main>".len();
    html[start..end].replace("><", ">\n<")
}

#[tokio::test]
async fn a_local_item_reads_without_javascript() {
    let (status, h, html) = get("/wiki/Item:Q6").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Six &#38; more (Q6) – librarybase</title>"));
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    let tags = h["cache-tag"].to_str().unwrap();
    assert!(tags.contains("entity:Q6"), "{tags}");
    assert!(
        tags.contains("entity:P2"),
        "the labels' tags are the page's too: {tags}"
    );
    assert!(h.get(header::ETAG).is_some());
    insta::assert_snapshot!("item_statements", main_of(&html));
}

#[tokio::test]
async fn item_tabs() {
    for (tab, name) in [
        ("identifiers", "item_identifiers"),
        ("labels", "item_labels"),
        ("sitelinks", "item_sitelinks"),
    ] {
        let (status, _, html) = get(&format!("/wiki/Item:Q6?tab={tab}")).await;
        assert_eq!(status, StatusCode::OK, "{tab}");
        insta::assert_snapshot!(name, main_of(&html));
    }
}

#[tokio::test]
async fn a_property_a_domain_and_a_foreign_item() {
    for (uri, name) in [
        ("/wiki/Property:P3", "property"),
        ("/wiki/Domain:wikipedia.org", "domain"),
        ("/wiki/Item:WDQ65", "foreign_item"),
        ("/wiki/Item:WDQ65?tab=sitelinks", "foreign_item_sitelinks"),
    ] {
        let (status, _, html) = get(uri).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        insta::assert_snapshot!(name, main_of(&html));
    }
    let (_, _, prop) = get("/wiki/Property:P3").await;
    assert!(
        !prop.contains("tab=sitelinks"),
        "a property has no Sitelinks tab"
    );
}

#[tokio::test]
async fn regions_are_the_pages_own_parts() {
    let (_, _, page) = get("/wiki/Item:Q6").await;
    let (_, _, identifiers_tab) = get("/wiki/Item:Q6?tab=identifiers").await;
    for (region, name) in [
        ("statements/P2", "region_statements_p2"),
        ("statements/P99", "region_statements_empty"),
        ("terms", "region_terms"),
        ("identifiers", "region_identifiers"),
    ] {
        let (status, h, html) = get(&format!(
            "/w/index.php?title=Item:Q6&action=render&region={region}"
        ))
        .await;
        assert_eq!(status, StatusCode::OK, "{region}");
        assert!(!html.contains("<html"), "{region} has no frame");
        assert!(
            h[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .starts_with("public")
        );
        let holder = if region == "identifiers" {
            &identifiers_tab
        } else {
            &page
        };
        if region != "statements/P99" {
            assert!(
                holder.contains(&html),
                "{region} is drawn on its page as served alone"
            );
        }
        insta::assert_snapshot!(name, html.replace("><", ">\n<"));
    }
    let (status, _, _) = get("/wiki/Item:Q6?action=render&region=nonsense").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let req = Request::get("/wiki/Item:Q6?action=render&region=terms")
        .header(header::HOST, "librarybase.org")
        .header("x-triplespace-ui-build", "an-older-build")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router(Client::new(FixtureApi::new()))
        .oneshot(req)
        .await
        .unwrap();
    assert_eq!(
        r.status(),
        StatusCode::CONFLICT,
        "build skew reloads instead of swapping"
    );
}

#[tokio::test]
async fn missing_and_invalid_entities_are_404() {
    for uri in ["/wiki/Item:Q404", "/wiki/Item:bogus"] {
        let (status, _, html) = get(uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
        assert!(html.contains("There is no entity at"), "{uri}");
    }
}

#[tokio::test]
async fn uselang_sets_language_direction_and_labels() {
    let (_, _, html) = get("/wiki/Item:Q6?uselang=ar").await;
    assert!(
        html.contains(
            "<html class=\"client-nojs skin-theme-clientpref-os\" lang=\"ar\" dir=\"rtl\">"
        )
    );
    assert!(html.contains(">ستة</h1>"), "the Arabic label");
    let (_, _, de) = get("/wiki/Item:Q6?uselang=de").await;
    assert!(de.contains(">Sechs</h1>"));
    assert!(
        de.contains(">Zahl</a>"),
        "linked labels follow the reader's language too"
    );
    let (_, _, fallback) = get("/wiki/Item:WDQ65?uselang=de").await;
    assert!(
        fallback.contains("lang=\"en\" dir=\"auto\">Los Angeles</h1>"),
        "an English fallback is marked"
    );
}

/// One group of a mirrored Wikidata item, as `action=render` serves it.
async fn wd_group(q: &str, p: &str) -> String {
    let (status, _, html) = get_from(
        FixtureApi::wikidata(),
        &format!("/w/index.php?title=Item:WD{q}&action=render&region=statements/WD{p}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{q} {p}");
    html
}

#[tokio::test]
async fn los_angeles_population_is_a_series_with_a_chart_and_a_table() {
    let html = wd_group("Q65", "P1082").await;
    assert!(html.contains("data-shape=\"series\""));
    assert!(html.contains("<svg class=\"ts-chart\""), "the chart");
    assert!(html.contains("ts-views__panel--b"), "a table to switch to");
    assert!(
        html.contains("<span class=\"ts-subtle\">in "),
        "when the headline was measured"
    );
    insta::assert_snapshot!("shape_series", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn los_angeles_mayors_are_a_timeline() {
    let html = wd_group("Q65", "P6").await;
    assert!(html.contains("data-shape=\"timeline\""));
    assert!(html.contains("<svg class=\"ts-axis\""));
    assert!(html.contains("class=\"ts-period\">since ") || html.contains("class=\"ts-period\">"));
    assert!(html.contains("Best value"), "the best value leads");
    insta::assert_snapshot!("shape_timeline", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn pluto_folds_its_other_classes_and_the_deprecated_planet() {
    let html = wd_group("Q339", "P31").await;
    assert!(html.contains(">5 other values</summary>"), "{html}");
    assert!(html.contains(">1 deprecated</summary>"));
    assert!(html.contains("ts-statement--deprecated"));
    assert!(
        html.contains("title=\"Queries and infoboxes return only this value\""),
        "the badge's tooltip"
    );
    let best = html.find("ts-chip--best").unwrap();
    assert!(
        best < html.find("other values").unwrap(),
        "the best value leads"
    );
    insta::assert_snapshot!("shape_best_and_deprecated", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn manganese_ionic_radii_are_a_table_with_a_matrix() {
    let html = wd_group("Q731", "P10685").await;
    assert!(html.contains("data-shape=\"table\""));
    assert!(html.contains(">Matrix</label>"));
    assert!(html.contains("<table class=\"cdx-table__table\">"));
    assert!(
        html.contains("<span class=\"ts-visually-hidden\">"),
        "repeated cells are hidden but kept"
    );
    insta::assert_snapshot!("shape_table_matrix", html.replace("><", ">\n<"));
}

#[tokio::test]
async fn large_groups_show_ten_values_then_the_rest() {
    let html = wd_group("Q731", "P2877").await;
    assert!(html.contains("data-shape=\"chips\""));
    assert!(html.contains(">Show 87 more values</summary>"));
    let names = wd_group("Q1520", "P1448").await;
    assert!(names.contains("data-shape=\"timeline\""));
    assert!(names.contains(">Show 18 more values</summary>"));
}

#[tokio::test]
async fn authors_by_ordinal_are_a_numbered_list_and_deprecated_only_is_no_current_value() {
    let html = wd_group("Q900001", "P2093").await;
    assert!(html.contains("<ol class=\"ts-numbered\">"));
    let order: Vec<usize> = [
        "value=\"1\"",
        "value=\"2\"",
        "value=\"3\"",
        "value=\"10\"",
        "value=\"12\"",
    ]
    .iter()
    .map(|v| html.find(v).unwrap_or_else(|| panic!("{v} in {html}")))
    .collect();
    assert!(
        order.windows(2).all(|w| w[0] < w[1]),
        "10 after 3, not after 1"
    );
    let none = wd_group("Q900001", "P31").await;
    assert!(none.contains(">No current value</p>"));
    assert!(none.contains("<details class=\"ts-fold ts-fold--deprecated\" open>"));
    assert!(
        none.contains("ts-deprecated__reason"),
        "labeled with its reason"
    );
}

#[tokio::test]
async fn every_mirrored_group_renders_on_its_page() {
    let (status, _, html) = get_from(FixtureApi::wikidata(), "/wiki/Item:WDQ731").await;
    assert_eq!(status, StatusCode::OK);
    let region = wd_group("Q731", "P10685").await;
    assert!(
        html.contains(&region),
        "the region is drawn on its page as served alone"
    );
}

/// Serves the fixture site, the mirrored Wikidata snapshots included, for screenshots
/// and axe: `TS_PREVIEW=127.0.0.1:8099 cargo test -p triplespace-ui --test entity
/// preview -- --ignored`, then open `/wiki/Item:WDQ65`.
#[tokio::test]
#[ignore = "serves until stopped"]
async fn preview() {
    let addr = std::env::var("TS_PREVIEW").unwrap_or_else(|_| "127.0.0.1:8099".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        triplespace_ui::router(Client::new(FixtureApi::wikidata())),
    )
    .await
    .unwrap();
}

/// The redirect a request answers with, if any.
async fn location(uri: &str) -> (StatusCode, Option<String>) {
    let (status, h, _) = get(uri).await;
    (
        status,
        h.get(header::LOCATION)
            .map(|v| v.to_str().unwrap().to_string()),
    )
}

#[tokio::test]
async fn the_header_box_goes_straight_to_an_entity() {
    for (text, to) in [
        ("Q6", "/wiki/Item:Q6"),
        ("q6", "/wiki/Item:Q6"),
        ("P3", "/wiki/Property:P3"),
        ("Item:Q6", "/wiki/Item:Q6"),
        ("WDQ65", "/wiki/Item:WDQ65"),
        ("wikipedia.org", "/wiki/Domain:wikipedia.org"),
        ("Domain:Wikipedia.org", "/wiki/Domain:wikipedia.org"),
    ] {
        let (status, loc) = location(&format!(
            "/w/index.php?title=Special:Search&search={}",
            text.replace(':', "%3A")
        ))
        .await;
        assert_eq!(status, StatusCode::FOUND, "{text}");
        assert_eq!(loc.as_deref(), Some(to), "{text}");
    }
}

#[tokio::test]
async fn search_results_are_grouped_by_kind() {
    let (status, h, html) = get("/w/index.php?title=Special:Search&search=six&fulltext=1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        h[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .starts_with("public")
    );
    assert!(html.contains("<title>Search – librarybase</title>"));
    assert!(html.contains("href=\"/wiki/Item:Q6\" class=\"ts-hit__title\""));
    assert!(html.contains(">Items</h2>"));
    assert!(
        !html.contains(">Properties</h2>"),
        "a kind with no results is left out"
    );
    assert!(html.contains("value=\"six\""), "the boxes keep the text");
    insta::assert_snapshot!("search_six", main_of(&html));

    let (_, _, alias) = get("/w/index.php?title=Special:Search&search=half&fulltext=1").await;
    assert!(alias.contains("Also known as <bdi><span>half dozen</span></bdi>"));

    let (_, _, props) = get("/w/index.php?title=Special:Search&search=in&fulltext=1&ns122=1").await;
    assert!(props.contains(">Properties</h2>"));
    assert!(props.contains(">instance of<"));
    assert!(!props.contains(">Items</h2>"));
    assert!(props.contains("aria-current=\"page\">Properties</a>"));
}

#[tokio::test]
async fn an_id_is_offered_or_reported_missing() {
    let (_, _, html) = get("/w/index.php?title=Special:Search&search=Q6&fulltext=1").await;
    assert!(html.contains(">Go to Item:Q6</a>"));
    let (status, _, missing) = get("/w/index.php?title=Special:Search&search=Q404").await;
    assert_eq!(status, StatusCode::OK);
    assert!(missing.contains("There is no entity at Item:Q404."));
    let (status, _, word) = get("/w/index.php?title=Special:Search&search=Paris").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a word is searched, not taken for an ID"
    );
    assert!(!word.contains("There is no entity"));
    assert!(word.contains("No items, properties or domains match “Paris”."));
    let (_, _, empty) = get("/wiki/Special:Search").await;
    assert!(empty.contains("class=\"ts-search-intro\""));
}

#[tokio::test]
async fn search_pages_through_one_kind() {
    let (_, _, first) =
        get("/w/index.php?title=Special:Search&search=&fulltext=1&ns122=1&limit=5").await;
    assert!(
        first.contains("class=\"ts-search-intro\""),
        "no text, no search"
    );
    let (_, _, page1) =
        get("/w/index.php?title=Special:Search&search=p&fulltext=1&ns122=1&limit=1").await;
    assert!(page1.contains("rel=\"next\""), "{page1}");
    assert!(!page1.contains("rel=\"prev\""));
    let (_, _, page2) =
        get("/w/index.php?title=Special:Search&search=p&fulltext=1&ns122=1&limit=1&offset=1").await;
    assert!(page2.contains("rel=\"prev\""));
}

async fn get_signed_in(uri: &str) -> (StatusCode, HeaderMap, String) {
    let req = Request::get(uri)
        .header(header::HOST, "librarybase.org")
        .header(header::COOKIE, "triplespace_session=alice")
        .body(Body::empty())
        .unwrap();
    let r = triplespace_ui::router(Client::new(FixtureApi::new()))
        .oneshot(req)
        .await
        .unwrap();
    let status = r.status();
    let headers = r.headers().clone();
    let body = r.into_body().collect().await.unwrap().to_bytes();
    (status, headers, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn a_signed_in_viewer_gets_the_edit_buttons_and_data_on_a_local_entity() {
    let (status, h, html) = get_signed_in("/wiki/Item:Q6").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(h[header::CACHE_CONTROL], "private, no-cache");
    assert!(html.contains("data-ts-edit=\"terms\""));
    assert!(
        html.contains(" hidden>Edit</button>"),
        "hidden until the script shows it"
    );
    let start = html
        .find("<script type=\"application/json\" id=\"ts-edit-data\">")
        .expect("the data block");
    let rest = &html[start..];
    let end = rest.find("</script>").unwrap();
    let json_text = &rest[rest.find('>').unwrap() + 1..end];
    let data: Value = serde_json::from_str(&json_text.replace("<\\/", "</")).unwrap();
    assert_eq!(data["id"], "Q6");
    assert_eq!(data["title"], "Item:Q6");
    assert_eq!(data["messages"]["ts-edit-save"], "Save");
    let (_, _, region) = {
        let req = "/w/index.php?title=Item:Q6&action=render&region=terms";
        get_signed_in(req).await
    };
    assert!(
        region.contains("data-ts-edit=\"terms\""),
        "the region keeps its button"
    );
    assert!(html.contains(
        "data-ts-edit=\"group\" data-property=\"P3\" aria-label=\"Edit the values of population\""
    ));
    assert!(html.contains("data-ts-edit=\"add-statement\""));
    assert_eq!(data["conceptBase"], "https://librarybase.org/entity/");
    let (_, _, group) =
        get_signed_in("/w/index.php?title=Item:Q6&action=render&region=statements/P3").await;
    assert!(
        group.contains("data-ts-edit=\"group\""),
        "a group's region keeps its button"
    );

    let (_, _, foreign) = get_signed_in("/wiki/Item:WDQ65").await;
    assert!(
        !foreign.contains("ts-edit-data"),
        "a mirrored entity is not edited here"
    );
    let (_, _, anon) = get("/wiki/Item:Q6").await;
    assert!(
        !anon.contains("ts-edit"),
        "nor by an anonymous reader, whose page is public"
    );
}

#[tokio::test]
async fn the_history_tab_lists_the_entitys_rows() {
    // A local item: one source, so no switch; the adoption and the bot's edit.
    let (status, h, html) = get("/wiki/Item:Q6?action=history").await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("<title>Six &#38; more (Q6) – librarybase</title>"));
    assert!(h["cache-tag"].to_str().unwrap().contains("entity:Q6"));
    let main = main_of(&html);
    assert!(main.contains("aria-current=\"page\">History</a>"), "{main}");
    assert!(main.contains("ts-change--edit"));
    assert!(
        !main.contains("ts-history__switch"),
        "one source: no switch"
    );
    assert!(!main.contains("ts-history__note"));
    assert!(main.contains(">adopt<"), "the adoption's row");
    assert!(main.contains("href=\"/wiki/Special:Contributions/Alice_bot\""));
    insta::assert_snapshot!("history_item", main);

    // A mirrored domain: the provider's sync with its chip, the latest-only note; the
    // switch needs two sources, which this fixture's provenance does not have.
    let (status, _, html) = get("/wiki/Domain:wikipedia.org?action=history").await;
    assert_eq!(status, StatusCode::OK);
    let main = main_of(&html);
    assert!(main.contains("ts-change--sync"), "{main}");
    assert!(main.contains("ts-chip--p-xd"), "the provider's chip");
    assert!(
        main.contains("Only the latest state from Internet Domains Wikibase is kept here"),
        "{main}"
    );
    insta::assert_snapshot!("history_domain", main);

    // The source switch narrows the feed; the fake API honours `source`.
    let (_, _, html) = get("/wiki/Domain:wikipedia.org?action=history&source=local").await;
    assert!(main_of(&html).contains("No history is held for this entity."));

    // An unknown entity's history is the entity's own 404.
    let (status, _, _) = get("/wiki/Item:Q404?action=history").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_diff_page_draws_a_change_as_statements() {
    // `diff={revid}` resolves the revision to its record, then draws the change.
    let (status, h, html) = get("/wiki/Item:Q6?diff=907").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(h["cache-tag"].to_str().unwrap().contains("entity:Q6"));
    let main = main_of(&html);
    assert!(main.contains("aria-current=\"page\">History</a>"));
    assert!(main.contains("Revision 907"));
    assert!(main.contains("rank and a value"));
    // Terms as keyed lists, with the marks.
    assert!(
        main.contains("ts-diff__cell--added\">\n<span lang=\"fr\" dir=\"auto\">Six</span>"),
        "{main}"
    );
    assert!(main.contains("eine gerade Zahl"));
    assert!(main.contains("6 | half dozen | VI"));
    assert!(main.contains(">enwiki<"));
    // Statements: the changed rank shows both sides, the unchanged one is context, the
    // added one has a label from the lookup.
    assert!(main.contains("ts-diff__row--changed"));
    assert!(main.contains("ts-diff__row--unchanged"));
    assert!(main.contains("ts-diff__row--added"));
    assert!(main.contains("Preferred rank"), "the rank after: {main}");
    assert!(
        main.contains("href=\"/wiki/Item:Q2\""),
        "the value's entity is linked: {main}"
    );
    // "About this edit" and the links around the change.
    assert!(main.contains("About this edit"));
    assert!(main.contains("4098:9"));
    assert!(main.contains("href=\"/wiki/Item:Q6?diff=906\""));
    assert!(main.contains("href=\"/wiki/Item:Q6?oldid=906&amp;diff=907\""));
    insta::assert_snapshot!("diff_change", main);

    // The change-set view shows the operation as stored.
    let (_, _, html) = get("/wiki/Item:Q6?diff=907&view=changeset").await;
    let main = main_of(&html);
    assert!(main.contains("ts-diff__changeset"));
    assert!(main.contains("&quot;op&quot;: &quot;add&quot;"), "{main}");

    // A comparison between two revisions.
    let (status, _, html) = get("/wiki/Item:Q6?oldid=900&diff=907").await;
    assert_eq!(status, StatusCode::OK);
    let main = main_of(&html);
    assert!(main.contains("Revision 900") && main.contains("Revision 907"));
    assert!(main.contains("ts-diff__row--changed"));
    assert!(
        !main.contains("About this edit"),
        "a comparison is not one edit"
    );

    // A sync: the upstream versions and the stored summary, no statement diff.
    let (status, _, html) = get("/wiki/Domain:wikipedia.org?diff=8194:41").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let main = main_of(&html);
    assert!(
        main.contains("Synced from upstream revision 56 to 57."),
        "{main}"
    );
    assert!(main.contains("What the sync changed"));
    assert!(main.contains("href=\"/wiki/Property:P12\""));
    insta::assert_snapshot!("diff_sync", main);

    // Unknown revisions.
    let (status, _, html) = get("/wiki/Item:Q6?diff=1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("There is no such revision of this entity."));
    let (status, _, _) = get("/wiki/Item:Q6?oldid=1&diff=907").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_old_revision_is_the_page_as_it_then_was() {
    let (status, _, html) = get("/wiki/Item:Q6?oldid=907").await;
    assert_eq!(status, StatusCode::OK, "{html}");
    let main = main_of(&html);
    assert!(
        main.contains(
            "This is revision 907 of this entity, as Alice bot saved it on 2026-10-02 14:05:11."
        ),
        "{main}"
    );
    assert!(main.contains("href=\"/wiki/Item:Q6\">Current version</a>"));
    assert!(
        main.contains("data-region=\"statements\"") || main.contains("ts-group"),
        "the statements tab: {main}"
    );
    let (status, _, html) = get("/wiki/Item:Q6?oldid=1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(html.contains("Revision 1 is not a local revision of this entity."));
}
