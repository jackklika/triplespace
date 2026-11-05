//! The report pages (0047 §4; 21 §2): `Special:WantedPages`, `Special:ShortPages`,
//! `Special:ListProperties` and the rest of [`REPORTS`], one page for all of them, from
//! `list=querypage`.
//!
//! Each is served under its name, its MediaWiki name and its aliases (`Special:Wantedpages`,
//! `Special:BrokenLinks`), in any case, as MediaWiki resolves them. MediaWiki's `limit`
//! (50 by default, at most 500) and `offset` page through the rows; a row shows its value
//! as the report measures it (links, members, categories, revisions, bytes, a time), a
//! redirect its target, a property its label. A title that does not exist (a wanted page,
//! a broken redirect's target) is a red link.
//!
//! Above the list, the report says how fresh it is: a live query says nothing more; a
//! report kept as pages change says so, and when changes are still being applied, how
//! many. `Special:LonelyPages` says that only links count, since transclusions are not
//! counted yet.

use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::{ReportPage, ReportQuery, ReportRow};

use crate::Site;
use crate::features::{Feature, Features};
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Params, Peer};
use crate::special::changes::{clock, date};

/// Rows to a page unless asked, as MediaWiki shows.
const LIMIT: u32 = 50;

/// The most rows to a page.
const MAX_LIMIT: u32 = 500;

