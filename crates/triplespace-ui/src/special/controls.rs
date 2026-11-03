//! Form controls drawn from a value's type (0085 §4, §5), shared by `Special:WikiSettings`,
//! `Special:InstanceSettings`, `Special:Preferences` and the moderation forms, and the way back: a posted field
//! read as a JSON value of that type.
//!
//! Every control is Codex's CSS-only markup and works without JavaScript. Lists are a
//! text area with one item a line; maps and JSON a text area holding JSON. A field left
//! empty means "no change" for every type but a list, where it is the empty list: the
//! row's **Reset** is how a value returns to inheriting.
//!
//! The drawing is Plain's `forms.html` (`control`, over [`Control::model`], and the
//! layouts that place controls; 19 §5.10, 0097 A8–A10). Parsing stays here.

use serde::Serialize;
use serde_json::Value;

use crate::i18n::Messages;

/// How a type is edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A checkbox.
    Boolean,
    /// A select of the choices.
    Choice,
    /// A number input.
    Integer,
    /// A one-line text input.
    Line,
    /// A text area of free text.
    Text,
    /// A text area, one string a line.
    Lines,
    /// A text area, one integer a line.
    IntegerLines,
    /// A text area, one namespace (number or name) a line.
    Namespaces,
    /// A text area holding JSON.
    Json,
    /// Shown, not edited here: a site asset (0085 §8).
    ReadOnly,
}

impl Kind {
    /// The kind a registry type is edited as.
    #[must_use]
    pub fn of(ty: &str) -> Self {
        match ty {
            "boolean" => Self::Boolean,
            "choice" => Self::Choice,
            "integer" | "size" => Self::Integer,
            "text" => Self::Text,
            "language-list" | "string-list" | "page-list" | "repository-list" | "property-list" => {
                Self::Lines
            }
            "integer-list" => Self::IntegerLines,
            "namespace-list" => Self::Namespaces,
            "map" | "json" => Self::Json,
            "site-asset" => Self::ReadOnly,
            _ => Self::Line,
        }
    }
}

/// A value as text: for a field's contents, or to show it read-only.
#[must_use]
pub fn as_text(kind: Kind, v: &Value) -> String {
    match (kind, v) {
        (_, Value::String(s)) => s.clone(),
        (Kind::Lines | Kind::IntegerLines | Kind::Namespaces, Value::Array(items)) => items
            .iter()
            .map(|i| match i {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        (Kind::Json, v) => serde_json::to_string_pretty(v).unwrap_or_default(),
        (Kind::ReadOnly, v) => match (
            v.get("title").and_then(Value::as_str),
            v.get("sha1").and_then(Value::as_str),
        ) {
            (Some(t), Some(h)) => format!("{t} ({})", &h[..h.len().min(8)]),
            _ => v.to_string(),
        },
        (_, v) => v.to_string(),
    }
}

/// A value shown to a reader: on and off in words, lists joined, an absent value as
/// `none`.
#[must_use]
pub fn shown(m: &Messages, kind: Kind, v: Option<&Value>) -> String {
    match (kind, v) {
        (_, None | Some(Value::Null)) => m.get("ts-settings-none"),
        (_, Some(Value::Bool(true))) => m.get("ts-settings-on"),
        (_, Some(Value::Bool(false))) => m.get("ts-settings-off"),
        (Kind::Lines | Kind::IntegerLines | Kind::Namespaces, Some(Value::Array(items)))
            if items.is_empty() =>
        {
            m.get("ts-settings-empty-list")
        }
        (Kind::Lines | Kind::IntegerLines | Kind::Namespaces, Some(v)) => {
            as_text(kind, v).replace('\n', ", ")
        }
        (Kind::Json, Some(v)) => v.to_string(),
        (_, Some(v)) => as_text(kind, v),
    }
}

/// What a control needs.
#[derive(Clone, Copy)]
pub struct Control<'a> {
    /// The element ID, unique on the page.
    pub id: &'a str,
    /// The form field's name.
    pub name: &'a str,
    /// How it is edited.
    pub kind: Kind,
    /// The choices, for a select.
    pub choices: &'a [String],
    /// The current value, or the text the reader typed when the form comes back with
    /// errors.
    pub value: Option<&'a Value>,
    /// What a `choice` select offers for "no value", where the value may be absent.
    pub empty_choice: Option<&'a str>,
    /// The ID of the element describing the control (its help), if any.
    pub described_by: Option<&'a str>,
    /// A label for a choice value, where the page has messages for them.
    pub choice_label: Option<&'a dyn Fn(&str) -> String>,
}

/// One option of a select.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OptionModel {
    /// The value posted.
    pub value: String,
    /// The text shown.
    pub label: String,
    /// Whether it is the current value.
    pub selected: bool,
}

