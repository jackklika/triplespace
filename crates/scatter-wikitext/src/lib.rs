//! Parsing and rendering the wikitext subset (0008 §8; 11 §1.2).
//!
//! [`render`] parses text with the vendored `parse-wiki-text-2`, lowers its nodes to a
//! render tree that keeps source spans (the `lower` module), places the table of contents, and
//! serializes the tree as HTML inside `<div class="mw-parser-output">`, sanitized once
//! more by `ammonia`. What the page links to, its categories, external links, template
//! calls, behaviour switches and sections come back with it, read from the same tree
//! walk, so extraction and rendering cannot disagree.
//!
//! The crate is pure and builds for `wasm32-unknown-unknown`, so the editor's preview
//! runs the same code as the server (0034). It renders what it is given: on the server,
//! the expanded text from `scatter-wikitext-expand`; in a preview that cannot expand,
//! the stored text, where template calls render as chips.
//!
//! **Spans** are byte ranges in the text [`render`] was given: the expanded text, or the
//! stored text where it was not expanded. Mapping expanded positions back to the
//! source is the expander's, in a later slice (11 §1.4); positions for the editor come
//! from parsing the source as source, as 11 §1.1 says.

#![forbid(unsafe_code)]

pub mod config;
pub mod html;
mod lower;
pub mod title;
pub mod tree;

use std::collections::HashSet;

pub use config::{Config, ConfigSource, Interwiki, Messages, Namespace};
pub use title::{PageInfo, Resolver, Target, Title, parse_target};
pub use tree::{Element, Node, Span, Text};

/// A link to a local or interwiki title.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InternalLink {
    /// The title, with any fragment.
    pub target: Title,
    /// The link's source.
    pub span: Span,
}

/// A category the text puts the page in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    /// The category's name, without the namespace.
    pub name: String,
    /// The sort key given with `[[Category:Name|key]]`.
    pub sort_key: Option<String>,
    /// The link's source.
    pub span: Span,
}

/// An external link, bracketed or bare.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalLink {
    /// The URL.
    pub url: String,
    /// Its source.
    pub span: Span,
}

/// A template call that rendered as a chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateCall {
    /// The name as written.
    pub name: String,
    /// The call's source.
    pub span: Span,
}

/// A section heading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// Its level, 1–6.
    pub level: u8,
    /// Its anchor (the `id` of its headline).
    pub anchor: String,
    /// Its rendered content.
    pub line: Vec<Node>,
    /// Its number in the table of contents (`2.1`).
    pub number: String,
    /// Its position among the page's headings, from 1.
    pub index: usize,
    /// The heading's source.
    pub span: Span,
}

/// What rendering reads out of the text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extracted {
    /// Links to titles, in order.
    pub links: Vec<InternalLink>,
    /// Category memberships, in order.
    pub categories: Vec<Category>,
    /// External links, in order.
    pub external_links: Vec<ExternalLink>,
    /// Template calls left unexpanded.
    pub templates: Vec<TemplateCall>,
    /// Behaviour switches, uppercase, without underscores.
    pub switches: Vec<String>,
    /// Section headings.
    pub sections: Vec<Section>,
    /// The redirect target, for a redirect.
    pub redirect: Option<Title>,
}

/// A rendered page.
#[derive(Clone, Debug)]
pub struct Rendered {
    /// The render tree: the content of `.mw-parser-output`.
    pub tree: Vec<Node>,
    /// The HTML, with its `mw-parser-output` wrapper.
    pub html: String,
    /// Links, categories and the rest.
    pub extracted: Extracted,
    /// The parser's warnings about malformed markup, as `start..end: message`.
    pub warnings: Vec<String>,
}

/// How long the parser may run on a server before it gives up (0033 §9.1: it bounds its
/// own running time). The browser preview has no clock and runs without a bound.
#[cfg(not(target_arch = "wasm32"))]
const PARSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// A range of the text to render whose top-level blocks carry attributes: which template
/// drew them, as `scatter-wikitext-expand` maps a page's calls to their output (0042 §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    /// The range of the text.
    pub range: Span,
    /// The attributes, `data-` ones, which the sanitizer keeps.
    pub attrs: Vec<(String, String)>,
}

