//! Golden tests: every revision of Librarybase's wikitext pages, rendered here and by
//! Librarybase itself (MediaWiki 1.39 `action=parse`, fetched by
//! `tools/librarybase_parse.py`), compared after both are parsed as HTML5 and normalized
//! (subset profile, `claude/librarybase-wikitext-subset.md`).
//!
//! **Normalization.** Both sides go through html5ever, as a browser would read them.
//! Attributes are sorted and class lists sorted; whitespace runs in text are collapsed
//! and whitespace-only text dropped, since paragraph breaks and indentation are
//! MediaWiki's formatting, not content.
//!
//! **The resolver** is built from the fixtures alone: existence from each parse's
//! `links`, redirects and entity labels from the anchors in Librarybase's HTML, URLs in
//! MediaWiki's `/wiki/` and `/w/index.php` forms.
//!
//! **Expansion** is the thin slice of `scatter-wikitext-expand`: parameterless
//! transclusion with `noinclude`, `includeonly` and `onlyinclude`, templates read from the
//! fixtures' own Template pages at their latest revision, as `action=parse` of an old
//! revision reads the current templates.
//!
//! **Known differences** are listed in `known-differences.txt` with their reason; the test
//! fails if one of them starts matching, so the list stays true.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use html5ever::tendril::TendrilSink as _;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use scatter_wikitext::html::url_encode_title;
use scatter_wikitext::{Config, ConfigSource, Interwiki, Namespace, PageInfo, Resolver, Title};
use scatter_wikitext_expand::{ExpandHost, Transcluded, expand_for_view};

/// Librarybase's pages and its own renders of them (`docs/samples/librarybase/parse/`).
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/samples/librarybase/parse")
}

/// This renderer's expected departures from Librarybase's renders, beside the test.
fn known_differences() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/librarybase/known-differences.txt")
}

/// Librarybase's namespaces: MediaWiki's, Wikibase's Item and Property, EntitySchema,
/// Module.
fn librarybase() -> Config {
    let mut s = ConfigSource::default();
    s.namespaces.extend([
        Namespace::new(120, "Item"),
        Namespace::new(121, "Item talk"),
        Namespace::new(122, "Property"),
        Namespace::new(123, "Property talk"),
        Namespace::new(640, "EntitySchema"),
        Namespace::new(641, "EntitySchema talk"),
        Namespace::new(828, "Module"),
        Namespace::new(829, "Module talk"),
    ]);
    s.interwiki.push(Interwiki {
        prefix: "wikipedia".into(),
        url: "https://en.wikipedia.org/wiki/$1".into(),
    });
    Config::new(s)
}

struct Fixture {
    config: Config,
    exists: HashMap<String, bool>,
    redirects: HashSet<String>,
    labels: HashMap<String, String>,
    /// Template pages' latest text, by title.
    templates: HashMap<String, String>,
}

impl Resolver for Fixture {
    fn info(&self, t: &Title) -> PageInfo {
        let key = t.prefixed(&self.config);
        PageInfo {
            // Special pages always exist.
            exists: t.namespace == -1 || self.exists.get(&key).copied().unwrap_or(false),
            redirect: self.redirects.contains(&key),
            label: self.labels.get(&key).cloned(),
        }
    }
    fn url(&self, t: &Title) -> String {
        format!("/wiki/{}", url_encode_title(&t.prefixed(&self.config)))
    }
    fn missing_url(&self, t: &Title) -> String {
        format!(
            "/w/index.php?title={}&action=edit&redlink=1",
            url_encode_title(&t.prefixed(&self.config))
        )
    }
}

impl ExpandHost for Fixture {
    fn transclude(&self, name: &str) -> Option<Transcluded> {
        let page = Title::new(10, "");
        let target = if let Some(main) = name.strip_prefix(':') {
            scatter_wikitext::parse_target(&format!(":{main}"), &self.config, &page)?
        } else {
            let t = scatter_wikitext::parse_target(name, &self.config, &page)?;
            if t.title.namespace == 0 {
                scatter_wikitext::parse_target(&format!("Template:{name}"), &self.config, &page)?
            } else {
                t
            }
        };
        let title = target.title.prefixed(&self.config);
        Some(match self.templates.get(&title) {
            Some(text) => Transcluded::Found(text.clone()),
            None => Transcluded::Missing(title),
        })
    }
}

struct Revision {
    title: String,
    ns: i32,
    pageid: u64,
    revid: u64,
}

