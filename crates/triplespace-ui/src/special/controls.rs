//! Form controls drawn from a value's type (0085 §4, §5), shared by `Special:WikiSettings`,
//! `Special:InstanceSettings` and `Special:Preferences`, and the way back: a posted field
//! read as a JSON value of that type.
//!
//! Every control is Codex's CSS-only markup and works without JavaScript. Lists are a
//! text area with one item a line; maps and JSON a text area holding JSON. A field left
//! empty means "no change" for every type but a list, where it is the empty list: the
//! row's **Reset** is how a value returns to inheriting.
//!
//! The drawing is Plain's `forms.html` (`control`, over [`Control::model`]); [`render`]
//! draws the same markup for the pages not yet switched, and a test holds the two equal
//! (19 §5.10). Parsing stays here.

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::Value;

use crate::html::esc;
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
    /// An inline checkbox.
    Checkbox {
        /// The element ID.
        id: String,
        /// The field's name.
        name: String,
        /// Whether it is on.
        checked: bool,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A select.
    Select {
        /// The element ID.
        id: String,
        /// The field's name.
        name: String,
        /// The options, "no value" first where the value may be absent.
        options: Vec<OptionModel>,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A one-line input: `number` or `text`.
    Input {
        /// The element ID.
        id: String,
        /// The field's name.
        name: String,
        /// `number` or `text`.
        input_type: String,
        /// The value as text.
        value: String,
        /// The element describing it.
        described_by: Option<String>,
    },
    /// A text area.
    TextArea {
        /// The element ID.
        id: String,
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
        let id = self.id.to_string();
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
                checked: self.value == Some(&Value::Bool(true)),
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
            Kind::ReadOnly => ControlModel::ReadOnly { id, value: text },
        }
    }
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
    /// The help's element ID.
    pub help_id: String,
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
    /// A note under the value.
    pub note: Option<String>,
}

/// The control's markup; the label is the caller's, pointing at `id`.
#[must_use]
pub fn render(c: &Control<'_>) -> String {
    let id = esc(c.id);
    let name = esc(c.name);
    let described = c
        .described_by
        .map(|d| format!(" aria-describedby=\"{}\"", esc(d)))
        .unwrap_or_default();
    let text = c.value.map(|v| as_text(c.kind, v)).unwrap_or_default();
    match c.kind {
        Kind::Boolean => format!(
            "<span class=\"cdx-checkbox cdx-checkbox--inline\"><input class=\"cdx-checkbox__input\" type=\"checkbox\" id=\"{id}\" name=\"{name}\" value=\"1\"{}{described}><span class=\"cdx-checkbox__icon\"></span></span>",
            if c.value == Some(&Value::Bool(true)) {
                " checked"
            } else {
                ""
            }
        ),
        Kind::Choice => {
            let current = c.value.and_then(Value::as_str);
            let mut s =
                format!("<select class=\"cdx-select\" id=\"{id}\" name=\"{name}\"{described}>");
            if let Some(empty) = c.empty_choice {
                let _ = write!(
                    s,
                    "<option value=\"\"{}>{}</option>",
                    if current.is_none() { " selected" } else { "" },
                    esc(empty)
                );
            }
            for choice in c.choices {
                let label = c.choice_label.map_or_else(|| choice.clone(), |f| f(choice));
                let _ = write!(
                    s,
                    "<option value=\"{}\"{}>{}</option>",
                    esc(choice),
                    if current == Some(choice.as_str()) {
                        " selected"
                    } else {
                        ""
                    },
                    esc(&label)
                );
            }
            s.push_str("</select>");
            s
        }
        Kind::Integer => format!(
            "<div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"number\" step=\"1\" id=\"{id}\" name=\"{name}\" value=\"{}\"{described}></div>",
            esc(&text)
        ),
        Kind::Line => format!(
            "<div class=\"cdx-text-input\"><input class=\"cdx-text-input__input\" type=\"text\" id=\"{id}\" name=\"{name}\" value=\"{}\"{described}></div>",
            esc(&text)
        ),
        Kind::Text | Kind::Lines | Kind::IntegerLines | Kind::Namespaces | Kind::Json => {
            let rows = text.lines().count().clamp(2, 12);
            format!(
                "<div class=\"cdx-text-area\"><textarea class=\"cdx-text-area__textarea\" id=\"{id}\" name=\"{name}\" rows=\"{rows}\"{}{described}>{}</textarea></div>",
                if c.kind == Kind::Json {
                    " spellcheck=\"false\""
                } else {
                    ""
                },
                esc(&text)
            )
        }
        Kind::ReadOnly => format!("<span id=\"{id}\">{}</span>", esc(&text)),
    }
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
    fn controls_escape_and_select() {
        let m = Messages::for_language("en");
        let choices = vec!["off".to_string(), "on".to_string()];
        let html = render(&Control {
            id: "x",
            name: "v.a\"b",
            kind: Kind::Choice,
            choices: &choices,
            value: Some(&json!("on")),
            empty_choice: None,
            described_by: Some("x-help"),
            choice_label: None,
        });
        assert!(html.contains("name=\"v.a&quot;b\""));
        assert!(html.contains("<option value=\"on\" selected>on</option>"));
        assert!(html.contains("aria-describedby=\"x-help\""));
        let html = render(&Control {
            id: "y",
            name: "v.y",
            kind: Kind::Text,
            choices: &[],
            value: Some(&json!("</textarea><script>")),
            empty_choice: None,
            described_by: None,
            choice_label: None,
        });
        assert!(html.contains("&lt;/textarea&gt;&lt;script&gt;"));
        assert_eq!(
            shown(&m, Kind::Boolean, Some(&json!(true))),
            m.get("ts-settings-on")
        );
    }
}
