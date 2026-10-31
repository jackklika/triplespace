//! Building HTML from escaped parts, for the pieces too small for a template.

use std::fmt::Write as _;

/// Escapes text for an element's content or a quoted attribute.
#[must_use]
pub fn esc(text: &str) -> String {
    if !text
        .bytes()
        .any(|b| matches!(b, b'&' | b'<' | b'>' | b'"' | b'\''))
    {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// A link: `<a href="…">text</a>`, the text escaped, with extra attributes already
/// escaped by the caller.
#[must_use]
pub fn link(href: &str, text_html: &str, extra: &str) -> String {
    let mut s = String::new();
    let _ = write!(s, "<a href=\"{}\"{extra}>{text_html}</a>", esc(href));
    s
}

/// Whether a URL is safe to link: `http`, `https`, `ftp` or `mailto`.
#[must_use]
pub fn linkable(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "ftp://", "mailto:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_and_links() {
        assert_eq!(
            esc("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
        assert_eq!(
            link("/wiki/A&B", "A &amp; B", ""),
            "<a href=\"/wiki/A&amp;B\">A &amp; B</a>"
        );
        assert!(linkable("https://example.org"));
        assert!(!linkable("javascript:alert(1)"));
        assert!(!linkable(" JavaScript:x"));
    }
}
