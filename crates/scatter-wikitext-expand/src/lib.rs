//! MediaWiki's preprocessor and frame expansion (0042 §4; 11 §1.4).
//!
//! **This is the first slice.** It implements what a page view needs to transclude a
//! template that takes no parameters:
//!
//! - the preprocessor's brace matching, with `[[…]]` tracked so a `|` inside a link does
//!   not split a call;
//! - `<noinclude>`, `<includeonly>` and `<onlyinclude>`, resolved for viewing a page or
//!   for transcluding it;
//! - extension tags and comments passed through untouched, their content never expanded;
//! - `{{Name}}` and `{{:Name}}` transcluded through [`ExpandHost`], recursively, with loop
//!   detection and MediaWiki's depth limit; a missing page becomes a red link to it;
//! - `{{{name|default}}}` in a frame with no arguments, which is every frame here: the
//!   default, or the literal text when there is none;
//! - the newline MediaWiki puts before an expansion that starts a list, a table or a
//!   definition, so the line markup still starts a line.
//!
//! Everything else is left as written, for the renderer to show as a chip: calls with
//! arguments, variables and parser functions (recognised from
//! `docs/registry/wikitext-functions.toml`), `subst:`, `msg:` and the other modifiers.
//! Frames with arguments, the core variables and functions, ParserFunctions, strip markers
//! and the limit report are the later slices of 11 §1.4.
//!
//! It is written from MediaWiki's documented behaviour, not translated from its PHP
//! (0042 §4). It does no I/O.

#![forbid(unsafe_code)]

mod pst;
mod registry;

pub use pst::{Signer, pre_save_transform, signature_time};
pub use registry::Registry;

/// What a transclusion found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transcluded {
    /// The page's text.
    Found(String),
    /// No such page; its title as a link target (`Template:Name`), for the red link
    /// MediaWiki renders.
    Missing(String),
}

/// The facts expansion needs from its host (11 §1.4). This slice asks only for page text.
pub trait ExpandHost {
    /// The text of the page a call names, written as it is in the call (`cwbn`,
    /// `:Main page`, `User:Example/box`): Template is the default namespace and a leading
    /// colon means the main one, as the host's title resolver decides. `None` for a name
    /// that is not a title.
    fn transclude(&self, name: &str) -> Option<Transcluded>;

    /// The text of an interface message for `{{int:key}}`, the wiki's override where it
    /// has one (0095 §2), with `$1`… unreplaced; `None` leaves the call as written. The key
    /// is as written after `int:`, trimmed.
    fn message(&self, _key: &str) -> Option<String> {
        None
    }
}

/// MediaWiki's limit on nested transclusion (`$wgMaxTemplateDepth`).
pub const MAX_DEPTH: usize = 100;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Viewing the page itself.
    View,
    /// Including it in another page.
    Transclude,
}

/// Expands `text` for viewing the page that holds it.
#[must_use]
pub fn expand_for_view(text: &str, host: &dyn ExpandHost) -> String {
    expand_for_view_mapped(text, host).text
}

/// A call in the page's own text that transcluded a page (0042 §4, A22; 11 §1.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The name it was called by, as written and trimmed (`cwbn`, `:Main page`).
    pub name: String,
    /// Where its `{{` is in the page's text, in bytes.
    pub at: usize,
    /// What it expanded to, as a range of the expanded text.
    pub output: std::ops::Range<usize>,
}

/// An expansion, and where each of the page's own calls put its output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    /// The expanded text.
    pub text: String,
    /// The calls written in the page itself, not those inside the templates they
    /// transclude, that transcluded a page; in the order written.
    pub calls: Vec<Call>,
}