/// The page sizes offered.
const SIZES: [u32; 5] = [20, 50, 100, 250, 500];

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/report.html` draws, one model for every report.
// ---------------------------------------------------------------------------------------

/// A link to a title, red where the title does not exist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TitleLink {
    /// The page.
    pub href: String,
    /// The text.
    pub text: String,
    /// Whether the title does not exist.
    pub missing: bool,
}

/// One row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportRowModel {
    /// The title, or a property's label.
    pub link: TitleLink,
    /// A labelled property's ID, after its label.
    pub id: Option<String>,
    /// A redirect's target.
    pub target: Option<TitleLink>,
    /// A double redirect's last page.
    pub last: Option<TitleLink>,
    /// The value the report measures, as text, where it has one.
    pub value: Option<String>,
}

/// A step of the pager: a link, or text where there is nowhere to go.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The text.
    pub label: String,
    /// The page, where it links.
    pub href: Option<String>,
}

/// **Previous**, **Next** and the page sizes, as MediaWiki's `viewprevnext` offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportPager {
    /// The accessible name.
    pub label: String,
    /// The previous page.
    pub previous: Step,
    /// The next page.
    pub next: Step,
    /// The page sizes, the current one as text.
    pub sizes: Vec<Step>,
}

/// A report page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReportPageModel {
    /// The report's note, where it has one.
    pub note: Option<String>,
    /// How fresh the report is, where it is kept rather than live.
    pub state: Option<String>,
    /// What the page says when it has no rows.
    pub empty: Option<String>,
    /// "Showing n results starting with #m".
    pub range: Option<String>,
    /// The first row's number.
    pub start: u32,
    /// The rows.
    pub rows: Vec<ReportRowModel>,
    /// The pager, above and below the rows; below the empty notice past the first page.
    pub pager: Option<ReportPager>,
    /// The arrow between a redirect and its target, in the page's direction.
    pub arrow: String,
}

/// What a row's value counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Unit {
    /// Nothing shown.
    None,
    /// Links to the title.
    Links,
    /// Pages in the category.
    Members,
    /// Categories the page is in.
    Categories,
    /// The page's revisions.
    Revisions,
    /// The page's size.
    Bytes,
    /// When the page was last edited.
    Time,
}

/// A report the site offers a page for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    /// The registry's name: the page's title and what `qppage` is given.
    pub name: &'static str,
    /// MediaWiki's name, where it differs.
    pub mediawiki_name: Option<&'static str>,
    /// Other names the page answers to.
    pub aliases: &'static [&'static str],
    /// Whether it lists document pages, and so needs them.
    pub pages: bool,
    unit: Unit,
    /// Whether its titles do not exist (red links).
    missing: bool,
    /// Whether its redirect targets do not exist.
    missing_target: bool,
}

const fn report(name: &'static str, mediawiki_name: &'static str, unit: Unit) -> Report {
    Report {
        name,
        mediawiki_name: Some(mediawiki_name),
        aliases: &[],
        pages: true,
        unit,
        missing: false,
        missing_target: false,
    }
}

/// The reports, in `Special:SpecialPages` order.
pub const REPORTS: &[Report] = &[
    Report {
        aliases: &["BrokenLinks"],
        missing: true,
        ..report("WantedPages", "Wantedpages", Unit::Links)
    },
    Report {
        missing: true,
        ..report("WantedCategories", "Wantedcategories", Unit::Members)
    },
    Report {
        aliases: &["MostLinked"],
        ..report("MostLinkedPages", "Mostlinked", Unit::Links)
    },
    Report {
        aliases: &["MostUsedCategories"],
        ..report(
            "MostLinkedCategories",
            "Mostlinkedcategories",
            Unit::Members,
        )
    },
    report("MostCategories", "Mostcategories", Unit::Categories),
    report("MostRevisions", "Mostrevisions", Unit::Revisions),
    report("FewestRevisions", "Fewestrevisions", Unit::Revisions),
    report("DeadendPages", "Deadendpages", Unit::None),
    Report {
        aliases: &["OrphanedPages"],
        ..report("LonelyPages", "Lonelypages", Unit::None)
    },
    report("UncategorizedPages", "Uncategorizedpages", Unit::None),
    report(
        "UncategorizedCategories",
        "Uncategorizedcategories",
        Unit::None,
    ),
    report(
        "UncategorizedTemplates",
        "Uncategorizedtemplates",
        Unit::None,
    ),
    report("UnusedCategories", "Unusedcategories", Unit::None),
    report("ShortPages", "Shortpages", Unit::Bytes),
    report("LongPages", "Longpages", Unit::Bytes),
    report("AncientPages", "Ancientpages", Unit::Time),
    report("ListRedirects", "Listredirects", Unit::None),
    Report {
        mediawiki_name: None,
        missing_target: true,
        ..report("BrokenRedirects", "", Unit::None)
    },
    Report {
        mediawiki_name: None,
        ..report("DoubleRedirects", "", Unit::None)
    },
    Report {
        mediawiki_name: None,
        pages: false,
        ..report("ListProperties", "", Unit::None)
    },
];

impl Report {
    /// The report a special page's name names: its name, MediaWiki name or an alias, in
    /// any case.
    #[must_use]
    pub fn of(name: &str) -> Option<&'static Self> {
        REPORTS.iter().find(|r| {
            r.name.eq_ignore_ascii_case(name)
                || r.mediawiki_name
                    .is_some_and(|m| m.eq_ignore_ascii_case(name))
                || r.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
    }

    /// The page's title.
    #[must_use]
    pub fn title(&self) -> String {
        format!("Special:{}", self.name)
    }

    /// The message stem: `ts-report-wantedpages`.
    fn key(&self) -> String {
        format!("ts-report-{}", self.name.to_ascii_lowercase())
    }

    /// Its label on `Special:SpecialPages` and its page's title.
    #[must_use]
    pub fn label_key(&self) -> String {
        format!("{}-title", self.key())
    }

    /// Its description on `Special:SpecialPages`.
    #[must_use]
    pub fn description_key(&self) -> String {
        format!("ts-specialpages-{}", self.name.to_ascii_lowercase())
    }

    /// Whether the site offers it here.
    #[must_use]
    pub fn offered(&self, features: &Features) -> bool {
        features.has(Feature::Reports) && (!self.pages || features.has(Feature::Pages))
    }
}

/// What the page was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Window {
    offset: u32,
    limit: u32,
}

impl Window {
    fn of(params: &Params) -> Self {
        let number = |k: &str| params.map.get(k).and_then(|v| v.trim().parse::<u32>().ok());
        Self {
            offset: number("offset").unwrap_or(0),
            limit: number("limit").unwrap_or(LIMIT).clamp(1, MAX_LIMIT),
        }
    }

    fn href(self, report: &Report) -> String {
        let mut s = format!("/w/index.php?title={}", report.title());
        if self.limit != LIMIT {
            let _ = write!(s, "&limit={}", self.limit);
        }
        if self.offset > 0 {
            let _ = write!(s, "&offset={}", self.offset);
        }
        s
    }
}

/// `GET Special:{report}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    report: &'static Report,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !report.offered(&cx.features) {
        return pages::no_page(&cx, headers, report.title());
    }
    let window = Window::of(params);
    let found = match site
        .client()
        .report(
            &incoming,
            &ReportQuery {
                name: report.name.to_string(),
                offset: window.offset,
                limit: window.limit,
            },
        )
        .await
    {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(found.cache);
    let body = cx.draw(
        "special/report.html",
        page_model(&cx.m, report, window, &found.value),
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(&report.label_key()),
        returnto: Some(report.title()),
        body,
        ..Page::default()
    })
}

