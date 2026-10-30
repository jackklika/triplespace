//! Comparing two renderings of a page (0097 §16; 19 §5.10): both are parsed as a browser
//! would read them and written back in one canonical form, so that two results are equal
//! when a browser would build the same DOM from them, and a difference is reported as the
//! path of the first node that differs.
//!
//! The canonical form is one line per node: an element with its attributes sorted by
//! name, or a text node. Whitespace-only text is dropped and runs of whitespace are
//! collapsed, except under `pre`, `textarea` and `script`/`style`, where a browser keeps
//! them. Comments are kept, since the output is meant to be identical, not merely
//! equivalent. Entity spelling is not a difference: the built-in renderer already writes
//! `&#60;` in its `askama` templates and `&lt;` in its string builders.
//!
//! The parity harness (`tests/parity.rs`, `cargo xtask skin-parity`) and the `compare`
//! renderer use this.

use std::fmt::Write as _;

use html5ever::tendril::TendrilSink as _;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// Whether a result is a whole page or a fragment (an `action=render` region).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A document: `<!DOCTYPE html>` and the `<html>` tree.
    Document,
    /// A fragment, read as the contents of a `<div>` in a body.
    Fragment,
}

impl Kind {
    /// What a result is, by how it starts.
    #[must_use]
    pub fn of(html: &str) -> Self {
        let head = html.trim_start();
        let starts = |p: &str| {
            head.get(..p.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(p))
        };
        if starts("<!") || starts("<html") {
            Self::Document
        } else {
            Self::Fragment
        }
    }
}

/// One node of the canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Where the node is: element names from the root, each with its index among its
    /// parent's element children (`html/body/main[1]/div[0]`).
    pub path: String,
    /// How deep the node is: 0 for the root element, its children 1, and so on.
    pub depth: usize,
    /// The node, canonically: `<name attr="v" …>`, `"text"`, `<!--comment-->` or
    /// `<!DOCTYPE name>`.
    pub text: String,
}

/// A rendering in canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Normalized {
    /// The nodes, in document order.
    pub lines: Vec<Line>,
}

impl std::fmt::Display for Normalized {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for l in &self.lines {
            writeln!(f, "{}{}", "  ".repeat(l.depth), l.text)?;
        }
        Ok(())
    }
}

/// The first node at which two renderings differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// The path of the node in the first rendering, or of the last node both share when
    /// one ends before the other.
    pub path: String,
    /// The node in the first rendering, if it has one there.
    pub ours: Option<String>,
    /// The node in the second rendering, if it has one there.
    pub theirs: Option<String>,
}

impl std::fmt::Display for Difference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "at {}", self.path)?;
        writeln!(f, "  a: {}", self.ours.as_deref().unwrap_or("(ends)"))?;
        write!(f, "  b: {}", self.theirs.as_deref().unwrap_or("(ends)"))
    }
}

/// Parses a rendering and writes it back canonically.
#[must_use]
pub fn normalize(html: &str, kind: Kind) -> Normalized {
    let mut lines = Vec::new();
    match kind {
        Kind::Document => {
            let dom = html5ever::parse_document(RcDom::default(), html5ever::ParseOpts::default())
                .one(html);
            walk(&dom.document, "", 0, false, &mut lines);
        }
        Kind::Fragment => {
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
            // The fragment parser wraps the content in an `html` element.
            let root = dom.document.children.borrow();
            for c in root.iter() {
                if let NodeData::Element { .. } = &c.data {
                    walk_children(c, "", 0, false, &mut lines);
                } else {
                    walk(c, "", 0, false, &mut lines);
                }
            }
        }
    }
    Normalized { lines }
}

/// Compares two renderings of the same kind.
#[must_use]
pub fn compare(a: &str, b: &str, kind: Kind) -> Option<Difference> {
    diff(&normalize(a, kind), &normalize(b, kind))
}

/// The first difference between two canonical forms.
#[must_use]
pub fn diff(a: &Normalized, b: &Normalized) -> Option<Difference> {
    let n = a.lines.len().min(b.lines.len());
    for i in 0..n {
        if a.lines[i].text != b.lines[i].text {
            return Some(Difference {
                path: a.lines[i].path.clone(),
                ours: Some(a.lines[i].text.clone()),
                theirs: Some(b.lines[i].text.clone()),
            });
        }
    }
    if a.lines.len() == b.lines.len() {
        return None;
    }
    let (longer, ours) = if a.lines.len() > b.lines.len() {
        (&a.lines[n], true)
    } else {
        (&b.lines[n], false)
    };
    Some(Difference {
        path: longer.path.clone(),
        ours: ours.then(|| longer.text.clone()),
        theirs: (!ours).then(|| longer.text.clone()),
    })
}

/// Elements whose text a browser keeps as written.
fn preformatted(name: &str) -> bool {
    matches!(name, "pre" | "textarea" | "script" | "style")
}