/// A control as Plain's `forms.html` draws it (19 §5.10): tagged by how it is drawn, with
/// the fields each needs. The label, the help and any error are the page's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ControlModel {
    /// An inline checkbox: bare, the row labelling it, or carrying its own label.
    Checkbox {
        /// The element ID, where the page refers to it.
        id: Option<String>,
        /// The field's name.
        name: String,
        /// The value posted when it is on.
        value: String,
        /// Whether it is on.
        checked: bool,
        /// Its own label, after the box: a `<label>` pointing at the ID, or wrapping
        /// the box where it has none.
        label: Option<String>,
        /// A note at the end of its label: the expiry of a group membership.
        note: Option<String>,
        /// Whether the viewer may not change it: shown, not hidden.
        disabled: bool,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A radio button with its label, drawn as a labelled checkbox is.
    Radio {
        /// The element ID; without one the label wraps the button.
        id: Option<String>,
        /// The field's name, shared by its group.
        name: String,
        /// The value posted when it is chosen.
        value: String,
        /// Whether it is chosen.
        checked: bool,
        /// Its label, after the button.
        label: String,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A select.
    Select {
        /// The element ID, where the page refers to it.
        id: Option<String>,
        /// The field's name.
        name: String,
        /// The options, "no value" first where the value may be absent.
        options: Vec<OptionModel>,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A one-line input: `number` or `text`.
    Input {
        /// The element ID, where the page refers to it.
        id: Option<String>,
        /// The field's name.
        name: String,
        /// `number` or `text`.
        input_type: String,
        /// The value as text.
        value: String,
        /// Whether the form needs it.
        required: bool,
        /// The keyboard a phone offers (`numeric` for a revision ID).
        inputmode: Option<String>,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A text area.
    TextArea {
        /// The element ID, where the page refers to it.
        id: Option<String>,
        /// The field's name.
        name: String,
        /// The value as text.
        value: String,
        /// Its height: the value's lines, from 2 to 12.
        rows: usize,
        /// Whether spellchecking is off (JSON).
        no_spellcheck: bool,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A value shown, not edited.
    ReadOnly {
        /// The element ID.
        id: String,
        /// The value as text.
        value: String,
    },
}

impl Control<'_> {
    /// The control as Plain's `forms.html` draws it.
    #[must_use]
    pub fn model(&self) -> ControlModel {
        let id = Some(self.id.to_string());
        let name = self.name.to_string();
        let described_by = self.described_by.map(str::to_string);
        let text = self
            .value
            .map(|v| as_text(self.kind, v))
            .unwrap_or_default();
        match self.kind {
            Kind::Boolean => ControlModel::Checkbox {
                id,
                name,
                value: "1".into(),
                checked: self.value == Some(&Value::Bool(true)),
                label: None,
                note: None,
                disabled: false,
                described_by,
            },
            Kind::Choice => {
                let current = self.value.and_then(Value::as_str);
                let mut options = Vec::new();
                if let Some(empty) = self.empty_choice {
                    options.push(OptionModel {
                        value: String::new(),
                        label: empty.to_string(),
                        selected: current.is_none(),
                    });
                }
                for choice in self.choices {
                    options.push(OptionModel {
                        value: choice.clone(),
                        label: self
                            .choice_label
                            .map_or_else(|| choice.clone(), |f| f(choice)),
                        selected: current == Some(choice.as_str()),
                    });
                }
                ControlModel::Select {
                    id,
                    name,
                    options,
                    described_by,
                }
            }
            Kind::Integer | Kind::Line => ControlModel::Input {
                id,
                name,
                input_type: if self.kind == Kind::Integer {
                    "number"
                } else {
                    "text"
                }
                .into(),
                value: text,
                required: false,
                inputmode: None,
                described_by,
            },
            Kind::Text | Kind::Lines | Kind::IntegerLines | Kind::Namespaces | Kind::Json => {
                ControlModel::TextArea {
                    id,
                    name,
                    rows: text.lines().count().clamp(2, 12),
                    value: text,
                    no_spellcheck: self.kind == Kind::Json,
                    described_by,
                }
            }
            Kind::ReadOnly => ControlModel::ReadOnly {
                id: self.id.to_string(),
                value: text,
            },
        }
    }
}

/// One item of a form that works by `GET`, as Plain's `forms.html` lays it out (`item`;
/// 0097 A8): a control under its label, a checkbox carrying its own label, or a fieldset
/// of items. The controls are [`ControlModel`]s, the same as a settings row's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FormItem {
    /// A control with its label above it, the label wrapping it.
    Field {
        /// The label.
        label: String,
        /// The control.
        control: ControlModel,
    },
    /// A checkbox with its label after it.
    Check {
        /// The checkbox.
        control: ControlModel,
    },
    /// A fieldset of items under a legend.
    Group {
        /// The legend.
        legend: String,
        /// The items.
        items: Vec<FormItem>,
    },
}

impl FormItem {
    /// A one-line text field.
    #[must_use]
    pub fn text(label: String, name: &str, value: &str) -> Self {
        Self::Field {
            label,
            control: ControlModel::Input {
                id: None,
                name: name.to_string(),
                input_type: "text".into(),
                value: value.to_string(),
                required: false,
                inputmode: None,
                described_by: None,
            },
        }
    }

    /// A one-line field for a number typed as text: a revision ID.
    #[must_use]
    pub fn numeric(label: String, name: &str, value: &str) -> Self {
        Self::Field {
            label,
            control: ControlModel::Input {
                id: None,
                name: name.to_string(),
                input_type: "text".into(),
                value: value.to_string(),
                required: false,
                inputmode: Some("numeric".into()),
                described_by: None,
            },
        }
    }

    /// A select of `(value, text, selected)` options.
    #[must_use]
    pub fn select(label: String, name: &str, options: Vec<(String, String, bool)>) -> Self {
        Self::Field {
            label,
            control: ControlModel::Select {
                id: None,
                name: name.to_string(),
                options: options
                    .into_iter()
                    .map(|(value, label, selected)| OptionModel {
                        value,
                        label,
                        selected,
                    })
                    .collect(),
                described_by: None,
            },
        }
    }

    /// An inline checkbox with its label.
    #[must_use]
    pub fn check(name: &str, value: &str, checked: bool, label: String) -> Self {
        Self::Check {
            control: ControlModel::Checkbox {
                id: None,
                name: name.to_string(),
                value: value.to_string(),
                checked,
                label: Some(label),
                note: None,
                disabled: false,
                described_by: None,
            },
        }
    }

    /// A fieldset of items.
    #[must_use]
    pub fn group(legend: String, items: Vec<FormItem>) -> Self {
        Self::Group { legend, items }
    }

    /// The field's name, for a test; `None` for a group.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        let control = match self {
            Self::Field { control, .. } | Self::Check { control } => control,
            Self::Group { .. } => return None,
        };
        match control {
            ControlModel::Checkbox { name, .. }
            | ControlModel::Radio { name, .. }
            | ControlModel::Select { name, .. }
            | ControlModel::Input { name, .. }
            | ControlModel::TextArea { name, .. } => Some(name),
            ControlModel::ReadOnly { .. } => None,
        }
    }
}