/// The report's note, where it has one.
fn note(m: &Messages, report: &Report) -> Option<String> {
    let note = format!("{}-note", report.key());
    m.has(&note).then(|| m.get(&note))
}

/// How fresh the report is: nothing for a live query; for a kept one, since when, or
/// how many changes it is still applying.
fn freshness(m: &Messages, page: &ReportPage) -> Option<String> {
    let state = &page.state;
    if state.backing != "projection" {
        None
    } else if state.waiting > 0 {
        Some(m.with("ts-report-catching-up", &[&state.waiting.to_string()]))
    } else if let Some(at) = &state.computed_at {
        Some(m.with(
            "ts-report-kept-since",
            &[&format!("{}, {}", clock(at), date(m, at))],
        ))
    } else {
        Some(m.get("ts-report-kept"))
    }
}

/// The arrow between a redirect and its target, in the page's direction.
fn arrow(m: &Messages) -> &'static str {
    if m.dir() == "rtl" {
        " \u{2190} "
    } else {
        " \u{2192} "
    }
}

/// A report page's model.
fn page_model(m: &Messages, report: &Report, window: Window, page: &ReportPage) -> ReportPageModel {
    let first = window.offset + 1;
    let count = u32::try_from(page.rows.len()).unwrap_or(u32::MAX);
    let empty = page.rows.is_empty();
    ReportPageModel {
        note: note(m, report),
        state: freshness(m, page),
        empty: empty.then(|| m.get("ts-report-none")),
        range: (!empty).then(|| {
            m.with(
                "ts-report-showing",
                &[
                    &count.to_string(),
                    &first.to_string(),
                    &(window.offset + count).to_string(),
                ],
            )
        }),
        start: first,
        rows: page.rows.iter().map(|r| row_model(m, report, r)).collect(),
        pager: (!empty || window.offset > 0)
            .then(|| pager_model(m, report, window, if empty { None } else { page.next })),
        arrow: arrow(m).to_string(),
    }
}

/// A link to a title's model, red where it does not exist.
fn title_link_model(title: &str, fragment: Option<&str>, missing: bool) -> TitleLink {
    let mut href = format!("/wiki/{}", title_url(title));
    let mut text = title.to_string();
    if let Some(f) = fragment.filter(|f| !f.is_empty()) {
        let _ = write!(href, "#{}", title_url(f));
        let _ = write!(text, "#{f}");
    }
    TitleLink {
        href,
        text,
        missing,
    }
}

