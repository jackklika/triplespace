//! A document page's diff views (0010 §6; 19 §3.8): what `?diff=` shows beside the
//! API's line diff of the source.
//!
//! - **The view switch** (`view=`): *Source*, the API's line diff of the page's source
//!   (the default); *Text only*, a line diff of the text a reader sees, one line per
//!   paragraph, heading, list item or table row, so that a change of prose shows among
//!   templates and markup; *Side by side*, both revisions rendered in two columns.
//! - **More context** (`context=`): the line diffs keep two lines of context around each
//!   change, as MediaWiki does; `context=N` keeps N, up to [`MAX_CONTEXT`], and
//!   `context=all` keeps every line. The control steps through [`STEPS`], then *all*.
//! - **The permalink** is the diff as a wiki link, `[[Special:Diff/{old}/{new}]]`, which
//!   `Special:Diff` reads back (19 §3.8).
//!
//! The views other than the default draw their own diff with [`scatter_pages::diff`], from
//! the API's sources or renderings, since `action=compare` follows MediaWiki's parameters,
//! which have neither (0010 §6, A48). Each stays within the page's four API calls (20):
//! the two sides' headers, then two sources or two renderings, then the rendering below.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use html5ever::tendril::TendrilSink as _;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use scatter_pages::diff::{self, ALL_LINES, CONTEXT};

/// The most lines of context a number may ask for; more is `all`.
pub const MAX_CONTEXT: usize = 500;

/// The steps "More context" takes from the default, before every line.
pub const STEPS: [usize; 2] = [10, 50];

/// Which view of the diff the page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The API's line diff of the source.
    Source,
    /// A line diff of the rendered text.
    Text,
    /// Both revisions rendered side by side.
    Rendered,
}

impl View {
    /// From `view=`; anything else is the default.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Self {
        match query.get("view").map(String::as_str) {
            Some("text") => Self::Text,
            Some("rendered") => Self::Rendered,
            _ => Self::Source,
        }
    }

    /// The value of `view=`; `None` for the default.
    #[must_use]
    pub fn param(self) -> Option<&'static str> {
        match self {
            Self::Source => None,
            Self::Text => Some("text"),
            Self::Rendered => Some("rendered"),
        }
    }

    /// Whether the view is a line diff, which context applies to.
    #[must_use]
    pub fn has_lines(self) -> bool {
        !matches!(self, Self::Rendered)
    }
}

/// Lines of context: `None` for the default, MediaWiki's two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context(pub Option<usize>);

impl Context {
    /// From `context=`: a number up to [`MAX_CONTEXT`], or `all`; anything else, and the
    /// default's own number, is the default.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Self {
        Self(match query.get("context").map(|c| c.trim()) {
            Some("all") => Some(ALL_LINES),
            Some(n) => n
                .parse::<usize>()
                .ok()
                .filter(|n| *n != CONTEXT)
                .map(|n| if n > MAX_CONTEXT { ALL_LINES } else { n }),
            None => None,
        })
    }

    /// The lines kept around a change.
    #[must_use]
    pub fn lines(self) -> usize {
        self.0.unwrap_or(CONTEXT)
    }

    /// The value of `context=`; `None` for the default.
    #[must_use]
    pub fn param(self) -> Option<String> {
        self.0.map(|n| {
            if n == ALL_LINES {
                "all".to_string()
            } else {
                n.to_string()
            }
        })
    }

    /// The next step of "More context"; `None` once every line is shown.
    #[must_use]
    pub fn more(self) -> Option<Self> {
        let n = self.lines();
        if n == ALL_LINES {
            return None;
        }
        Some(Self(Some(
            STEPS.iter().copied().find(|s| *s > n).unwrap_or(ALL_LINES),
        )))
    }

    /// Every line.
    #[must_use]
    pub fn all() -> Self {
        Self(Some(ALL_LINES))
    }

    /// Whether every line is shown.
    #[must_use]
    pub fn is_all(self) -> bool {
        self.0 == Some(ALL_LINES)
    }
}

