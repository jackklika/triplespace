//! Line diffs of a page's source (10 §4.3; 19 §3.8): what changed between two texts,
//! line by line, with the words that changed marked inside a changed line, and the two
//! layouts MediaWiki's `action=compare` writes for them: the table (`difftype=table`) and
//! the inline one (`difftype=inline`), a single column for a narrow screen.
//!
//! The diff is MediaWiki's shape: hunks with two lines of context, joined where their
//! context would meet, each opened by a `Line N:` row; a block of replaced lines is
//! paired line by line, and each pair is shown as one changed row with its changed words
//! marked, the lines left over as plain deletions or additions. The table rows carry
//! MediaWiki's class names (`diff-lineno`, `diff-marker`, `diff-deletedline`,
//! `diff-addedline`, `diff-context`, `diff-empty`, `diffchange-inline`), which clients
//! such as Pywikibot read.

use std::fmt::Write as _;

use similar::{ChangeTag, DiffOp, TextDiff};

/// Lines of context around a change, as MediaWiki shows.
pub const CONTEXT: usize = 2;

/// Context that keeps every line. Not `usize::MAX`: the grouping doubles it.
pub const ALL_LINES: usize = usize::MAX / 4;

/// A run of text in a changed line, and whether it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// The text.
    pub text: String,
    /// Whether this run is what changed.
    pub changed: bool,
}

/// Which side a run of a changed line is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Both: unchanged.
    Both,
    /// The old side only: removed.
    Old,
    /// The new side only: added.
    New,
}

/// A run of a changed line, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Part {
    /// The text.
    pub text: String,
    /// Its side.
    pub side: Side,
}

/// One row of a hunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// A line on both sides.
    Context(String),
    /// A line only on the new side.
    Added(String),
    /// A line only on the old side.
    Deleted(String),
    /// A line changed: its runs in order, unchanged, removed and added.
    Changed(Vec<Part>),
}

/// A changed line's runs as each side reads them: the old side's, then the new side's,
/// each run marked changed or not.
#[must_use]
pub fn sides(parts: &[Part]) -> (Vec<Segment>, Vec<Segment>) {
    let (mut a, mut b): (Vec<Segment>, Vec<Segment>) = (Vec::new(), Vec::new());
    let push = |side: &mut Vec<Segment>, text: &str, changed: bool| match side.last_mut() {
        Some(last) if last.changed == changed => last.text.push_str(text),
        _ => side.push(Segment {
            text: text.to_string(),
            changed,
        }),
    };
    for p in parts {
        match p.side {
            Side::Both => {
                push(&mut a, &p.text, false);
                push(&mut b, &p.text, false);
            }
            Side::Old => push(&mut a, &p.text, true),
            Side::New => push(&mut b, &p.text, true),
        }
    }
    (a, b)
}

/// A hunk: where it starts on each side, 1-based, and its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    /// The first old line.
    pub old_line: usize,
    /// The first new line.
    pub new_line: usize,
    /// The rows.
    pub rows: Vec<Row>,
}

/// The lines of a text, without their line ends.
fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    }
}

/// The runs of a changed line pair, in order, adjacent runs of one side joined.
fn words(old: &str, new: &str) -> Vec<Part> {
    let diff = TextDiff::from_words(old, new);
    let mut parts: Vec<Part> = Vec::new();
    for change in diff.iter_all_changes() {
        let side = match change.tag() {
            ChangeTag::Equal => Side::Both,
            ChangeTag::Delete => Side::Old,
            ChangeTag::Insert => Side::New,
        };
        match parts.last_mut() {
            Some(last) if last.side == side => last.text.push_str(change.value()),
            _ => parts.push(Part {
                text: change.value().to_string(),
                side,
            }),
        }
    }
    parts
}

/// The hunks between two texts; none when they are equal.
#[must_use]
pub fn diff(old: &str, new: &str) -> Vec<Hunk> {
    diff_with_context(old, new, CONTEXT)
}