fn walk(h: &Handle, parent: &str, depth: usize, pre: bool, out: &mut Vec<Line>) {
    match &h.data {
        NodeData::Document => walk_children(h, parent, depth, pre, out),
        NodeData::Doctype { name, .. } => out.push(Line {
            path: parent.to_string(),
            depth,
            text: format!("<!DOCTYPE {name}>"),
        }),
        NodeData::Text { contents } => {
            let t = contents.borrow();
            let text = if pre {
                t.to_string()
            } else {
                t.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            if !text.is_empty() {
                out.push(Line {
                    path: parent.to_string(),
                    depth,
                    text: format!("{text:?}"),
                });
            }
        }
        NodeData::Comment { contents } => out.push(Line {
            path: parent.to_string(),
            depth,
            text: format!("<!--{contents}-->"),
        }),
        NodeData::Element { name, attrs, .. } => {
            let mut a: Vec<(String, String)> = attrs
                .borrow()
                .iter()
                .map(|a| (a.name.local.to_string(), a.value.to_string()))
                .collect();
            a.sort();
            let mut text = format!("<{}", name.local);
            for (k, v) in a {
                let _ = write!(text, " {k}={v:?}");
            }
            text.push('>');
            // `parent` is this element's own path: the parent set it, counting its
            // element children.
            out.push(Line {
                path: parent.to_string(),
                depth,
                text,
            });
            walk_children(h, parent, depth + 1, pre || preformatted(&name.local), out);
        }
        NodeData::ProcessingInstruction { .. } => {}
    }
}

fn walk_children(h: &Handle, parent: &str, depth: usize, pre: bool, out: &mut Vec<Line>) {
    let mut index = 0usize;
    for c in h.children.borrow().iter() {
        if let NodeData::Element { name, .. } = &c.data {
            let path = if parent.is_empty() {
                format!("{}[{index}]", name.local)
            } else {
                format!("{parent}/{}[{index}]", name.local)
            };
            index += 1;
            walk(c, &path, depth, pre, out);
        } else {
            walk(c, parent, depth, pre, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_spelling_and_attribute_order_are_not_differences() {
        let a = "<!DOCTYPE html><html><body><p class=\"x\" id=\"y\">A &#60; B</p></body></html>";
        let b =
            "<!DOCTYPE html>\n<html>\n<body>\n<p id=\"y\" class=\"x\">A &lt; B</p>\n</body></html>";
        assert_eq!(compare(a, b, Kind::Document), None);
    }

    #[test]
    fn whitespace_is_kept_where_it_renders() {
        let a = "<pre>a\n  b</pre>";
        let b = "<pre>a b</pre>";
        let d = compare(a, b, Kind::Fragment).expect("differs");
        assert_eq!(d.path, "pre[0]");
        assert_eq!(d.ours.as_deref(), Some("\"a\\n  b\""));
        let a = "<p>a\n  b</p>";
        let b = "<p>a b</p>";
        assert_eq!(compare(a, b, Kind::Fragment), None);
        // An extra space inside a paragraph collapses; one between elements is dropped.
        assert_eq!(
            compare(
                "<ul>\n<li>x</li>\n</ul>",
                "<ul><li>x</li></ul>",
                Kind::Fragment
            ),
            None
        );
    }

    #[test]
    fn the_first_difference_has_a_path() {
        let a = "<!DOCTYPE html><html><head><title>t</title></head><body><main><div></div><div><a href=\"/x\">x</a></div></main></body></html>";
        let b = "<!DOCTYPE html><html><head><title>t</title></head><body><main><div></div><div><a href=\"/y\">x</a></div></main></body></html>";
        let d = compare(a, b, Kind::Document).expect("differs");
        assert_eq!(d.path, "html[0]/body[1]/main[0]/div[1]/a[0]");
        assert_eq!(d.ours.as_deref(), Some("<a href=\"/x\">"));
        assert_eq!(d.theirs.as_deref(), Some("<a href=\"/y\">"));
        // One ends before the other.
        let c = "<!DOCTYPE html><html><head><title>t</title></head><body><main><div></div></main></body></html>";
        let d = compare(a, c, Kind::Document).expect("differs");
        assert_eq!(d.path, "html[0]/body[1]/main[0]/div[1]");
        assert!(d.theirs.is_none());
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::of("<!DOCTYPE html>\n<html>"), Kind::Document);
        assert_eq!(Kind::of("  <html lang=en>"), Kind::Document);
        assert_eq!(Kind::of("<div>x</div>"), Kind::Fragment);
    }

    #[test]
    fn the_canonical_form_prints() {
        let n = normalize("<p>a<b>b</b></p>", Kind::Fragment);
        assert_eq!(n.to_string(), "<p>\n  \"a\"\n  <b>\n    \"b\"\n");
    }
}
