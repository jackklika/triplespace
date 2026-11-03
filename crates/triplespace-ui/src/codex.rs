//! The Codex components (0034 §1.4) as the models the skin draws: Codex's CSS-only
//! markup, so that a page reads without JavaScript and in either direction, styled by
//! `codex.style.css` and themed by the token values of [`crate::theme`].
//!
//! Codex's components: button, search input, message, table, accordion, card, field with
//! a text input, and the indeterminate progress bar (a determinate one sets its width in
//! a `style` attribute, which the content security policy refuses). Page tabs are links
//! styled from tokens in the frame (`ts-tabs`), since Codex's Tabs are in-page panels.
//! The chips of 0010 §2 are the site's own (`ts-chip`), built from Codex tokens; a
//! provider's colours come from the theme stylesheet ([`crate::theme`]).
//!
//! Each component's `model()` is the component as the skin's macro draws it
//! (`skins/plain/templates/codex.html`; 19 §5.2): the fields with every class computed.
//! The macro escapes what it is given, except a field named `…_html`, which the caller
//! builds from escaped parts. `tests/codex_macros.rs` holds each macro's markup.

/// What a button does (Codex `action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Action {
    /// A neutral action.
    #[default]
    Default,
    /// The action that moves the reader forward.
    Progressive,
    /// An action that removes or ends something.
    Destructive,
}

/// How loud a button is (Codex `weight`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Weight {
    /// Bordered.
    #[default]
    Normal,
    /// Filled.
    Primary,
    /// Borderless.
    Quiet,
}

/// A Codex button.
#[derive(Debug, Clone)]
pub struct Button<'a> {
    /// The label.
    pub label: &'a str,
    /// `submit` or `button`.
    pub kind: &'a str,
    /// What it does.
    pub action: Action,
    /// How loud it is.
    pub weight: Weight,
}

/// A button as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ButtonModel {
    /// The label.
    pub label: String,
    /// `submit` or `button`.
    pub kind: String,
    /// The `class` attribute.
    pub classes: String,
}

impl<'a> Button<'a> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> ButtonModel {
        ButtonModel {
            label: self.label.to_string(),
            kind: self.kind.to_string(),
            classes: self.classes(),
        }
    }

    /// A neutral submit button.
    #[must_use]
    pub fn submit(label: &'a str) -> Self {
        Self {
            label,
            kind: "submit",
            action: Action::Default,
            weight: Weight::Normal,
        }
    }

    /// The `class` attribute.
    #[must_use]
    pub fn classes(&self) -> String {
        let mut c = String::from("cdx-button");
        match self.action {
            Action::Default => {}
            Action::Progressive => c.push_str(" cdx-button--action-progressive"),
            Action::Destructive => c.push_str(" cdx-button--action-destructive"),
        }
        match self.weight {
            Weight::Normal => {}
            Weight::Primary => c.push_str(" cdx-button--weight-primary"),
            Weight::Quiet => c.push_str(" cdx-button--weight-quiet"),
        }
        c
    }
}

/// A search input as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SearchModel {
    /// The field's name.
    pub name: String,
    /// What is already in it.
    pub value: String,
    /// The placeholder.
    pub placeholder: String,
    /// The field's accessible name.
    pub label: String,
    /// The button's label.
    pub button: String,
}

impl SearchInput<'_> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> SearchModel {
        SearchModel {
            name: self.name.to_string(),
            value: self.value.to_string(),
            placeholder: self.placeholder.to_string(),
            label: self.label.to_string(),
            button: self.button.to_string(),
        }
    }
}

/// A Codex search input with its button, for inside a `GET` form.
#[derive(Debug, Clone)]
pub struct SearchInput<'a> {
    /// The field's name.
    pub name: &'a str,
    /// What is already in it.
    pub value: &'a str,
    /// The placeholder.
    pub placeholder: &'a str,
    /// The field's accessible name.
    pub label: &'a str,
    /// The button's label.
    pub button: &'a str,
}

/// The kind of a message (Codex `type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MessageKind {
    /// Information.
    #[default]
    Notice,
    /// Something to be careful of.
    Warning,
    /// Something failed.
    Error,
    /// Something succeeded.
    Success,
}

/// A Codex message, of plain text.
#[derive(Debug, Clone)]
pub struct Message<'a> {
    /// The kind.
    pub kind: MessageKind,
    /// Inline rather than block.
    pub inline: bool,
    /// The text.
    pub text: &'a str,
}

/// A message as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MessageModel {
    /// The `class` attribute.
    pub classes: String,
    /// The live-region attribute, HTML.
    pub live: String,
    /// The text.
    pub text: String,
}