fn load() -> (Fixture, Vec<Revision>) {
    let dir = fixtures();
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("manifest.json")).expect("manifest"),
    )
    .expect("json");
    let mut revisions = Vec::new();
    let mut exists = HashMap::new();
    let mut redirects = HashSet::new();
    let mut labels = HashMap::new();
    let mut templates = HashMap::new();
    for p in manifest["pages"].as_array().expect("pages") {
        let ns = i32::try_from(p["ns"].as_i64().expect("ns")).expect("small");
        let title = p["title"].as_str().expect("title").to_string();
        let pageid = p["pageid"].as_u64().expect("pageid");
        exists.insert(title.clone(), true);
        if p["redirect"].as_bool() == Some(true) {
            redirects.insert(title.clone());
        }
        let latest = p["lastrevid"].as_u64().expect("lastrevid");
        if ns == 10 {
            let text = std::fs::read_to_string(dir.join(format!("pages/{pageid}/{latest}.wiki")))
                .expect("template");
            templates.insert(title.clone(), text);
        }
        for r in p["revisions"].as_array().expect("revisions") {
            let revid = r["revid"].as_u64().expect("revid");
            let base = dir.join(format!("pages/{pageid}/{revid}"));
            let meta: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(base.with_extension("json")).expect("json"),
            )
            .expect("json");
            for l in meta["links"].as_array().into_iter().flatten() {
                let t = l["title"].as_str().expect("title").to_string();
                let e = l["exists"].as_bool().unwrap_or(false);
                exists.entry(t).and_modify(|x| *x |= e).or_insert(e);
            }
            let html = std::fs::read_to_string(base.with_extension("html")).expect("html");
            scan_anchors(&html, &mut redirects, &mut labels);
            // The MediaWiki namespace holds interface messages, which Triplespace keeps
            // reserved (11 §2.3): they are not pages it renders.
            if ns != 8 {
                revisions.push(Revision {
                    title: title.clone(),
                    ns,
                    pageid,
                    revid,
                });
            }
        }
    }
    let fixture = Fixture {
        config: librarybase(),
        exists,
        redirects,
        labels,
        templates,
    };
    (fixture, revisions)
}

/// Reads redirect flags and entity labels out of Librarybase's anchors.
fn scan_anchors(html: &str, redirects: &mut HashSet<String>, labels: &mut HashMap<String, String>) {
    for chunk in html.split("<a ").skip(1) {
        let Some(close) = chunk.find('>') else {
            continue;
        };
        let attrs = &chunk[..close];
        let text_end = chunk[close..].find("</a>").map_or(close, |e| close + e);
        let text = &chunk[close + 1..text_end];
        let attr = |name: &str| {
            let key = format!("{name}=\"");
            attrs.find(&key).map(|i| {
                let v = &attrs[i + key.len()..];
                v[..v.find('"').unwrap_or(v.len())]
                    .replace("&amp;", "&")
                    .replace("&quot;", "\"")
            })
        };
        let Some(title) = attr("title") else { continue };
        if attr("class").is_some_and(|c| c.split(' ').any(|c| c == "mw-redirect")) {
            redirects.insert(title.clone());
        }
        if let Some(id) = title
            .strip_prefix("Item:")
            .or_else(|| title.strip_prefix("Property:"))
            && text.ends_with(&format!(" ({id})"))
            && !text.contains('<')
        {
            labels.insert(title.clone(), text.replace("&amp;", "&"));
        }
    }
}

// ---- normalization ------------------------------------------------------------------

fn normalize(html: &str) -> String {
    let dom =
        html5ever::parse_document(RcDom::default(), html5ever::ParseOpts::default()).one(html);
    let mut out = String::new();
    // document > html > body
    let body = find(&dom.document, "body").expect("body");
    for c in body.children.borrow().iter() {
        walk(c, 0, &mut out);
    }
    out
}

fn find(h: &Handle, name: &str) -> Option<Handle> {
    if let NodeData::Element { name: n, .. } = &h.data
        && &*n.local == name
    {
        return Some(h.clone());
    }
    h.children.borrow().iter().find_map(|c| find(c, name))
}

