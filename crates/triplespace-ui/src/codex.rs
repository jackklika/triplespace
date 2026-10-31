//! The Codex builder (0034 §1.4): Codex components as their CSS-only markup, so that a page
//! reads without JavaScript and in either direction, styled by `codex.style.css` and
//! themed by the token values of [`crate::theme`].
//!
//! Each component is an askama template that escapes what it is given; its rendering is
//! therefore safe to place in another template unescaped, except where a field is named
//! `…_html` or is a [`Cell`]'s HTML, which the caller builds from escaped parts.
//!
//! Codex's components: button, search input, message, table, accordion, card, field with
//! a text input, and the indeterminate progress bar (a determinate one sets its width in
//! a `style` attribute, which the content security policy refuses). Page tabs are links
//! styled from tokens in the frame (`ts-tabs`), since Codex's Tabs are in-page panels.
//! The chips of 0010 §2 are the site's own (`ts-chip`), built from Codex tokens; a
//! provider's colours come from the theme stylesheet ([`crate::theme`]).
//!
//! Each builder's `model()` is the same component as the skin's macro draws it
//! (`skins/plain/templates/codex.html`; 19 §5.2): the fields with every class computed.
//! Until parity both exist, and `tests/codex_macros.rs` holds each macro to its builder.

use askama::Template;

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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/button.html")]
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

impl SearchModel {
    /// The built-in rendering of the model.
    #[must_use]
    pub fn render_builtin(&self) -> String {
        SearchInput {
            name: &self.name,
            value: &self.value,
            placeholder: &self.placeholder,
            label: &self.label,
            button: &self.button,
        }
        .render()
        .unwrap_or_default()
    }
}

/// A Codex search input with its button, for inside a `GET` form.
#[derive(Debug, Clone, Template)]
#[template(path = "codex/search.html")]
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/message.html")]
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/chip.html")]
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

/// A table column.
#[derive(Debug, Clone)]
pub struct Column {
    /// The heading.
    pub label: String,
    /// How its cells align.
    pub align: Align,
}

/// A table cell: HTML built from escaped parts.
#[derive(Debug, Clone, Default)]
pub struct Cell {
    /// The content.
    pub html: String,
    /// How it aligns.
    pub align: Align,
    /// A row header (`<th scope="row">`).
    pub header: bool,
    /// The language of its text, where it differs from the page's.
    pub lang: Option<String>,
}

impl Cell {
    /// A cell of HTML.
    #[must_use]
    pub fn html(html: String) -> Self {
        Self {
            html,
            ..Self::default()
        }
    }
}

/// A Codex table.
#[derive(Debug, Clone, Template)]
#[template(path = "codex/table.html")]
pub struct Table {
    /// A visible heading above the table, if any.
    pub header: Option<String>,
    /// The caption, for screen readers.
    pub caption: String,
    /// The columns.
    pub columns: Vec<Column>,
    /// The rows.
    pub rows: Vec<Vec<Cell>>,
    /// What an empty table says.
    pub empty: String,
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
    /// The cell, HTML.
    pub html: String,
    /// The alignment class.
    pub align_class: Option<String>,
    /// Whether it is a row heading.
    pub header: bool,
    /// Its language, where it differs from the page's.
    pub lang: Option<String>,
}

