//! Group 2 of the skin migration (19 §5.10): each Codex component as Plain's macro
//! (`skins/plain/templates/codex.html`) draws the same markup as the built-in builder
//! (`triplespace_ui::codex`) over the same inputs, compared as a browser reads them
//! (0097 §16, A3), and byte for byte but for entity spelling.

use askama::Template as _;
use triplespace_ui::codex::{
    Accordion, Action, Align, Button, Card, Cell, Chip, ChipKind, Column, Field, Message,
    MessageKind, ProgressBar, SearchInput, Table, Weight,
};
use triplespace_ui::i18n::Messages;
use triplespace_ui::parity::{Kind, compare};
use triplespace_ui::skin::Skins;

/// The built-in result with its entities spelled as `html::esc` spells them.
fn spelled(html: &str) -> String {
    html.replace("&#38;", "&amp;")
        .replace("&#60;", "&lt;")
        .replace("&#62;", "&gt;")
        .replace("&#34;", "&quot;")
}

fn holds(name: &str, macro_call: &str, model: impl serde::Serialize, builtin: &str) {
    let skins = Skins::load(None);
    let plain = skins.get("plain").expect("plain");
    let m = Messages::for_language("en");
    let source = format!("{{% import \"codex.html\" as codex %}}{{{{ codex.{macro_call} }}}}");
    let ours = plain
        .render_str(&source, &m, serde_json::json!({ "x": model }))
        .unwrap_or_else(|e| panic!("{name}: {e:#}"));
    assert_eq!(
        compare(builtin, &ours, Kind::Fragment),
        None,
        "{name}:\n--- builtin\n{builtin}\n--- macro\n{ours}"
    );
    assert_eq!(ours, spelled(builtin), "{name}: byte for byte");
}

#[test]
fn buttons() {
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
        holds("button", "button(x)", b.model(), &b.render().unwrap());
    }
}

#[test]
fn search_inputs() {
    let s = SearchInput {
        name: "search",
        value: "\"><script>",
        placeholder: "Search items",
        label: "Search",
        button: "Go",
    };
    holds("search", "search(x)", s.model(), &s.render().unwrap());
}

#[test]
fn messages() {
    for kind in [
        MessageKind::Notice,
        MessageKind::Warning,
        MessageKind::Error,
        MessageKind::Success,
    ] {
        let m = Message::block(kind, "a & b <c>");
        holds("message", "message(x)", m.model(), &m.render().unwrap());
        let inline = Message {
            inline: true,
            ..Message::block(kind, "x")
        };
        holds(
            "message",
            "message(x)",
            inline.model(),
            &inline.render().unwrap(),
        );
    }
}

#[test]
fn chips() {
    for c in [
        Chip::new(ChipKind::Provider("WD\"x".into()), "WDQ65"),
        Chip {
            title: Some("Made <here>"),
            ..Chip::new(ChipKind::Local, "Local")
        },
        Chip::new(ChipKind::Best, "Best"),
    ] {
        holds("chip", "chip(x)", c.model(), &c.render().unwrap());
    }
}

#[test]
fn tables() {
    let t = Table {
        header: Some("Header <h>".into()),
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
            Column {
                label: "C".into(),
                align: Align::Center,
            },
        ],
        rows: vec![
            vec![
                Cell {
                    header: true,
                    lang: Some("de".into()),
                    ..Cell::html("<b>de</b>".into())
                },
                Cell {
                    align: Align::Number,
                    ..Cell::html("3".into())
                },
                Cell {
                    align: Align::End,
                    ..Cell::html("e".into())
                },
            ],
            vec![
                Cell::html("x".into()),
                Cell::html("y".into()),
                Cell::html("z".into()),
            ],
        ],
        empty: "None".into(),
    };
    holds("table", "table(x)", t.model(), &t.render().unwrap());
    let empty = Table {
        header: None,
        rows: vec![],
        ..t
    };
    holds("table", "table(x)", empty.model(), &empty.render().unwrap());
}

#[test]
fn accordions_cards_fields_and_progress() {
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
        holds("accordion", "accordion(x)", a.model(), &a.render().unwrap());
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
        holds("card", "card(x)", c.model(), &c.render().unwrap());
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
        holds("field", "field(x)", f.model(), &f.render().unwrap());
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
        holds("progress", "progress(x)", p.model(), &p.render().unwrap());
    }
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