/// Expands `text` for viewing the page that holds it, and maps the page's own calls to
/// their output, so a renderer can say which template drew which block.
#[must_use]
pub fn expand_for_view_mapped(text: &str, host: &dyn ExpandHost) -> Expansion {
    let registry = Registry::embedded();
    let mut e = Expander {
        host,
        registry,
        stack: Vec::new(),
    };
    let nodes = preprocess(text, Mode::View, registry);
    let mut out = String::with_capacity(text.len());
    let mut calls = Vec::new();
    for n in &nodes {
        if let PNode::Template { parts, at } = n {
            let start = out.len();
            if let Some(name) = e.template(n, parts, &mut out) {
                calls.push(Call {
                    name,
                    at: *at,
                    output: start..out.len(),
                });
            }
        } else {
            e.expand(std::slice::from_ref(n), &mut out);
        }
    }
    Expansion { text: out, calls }
}

// ---- the preprocessor tree ----------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
enum PNode {
    Text(String),
    /// `{{…}}`: the name, then the arguments; `at` is where its `{{` starts in the text
    /// it was read from.
    Template {
        parts: Vec<Vec<PNode>>,
        at: usize,
    },
    /// `{{{…}}}`: the name, then the default.
    Argument {
        parts: Vec<Vec<PNode>>,
    },
}

struct Frame {
    brace: u8,
    count: usize,
    parts: Vec<Vec<PNode>>,
    /// Where its first brace is.
    start: usize,
}

impl Frame {
    fn current(&mut self) -> &mut Vec<PNode> {
        self.parts.last_mut().expect("a frame has a part")
    }
}

fn push_text(nodes: &mut Vec<PNode>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(PNode::Text(t)) = nodes.last_mut() {
        t.push_str(s);
    } else {
        nodes.push(PNode::Text(s.to_string()));
    }
}

/// The source a frame would be if it never closed: its braces, then its parts joined by
/// `|`.
fn unwind(f: Frame, into: &mut Vec<PNode>) {
    let open = if f.brace == b'{' { "{" } else { "[" };
    push_text(into, &open.repeat(f.count));
    for (i, part) in f.parts.into_iter().enumerate() {
        if i > 0 {
            push_text(into, "|");
        }
        for n in part {
            match n {
                PNode::Text(t) => push_text(into, &t),
                other => into.push(other),
            }
        }
    }
}

/// Keeps only what `<onlyinclude>` marks, when it marks anything (transclusion only).
fn only_included(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if !lower.contains("<onlyinclude>") {
        return None;
    }
    let mut out = String::new();
    let mut rest = 0;
    while let Some(o) = lower[rest..].find("<onlyinclude>") {
        let start = rest + o + "<onlyinclude>".len();
        let end = lower[start..]
            .find("</onlyinclude>")
            .map_or(text.len(), |e| start + e);
        out.push_str(&text[start..end]);
        rest = (end + "</onlyinclude>".len()).min(text.len());
    }
    Some(out)
}

/// The tag at `i` (`<name …>`, `</name>`, `<name/>`): its name, whether it closes, and
/// its end.
fn tag_at(src: &str, i: usize) -> Option<(String, bool, bool, usize)> {
    let rest = &src[i + 1..];
    let (closing, rest) = match rest.strip_prefix('/') {
        Some(r) => (true, r),
        None => (false, rest),
    };
    let name_len = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }
    let name = rest[..name_len].to_ascii_lowercase();
    let after = &rest[name_len..];
    if !after.starts_with(['>', '/', ' ', '\t', '\n']) {
        return None;
    }
    let gt = after.find('>')?;
    let self_closing = after[..gt].ends_with('/');
    let end = i + 1 + usize::from(closing) + name_len + gt + 1;
    Some((name, closing, self_closing, end))
}

/// Where the end tag `</name>` after `from` ends, case-insensitively; the end of the text
/// when there is none.
fn end_of_element(src: &str, from: usize, name: &str) -> usize {
    let lower = src[from..].to_ascii_lowercase();
    let close = format!("</{name}");
    let mut search = 0;
    while let Some(p) = lower[search..].find(&close) {
        let at = from + search + p;
        if let Some((n, true, _, end)) = tag_at(src, at)
            && n == name
        {
            return end;
        }
        search += p + close.len();
    }
    src.len()
}