impl ControlModel {
    /// An inline checkbox with its own label, pointing at its ID.
    #[must_use]
    pub fn check(id: &str, name: &str, checked: bool, label: String) -> Self {
        Self::Checkbox {
            id: Some(id.to_string()),
            name: name.to_string(),
            value: "1".into(),
            checked,
            label: Some(label),
            note: None,
            disabled: false,
            described_by: None,
        }
    }

    /// A one-line text input.
    #[must_use]
    pub fn text(id: &str, name: &str, value: &str, required: bool) -> Self {
        Self::Input {
            id: Some(id.to_string()),
            name: name.to_string(),
            input_type: "text".into(),
            value: value.to_string(),
            required,
            inputmode: None,
            described_by: None,
        }
    }
}

/// A control under its label with its help, as Codex lays out a field (`forms.html`'s
/// `field`): the layout of a form that works by `POST`, where `item` is one by `GET`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldRow {
    /// The control's element ID, which the label points at.
    pub id: String,
    /// The label.
    pub label: String,
    /// A class beside `cdx-field`.
    pub class: Option<String>,
    /// The control.
    pub control: ControlModel,
    /// The help under it.
    pub help: Option<String>,
    /// The help's element ID, where the control points at it.
    pub help_id: Option<String>,
}

impl FieldRow {
    /// A text field with no help.
    #[must_use]
    pub fn text(id: &str, name: &str, label: String, value: &str, required: bool) -> Self {
        Self {
            id: id.to_string(),
            label,
            class: None,
            control: ControlModel::text(id, name, value, required),
            help: None,
            help_id: None,
        }
    }
}