/// The hunks between two texts with `context` lines of context around each change
/// instead of [`CONTEXT`]; [`ALL_LINES`] keeps every line, as one hunk (0010 §6).
#[must_use]
pub fn diff_with_context(old: &str, new: &str, context: usize) -> Vec<Hunk> {
    let (a, b) = (lines(old), lines(new));
    let ops = similar::capture_diff_slices(similar::Algorithm::Myers, &a, &b);
    let groups = similar::group_diff_ops(ops, context);
    let mut hunks = Vec::new();
    for group in groups {
        let Some(first) = group.first() else { continue };
        let mut hunk = Hunk {
            old_line: first.old_range().start + 1,
            new_line: first.new_range().start + 1,
            rows: Vec::new(),
        };
        for op in &group {
            match *op {
                DiffOp::Equal { old_index, len, .. } => {
                    for line in &a[old_index..old_index + len] {
                        hunk.rows.push(Row::Context((*line).to_string()));
                    }
                }
                DiffOp::Delete {
                    old_index, old_len, ..
                } => {
                    for line in &a[old_index..old_index + old_len] {
                        hunk.rows.push(Row::Deleted((*line).to_string()));
                    }
                }
                DiffOp::Insert {
                    new_index, new_len, ..
                } => {
                    for line in &b[new_index..new_index + new_len] {
                        hunk.rows.push(Row::Added((*line).to_string()));
                    }
                }
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    let paired = old_len.min(new_len);
                    for i in 0..paired {
                        hunk.rows
                            .push(Row::Changed(words(a[old_index + i], b[new_index + i])));
                    }
                    for line in &a[old_index + paired..old_index + old_len] {
                        hunk.rows.push(Row::Deleted((*line).to_string()));
                    }
                    for line in &b[new_index + paired..new_index + new_len] {
                        hunk.rows.push(Row::Added((*line).to_string()));
                    }
                }
            }
        }
        hunks.push(hunk);
    }
    hunks
}

/// Escapes text for an element's content.
fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

/// A line as a cell's content: escaped, and a line break where it is empty, as
/// MediaWiki writes it so the row keeps its height.
fn line_html(text: &str) -> String {
    if text.is_empty() {
        "<div><br></div>".into()
    } else {
        format!("<div>{}</div>", esc(text))
    }
}

/// A changed line's runs, the changed ones in `tag`.
fn segments_html(segments: &[Segment], tag: &str) -> String {
    let mut s = String::from("<div>");
    for seg in segments {
        if seg.changed {
            {
                let _ = write!(
                    s,
                    "<{tag} class=\"diffchange diffchange-inline\">{}</{tag}>",
                    esc(&seg.text)
                );
            };
        } else {
            s.push_str(&esc(&seg.text));
        }
    }
    if segments.iter().all(|seg| seg.text.is_empty()) {
        s.push_str("<br>");
    }
    s.push_str("</div>");
    s
}

const DELETED_MARKER: &str = "<td class=\"diff-marker\" data-marker=\"\u{2212}\"></td>";
const ADDED_MARKER: &str = "<td class=\"diff-marker\" data-marker=\"+\"></td>";
const NO_MARKER: &str = "<td class=\"diff-marker\"></td>";

/// The table rows of `action=compare`'s `body`, as MediaWiki's table formatter writes
/// them; the caller supplies the `<table>` and its columns. `line` words the hunk
/// headers: `Line {n}:` in English.
#[must_use]
pub fn table_rows(hunks: &[Hunk], line: impl Fn(usize) -> String) -> String {
    let mut s = String::new();
    for h in hunks {
        {
            let _ = writeln!(
                s,
                "<tr><td colspan=\"2\" class=\"diff-lineno\">{}</td>\n<td colspan=\"2\" class=\"diff-lineno\">{}</td></tr>",
                esc(&line(h.old_line)),
                esc(&line(h.new_line))
            );
        };
        for row in &h.rows {
            match row {
                Row::Context(text) => {
                    let _ = writeln!(
                        s,
                        "<tr>{NO_MARKER}<td class=\"diff-context diff-side-deleted\">{l}</td>{NO_MARKER}<td class=\"diff-context diff-side-added\">{l}</td></tr>",
                        l = line_html(text)
                    );
                }
                Row::Deleted(text) => {
                    let _ = writeln!(
                        s,
                        "<tr>{DELETED_MARKER}<td class=\"diff-deletedline diff-side-deleted\">{}</td><td colspan=\"2\" class=\"diff-empty diff-side-added\"></td></tr>",
                        line_html(text)
                    );
                }
                Row::Added(text) => {
                    let _ = writeln!(
                        s,
                        "<tr><td colspan=\"2\" class=\"diff-empty diff-side-deleted\"></td>{ADDED_MARKER}<td class=\"diff-addedline diff-side-added\">{}</td></tr>",
                        line_html(text)
                    );
                }
                Row::Changed(parts) => {
                    let (old, new) = sides(parts);
                    let _ = writeln!(
                        s,
                        "<tr>{DELETED_MARKER}<td class=\"diff-deletedline diff-side-deleted\">{}</td>{ADDED_MARKER}<td class=\"diff-addedline diff-side-added\">{}</td></tr>",
                        segments_html(&old, "del"),
                        segments_html(&new, "ins")
                    );
                }
            }
        }
    }
    s
}