// One state machine over the text, as the preprocessor's own is; splitting it would
// scatter the frame stack across functions.
#[allow(clippy::too_many_lines, clippy::many_single_char_names)]
fn preprocess(text: &str, mode: Mode, registry: &Registry) -> Vec<PNode> {
    let owned;
    let src = match (mode, only_included(text)) {
        (Mode::Transclude, Some(t)) => {
            owned = t;
            owned.as_str()
        }
        _ => text,
    };
    let b = src.as_bytes();
    let mut root: Vec<PNode> = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = 0;
    let mut text_start = 0;

    macro_rules! target {
        () => {
            match stack.last_mut() {
                Some(f) => f.current(),
                None => &mut root,
            }
        };
    }
    macro_rules! flush {
        ($to:expr) => {
            if text_start < $to {
                let piece = &src[text_start..$to];
                push_text(target!(), piece);
            }
        };
    }

    while i < b.len() {
        let c = b[i];
        if c == b'<' {
            if src[i..].starts_with("<!--") {
                let end = src[i + 4..]
                    .find("-->")
                    .map_or(src.len(), |e| i + 4 + e + 3);
                i = end;
                continue;
            }
            if let Some((name, closing, self_closing, end)) = tag_at(src, i) {
                match (name.as_str(), mode) {
                    ("includeonly", Mode::View) if !closing => {
                        flush!(i);
                        i = if self_closing {
                            end
                        } else {
                            end_of_element(src, end, "includeonly")
                        };
                        text_start = i;
                        continue;
                    }
                    ("noinclude", Mode::Transclude) if !closing => {
                        flush!(i);
                        i = if self_closing {
                            end
                        } else {
                            end_of_element(src, end, "noinclude")
                        };
                        text_start = i;
                        continue;
                    }
                    ("includeonly" | "noinclude" | "onlyinclude", _) => {
                        flush!(i);
                        i = end;
                        text_start = i;
                        continue;
                    }
                    (n, _) if !closing && registry.is_extension_tag(n) => {
                        // Its content is the extension's, never expanded.
                        i = if self_closing {
                            end
                        } else {
                            end_of_element(src, end, n)
                        };
                        continue;
                    }
                    _ => {}
                }
            }
            i += 1;
            continue;
        }
        if (c == b'{' || c == b'[') && b.get(i + 1) == Some(&c) {
            let n = b[i..].iter().take_while(|&&x| x == c).count();
            flush!(i);
            stack.push(Frame {
                brace: c,
                count: n,
                parts: vec![Vec::new()],
                start: i,
            });
            i += n;
            text_start = i;
            continue;
        }
        if c == b'|' && !stack.is_empty() {
            flush!(i);
            if let Some(f) = stack.last_mut() {
                if f.brace == b'{' {
                    f.parts.push(Vec::new());
                } else {
                    f.current().push(PNode::Text("|".into()));
                }
            }
            i += 1;
            text_start = i;
            continue;
        }
        if c == b']' && stack.last().is_some_and(|f| f.brace == b'[') && b.get(i + 1) == Some(&b']')
        {
            flush!(i);
            let mut f = stack.pop().expect("checked");
            let open_extra = f.count.saturating_sub(2);
            f.count = 2;
            let parent = target!();
            push_text(parent, &"[".repeat(open_extra));
            unwind(f, parent);
            push_text(parent, "]]");
            i += 2;
            text_start = i;
            continue;
        }
        if c == b'}' && stack.last().is_some_and(|f| f.brace == b'{') {
            let mut m = b[i..].iter().take_while(|&&x| x == b'}').count();
            flush!(i);
            let mut pos = i;
            while m >= 2 && stack.last().is_some_and(|f| f.brace == b'{') {
                let mut f = stack.pop().expect("checked");
                let matching = f.count.min(m).min(3);
                if matching < 2 {
                    stack.push(f);
                    break;
                }
                f.count -= matching;
                let close_end = pos + matching;
                let parts = std::mem::take(&mut f.parts);
                // The innermost braces close: this element starts after those left open.
                let node = if matching == 3 {
                    PNode::Argument { parts }
                } else {
                    PNode::Template {
                        parts,
                        at: f.start + f.count,
                    }
                };
                m -= matching;
                pos = close_end;
                if f.count >= 2 {
                    // The braces left open start a new element holding this one.
                    f.parts = vec![vec![node]];
                    stack.push(f);
                } else {
                    let parent = target!();
                    push_text(parent, &"{".repeat(f.count));
                    parent.push(node);
                }
            }
            i = pos;
            text_start = i;
            continue;
        }
        i += 1;
    }
    flush!(b.len());
    while let Some(f) = stack.pop() {
        let parent = target!();
        unwind(f, parent);
    }
    root
}