/// One row's model.
fn row_model(m: &Messages, report: &Report, row: &ReportRow) -> ReportRowModel {
    if report.name == "ListProperties" {
        let id = row
            .title
            .rsplit_once(':')
            .map_or(row.title.as_str(), |(_, id)| id);
        let href = format!("/wiki/{}", title_url(&row.title));
        let (text, id) = match row.label.as_deref().filter(|l| !l.is_empty()) {
            Some(label) => (label.to_string(), Some(id.to_string())),
            None => (id.to_string(), None),
        };
        return ReportRowModel {
            link: TitleLink {
                href,
                text,
                missing: false,
            },
            id,
            target: None,
            last: None,
            value: None,
        };
    }
    let value = (!row.value.is_empty())
        .then(|| match report.unit {
            Unit::None => None,
            Unit::Time => Some(format!("{}, {}", clock(&row.value), date(m, &row.value))),
            unit => Some(m.with(unit_key(unit)?, &[&row.value])),
        })
        .flatten();
    ReportRowModel {
        link: title_link_model(&row.title, None, report.missing),
        id: None,
        target: row
            .target
            .as_deref()
            .map(|t| title_link_model(t, row.fragment.as_deref(), report.missing_target)),
        last: row
            .last
            .as_deref()
            .map(|l| title_link_model(l, None, false)),
        value,
    }
}

/// The message a value is shown in.
fn unit_key(unit: Unit) -> Option<&'static str> {
    match unit {
        Unit::None | Unit::Time => None,
        Unit::Links => Some("ts-report-links"),
        Unit::Members => Some("ts-report-members"),
        Unit::Categories => Some("ts-report-categories"),
        Unit::Revisions => Some("ts-report-revisions"),
        Unit::Bytes => Some("ts-report-bytes"),
    }
}