/// Parses and renders `text` as the page `page`.
#[must_use]
pub fn render(text: &str, config: &Config, resolver: &dyn Resolver, page: &Title) -> Rendered {
    render_marked(text, config, resolver, page, &[])
}

/// Renders as [`render`] does, and gives each top-level element that lies wholly within a
/// mark's range that mark's attributes: a block a template drew is marked, a paragraph
/// that only holds a template's inline output is not.
#[must_use]
pub fn render_marked(
    text: &str,
    config: &Config,
    resolver: &dyn Resolver,
    page: &Title,
    marks: &[Mark],
) -> Rendered {
    #[cfg(not(target_arch = "wasm32"))]
    let parsed = config.parser.parse_with_timeout(text, PARSE_TIMEOUT);
    #[cfg(target_arch = "wasm32")]
    let parsed = config.parser.parse_without_timeout(text);
    let output = match parsed {
        Ok(o) => o,
        Err(parse_wiki_text_2::ParseError::TimedOut { .. }) => {
            // Never fail: show the source as text.
            let mut pre = Element::new("pre", Vec::new(), Some(0..text.len()));
            pre.push_text(text);
            let tree = vec![Node::Element(pre)];
            return finish(
                tree,
                Extracted::default(),
                vec!["0..0: parse timed out".into()],
            );
        }
    };
    let warnings = output
        .warnings
        .iter()
        .map(|w| format!("{}..{}: {}", w.start, w.end, w.message))
        .collect();
    let mut l = lower::Lowerer::new(text, config, resolver, page);
    l.blocks(&output.nodes, false);
    let toc_here = l.toc_here;
    let mut extracted = std::mem::take(&mut l.out);
    let mut tree = l.b.finish();
    place_toc(&mut tree, &mut extracted, config, toc_here);
    mark(&mut tree, marks);
    finish(tree, extracted, warnings)
}

/// The text an element was drawn from: from the first to the last place any of it, or of
/// what it holds, came from. A generated element (a paragraph, a list) has no span of its
/// own, and an element's own span is its start tag, so the extent is read from its
/// contents too.
fn extent(e: &Element) -> Option<Span> {
    fn walk(nodes: &[Node], out: &mut Option<Span>) {
        for n in nodes {
            let (span, children) = match n {
                Node::Element(e) => (e.span.as_ref(), Some(&e.children)),
                Node::Text(t) => (t.span.as_ref(), None),
            };
            if let Some(s) = span.filter(|s| s.start < s.end) {
                *out = Some(match out.take() {
                    Some(o) => o.start.min(s.start)..o.end.max(s.end),
                    None => s.clone(),
                });
            }
            if let Some(c) = children {
                walk(c, out);
            }
        }
    }
    let mut out = e.span.clone().filter(|s| s.start < s.end);
    walk(&e.children, &mut out);
    out
}

/// Gives the top-level elements within a mark's range its attributes.
fn mark(tree: &mut [Node], marks: &[Mark]) {
    if marks.is_empty() {
        return;
    }
    for node in tree {
        let Node::Element(e) = node else { continue };
        let Some(span) = extent(e) else { continue };
        if let Some(m) = marks
            .iter()
            .find(|m| m.range.start <= span.start && span.end <= m.range.end)
        {
            for (k, v) in &m.attrs {
                if e.attr(k).is_none() {
                    e.attrs.push((k.clone(), v.clone()));
                }
            }
        }
    }
}