/// A node as text again: what is left unexpanded is written as the view sees it, with
/// `<includeonly>` and the other inclusion tags already resolved.
fn write_source(n: &PNode, out: &mut String) {
    let (open, close, parts) = match n {
        PNode::Text(t) => {
            out.push_str(t);
            return;
        }
        PNode::Template { parts, .. } => ("{{", "}}", parts),
        PNode::Argument { parts } => ("{{{", "}}}", parts),
    };
    out.push_str(open);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push('|');
        }
        for c in part {
            write_source(c, out);
        }
    }
    out.push_str(close);
}

// ---- expansion ----------------------------------------------------------------------

struct Expander<'h> {
    host: &'h dyn ExpandHost,
    registry: &'static Registry,
    stack: Vec<String>,
}

impl Expander<'_> {
    fn expand(&mut self, nodes: &[PNode], out: &mut String) {
        for n in nodes {
            match n {
                PNode::Text(t) => out.push_str(t),
                PNode::Argument { parts } => match parts.get(1) {
                    // No frame here has arguments: the default, or the text as written.
                    Some(default) => self.expand(default, out),
                    None => write_source(n, out),
                },
                PNode::Template { parts, .. } => {
                    self.template(n, parts, out);
                }
            }
        }
    }

    /// Expands a call into `out`; the name it was called by when it transcluded a page,
    /// which [`expand_for_view_mapped`] records.
    fn template(&mut self, node: &PNode, parts: &[Vec<PNode>], out: &mut String) -> Option<String> {
        let mut name = String::new();
        self.expand(&parts[0], &mut name);
        let name = name.trim();
        if let Some(key) = name
            .split_once(':')
            .filter(|(m, _)| m.trim().eq_ignore_ascii_case("int"))
            .map(|(_, k)| k.trim())
        {
            self.message(node, key, &parts[1..], out);
            return None;
        }
        if parts.len() > 1 {
            write_source(node, out);
            return None;
        }
        if name.is_empty() || self.registry.is_magic(name) {
            write_source(node, out);
            return None;
        }
        let Some(found) = self.host.transclude(name) else {
            write_source(node, out);
            return None;
        };
        match found {
            Transcluded::Missing(title) => {
                out.push_str("[[:");
                out.push_str(&title);
                out.push_str("]]");
                None
            }
            Transcluded::Found(text) => {
                let key = name.to_string();
                if self.stack.contains(&key) {
                    out.push_str("<span class=\"error\">Template loop detected: [[");
                    out.push_str(name);
                    out.push_str("]]</span>");
                    return None;
                }
                if self.stack.len() >= MAX_DEPTH {
                    out.push_str(
                        "<span class=\"error\">Template recursion depth limit exceeded</span>",
                    );
                    return None;
                }
                self.stack.push(key);
                let nodes = preprocess(&text, Mode::Transclude, self.registry);
                let mut expanded = String::new();
                self.expand(&nodes, &mut expanded);
                self.stack.pop();
                // Line markup at the start of an expansion still starts a line.
                if (expanded.starts_with(['*', '#', ':', ';']) || expanded.starts_with("{|"))
                    && !out.is_empty()
                    && !out.ends_with('\n')
                {
                    out.push('\n');
                }
                out.push_str(&expanded);
                Some(name.to_string())
            }
        }
    }
}