/// The pager's model.
fn pager_model(m: &Messages, report: &Report, window: Window, next: Option<u32>) -> ReportPager {
    let size = window.limit.to_string();
    ReportPager {
        label: m.get("ts-report-pages-label"),
        previous: Step {
            label: m.with("ts-report-previous", &[&size]),
            href: (window.offset > 0).then(|| {
                Window {
                    offset: window.offset.saturating_sub(window.limit),
                    ..window
                }
                .href(report)
            }),
        },
        next: Step {
            label: m.with("ts-report-next", &[&size]),
            href: next.map(|offset| Window { offset, ..window }.href(report)),
        },
        sizes: SIZES
            .iter()
            .map(|&limit| Step {
                label: limit.to_string(),
                href: (limit != window.limit).then(|| Window { limit, ..window }.href(report)),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_aliases_and_case() {
        assert_eq!(Report::of("WantedPages").unwrap().name, "WantedPages");
        assert_eq!(Report::of("Wantedpages").unwrap().name, "WantedPages");
        assert_eq!(Report::of("BrokenLinks").unwrap().name, "WantedPages");
        assert_eq!(Report::of("Mostlinked").unwrap().name, "MostLinkedPages");
        assert_eq!(Report::of("OrphanedPages").unwrap().name, "LonelyPages");
        assert_eq!(
            Report::of("ListProperties").unwrap().title(),
            "Special:ListProperties"
        );
        assert!(Report::of("AllPages").is_none());
    }

    #[derive(serde::Deserialize)]
    struct Registry {
        page: Vec<Entry>,
    }

    #[derive(serde::Deserialize)]
    struct Entry {
        name: String,
        mediawiki_name: Option<String>,
        #[serde(default)]
        aliases: Vec<String>,
        status: String,
        report: Option<toml::Value>,
    }

    /// Every report page is a served report of the registry, under its names.
    #[test]
    fn the_reports_follow_the_registry() {
        let r: Registry =
            toml::from_str(include_str!("../../../../docs/registry/special-pages.toml"))
                .expect("special-pages.toml parses");
        for report in REPORTS {
            let e = r
                .page
                .iter()
                .find(|p| p.name == report.name)
                .unwrap_or_else(|| panic!("{} is not in the registry", report.name));
            assert!(e.report.is_some(), "{} is not a report", report.name);
            assert_eq!(e.status, "served", "{}", report.name);
            assert_eq!(
                e.mediawiki_name.as_deref(),
                report.mediawiki_name,
                "{}",
                report.name
            );
            let aliases: Vec<&str> = e.aliases.iter().map(String::as_str).collect();
            assert_eq!(aliases, report.aliases, "{}", report.name);
        }
    }

    fn row(title: &str, value: &str) -> ReportRow {
        ReportRow {
            title: title.into(),
            value: value.into(),
            ..ReportRow::default()
        }
    }

    /// A report's model (19 §5.10): the range, the pager both sides, a row's value in
    /// its unit, a missing title red, a redirect's target with its fragment, a labelled
    /// property with its ID.
    #[test]
    fn a_report_is_a_model() {
        let m = Messages::for_language("en");
        let short = Report::of("ShortPages").unwrap();
        let page = ReportPage {
            rows: vec![row("Project:Stub", "6"), row("Project:Home", "120")],
            next: Some(2),
            ..ReportPage::default()
        };
        let w = Window {
            offset: 0,
            limit: 2,
        };
        let p = page_model(&m, short, w, &page);
        assert_eq!(p.start, 1);
        assert_eq!(p.rows[0].value.as_deref(), Some("6 bytes"));
        assert!(!p.rows[0].link.missing);
        let pager = p.pager.unwrap();
        assert!(pager.previous.href.is_none());
        assert_eq!(
            pager.next.href.as_deref(),
            Some("/w/index.php?title=Special:ShortPages&limit=2&offset=2")
        );
        assert_eq!(
            pager.sizes.iter().filter(|s| s.href.is_none()).count(),
            0,
            "2 is not one of the sizes, so every size links"
        );

        let wanted = Report::of("WantedPages").unwrap();
        let p = page_model(
            &m,
            wanted,
            w,
            &ReportPage {
                rows: vec![row("Project:Style guide", "2")],
                ..ReportPage::default()
            },
        );
        assert!(p.rows[0].link.missing);
        assert_eq!(p.rows[0].value.as_deref(), Some("2 links"));

        let broken = Report::of("BrokenRedirects").unwrap();
        let mut r = row("Project:Old name", "");
        r.target = Some("Project:Never written".into());
        r.fragment = Some("Dates".into());
        let p = page_model(
            &m,
            broken,
            w,
            &ReportPage {
                rows: vec![r],
                ..ReportPage::default()
            },
        );
        let target = p.rows[0].target.as_ref().unwrap();
        assert_eq!(target.href, "/wiki/Project:Never_written#Dates");
        assert_eq!(target.text, "Project:Never written#Dates");
        assert!(target.missing && p.rows[0].value.is_none());

        let props = Report::of("ListProperties").unwrap();
        let mut labelled = row("Property:P3", "");
        labelled.label = Some("population".into());
        let p = page_model(
            &m,
            props,
            w,
            &ReportPage {
                rows: vec![labelled, row("Property:P4", "")],
                ..ReportPage::default()
            },
        );
        assert_eq!(
            (p.rows[0].link.text.as_str(), p.rows[0].id.as_deref()),
            ("population", Some("P3"))
        );
        assert_eq!(
            (p.rows[1].link.text.as_str(), p.rows[1].id.as_deref()),
            ("P4", None)
        );

        let past = page_model(
            &m,
            short,
            Window {
                offset: 500,
                limit: 50,
            },
            &ReportPage::default(),
        );
        assert!(past.empty.is_some() && past.range.is_none());
        assert!(past.pager.unwrap().previous.href.is_some());
        let first = page_model(
            &m,
            short,
            Window {
                offset: 0,
                limit: 50,
            },
            &ReportPage::default(),
        );
        assert!(first.pager.is_none());
    }

    #[test]
    fn windows_link_back_to_the_page() {
        let w = Window {
            offset: 100,
            limit: 20,
        };
        let r = Report::of("ShortPages").unwrap();
        assert_eq!(
            w.href(r),
            "/w/index.php?title=Special:ShortPages&limit=20&offset=100"
        );
        let w = Window {
            offset: 0,
            limit: LIMIT,
        };
        assert_eq!(w.href(r), "/w/index.php?title=Special:ShortPages");
    }
}