/// The query string a view and a context add to a diff's address: `&view=…&context=…`,
/// empty for the defaults.
#[must_use]
pub fn keep(view: View, context: Context) -> String {
    let mut s = String::new();
    if let Some(v) = view.param() {
        let _ = write!(s, "&view={v}");
    }
    if view.has_lines()
        && let Some(c) = context.param()
    {
        let _ = write!(s, "&context={c}");
    }
    s
}

/// The diff as a wiki link: `[[Special:Diff/{old}/{new}]]`, or `[[Special:Diff/{new}]]`
/// for a page's first revision.
#[must_use]
pub fn permalink(from: Option<u64>, to: u64) -> String {
    match from {
        Some(f) => format!("[[Special:Diff/{f}/{to}]]"),
        None => format!("[[Special:Diff/{to}]]"),
    }
}

/// MediaWiki's table rows and inline layout of the line diff between two texts, with
/// `context` lines around each change; both empty when the texts are equal. `line` words
/// a hunk's header, given its first line.
#[must_use]
pub fn rows(
    old: &str,
    new: &str,
    context: Context,
    line: impl Fn(usize) -> String,
) -> (String, String) {
    let hunks = diff::diff_with_context(old, new, context.lines());
    if hunks.is_empty() {
        return (String::new(), String::new());
    }
    (
        diff::table_rows(&hunks, &line),
        diff::inline_rows(&hunks, &line),
    )
}

/// Elements whose text is not prose: scripts, styles, section-edit links and the
/// footnote markers of MediaWiki's Cite.
fn skipped(name: &str, class: &str) -> bool {
    matches!(name, "script" | "style" | "template")
        || class
            .split_ascii_whitespace()
            .any(|c| matches!(c, "mw-editsection" | "reference"))
}

/// Elements that start a line of their own.
fn breaks(name: &str) -> bool {
    matches!(
        name,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "dt"
            | "dd"
            | "blockquote"
            | "pre"
            | "tr"
            | "caption"
            | "figcaption"
            | "br"
            | "hr"
            | "ul"
            | "ol"
            | "dl"
            | "table"
            | "section"
            | "figure"
    )
}

/// Builds the text a line at a time.
struct Lines {
    done: Vec<String>,
    line: String,
}

impl Lines {
    fn end(&mut self) {
        let l = self.line.split_whitespace().collect::<Vec<_>>().join(" ");
        if !l.is_empty() {
            self.done.push(l);
        }
        self.line.clear();
    }

    fn walk(&mut self, h: &Handle, pre: bool) {
        match &h.data {
            NodeData::Text { contents } => {
                let t = contents.borrow();
                if pre {
                    let mut parts = t.split('\n');
                    if let Some(first) = parts.next() {
                        self.line.push_str(first);
                    }
                    for p in parts {
                        self.end();
                        self.line.push_str(p);
                    }
                } else {
                    self.line.push_str(&t);
                }
            }
            NodeData::Element { name, attrs, .. } => {
                let local = name.local.as_ref();
                let class = attrs
                    .borrow()
                    .iter()
                    .find(|a| a.name.local.as_ref() == "class")
                    .map(|a| a.value.to_string())
                    .unwrap_or_default();
                if skipped(local, &class) {
                    return;
                }
                let block = breaks(local);
                if block {
                    self.end();
                }
                // A table's cells, one row to a line.
                if matches!(local, "td" | "th") && !self.line.trim().is_empty() {
                    self.line.push_str(" | ");
                } else if matches!(local, "td" | "th") {
                    self.line.push(' ');
                }
                let pre = pre || local == "pre";
                for c in h.children.borrow().iter() {
                    self.walk(c, pre);
                }
                if block {
                    self.end();
                }
            }
            NodeData::Document => {
                for c in h.children.borrow().iter() {
                    self.walk(c, pre);
                }
            }
            _ => {}
        }
    }
}