impl Table {
    /// The component as the skin's macro draws it.
    #[must_use]
    pub fn model(&self) -> TableModel {
        TableModel {
            header: self.header.clone(),
            caption: self.caption.clone(),
            columns: self
                .columns
                .iter()
                .map(|c| ColumnModel {
                    label: c.label.clone(),
                    align_class: c.align.class().map(str::to_string),
                })
                .collect(),
            rows: self
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| CellModel {
                            html: c.html.clone(),
                            align_class: c.align.class().map(str::to_string),
                            header: c.header,
                            lang: c.lang.clone(),
                        })
                        .collect()
                })
                .collect(),
            empty: self.empty.clone(),
        }
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/accordion.html")]
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/card.html")]
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/field.html")]
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
#[derive(Debug, Clone, Template)]
#[template(path = "codex/progress.html")]
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
            b.render().unwrap(),
            "<button type=\"submit\" class=\"cdx-button cdx-button--action-progressive \
             cdx-button--weight-primary\">Save &#60;now&#62;</button>"
        );
        assert_eq!(Button::submit("Go").classes(), "cdx-button");
    }

    #[test]
    fn messages_escape_and_announce() {
        let m = Message::block(MessageKind::Error, "a & b")
            .render()
            .unwrap();
        assert!(m.contains("class=\"cdx-message cdx-message--error\" role=\"alert\""));
        assert!(m.contains("a &#38; b"));
        let n = Message::block(MessageKind::Notice, "x").render().unwrap();
        assert!(n.contains("class=\"cdx-message\" aria-live=\"polite\""));
    }

    #[test]
    fn chips() {
        let c = Chip::new(ChipKind::Provider("WD\"x".into()), "WDQ65")
            .render()
            .unwrap();
        assert_eq!(
            c,
            "<span class=\"ts-chip ts-chip--provider ts-chip--p-wdx\">WDQ65</span>"
        );
        let l = Chip {
            title: Some("Made here"),
            ..Chip::new(ChipKind::Local, "Local")
        };
        assert!(l.render().unwrap().contains("title=\"Made here\""));
    }

    #[test]
    fn tables_escape_headings_and_keep_cell_html() {
        let t = Table {
            header: None,
            caption: "Labels <all>".into(),
            columns: vec![
                Column {
                    label: "Language".into(),
                    align: Align::Start,
                },
                Column {
                    label: "Count".into(),
                    align: Align::Number,
                },
            ],
            rows: vec![vec![
                Cell {
                    header: true,
                    lang: Some("de".into()),
                    ..Cell::html("<b>de</b>".into())
                },
                Cell {
                    align: Align::Number,
                    ..Cell::html("3".into())
                },
            ]],
            empty: "None".into(),
        }
        .render()
        .unwrap();
        assert!(t.contains("<caption>Labels &#60;all&#62;</caption>"));
        assert!(t.contains("<th scope=\"row\" lang=\"de\" dir=\"auto\"><b>de</b></th>"));
        assert!(t.contains("class=\"cdx-table__table__cell--align-number\">3</td>"));
        let empty = Table {
            rows: vec![],
            ..Table {
                header: Some("H".into()),
                caption: "c".into(),
                columns: vec![Column {
                    label: "a".into(),
                    align: Align::Start,
                }],
                rows: vec![],
                empty: "Nothing".into(),
            }
        }
        .render()
        .unwrap();
        assert!(empty.contains("colspan=\"1\">Nothing</td>"));
        assert!(empty.contains("cdx-table__header__caption"));
    }

    #[test]
    fn accordion_card_field_progress() {
        let a = Accordion {
            title: "2 references",
            description: None,
            content: "<p>x</p>",
            open: false,
        }
        .render()
        .unwrap();
        assert!(a.starts_with("<details class=\"cdx-accordion\">"));
        assert!(a.contains("<p>x</p>"));
        let c = Card {
            title: "T",
            description: Some("d"),
            supporting: None,
            href: Some("/wiki/X"),
        }
        .render()
        .unwrap();
        assert!(c.starts_with("<a class=\"cdx-card cdx-card--is-link\" href=\"/wiki/X\">"));
        assert!(c.trim_end().ends_with("</a>"));
        let f = Field {
            id: "wpName",
            name: "username",
            kind: "text",
            value: "\"",
            label: "Username",
            description: Some("Your name"),
            help: None,
            required: true,
            autocomplete: Some("username"),
        }
        .render()
        .unwrap();
        assert!(f.contains("aria-describedby=\"wpName-description\""));
        assert!(f.contains("value=\"&#34;\""));
        assert!(f.contains(" required autocomplete=\"username\">"));
        let p = ProgressBar {
            label: "Loading",
            inline: true,
        }
        .render()
        .unwrap();
        assert!(p.contains("cdx-progress-bar--inline"));
    }

    #[test]
    fn search_input_escapes_its_value() {
        let s = SearchInput {
            name: "search",
            value: "\"><script>",
            placeholder: "Search",
            label: "Search",
            button: "Search",
        }
        .render()
        .unwrap();
        assert!(s.contains("value=\"&#34;&#62;&#60;script&#62;\""));
        assert!(s.contains("cdx-search-input__end-button"));
    }
}
