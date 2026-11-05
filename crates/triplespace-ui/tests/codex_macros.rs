//! Plain's macros (19 §5.2, §5.10) as snapshots: each Codex component as
//! `skins/plain/templates/codex.html` draws it, the watch control and the form controls.
//! Until the built-in renderer was removed (2026-11-03; 0097 A12) each macro was held byte
//! for byte to the builder it replaced, entity spelling aside; the snapshots hold the
//! markup they drew then, the components' DOM contract of Plain 0.x.

use triplespace_ui::codex::{
    Accordion, Action, Align, Button, Card, CellModel, Chip, ChipKind, ColumnModel, Field, Message,
    MessageKind, ProgressBar, SearchInput, TableModel, Weight,
};
use triplespace_ui::i18n::Messages;
use triplespace_ui::inline::{Inline, text};
use triplespace_ui::skin::Skins;

/// A model as a template value.
fn json(model: impl serde::Serialize) -> serde_json::Value {
    serde_json::to_value(model).expect("a model serializes")
}

/// Each component drawn by its macro, one after another, each under its call.
#[test]
#[allow(clippy::too_many_lines)]
fn components() {
    let skins = Skins::load(None);
    let plain = skins.get("plain").expect("plain");
    let m = Messages::for_language("en");
    let mut out = String::new();
    let mut draw = |call: &str, model: serde_json::Value| {
        let source = format!("{{% import \"codex.html\" as codex %}}{{{{ codex.{call} }}}}");
        let html = plain
            .render_str(&source, &m, serde_json::json!({ "x": model }))
            .unwrap_or_else(|e| panic!("{call}: {e:#}"));
        out.push_str(call);
        out.push('\n');
        out.push_str(&html);
        out.push_str("\n\n");
    };
    for b in [
        Button::submit("Save <now>"),
        Button {
            kind: "button",
            action: Action::Progressive,
            weight: Weight::Primary,
            ..Button::submit("Go")
        },
        Button {
            action: Action::Destructive,
            weight: Weight::Quiet,
            ..Button::submit("Delete & forget")
        },
    ] {
        draw("button(x)", json(b.model()));
    }
    draw(
        "search(x)",
        json(
            SearchInput {
                name: "search",
                value: "\"><script>",
                placeholder: "Search items",
                label: "Search",
                button: "Go",
            }
            .model(),
        ),
    );
    for kind in [
        MessageKind::Notice,
        MessageKind::Warning,
        MessageKind::Error,
        MessageKind::Success,
    ] {
        draw(
            "message(x)",
            json(Message::block(kind, "a & b <c>").model()),
        );
        draw(
            "message(x)",
            json(
                Message {
                    inline: true,
                    ..Message::block(kind, "x")
                }
                .model(),
            ),
        );
    }
    for c in [
        Chip::new(ChipKind::Provider("WD\"x".into()), "WDQ65"),
        Chip {
            title: Some("Made <here>"),
            ..Chip::new(ChipKind::Local, "Local")
        },
        Chip::new(ChipKind::Best, "Best"),
    ] {
        draw("chip(x)", json(c.model()));
    }
    let table = TableModel {
        header: Some("Header <h>".into()),
        caption: "Labels <all>".into(),
        columns: vec![
            ColumnModel::new("Language".into(), Align::Start),
            ColumnModel::new("Count".into(), Align::Number),
            ColumnModel::new("C".into(), Align::Center),
        ],
        rows: vec![
            vec![
                CellModel {
                    header: true,
                    lang: Some("de".into()),
                    ..CellModel::of(vec![
                        Inline::code("de"),
                        Inline::text(" & "),
                        Inline::link("/wiki/X", "x"),
                    ])
                },
                CellModel {
                    align_class: Align::Number.class().map(str::to_string),
                    ..CellModel::of(text("3"))
                },
                CellModel {
                    align_class: Align::End.class().map(str::to_string),
                    ..CellModel::of(text("e"))
                },
            ],
            vec![
                CellModel::text("x"),
                CellModel::text("y"),
                CellModel::text("z"),
            ],
        ],
        empty: "None".into(),
    };
    draw("table(x)", json(&table));
    draw(
        "table(x)",
        json(TableModel {
            header: None,
            rows: vec![],
            ..table
        }),
    );
    for a in [
        Accordion {
            title: "2 references",
            description: None,
            content: "<p>x & y</p>",
            open: false,
        },
        Accordion {
            title: "T",
            description: Some("d <e>"),
            content: "",
            open: true,
        },
    ] {
        draw("accordion(x)", json(a.model()));
    }
    for c in [
        Card {
            title: "T",
            description: Some("d"),
            supporting: None,
            href: Some("/wiki/X?a=1&b=2"),
        },
        Card {
            title: "T <t>",
            description: None,
            supporting: Some("s"),
            href: None,
        },
    ] {
        draw("card(x)", json(c.model()));
    }
    for f in [
        Field {
            id: "wpName",
            name: "username",
            kind: "text",
            value: "\"",
            label: "Username",
            description: Some("Your name"),
            help: None,
            required: true,
            autocomplete: Some("username"),
        },
        Field {
            id: "wpPassword",
            name: "password",
            kind: "password",
            value: "",
            label: "Password",
            description: None,
            help: Some("At least 8 <characters>"),
            required: false,
            autocomplete: None,
        },
    ] {
        draw("field(x)", json(f.model()));
    }
    for p in [
        ProgressBar {
            label: "Loading",
            inline: true,
        },
        ProgressBar {
            label: "L <l>",
            inline: false,
        },
    ] {
        draw("progress(x)", json(p.model()));
    }
    insta::assert_snapshot!("components", out);
}