impl<'a> Message<'a> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> MessageModel {
        MessageModel {
            classes: self.classes(),
            live: self.live().to_string(),
            text: self.text.to_string(),
        }
    }

    /// A block message.
    #[must_use]
    pub fn block(kind: MessageKind, text: &'a str) -> Self {
        Self {
            kind,
            inline: false,
            text,
        }
    }

    /// The `class` attribute.
    #[must_use]
    pub fn classes(&self) -> String {
        let mut c = String::from("cdx-message");
        if self.inline {
            c.push_str(" cdx-message--inline");
        }
        c.push_str(match self.kind {
            MessageKind::Notice => "",
            MessageKind::Warning => " cdx-message--warning",
            MessageKind::Error => " cdx-message--error",
            MessageKind::Success => " cdx-message--success",
        });
        c
    }

    /// How a screen reader announces it, as Codex's component does: an error at once, the
    /// rest politely.
    #[must_use]
    pub fn live(&self) -> &'static str {
        match self.kind {
            MessageKind::Error => "role=\"alert\"",
            _ => "aria-live=\"polite\"",
        }
    }
}

/// What a chip marks (0010 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChipKind {
    /// A provider's code, in the provider's colours.
    Provider(String),
    /// Made on this instance.
    Local,
    /// A namespace or kind of page.
    Namespace,
    /// An import job.
    Job,
    /// A log action.
    Log,
    /// A local correction of a mirrored value.
    Corrected,
    /// The value a reader should use (0003 §9).
    Best,
    /// A rank other than normal.
    Rank,
    /// The instance itself, as the server's operator (0095 §3).
    Operator,
}

/// A chip.
#[derive(Debug, Clone)]
pub struct Chip<'a> {
    /// What it marks.
    pub kind: ChipKind,
    /// Its text.
    pub text: &'a str,
    /// A tooltip.
    pub title: Option<&'a str>,
}

/// A chip as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChipModel {
    /// The `class` attribute.
    pub classes: String,
    /// The text.
    pub text: String,
    /// The tooltip.
    pub title: Option<String>,
}

impl<'a> Chip<'a> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> ChipModel {
        ChipModel {
            classes: self.classes(),
            text: self.text.to_string(),
            title: self.title.map(str::to_string),
        }
    }

    /// A chip of a kind.
    #[must_use]
    pub fn new(kind: ChipKind, text: &'a str) -> Self {
        Self {
            kind,
            text,
            title: None,
        }
    }

    /// The `class` attribute. A provider's class is its code in lower case, which the
    /// registry restricts to letters (0017 §2).
    #[must_use]
    pub fn classes(&self) -> String {
        match &self.kind {
            ChipKind::Provider(code) => {
                let code: String = code
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .map(|c| c.to_ascii_lowercase())
                    .collect();
                format!("ts-chip ts-chip--provider ts-chip--p-{code}")
            }
            ChipKind::Local => "ts-chip ts-chip--local".into(),
            ChipKind::Namespace => "ts-chip ts-chip--namespace".into(),
            ChipKind::Job => "ts-chip ts-chip--job".into(),
            ChipKind::Log => "ts-chip ts-chip--log".into(),
            ChipKind::Corrected => "ts-chip ts-chip--corrected".into(),
            ChipKind::Best => "ts-chip ts-chip--best".into(),
            ChipKind::Rank => "ts-chip ts-chip--rank".into(),
            ChipKind::Operator => "ts-chip ts-chip--operator".into(),
        }
    }
}

/// How a column's cells align.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// At the start of the line.
    #[default]
    Start,
    /// Centred.
    Center,
    /// At the end of the line.
    End,
    /// As numbers: at the right in either direction.
    Number,
}

impl Align {
    /// The cell class, if any.
    #[must_use]
    pub fn class(self) -> Option<&'static str> {
        match self {
            Self::Start => None,
            Self::Center => Some("cdx-table__table__cell--align-center"),
            Self::End => Some("cdx-table__table__cell--align-end"),
            Self::Number => Some("cdx-table__table__cell--align-number"),
        }
    }
}

/// A table as the skin's macro draws it: each column and cell with its alignment class.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TableModel {
    /// The header caption, above the table.
    pub header: Option<String>,
    /// The caption.
    pub caption: String,
    /// The columns.
    pub columns: Vec<ColumnModel>,
    /// The rows.
    pub rows: Vec<Vec<CellModel>>,
    /// What an empty table says.
    pub empty: String,
}

/// A column of [`TableModel`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ColumnModel {
    /// The heading.
    pub label: String,
    /// The alignment class.
    pub align_class: Option<String>,
}

/// A cell of [`TableModel`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CellModel {
    /// The cell, as parts.
    pub content: Vec<crate::inline::Inline>,
    /// The alignment class.
    pub align_class: Option<String>,
    /// Whether it is a row heading.
    pub header: bool,
    /// Its language, where it differs from the page's.
    pub lang: Option<String>,
}

impl ColumnModel {
    /// A column aligned as `align`.
    #[must_use]
    pub fn new(label: String, align: Align) -> Self {
        Self {
            label,
            align_class: align.class().map(str::to_string),
        }
    }
}

impl CellModel {
    /// A cell of parts.
    #[must_use]
    pub fn of(content: Vec<crate::inline::Inline>) -> Self {
        Self {
            content,
            align_class: None,
            header: false,
            lang: None,
        }
    }

    /// A row heading of parts.
    #[must_use]
    pub fn heading(content: Vec<crate::inline::Inline>) -> Self {
        Self {
            header: true,
            ..Self::of(content)
        }
    }

