//! Inline text as data (19 §5.10): a sentence, a table cell or a list item as a sequence
//! of parts, text, links, code and line breaks, which Plain's `inline.html` draws. A
//! message whose parameters are links is filled with parts, never with HTML, so that a
//! view model carries no markup (0097 §11).

use serde::Serialize;

use crate::i18n::Messages;

/// One part of a line of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Inline {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// A link.
    Link {
        /// Where it goes.
        href: String,
        /// Its text.
        text: String,
        /// Off the site: `class="external" rel="nofollow"`.
        external: bool,
    },
    /// Code: a key, a value as typed, a commit.
    Code {
        /// The text.
        text: String,
    },
    /// Text in the monospace face, not code: a language code.
    Mono {
        /// The text.
        text: String,
    },
    /// A line break.
    Break,
    /// A list too long to show, opening on demand.
    Details {
        /// What it is, and how many.
        title: String,
        /// The list, as text.
        text: String,
    },
}

impl Inline {
    /// Text.
    #[must_use]
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text { text: s.into() }
    }

    /// A link on the site.
    #[must_use]
    pub fn link(href: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Link {
            href: href.into(),
            text: text.into(),
            external: false,
        }
    }

    /// A link off the site.
    #[must_use]
    pub fn external(href: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Link {
            href: href.into(),
            text: text.into(),
            external: true,
        }
    }

    /// Code.
    #[must_use]
    pub fn code(s: impl Into<String>) -> Self {
        Self::Code { text: s.into() }
    }
}

/// Text alone, as parts.
#[must_use]
pub fn text(s: impl Into<String>) -> Vec<Inline> {
    let s = s.into();
    if s.is_empty() {
        Vec::new()
    } else {
        vec![Inline::text(s)]
    }
}

/// Parts joined by a separator.
#[must_use]
pub fn join(items: Vec<Vec<Inline>>, separator: &str) -> Vec<Inline> {
    let mut out = Vec::new();
    for (i, item) in items.into_iter().enumerate() {
        if i > 0 {
            out.push(Inline::text(separator));
        }
        out.extend(item);
    }
    merge(out)
}

/// The private-use characters a message's parameters stand in as while it is filled.
fn slot(i: usize) -> String {
    format!("\u{F000}{i}\u{F001}")
}

/// A message whose parameters are parts: the message's text is text, each `$n` the parts
/// given for it.
#[must_use]
pub fn message(m: &Messages, key: &str, params: Vec<Vec<Inline>>) -> Vec<Inline> {
    let slots: Vec<String> = (0..params.len()).map(slot).collect();
    let refs: Vec<&str> = slots.iter().map(String::as_str).collect();
    fill(&m.with(key, &refs), params)
}

/// A list in the reader's language, `A`, `A and B`, `A, B and C`, of parts.
#[must_use]
pub fn list(m: &Messages, items: Vec<Vec<Inline>>) -> Vec<Inline> {
    let slots: Vec<String> = (0..items.len()).map(slot).collect();
    let joined = match slots.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, middle @ .., last] => {
            let head = middle.iter().fold(first.clone(), |acc, item| {
                m.with("ts-version-list-comma", &[&acc, item])
            });
            m.with("ts-version-list-and", &[&head, last])
        }
    };
    fill(&joined, items)
}

/// Text with slots, each slot replaced by its parts.
fn fill(text: &str, mut params: Vec<Vec<Inline>>) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('\u{F000}') {
        let Some(close) = rest[open..].find('\u{F001}').map(|c| open + c) else {
            break;
        };
        if open > 0 {
            out.push(Inline::text(&rest[..open]));
        }
        if let Ok(i) = rest[open + '\u{F000}'.len_utf8()..close].parse::<usize>()
            && let Some(p) = params.get_mut(i)
        {
            out.append(p);
        }
        rest = &rest[close + '\u{F001}'.len_utf8()..];
    }
    if !rest.is_empty() {
        out.push(Inline::text(rest));
    }
    merge(out)
}

/// Adjacent text parts as one.
fn merge(parts: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(parts.len());
    for p in parts {
        if let (Some(Inline::Text { text: last }), Inline::Text { text }) = (out.last_mut(), &p) {
            last.push_str(text);
            continue;
        }
        if matches!(&p, Inline::Text { text } if text.is_empty()) {
            continue;
        }
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_filled_with_parts() {
        let m = Messages::for_language("en");
        let parts = message(
            &m,
            "ts-version-license-copy",
            vec![
                vec![Inline::link("/wiki/Special:Version/License", "a copy")],
                vec![Inline::external("https://www.gnu.org/licenses/", "gnu")],
            ],
        );
        assert!(
            parts
                .iter()
                .any(|p| matches!(p, Inline::Link { external: true, .. }))
        );
        assert!(parts.iter().any(|p| matches!(
            p,
            Inline::Link {
                external: false,
                ..
            }
        )));
        assert!(!format!("{parts:?}").contains('\u{F000}'));
    }

    #[test]
    fn lists_in_the_readers_language() {
        let m = Messages::for_language("en");
        let three = list(
            &m,
            vec![text("A"), text("B"), vec![Inline::link("/c", "C")]],
        );
        assert_eq!(
            three,
            vec![Inline::text("A, B and "), Inline::link("/c", "C")]
        );
        assert_eq!(list(&m, vec![text("A")]), text("A"));
        assert!(list(&m, Vec::new()).is_empty());
    }
}