/// The inline layout of `action=compare&difftype=inline`, MediaWiki's single column: each
/// hunk opened by its header (`line` words it, given the new side's first line), context
/// lines as they are, a removed line in `<del>` and an added one in `<ins>`, and a changed
/// line once, its removed and added words marked in place. Each changed line carries its
/// marker (`−`, `+` or `~`) in `data-marker`, so a reader need not tell them apart by
/// colour.
#[must_use]
pub fn inline_rows(hunks: &[Hunk], line: impl Fn(usize) -> String) -> String {
    let text = |t: &str| {
        if t.is_empty() {
            "<br>".to_string()
        } else {
            esc(t)
        }
    };
    let mut s = String::new();
    for h in hunks {
        let _ = writeln!(
            s,
            "<div class=\"mw-diff-inline-header\">{}</div>",
            esc(&line(h.new_line))
        );
        for row in &h.rows {
            match row {
                Row::Context(t) => {
                    let _ = writeln!(s, "<div class=\"mw-diff-inline-context\">{}</div>", text(t));
                }
                Row::Deleted(t) => {
                    let _ = writeln!(
                        s,
                        "<div class=\"mw-diff-inline-deleted\" data-marker=\"\u{2212}\"><del>{}</del></div>",
                        text(t)
                    );
                }
                Row::Added(t) => {
                    let _ = writeln!(
                        s,
                        "<div class=\"mw-diff-inline-added\" data-marker=\"+\"><ins>{}</ins></div>",
                        text(t)
                    );
                }
                Row::Changed(parts) => {
                    s.push_str("<div class=\"mw-diff-inline-changed\" data-marker=\"~\">");
                    for p in parts {
                        match p.side {
                            Side::Both => s.push_str(&esc(&p.text)),
                            Side::Old => {
                                let _ = write!(
                                    s,
                                    "<del class=\"diffchange diffchange-inline\">{}</del>",
                                    esc(&p.text)
                                );
                            }
                            Side::New => {
                                let _ = write!(
                                    s,
                                    "<ins class=\"diffchange diffchange-inline\">{}</ins>",
                                    esc(&p.text)
                                );
                            }
                        }
                    }
                    if parts.iter().all(|p| p.text.is_empty()) {
                        s.push_str("<br>");
                    }
                    s.push_str("</div>\n");
                }
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(n: usize) -> String {
        format!("Line {n}:")
    }

    #[test]
    fn equal_texts_have_no_hunks() {
        assert!(diff("a\nb", "a\nb").is_empty());
        assert!(diff("", "").is_empty());
        assert_eq!(table_rows(&[], line), "");
    }

    // 0010 §6: the diff page's "More context" asks for more lines around each change.
    #[test]
    fn context_widens_and_joins_hunks() {
        let old: Vec<String> = (1..=20).map(|i| format!("l{i}")).collect();
        let mut new = old.clone();
        new[4] = "changed 5".into();
        new[14] = "changed 15".into();
        let (old, new) = (old.join("\n"), new.join("\n"));
        let two = diff(&old, &new);
        assert_eq!(two.len(), 2, "two changes ten lines apart are two hunks");
        assert_eq!(two[0].rows.len(), 5);
        let five = diff_with_context(&old, &new, 5);
        assert_eq!(five.len(), 1, "their context meets");
        assert_eq!((five[0].old_line, five[0].rows.len()), (1, 20));
        let three = diff_with_context(&old, &new, 3);
        assert_eq!(three.len(), 2);
        assert_eq!((three[1].old_line, three[1].rows.len()), (12, 7));
        let middle = old.replace("l10\n", "ten\n");
        assert_eq!(diff(&old, &middle)[0].rows.len(), 5);
        let all = diff_with_context(&old, &middle, ALL_LINES);
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].old_line, all[0].rows.len()), (1, 20), "every line");
        assert!(diff_with_context(&old, &old, ALL_LINES).is_empty());
    }

    #[test]
    fn a_changed_line_marks_its_words() {
        let h = diff(
            "one\ntwo\nthree\nfour\nfive\nsix",
            "one\ntwo\nthree\nFOUR\nfive\nsix",
        );
        assert_eq!(h.len(), 1);
        assert_eq!(
            (h[0].old_line, h[0].new_line),
            (2, 2),
            "two lines of context"
        );
        assert_eq!(h[0].rows.len(), 5);
        let Row::Changed(parts) = &h[0].rows[2] else {
            panic!("{:?}", h[0].rows)
        };
        let (old, new) = sides(parts);
        assert_eq!(
            old[0],
            Segment {
                text: "four".into(),
                changed: true
            }
        );
        assert_eq!(
            new[0],
            Segment {
                text: "FOUR".into(),
                changed: true
            }
        );
        let html = table_rows(&h, line);
        assert!(html.starts_with("<tr><td colspan=\"2\" class=\"diff-lineno\">Line 2:</td>"));
        assert!(html.contains("<del class=\"diffchange diffchange-inline\">four</del>"));
        assert!(html.contains("<ins class=\"diffchange diffchange-inline\">FOUR</ins>"));
        assert!(html.contains("data-marker=\"\u{2212}\""));
        assert_eq!(html.matches("diff-context diff-side-deleted").count(), 4);
    }

    #[test]
    fn additions_deletions_and_separate_hunks() {
        let old: String = (1..=20).fold(String::new(), |mut s, n| {
            let _ = writeln!(s, "l{n}");
            s
        });
        let mut new = old.replace("l3\n", "").replace("l17\n", "l17\nnew <b>&\n");
        new.push_str("tail");
        let h = diff(&old, &new);
        assert_eq!(h.len(), 2, "{h:?}");
        assert_eq!(
            h[0].rows
                .iter()
                .filter(|r| matches!(r, Row::Deleted(_)))
                .count(),
            1
        );
        let html = table_rows(&h, line);
        assert!(html.contains("class=\"diff-empty diff-side-added\""));
        assert!(html.contains("<div>new &lt;b&gt;&amp;</div>"));
        // The old text ended with a newline, so its last line was empty: `tail` changes
        // it, and the empty side keeps its height.
        assert!(html.contains("<ins class=\"diffchange diffchange-inline\">tail</ins>"));
        assert!(html.contains("<div><br></div>"));
    }

    #[test]
    fn the_inline_layout_writes_a_changed_line_once() {
        let h = diff("a\nthe old word\nc", "a\nthe new word\nc\nd <e>");
        let html = inline_rows(&h, |n| format!("Line {n}:"));
        assert!(
            html.starts_with("<div class=\"mw-diff-inline-header\">Line 1:</div>"),
            "{html}"
        );
        assert!(html.contains(
            "<div class=\"mw-diff-inline-changed\" data-marker=\"~\">the <del class=\"diffchange diffchange-inline\">old</del><ins class=\"diffchange diffchange-inline\">new</ins> word</div>"
        ), "{html}");
        assert!(html.contains(
            "<div class=\"mw-diff-inline-added\" data-marker=\"+\"><ins>d &lt;e&gt;</ins></div>"
        ));
        assert_eq!(html.matches("mw-diff-inline-context").count(), 2);
        assert!(!html.contains("<table") && !html.contains("<tr"));
        assert_eq!(inline_rows(&[], |n| n.to_string()), "");
    }

    #[test]
    fn a_new_page_is_all_additions() {
        let h = diff("", "a\nb");
        assert_eq!(
            h[0].rows,
            vec![Row::Added("a".into()), Row::Added("b".into())]
        );
        assert_eq!((h[0].old_line, h[0].new_line), (1, 1));
    }
}