fn walk(h: &Handle, depth: usize, out: &mut String) {
    match &h.data {
        NodeData::Text { contents } => {
            let t = contents.borrow();
            let collapsed = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if !collapsed.is_empty() {
                out.push_str(&"  ".repeat(depth));
                let _ = writeln!(out, "{collapsed:?}");
            }
        }
        NodeData::Element { name, attrs, .. } => {
            let mut a: Vec<(String, String)> = attrs
                .borrow()
                .iter()
                .map(|a| {
                    let k = a.name.local.to_string();
                    let mut v = a.value.to_string();
                    if k == "class" {
                        let mut c: Vec<&str> = v.split_whitespace().collect();
                        c.sort_unstable();
                        v = c.join(" ");
                    }
                    (k, v)
                })
                .collect();
            a.sort();
            out.push_str(&"  ".repeat(depth));
            out.push('<');
            out.push_str(&name.local);
            for (k, v) in a {
                let _ = write!(out, " {k}={v:?}");
            }
            out.push_str(">\n");
            for c in h.children.borrow().iter() {
                walk(c, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn first_difference(a: &str, b: &str) -> String {
    let (al, bl): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    let i = al
        .iter()
        .zip(&bl)
        .position(|(x, y)| x != y)
        .unwrap_or(al.len().min(bl.len()));
    let from = i.saturating_sub(3);
    let show = |v: &[&str]| v[from.min(v.len())..(i + 4).min(v.len())].join("\n");
    format!(
        "line {}\n--- ours\n{}\n--- librarybase\n{}",
        i + 1,
        show(&al),
        show(&bl)
    )
}

fn known() -> BTreeMap<u64, String> {
    std::fs::read_to_string(known_differences())
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (id, why) = l.split_once(char::is_whitespace)?;
            Some((id.parse().ok()?, why.trim().to_string()))
        })
        .collect()
}

#[test]
fn librarybase_renders_as_librarybase_does() {
    let (fixture, revisions) = load();
    let known = known();
    let mut failures = Vec::new();
    let mut now_passing = Vec::new();
    let mut passed = 0;
    for r in &revisions {
        let base = fixtures().join(format!("pages/{}/{}", r.pageid, r.revid));
        let source = std::fs::read_to_string(base.with_extension("wiki")).expect("wiki");
        let expected = std::fs::read_to_string(base.with_extension("html")).expect("html");
        let text_title = r
            .title
            .split_once(':')
            .filter(|_| r.ns != 0)
            .map_or(r.title.as_str(), |(_, t)| t);
        let page = Title::new(r.ns, text_title);
        let expanded = expand_for_view(&source, &fixture);
        let ours = scatter_wikitext::render(&expanded, &fixture.config, &fixture, &page).html;
        let (a, b) = (normalize(&ours), normalize(&expected));
        let same = a == b;
        match (same, known.contains_key(&r.revid)) {
            (true, false) => passed += 1,
            (true, true) => now_passing.push(format!("{} {}", r.revid, r.title)),
            (false, true) => {}
            (false, false) => failures.push(format!(
                "{} ({}): {}",
                r.title,
                r.revid,
                first_difference(&a, &b)
            )),
        }
    }
    let total = revisions.len();
    eprintln!(
        "golden: {passed} of {total} revisions match; {} known differences",
        known.len()
    );
    assert!(
        now_passing.is_empty(),
        "known differences that now match; remove them from known-differences.txt:\n{}",
        now_passing.join("\n")
    );
    assert!(
        failures.is_empty(),
        "{} revisions differ:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

// 0042 §4: a page's own template calls mark the top-level blocks they drew, from the
// expander's map of calls to output; a template's output inside a paragraph marks
// nothing, and nor does the page's own text.
#[test]
fn template_blocks_are_marked_with_their_template() {
    let fixture = Fixture {
        config: librarybase(),
        exists: HashMap::new(),
        redirects: HashSet::new(),
        labels: HashMap::new(),
        templates: HashMap::from([
            (
                "Template:Box".to_string(),
                "<div class=\"box\">Boxed</div>".to_string(),
            ),
            ("Template:Items".to_string(), "* one\n* two".to_string()),
            ("Template:Word".to_string(), "word".to_string()),
            (
                "Template:Two".to_string(),
                "== Heading ==\nA paragraph of its own.".to_string(),
            ),
        ]),
    };
    let source = "Intro with a {{word}} in it.\n\n{{Box}}\n\nMiddle.\n{{items}}\n\n{{two}}";
    let e = scatter_wikitext_expand::expand_for_view_mapped(source, &fixture);
    let marks: Vec<scatter_wikitext::Mark> = e
        .calls
        .iter()
        .map(|c| scatter_wikitext::Mark {
            range: c.output.clone(),
            attrs: vec![
                ("data-ts-template".into(), format!("Template:{}", c.name)),
                ("data-ts-template-at".into(), c.at.to_string()),
            ],
        })
        .collect();
    let page = Title::new(0, "Marked");
    let html =
        scatter_wikitext::render_marked(&e.text, &fixture.config, &fixture, &page, &marks).html;
    let at = |name: &str| source.find(&format!("{{{{{name}")).unwrap();
    assert!(
        html.contains(&format!(
            "<div class=\"box\" data-ts-template=\"Template:Box\" data-ts-template-at=\"{}\">Boxed</div>",
            at("Box")
        )),
        "{html}"
    );
    assert!(
        html.contains(&format!(
            "<ul data-ts-template=\"Template:items\" data-ts-template-at=\"{}\">",
            at("items")
        )),
        "{html}"
    );
    // A template that draws two blocks marks both.
    assert_eq!(
        html.matches("data-ts-template=\"Template:two\"").count(),
        2,
        "{html}"
    );
    // Inline output, and the page's own paragraphs, are not marked.
    assert!(!html.contains("Template:word"), "{html}");
    assert_eq!(html.matches("data-ts-template=").count(), 4, "{html}");
    // Without marks the render is unchanged.
    assert_eq!(
        scatter_wikitext::render_marked(&e.text, &fixture.config, &fixture, &page, &[]).html,
        scatter_wikitext::render(&e.text, &fixture.config, &fixture, &page).html
    );
}