/// A checkbox with its help under it (`forms.html`'s `check`): an option of a form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckRow {
    /// The class of the element around it.
    pub class: String,
    /// The checkbox, carrying its own label.
    pub control: ControlModel,
    /// The help.
    pub help: String,
}

/// A form that asks by `GET` for the page to go to (`forms.html`'s `ask`): a hidden title,
/// one field and its button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AskForm {
    /// The form's class.
    pub class: String,
    /// The special page it goes to.
    pub title: String,
    /// The field.
    pub item: FormItem,
    /// The button.
    pub button: crate::codex::ButtonModel,
}

/// A **Reset** button: submits its form asking to reset one key only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResetModel {
    /// The key it resets.
    pub value: String,
    /// Its accessible name: "Reset Interface language".
    pub aria_label: String,
    /// **Reset**.
    pub label: String,
}

/// One row of a settings or preferences form as Plain's `forms.html` draws it (`setting`):
/// the label and key, the help, the row's errors, the control or the value shown, a
/// **Reset** where the value is the reader's own, and a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SettingRow {
    /// The row's element ID: `pref-ui.mode`.
    pub row_id: String,
    /// The key.
    pub key: String,
    /// The label.
    pub label: String,
    /// The control the label is for; `None` draws the label as text.
    pub label_for: Option<String>,
    /// The origin chips after the label (the settings pages').
    pub chips: Vec<crate::codex::ChipModel>,
    /// The help's element ID, where a control points at it.
    pub help_id: Option<String>,
    /// The help.
    pub help: Option<String>,
    /// What the form refused about this row.
    pub errors: Vec<crate::codex::MessageModel>,
    /// The control.
    pub control: Option<ControlModel>,
    /// The value shown, where it is not edited here.
    pub shown: Option<String>,
    /// **Reset**.
    pub reset: Option<ResetModel>,
    /// A checkbox after the control: **Lock** on the instance's defaults for wikis.
    pub lock: Option<ControlModel>,
    /// A chip after the value shown: a default that is locked.
    pub shown_chip: Option<crate::codex::ChipModel>,
    /// A note under the value.
    pub note: Option<String>,
}