/// The text a reader sees in a rendering, one line per paragraph, heading, list item or
/// table row (cells joined by ` | `), a preformatted block line by line, whitespace
/// collapsed; scripts, styles, section-edit
/// links and footnote markers left out.
#[must_use]
pub fn prose(html: &str) -> String {
    let context =
        html5ever::QualName::new(None, html5ever::ns!(html), html5ever::local_name!("div"));
    let dom = html5ever::parse_fragment(
        RcDom::default(),
        html5ever::ParseOpts::default(),
        context,
        vec![],
        false,
    )
    .one(html);
    let mut lines = Lines {
        done: Vec::new(),
        line: String::new(),
    };
    lines.walk(&dom.document, false);
    lines.end();
    lines.done.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    // 0010 §6: the view switch and the context control read the query.
    #[test]
    fn the_query_names_a_view_and_a_context() {
        assert_eq!(View::of(&q(&[])), View::Source);
        assert_eq!(View::of(&q(&[("view", "text")])), View::Text);
        assert_eq!(View::of(&q(&[("view", "rendered")])), View::Rendered);
        assert_eq!(View::of(&q(&[("view", "json")])), View::Source);
        assert_eq!(Context::of(&q(&[])), Context(None));
        assert_eq!(Context::of(&q(&[("context", "2")])), Context(None));
        assert_eq!(Context::of(&q(&[("context", "7")])).lines(), 7);
        assert!(Context::of(&q(&[("context", "all")])).is_all());
        assert!(Context::of(&q(&[("context", "9999")])).is_all());
        assert_eq!(Context::of(&q(&[("context", "x")])), Context(None));
    }

    #[test]
    fn more_context_steps_then_shows_every_line() {
        let c = Context(None);
        let c = c.more().unwrap();
        assert_eq!(c.param().as_deref(), Some("10"));
        let c = c.more().unwrap();
        assert_eq!(c.param().as_deref(), Some("50"));
        let c = c.more().unwrap();
        assert_eq!(c.param().as_deref(), Some("all"));
        assert_eq!(c.more(), None);
        assert_eq!(Context(Some(30)).more().unwrap().lines(), 50);
    }

    #[test]
    fn a_link_keeps_the_view_and_its_context() {
        assert_eq!(keep(View::Source, Context(None)), "");
        assert_eq!(keep(View::Text, Context(Some(10))), "&view=text&context=10");
        assert_eq!(keep(View::Source, Context::all()), "&context=all");
        assert_eq!(
            keep(View::Rendered, Context(Some(10))),
            "&view=rendered",
            "side by side has no context"
        );
    }

    #[test]
    fn the_permalink_is_a_special_diff_link() {
        assert_eq!(permalink(Some(4), 9), "[[Special:Diff/4/9]]");
        assert_eq!(permalink(None, 9), "[[Special:Diff/9]]");
    }

    #[test]
    fn prose_is_a_line_per_block_without_markup() {
        let html = r#"<h2><span class="mw-headline" id="A">History</span><span class="mw-editsection">[edit]</span></h2>
<p>The <b>library</b> opened in <a href="/wiki/1901">1901</a>.<sup class="reference">[1]</sup></p>
<style>.x{color:red}</style>
<ul><li>One</li><li>Two <i>more</i></li></ul>
<table><tr><th>Year</th><th>Books</th></tr><tr><td>1901</td><td>500</td></tr></table>
<pre>a  b
c</pre>"#;
        assert_eq!(
            prose(html),
            "History\nThe library opened in 1901.\nOne\nTwo more\nYear | Books\n1901 | 500\na b\nc"
        );
    }

    #[test]
    fn the_local_diff_is_mediawikis_table_with_more_context() {
        let old = "a\nb\nc\nd\ne\nf\ng";
        let new = "a\nb\nc\nD\ne\nf\ng";
        let line = |n: usize| format!("Line {n}:");
        let (table, inline) = rows(old, new, Context(None), line);
        assert!(table.contains("diff-lineno"));
        assert_eq!(table.matches("diff-context diff-side-deleted").count(), 4);
        assert!(inline.contains("mw-diff-inline-context"));
        let (table, _) = rows(old, new, Context::all(), line);
        assert_eq!(table.matches("diff-context diff-side-deleted").count(), 6);
        assert_eq!(
            rows(old, old, Context::all(), line),
            (String::new(), String::new())
        );
    }
}