impl Expander<'_> {
    /// `{{int:key|arg|…}}`: the host's message with `$1`… replaced by the expanded
    /// arguments, itself expanded as transcluded text; left as written where the host has
    /// none or the key is empty.
    fn message(&mut self, node: &PNode, key: &str, args: &[Vec<PNode>], out: &mut String) {
        let Some(text) = (!key.is_empty()).then(|| self.host.message(key)).flatten() else {
            write_source(node, out);
            return;
        };
        let guard = format!("int:{key}");
        if self.stack.contains(&guard) || self.stack.len() >= MAX_DEPTH {
            write_source(node, out);
            return;
        }
        let mut text = text;
        let values: Vec<String> = args
            .iter()
            .map(|a| {
                let mut v = String::new();
                self.expand(a, &mut v);
                v
            })
            .collect();
        for (i, v) in values.iter().enumerate().rev() {
            text = text.replace(&format!("${}", i + 1), v);
        }
        self.stack.push(guard);
        let nodes = preprocess(&text, Mode::Transclude, self.registry);
        self.expand(&nodes, out);
        self.stack.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Host(HashMap<&'static str, &'static str>);

    impl ExpandHost for Host {
        fn transclude(&self, name: &str) -> Option<Transcluded> {
            let mut t = name.trim().replace('_', " ");
            if let Some(first) = t.get(..1) {
                t = first.to_uppercase() + &t[1..];
            }
            let title = format!("Template:{t}");
            Some(match self.0.get(t.as_str()) {
                Some(text) => Transcluded::Found((*text).to_string()),
                None => Transcluded::Missing(title),
            })
        }
    }

    fn host() -> Host {
        Host(HashMap::from([
            (
                "Cwbn",
                "<div>footer</div><noinclude>[[Category:Templates]]</noinclude>",
            ),
            ("Only", "outside<onlyinclude>inside</onlyinclude>after"),
            (
                "Inc",
                "<includeonly>shown</includeonly><noinclude>doc</noinclude>",
            ),
            ("List", "* a\n* b"),
            ("Loop", "x{{loop}}"),
            ("Arg", "[{{{1|one}}}] [{{{name}}}]"),
            ("Nested", "<{{cwbn}}>"),
        ]))
    }

    fn x(s: &str) -> String {
        expand_for_view(s, &host())
    }

    #[test]
    fn transclusion() {
        assert_eq!(x("a {{cwbn}} b"), "a <div>footer</div> b");
        assert_eq!(x("{{only}}"), "inside");
        assert_eq!(x("{{inc}}"), "shown");
        assert_eq!(x("{{nested}}"), "<<div>footer</div>>");
        assert_eq!(x("{{nothere}}"), "[[:Template:Nothere]]");
    }

    #[test]
    fn viewing_the_page_itself() {
        assert_eq!(
            x("<includeonly>hidden</includeonly><noinclude>shown</noinclude>"),
            "shown"
        );
        assert_eq!(x("<onlyinclude>a</onlyinclude>b"), "ab");
        // Comments stay for the renderer, which drops them; their content is not expanded.
        assert_eq!(x("x<!-- {{cwbn}} -->y"), "x<!-- {{cwbn}} -->y");
    }

    #[test]
    fn left_as_written() {
        assert_eq!(x("{{cwbn|x}}"), "{{cwbn|x}}");
        assert_eq!(
            x("{{REVISIONUSER}} {{#if:a|b}} {{subst:cwbn}} {{lc:X}}"),
            "{{REVISIONUSER}} {{#if:a|b}} {{subst:cwbn}} {{lc:X}}"
        );
        assert_eq!(
            x("<nowiki>{{cwbn}}</nowiki> <pre>{{cwbn}}</pre>"),
            "<nowiki>{{cwbn}}</nowiki> <pre>{{cwbn}}</pre>"
        );
        assert_eq!(x("{{unclosed"), "{{unclosed");
        assert_eq!(x("}} {{a|[[b|c]]}}"), "}} {{a|[[b|c]]}}");
    }

    #[test]
    fn arguments_without_a_frame() {
        assert_eq!(x("{{arg}}"), "[one] [{{{name}}}]");
        assert_eq!(x("{{{x|d}}}"), "d");
    }

    #[test]
    fn lists_start_a_line_and_loops_stop() {
        assert_eq!(x("text {{list}}"), "text \n* a\n* b");
        assert!(x("{{loop}}").contains("Template loop detected"));
    }

    #[test]
    fn braces() {
        assert_eq!(x("{{{cwbn}}}"), "{{{cwbn}}}");
    }

    struct Messages;

    impl ExpandHost for Messages {
        fn transclude(&self, name: &str) -> Option<Transcluded> {
            (name == "who").then(|| Transcluded::Found("Ann".into()))
        }
        fn message(&self, key: &str) -> Option<String> {
            match key {
                "welcome" => Some("Welcome, $1, to $2!".into()),
                "self" => Some("{{int:self}}".into()),
                "sig" => Some("by {{who}}".into()),
                _ => None,
            }
        }
    }

    #[test]
    fn int_reads_the_host_s_messages() {
        let e = |s: &str| expand_for_view(s, &Messages);
        assert_eq!(e("{{int:welcome|Bob|{{who}}}}"), "Welcome, Bob, to Ann!");
        assert_eq!(e("{{ INT: welcome |x}}"), "Welcome, x, to $2!");
        assert_eq!(e("{{int:sig}}"), "by Ann");
        assert_eq!(
            e("{{int:nope}}"),
            "{{int:nope}}",
            "left for the renderer's chip"
        );
        assert_eq!(e("{{int:}}"), "{{int:}}");
        assert_eq!(e("{{int:self}}"), "{{int:self}}", "no loop");
        // A host without messages leaves every call as written, as before.
        assert_eq!(
            expand_for_view("{{int:welcome}}", &host()),
            "{{int:welcome}}"
        );
    }

    // 0042 §4: the page's own calls are mapped to their output, so a renderer can mark
    // which template drew which block.
    #[test]
    fn the_page_s_own_calls_are_mapped_to_their_output() {
        let text = "Intro\n{{cwbn}}\nText {{ Nested }} {{missing}} {{#if:x|y}} {{list}}";
        let e = expand_for_view_mapped(text, &host());
        assert_eq!(e.text, expand_for_view(text, &host()));
        let names: Vec<&str> = e.calls.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["cwbn", "Nested", "list"],
            "found pages only, not the inner call"
        );
        assert_eq!(e.calls[0].at, text.find("{{cwbn").unwrap());
        assert_eq!(&e.text[e.calls[0].output.clone()], "<div>footer</div>");
        assert_eq!(e.calls[1].at, text.find("{{ Nested").unwrap());
        assert_eq!(&e.text[e.calls[1].output.clone()], "<<div>footer</div>>");
        // A list starts a line: the newline put before it is part of the call's output.
        assert_eq!(&e.text[e.calls[2].output.clone()], "\n* a\n* b");
        // Braces left open around a call: it starts after them.
        let e = expand_for_view_mapped("{{{{cwbn}}", &host());
        assert_eq!(e.calls.iter().map(|c| c.at).collect::<Vec<_>>(), [2]);
    }
}