/// A posted field read as a value of the kind: `Ok(None)` for "no change" (an empty
/// field, or a read-only one), `Err` with why it is not a value of the type. A checkbox
/// that was on the page but is absent from the form is `false`; the caller knows which
/// were on the page.
pub fn parse(kind: Kind, raw: Option<&str>) -> Result<Option<Value>, String> {
    let raw = raw.map(|s| s.replace("\r\n", "\n"));
    let trimmed = raw.as_deref().map(str::trim).unwrap_or_default();
    match kind {
        Kind::Boolean => Ok(Some(Value::Bool(raw.is_some()))),
        Kind::ReadOnly => Ok(None),
        Kind::Lines | Kind::IntegerLines | Kind::Namespaces if raw.is_some() => {
            let items: Vec<&str> = trimmed
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect();
            let values: Result<Vec<Value>, String> = items
                .iter()
                .map(|i| match kind {
                    Kind::IntegerLines => i
                        .parse::<i64>()
                        .map(Value::from)
                        .map_err(|_| format!("`{i}` is not a whole number")),
                    Kind::Namespaces => Ok(i
                        .parse::<i64>()
                        .map_or_else(|_| Value::String((*i).to_string()), Value::from)),
                    _ => Ok(Value::String((*i).to_string())),
                })
                .collect();
            values.map(|v| Some(Value::Array(v)))
        }
        _ if trimmed.is_empty() => Ok(None),
        Kind::Integer => trimmed
            .parse::<i64>()
            .map(|n| Some(Value::from(n)))
            .map_err(|_| format!("`{trimmed}` is not a whole number")),
        Kind::Json => serde_json::from_str(trimmed)
            .map(Some)
            .map_err(|e| format!("not JSON: {e}")),
        Kind::Text => Ok(raw.map(Value::String)),
        _ => Ok(Some(Value::String(trimmed.to_string()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn kinds() {
        assert_eq!(Kind::of("boolean"), Kind::Boolean);
        assert_eq!(Kind::of("size"), Kind::Integer);
        assert_eq!(Kind::of("language-list"), Kind::Lines);
        assert_eq!(Kind::of("duration"), Kind::Line);
        assert_eq!(Kind::of("map"), Kind::Json);
        assert_eq!(Kind::of("site-asset"), Kind::ReadOnly);
    }

    #[test]
    fn round_trips() {
        assert_eq!(parse(Kind::Boolean, Some("1")), Ok(Some(json!(true))));
        assert_eq!(parse(Kind::Boolean, None), Ok(Some(json!(false))));
        assert_eq!(parse(Kind::Integer, Some(" 20 ")), Ok(Some(json!(20))));
        assert!(parse(Kind::Integer, Some("twenty")).is_err());
        assert_eq!(parse(Kind::Integer, Some("")), Ok(None));
        assert_eq!(parse(Kind::Line, Some("P30D")), Ok(Some(json!("P30D"))));
        assert_eq!(
            parse(Kind::Lines, Some("en\r\n de\n\n")),
            Ok(Some(json!(["en", "de"])))
        );
        assert_eq!(parse(Kind::Lines, Some("")), Ok(Some(json!([]))));
        assert_eq!(
            parse(Kind::Namespaces, Some("0\nProject")),
            Ok(Some(json!([0, "Project"])))
        );
        assert_eq!(
            parse(Kind::Json, Some("{\"a\": 1}")),
            Ok(Some(json!({"a": 1})))
        );
        assert!(parse(Kind::Json, Some("{")).is_err());
        assert_eq!(parse(Kind::ReadOnly, Some("x")), Ok(None));
        assert_eq!(as_text(Kind::Lines, &json!(["en", "de"])), "en\nde");
        assert_eq!(as_text(Kind::Integer, &json!(20)), "20");
    }

    #[test]
    fn a_value_shown_reads_as_words() {
        let m = Messages::for_language("en");
        assert_eq!(
            shown(&m, Kind::Boolean, Some(&json!(true))),
            m.get("ts-settings-on")
        );
    }
}