/// What [`render`] reads out of the text, without the HTML: for the page projection,
/// which indexes a page's links, categories and redirect from its stored text (10 §5.2,
/// §5.3). Existence and labels do not change what is extracted, so no resolver is asked.
#[must_use]
pub fn extract(text: &str, config: &Config, page: &Title) -> Extracted {
    struct Nothing;
    impl Resolver for Nothing {
        fn info(&self, _: &Title) -> PageInfo {
            PageInfo::default()
        }
        fn url(&self, _: &Title) -> String {
            String::new()
        }
        fn missing_url(&self, _: &Title) -> String {
            String::new()
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let parsed = config.parser.parse_with_timeout(text, PARSE_TIMEOUT);
    #[cfg(target_arch = "wasm32")]
    let parsed = config.parser.parse_without_timeout(text);
    let Ok(output) = parsed else {
        return Extracted::default();
    };
    let mut l = lower::Lowerer::new(text, config, &Nothing, page);
    l.blocks(&output.nodes, false);
    let mut extracted = std::mem::take(&mut l.out);
    number_sections(&mut extracted.sections);
    extracted
}

fn finish(tree: Vec<Node>, extracted: Extracted, warnings: Vec<String>) -> Rendered {
    let mut inner = String::new();
    tree::to_html(&tree, &mut inner);
    let html = format!(
        "<div class=\"mw-parser-output\">{}</div>",
        sanitizer().clean(&inner)
    );
    Rendered {
        tree,
        html,
        extracted,
        warnings,
    }
}

/// Sanitizes HTML to the allow-list of 0008 §8, as the renderer's own output is: for HTML
/// another renderer wrote, such as a markdown post's (14 §1.6).
#[must_use]
pub fn sanitize(html: &str) -> String {
    sanitizer().clean(html).to_string()
}

/// The final sanitizer: everything the renderer may write, and nothing else.
#[allow(clippy::too_many_lines)] // the allow-lists are data
fn sanitizer() -> &'static ammonia::Builder<'static> {
    static B: std::sync::OnceLock<ammonia::Builder<'static>> = std::sync::OnceLock::new();
    B.get_or_init(|| {
        let tags: HashSet<&str> = [
            "a",
            "abbr",
            "b",
            "bdi",
            "bdo",
            "big",
            "blockquote",
            "br",
            "caption",
            "center",
            "cite",
            "code",
            "data",
            "dd",
            "del",
            "dfn",
            "div",
            "dl",
            "dt",
            "em",
            "font",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "hr",
            "i",
            "input",
            "ins",
            "kbd",
            "label",
            "li",
            "mark",
            "ol",
            "p",
            "pre",
            "q",
            "rb",
            "rp",
            "rt",
            "rtc",
            "ruby",
            "s",
            "samp",
            "small",
            "span",
            "strike",
            "strong",
            "sub",
            "sup",
            "table",
            "tbody",
            "td",
            "tfoot",
            "th",
            "thead",
            "time",
            "tr",
            "tt",
            "u",
            "ul",
            "var",
            "wbr",
        ]
        .into_iter()
        .collect();
        let generic: HashSet<&str> = [
            "id",
            "class",
            "style",
            "lang",
            "dir",
            "title",
            "role",
            "aria-describedby",
            "aria-flowto",
            "aria-hidden",
            "aria-label",
            "aria-labelledby",
            "aria-level",
            "aria-owns",
            "about",
            "property",
            "resource",
            "datatype",
            "typeof",
            "itemid",
            "itemprop",
            "itemref",
            "itemscope",
            "itemtype",
            "align",
            "valign",
            "width",
            "height",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "frame",
            "rules",
            "summary",
            "abbr",
            "axis",
            "headers",
            "scope",
            "rowspan",
            "colspan",
            "nowrap",
            "type",
            "start",
            "reversed",
            "value",
            "cite",
            "datetime",
            "clear",
            "size",
            "color",
            "face",
        ]
        .into_iter()
        .collect();
        let mut tag_attrs = std::collections::HashMap::new();
        tag_attrs.insert("a", ["href", "rel"].into_iter().collect::<HashSet<_>>());
        tag_attrs.insert("input", ["type"].into_iter().collect());
        tag_attrs.insert("label", ["for"].into_iter().collect());
        let schemes: HashSet<&str> = [
            "bitcoin",
            "ftp",
            "ftps",
            "geo",
            "git",
            "gopher",
            "http",
            "https",
            "irc",
            "ircs",
            "magnet",
            "mailto",
            "matrix",
            "mms",
            "news",
            "nntp",
            "redis",
            "sftp",
            "sip",
            "sips",
            "sms",
            "ssh",
            "svn",
            "tel",
            "telnet",
            "urn",
            "worldwind",
            "xmpp",
        ]
        .into_iter()
        .collect();
        let mut b = ammonia::Builder::default();
        b.tags(tags)
            .generic_attributes(generic)
            .generic_attribute_prefixes(["data-"].into_iter().collect())
            .tag_attributes(tag_attrs)
            .url_schemes(schemes)
            .link_rel(None)
            .strip_comments(true);
        b
    })
}

/// Puts the table of contents where MediaWiki would: at `__TOC__`, or before the first
/// heading when there are four or more headings or `__FORCETOC__`, never with
/// `__NOTOC__`. Numbers the sections as it goes.
fn place_toc(tree: &mut Vec<Node>, ex: &mut Extracted, config: &Config, toc_here: bool) {
    number_sections(&mut ex.sections);
    let has = |w: &str| ex.switches.iter().any(|s| s == w);
    let show = !has("NOTOC")
        && !ex.sections.is_empty()
        && (toc_here || has("FORCETOC") || ex.sections.len() >= 4);
    let mut toc = show.then(|| toc_element(&ex.sections, config));
    if !replace_marker(tree, &mut toc)
        && let Some(t) = toc
    {
        insert_before_first_heading(tree, t);
    }
}

/// Section numbers as MediaWiki computes them: a deeper heading starts a level, a
/// shallower one returns to the level it last matched.
fn number_sections(sections: &mut [Section]) {
    let mut toclevel = 0usize;
    let mut prev = 0u8;
    let mut level_at: Vec<u8> = vec![0];
    let mut counts: Vec<usize> = vec![0];
    for s in sections.iter_mut() {
        let level = s.level;
        if level > prev {
            toclevel += 1;
            if counts.len() <= toclevel {
                counts.resize(toclevel + 1, 0);
                level_at.resize(toclevel + 1, 0);
            }
            counts[toclevel] = 0;
        } else if level < prev && toclevel > 1 {
            let mut found = None;
            for i in (1..=toclevel).rev() {
                if level_at[i] == level {
                    found = Some(i);
                    break;
                }
                if level_at[i] < level {
                    found = Some(i + 1);
                    break;
                }
            }
            toclevel = found.unwrap_or(1);
        }
        level_at[toclevel] = level;
        counts[toclevel] += 1;
        for c in counts.iter_mut().skip(toclevel + 1) {
            *c = 0;
        }
        s.number = (1..=toclevel)
            .filter(|&i| counts[i] > 0)
            .map(|i| counts[i].to_string())
            .collect::<Vec<_>>()
            .join(".");
        prev = level;
    }
}

fn toc_element(sections: &[Section], config: &Config) -> Element {
    let m = config.messages();
    let attr = |k: &str, v: &str| (k.to_string(), v.to_string());
    let mut toc = Element::new(
        "div",
        vec![
            attr("id", "toc"),
            attr("class", "toc"),
            attr("role", "navigation"),
            attr("aria-labelledby", "mw-toc-heading"),
        ],
        None,
    );
    toc.push(Element::new(
        "input",
        vec![
            attr("type", "checkbox"),
            attr("role", "button"),
            attr("id", "toctogglecheckbox"),
            attr("class", "toctogglecheckbox"),
            attr("style", "display:none"),
        ],
        None,
    ));
    let title = toc.push(Element::new(
        "div",
        vec![
            attr("class", "toctitle"),
            attr("lang", &m.lang),
            attr("dir", &m.dir),
        ],
        None,
    ));
    title
        .push(Element::new("h2", vec![attr("id", "mw-toc-heading")], None))
        .push_text(&m.toc);
    title
        .push(Element::new(
            "span",
            vec![attr("class", "toctogglespan")],
            None,
        ))
        .push(Element::new(
            "label",
            vec![
                attr("class", "toctogglelabel"),
                attr("for", "toctogglecheckbox"),
            ],
            None,
        ));

    // Nest items by their depth in the numbering.
    let mut items = Vec::new();
    for s in sections {
        let depth = s.number.split('.').count().max(1);
        let mut li = Element::new(
            "li",
            vec![attr(
                "class",
                &format!("toclevel-{depth} tocsection-{}", s.index),
            )],
            None,
        );
        let a = li.push(Element::new(
            "a",
            vec![attr("href", &format!("#{}", s.anchor))],
            None,
        ));
        a.push(Element::new("span", vec![attr("class", "tocnumber")], None))
            .push_text(&s.number);
        a.push_text(" ");
        a.push(Element::new("span", vec![attr("class", "toctext")], None))
            .children = toc_text(&s.line);
        items.push((depth, li));
    }
    let root = nest(&mut items.into_iter().peekable(), 1);
    toc.push(root);
    toc
}

/// Builds a `ul` of the items at `depth`, each holding the deeper items after it.
fn nest(
    items: &mut std::iter::Peekable<std::vec::IntoIter<(usize, Element)>>,
    depth: usize,
) -> Element {
    let mut ul = Element::new("ul", Vec::new(), None);
    while let Some(&(d, _)) = items.peek() {
        if d < depth {
            break;
        }
        if d == depth {
            if let Some((_, li)) = items.next() {
                ul.children.push(Node::Element(li));
            }
            continue;
        }
        let sub = nest(items, depth + 1);
        match ul.children.last_mut() {
            Some(Node::Element(li)) => li.children.push(Node::Element(sub)),
            _ => ul.children.push(Node::Element(sub)),
        }
    }
    ul
}

/// A heading's content for the TOC: links unwrapped, and only simple formatting kept.
fn toc_text(nodes: &[Node]) -> Vec<Node> {
    let mut out = Vec::new();
    for n in nodes {
        match n {
            Node::Text(_) => out.push(n.clone()),
            Node::Element(e) => {
                let children = toc_text(&e.children);
                if matches!(
                    e.name.as_str(),
                    "b" | "i" | "sup" | "sub" | "s" | "u" | "del" | "ins" | "bdi"
                ) {
                    let mut kept = Element::new(&e.name, Vec::new(), None);
                    kept.children = children;
                    out.push(Node::Element(kept));
                } else {
                    out.extend(children);
                }
            }
        }
    }
    out
}

fn replace_marker(nodes: &mut Vec<Node>, toc: &mut Option<Element>) -> bool {
    for i in 0..nodes.len() {
        if let Node::Element(e) = &mut nodes[i] {
            if e.name == lower::TOC_MARKER {
                match toc.take() {
                    Some(t) => nodes[i] = Node::Element(t),
                    None => {
                        nodes.remove(i);
                    }
                }
                return true;
            }
            if replace_marker(&mut e.children, toc) {
                return true;
            }
        }
    }
    false
}

fn insert_before_first_heading(nodes: &mut Vec<Node>, toc: Element) -> Option<Element> {
    let mut toc = Some(toc);
    for i in 0..nodes.len() {
        if let Node::Element(e) = &mut nodes[i] {
            let is_section = e.name.len() == 2
                && e.name.starts_with('h')
                && e.children.iter().any(
                    |c| matches!(c, Node::Element(s) if s.attr("class") == Some("mw-headline")),
                );
            if is_section {
                nodes.insert(i, Node::Element(toc.take().expect("present")));
                return None;
            }
            toc = insert_before_first_heading(&mut e.children, toc.take().expect("present"));
            toc.as_ref()?;
        }
    }
    toc
}

#[cfg(test)]
mod tests;