    /// Text.
    #[must_use]
    pub fn text(s: &str) -> Self {
        Self::of(crate::inline::text(s))
    }
}

/// An accordion as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AccordionModel {
    /// The title.
    pub title: String,
    /// The description.
    pub description: Option<String>,
    /// The content, HTML.
    pub content_html: String,
    /// Whether it starts open.
    pub open: bool,
}

impl Accordion<'_> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> AccordionModel {
        AccordionModel {
            title: self.title.to_string(),
            description: self.description.map(str::to_string),
            content_html: self.content.to_string(),
            open: self.open,
        }
    }
}

/// A card as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CardModel {
    /// The title.
    pub title: String,
    /// The description.
    pub description: Option<String>,
    /// The supporting text.
    pub supporting: Option<String>,
    /// Where it links.
    pub href: Option<String>,
}

impl Card<'_> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> CardModel {
        CardModel {
            title: self.title.to_string(),
            description: self.description.map(str::to_string),
            supporting: self.supporting.map(str::to_string),
            href: self.href.map(str::to_string),
        }
    }
}

/// A field as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FieldModel {
    /// The input's `id`.
    pub id: String,
    /// The input's name.
    pub name: String,
    /// The input's type.
    pub kind: String,
    /// The value.
    pub value: String,
    /// The label.
    pub label: String,
    /// The description.
    pub description: Option<String>,
    /// The help text.
    pub help: Option<String>,
    /// Whether it is required.
    pub required: bool,
    /// The `autocomplete` hint.
    pub autocomplete: Option<String>,
}

impl Field<'_> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> FieldModel {
        FieldModel {
            id: self.id.to_string(),
            name: self.name.to_string(),
            kind: self.kind.to_string(),
            value: self.value.to_string(),
            label: self.label.to_string(),
            description: self.description.map(str::to_string),
            help: self.help.map(str::to_string),
            required: self.required,
            autocomplete: self.autocomplete.map(str::to_string),
        }
    }
}

/// A progress bar as the skin's macro draws it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ProgressModel {
    /// The accessible name.
    pub label: String,
    /// Whether it is inline.
    pub inline: bool,
}

impl ProgressBar<'_> {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> ProgressModel {
        ProgressModel {
            label: self.label.to_string(),
            inline: self.inline,
        }
    }
}

/// A Codex accordion: a `<details>` that opens without JavaScript.
#[derive(Debug, Clone)]
pub struct Accordion<'a> {
    /// The title.
    pub title: &'a str,
    /// A line under the title.
    pub description: Option<&'a str>,
    /// The content, as HTML built from escaped parts.
    pub content: &'a str,
    /// Open to start with.
    pub open: bool,
}

/// A Codex card, a link when it has an `href`.
#[derive(Debug, Clone)]
pub struct Card<'a> {
    /// The title.
    pub title: &'a str,
    /// The description.
    pub description: Option<&'a str>,
    /// The supporting text.
    pub supporting: Option<&'a str>,
    /// Where it goes.
    pub href: Option<&'a str>,
}

/// A Codex field: a label, a text input, a description and help text.
#[derive(Debug, Clone)]
pub struct Field<'a> {
    /// The input's `id`.
    pub id: &'a str,
    /// The input's `name`.
    pub name: &'a str,
    /// The input's `type`: `text`, `password`, `email`, ….
    pub kind: &'a str,
    /// Its value.
    pub value: &'a str,
    /// The label.
    pub label: &'a str,
    /// A description under the label.
    pub description: Option<&'a str>,
    /// Help under the input.
    pub help: Option<&'a str>,
    /// Whether it must be filled.
    pub required: bool,
    /// The input's `autocomplete` hint (`username`, `current-password`).
    pub autocomplete: Option<&'a str>,
}

/// A Codex progress bar, indeterminate.
#[derive(Debug, Clone)]
pub struct ProgressBar<'a> {
    /// Its accessible name.
    pub label: &'a str,
    /// The inline (thin) variant.
    pub inline: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons() {
        let b = Button {
            weight: Weight::Primary,
            action: Action::Progressive,
            ..Button::submit("Save <now>")
        };
        assert_eq!(
            b.model().classes,
            "cdx-button cdx-button--action-progressive cdx-button--weight-primary"
        );
        assert_eq!(Button::submit("Go").classes(), "cdx-button");
    }

    #[test]
    fn messages_announce() {
        let m = Message::block(MessageKind::Error, "a & b").model();
        assert_eq!(m.classes, "cdx-message cdx-message--error");
        assert_eq!(m.live, "role=\"alert\"");
        let n = Message::block(MessageKind::Notice, "x").model();
        assert_eq!(n.classes, "cdx-message");
        assert_eq!(n.live, "aria-live=\"polite\"");
    }

    #[test]
    fn chips() {
        let c = Chip::new(ChipKind::Provider("WD\"x".into()), "WDQ65").model();
        assert_eq!(c.classes, "ts-chip ts-chip--provider ts-chip--p-wdx");
    }
}