/// The watch control's template (`watch.html`), as a snapshot: it was held byte for byte
/// to the built-in builder it replaced when group 1 switched (2026-10-30).
#[test]
fn watch_control() {
    use triplespace_ui::frame::{WatchControl, watch_model};
    let skins = Skins::load(None);
    let plain = skins.get("plain").expect("plain");
    let m = Messages::for_language("en");
    let mut out = String::new();
    for w in [
        WatchControl {
            title: "Item:Q6".into(),
            watched: false,
            expires: None,
            notify: false,
            token: "c+\\".into(),
        },
        WatchControl {
            title: "Talk:A & B".into(),
            watched: true,
            expires: Some("2026-11-06T10:00:00Z".into()),
            notify: true,
            token: "t".into(),
        },
        WatchControl {
            title: "Item:Q6".into(),
            watched: true,
            expires: None,
            notify: false,
            token: "t".into(),
        },
    ] {
        let ours = plain
            .render_str(
                "{% import \"watch.html\" as watch %}{{ watch.control(x) }}",
                &m,
                serde_json::json!({ "x": watch_model(&m, &w) }),
            )
            .unwrap_or_else(|e| panic!("{e:#}"));
        out.push_str(&ours.replace("><", ">\n<"));
        out.push_str("\n\n");
    }
    insta::assert_snapshot!("watch_control", out);
}

/// Group 9a (19 §5.10): each form control as `forms.html`'s `control` draws it, in every
/// kind, with and without help, with no value, with the reader's own text coming back after
/// an error, and escaped. Until group 11 switched this compared the macro with the built-in
/// `special::controls::render`, byte for byte in all 480 combinations; with that builder
/// deleted, the snapshot holds the markup it drew.
#[test]
fn form_controls() {
    use serde_json::{Value, json};
    use triplespace_ui::special::controls::{Control, Kind as ControlKind};

    let skins = Skins::load(None);
    let plain = skins.get("plain").expect("plain");
    let m = Messages::for_language("en");
    let choices = vec!["light".to_string(), "dark & \"deep\"".to_string()];
    let label = |c: &str| format!("Mode: {c}");
    let values: [Option<Value>; 6] = [
        None,
        Some(json!(true)),
        Some(json!("dark & \"deep\"")),
        Some(json!(20)),
        // The reader's text, back after the form refused it.
        Some(json!("twenty <b>")),
        Some(json!(["en", "de", "</textarea><script>"])),
    ];
    let kinds = [
        ControlKind::Boolean,
        ControlKind::Choice,
        ControlKind::Integer,
        ControlKind::Line,
        ControlKind::Text,
        ControlKind::Lines,
        ControlKind::IntegerLines,
        ControlKind::Namespaces,
        ControlKind::Json,
        ControlKind::ReadOnly,
    ];
    let mut checked = 0;
    let mut out = String::new();
    for kind in kinds {
        for value in &values {
            for described_by in [None, Some("x-help")] {
                for empty_choice in [None, Some("(inherit)")] {
                    for labelled in [false, true] {
                        let c = Control {
                            id: "x\"id",
                            name: "v.a&b",
                            kind,
                            choices: &choices,
                            value: value.as_ref(),
                            empty_choice,
                            described_by,
                            choice_label: labelled.then_some(&label as &dyn Fn(&str) -> String),
                        };
                        let ours = plain
                            .render_str(
                                "{% import \"forms.html\" as forms %}{{ forms.control(x) }}",
                                &m,
                                json!({ "x": c.model() }),
                            )
                            .unwrap_or_else(|e| panic!("{kind:?}: {e:#}"));
                        let what = format!(
                            "{kind:?} {value:?} {described_by:?} {empty_choice:?} {labelled}"
                        );
                        out.push_str(&what);
                        out.push('\n');
                        out.push_str(&ours);
                        out.push_str("\n\n");
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 10 * 6 * 2 * 2 * 2);
    insta::assert_snapshot!("form_controls", out);
}
