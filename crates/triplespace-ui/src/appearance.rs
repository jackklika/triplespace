//! The Appearance menu (0085 §6; 19 §2.7) and the reader's appearance on `<html>`
//! (0086 §4; 19 §5.9).
//!
//! Every page's header has an **Appearance** disclosure, for every viewer, holding the
//! preference keys of the `appearance` group. Signed in, its values are the account's,
//! the server writes them on `<html>`, and the panel is a form the web tier saves
//! without JavaScript. Not logged in, the page is public and carries no choice: the head
//! script applies what the reader keeps in `localStorage` before the first paint, and
//! without JavaScript the panel says the page follows the device.
//!
//! The panel offers colour mode, contrast and text size as groups of icon buttons, one
//! of which is chosen (radio buttons, so the form works without JavaScript); the
//! colour-vision alternate (0094 §1); and, as selects, the palettes and type sets the
//! wiki lets readers choose (0086 §4), the wiki's own first, labelled "(default)". None
//! of the first four is ever withheld.

use std::fmt::Write as _;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::html::esc;
use crate::i18n::Messages;
use crate::special::controls::OptionModel;
use crate::theme::{Sheet, Theme, retired_palette};

/// The keys that choose a stylesheet, whose choices are the wiki's.
pub const SHEET_KEYS: [(&str, Sheet); 2] = [
    ("ui.palette", Sheet::Palette),
    ("ui.typeset", Sheet::Typeset),
];

/// A reader's palette or type set where the wiki lets them choose it; empty for the
/// wiki's own.
fn sheet_value(
    theme: &Theme,
    options: Option<&Map<String, Value>>,
    key: &str,
    sheet: Sheet,
) -> String {
    let own = options
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty());
    let chosen = theme.chosen(sheet, own);
    if own == Some(chosen.as_str()) && chosen != default_of(theme, sheet) {
        chosen
    } else {
        String::new()
    }
}

fn default_of(theme: &Theme, sheet: Sheet) -> &str {
    match sheet {
        Sheet::Palette => theme.default_palette(),
        Sheet::Typeset => theme.default_typeset(),
    }
}

/// The keys the panel offers, with their choices and defaults, as `preferences.toml`
/// registers them (a test holds the two together).
pub const KEYS: &[(&str, &[&str], &str)] = &[
    ("ui.mode", &["auto", "light", "dark"], "auto"),
    ("ui.contrast", &["auto", "more"], "auto"),
    (
        "ui.text-size",
        &["small", "medium", "large", "x-large"],
        "medium",
    ),
    ("ui.vision", &["none", "by", "rg"], "none"),
];

/// The keys the panel offers as icon buttons, one chosen.
const ICON_KEYS: [&str; 3] = ["ui.mode", "ui.contrast", "ui.text-size"];

/// A reader's preferences with a retired palette read as what replaced it (0094 §4):
/// `ui.palette = parchment-rg` is `parchment`, with `ui.vision = rg` where the reader
/// has chosen no alternate. `None` where nothing changes.
#[must_use]
pub fn resolve(options: Option<&Map<String, Value>>) -> Option<Map<String, Value>> {
    let o = options?;
    let (palette, alternate) = retired_palette(o.get("ui.palette")?.as_str()?)?;
    let mut out = o.clone();
    out.insert("ui.palette".into(), json!(palette));
    let vision = o.get("ui.vision").and_then(Value::as_str);
    if let Some(a) = alternate
        && matches!(vision, None | Some("" | "none"))
    {
        out.insert("ui.vision".into(), json!(a));
    }
    Some(out)
}

/// The browser storage entry a reader's choices are kept under, one per host.
pub const STORAGE_KEY: &str = "ts-appearance";

/// A value of one of [`KEYS`], or its default when the value is not one of its choices.
fn value_of<'a>(options: Option<&'a Map<String, Value>>, key: &str) -> &'a str {
    let (_, choices, default) =
        KEYS.iter()
            .find(|(k, _, _)| *k == key)
            .copied()
            .unwrap_or(("", &[], ""));
    options
        .and_then(|o| o.get(key))
        .and_then(Value::as_str)
        .filter(|v| choices.contains(v))
        .unwrap_or(default)
}

/// The attributes and classes `<html>` carries (19 §5.9): for a signed-in reader the
/// account's choices and `data-appearance="account"`, which tells the head script the
/// server has decided; for anyone else `skin-theme-clientpref-os` alone, so the page
/// stays the same for every reader and the head script applies their own.
#[must_use]
pub fn html_attributes(options: Option<&Map<String, Value>>) -> (String, String) {
    let Some(options) = options else {
        return ("skin-theme-clientpref-os".into(), String::new());
    };
    let mode = value_of(Some(options), "ui.mode");
    let class = match mode {
        "dark" => "skin-theme-clientpref-night",
        "light" => "skin-theme-clientpref-day",
        _ => "skin-theme-clientpref-os",
    };
    let mut attrs = String::from(" data-appearance=\"account\"");
    if mode != "auto" {
        let _ = write!(attrs, " data-mode=\"{}\"", esc(mode));
    }
    if value_of(Some(options), "ui.contrast") == "more" {
        attrs.push_str(" data-contrast=\"more\"");
    }
    let _ = write!(
        attrs,
        " data-text-size=\"{}\"",
        esc(value_of(Some(options), "ui.text-size"))
    );
    let vision = value_of(Some(options), "ui.vision");
    if vision != "none" {
        let _ = write!(attrs, " data-vision=\"{}\"", esc(vision));
    }
    (class.into(), attrs)
}

/// The data block the site's script reads: whether the reader is signed in, the
/// account's values, which keys the account holds itself, and the palettes and type sets
/// a reader may choose.
#[must_use]
pub fn data(theme: &Theme, options: Option<&Map<String, Value>>, own: Option<&[String]>) -> String {
    let mut values: Map<String, Value> = KEYS
        .iter()
        .map(|(k, _, _)| ((*k).to_string(), json!(value_of(options, k))))
        .collect();
    let mut choices = Map::new();
    for (key, sheet) in SHEET_KEYS {
        values.insert(key.into(), json!(sheet_value(theme, options, key, sheet)));
        let names: Vec<String> = theme
            .choices(sheet)
            .into_iter()
            .map(|o| o.name)
            .filter(|n| n != default_of(theme, sheet))
            .collect();
        choices.insert(key.into(), json!(names));
    }
    let mut keys: Vec<&str> = KEYS.iter().map(|(k, _, _)| *k).collect();
    keys.extend(SHEET_KEYS.iter().map(|(k, _)| *k));
    let v = json!({
        "signedIn": options.is_some(),
        "storageKey": STORAGE_KEY,
        "keys": keys,
        "choices": choices,
        "values": if options.is_some() { Value::Object(values) } else { Value::Null },
        "own": own,
    });
    // A JSON block's text must not close the element.
    v.to_string().replace("</", "<\\/")
}

/// The icon of a choice of [`ICON_KEYS`], as an inline SVG drawn in `currentcolor`:
/// Codex's icons where one fits (sun, moon, the letter A), a screen for following the
/// device, and a half-filled circle for more contrast. Text sizes are the letter A at
/// four sizes.
fn icon(key: &str, choice: &str) -> String {
    const SCREEN: &str = "M2 3h16a1 1 0 011 1v10a1 1 0 01-1 1h-6v2h3v2H5v-2h3v-2H2a1 1 0 01-1-1V4a1 1 0 011-1zm1 2v8h14V5z";
    const SUN: &str = "m7.866 15.696-2 3.464-1.732-1 2-3.464zm8 2.464-1.732 1-2-3.464 1.732-1zM10 6a4 4 0 110 8 4 4 0 010-8m0 2a2 2 0 100 4 2 2 0 000-4m-6 3H0V9h4zm16 0h-4V9h4zM7.866 4.304l-1.732 1-2-3.464 1.732-1zm8-2.464-2 3.464-1.732-1 2-3.464z";
    const MOON: &str = "M11.426 2.525c-1.287 2.085-1.379 4.72 0 6.948 1.385 2.239 3.924 3.522 6.575 3.525l.85 1.525a9.27 9.27 0 01-3.515 3.285c-4.54 2.445-10.375 1.018-13.037-3.283C-.39 10.18 1.24 4.657 5.814 2.191A10 10 0 0110.577 1zm-2.511.644a8 8 0 00-2.153.783C3.137 5.905 1.954 10.166 4 13.472c2.073 3.352 6.726 4.547 10.387 2.575a7.5 7.5 0 001.687-1.236c-2.562-.496-4.916-1.97-6.35-4.286a8.5 8.5 0 01-.81-7.356Z";
    const CONTRAST: &str = "M10 1a9 9 0 110 18 9 9 0 010-18m0 2a7 7 0 000 14z";
    const LETTER: &str = "m15.692 18-2.174-5H6.481l-2.173 5h-2.18L9.082 2.002h1.834L17.873 18zm-8.34-7h5.296L10 4.907z";
    let (path, size) = match (key, choice) {
        ("ui.mode", "light") => (SUN, 20),
        ("ui.mode", "dark") => (MOON, 20),
        ("ui.contrast", "more") => (CONTRAST, 20),
        ("ui.text-size", "small") => (LETTER, 12),
        ("ui.text-size", "medium") => (LETTER, 15),
        ("ui.text-size", "large") => (LETTER, 18),
        ("ui.text-size", _) => (LETTER, 21),
        _ => (SCREEN, 20),
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\" viewBox=\"0 0 20 20\" aria-hidden=\"true\" focusable=\"false\"><path fill-rule=\"evenodd\" d=\"{path}\"/></svg>"
    )
}

/// The options of a palette or type-set select: the wiki's own as "Parchment
/// (default)", then the others the reader may choose, the accessibility ones marked.
#[must_use]
pub fn sheet_options(m: &Messages, theme: &Theme, sheet: Sheet) -> Vec<(String, String)> {
    let all = theme.choices(sheet);
    let default = default_of(theme, sheet);
    let default_label = all
        .iter()
        .find(|o| o.name == default)
        .map_or_else(|| default.to_string(), |o| o.label.clone());
    let mut out = vec![(
        String::new(),
        m.with("ts-appearance-default", &[&default_label]),
    )];
    for o in all.into_iter().filter(|o| o.name != default) {
        let label = if o.accessibility {
            m.with("ts-appearance-accessibility-choice", &[&o.label])
        } else {
            o.label
        };
        out.push((o.name, label));
    }
    out
}

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what Plain's `appearance.html` draws.
// ---------------------------------------------------------------------------------------

/// One choice of an icon group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IconOption {
    /// The value.
    pub value: String,
    /// Its name, which is also its tooltip.
    pub label: String,
    /// Whether it is the current one.
    pub checked: bool,
    /// Its icon: an SVG the binary draws (0097 A4).
    pub icon_html: String,
}

/// One control of the panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AppearanceControl {
    /// A group of icon buttons of which one is chosen: radio buttons in a fieldset.
    Icons {
        /// The fieldset's ID.
        id: String,
        /// The preference key.
        key: String,
        /// The legend.
        legend: String,
        /// The choices.
        options: Vec<IconOption>,
    },
    /// A labelled select.
    Select {
        /// The select's ID.
        id: String,
        /// The preference key.
        key: String,
        /// The label.
        label: String,
        /// The options.
        options: Vec<OptionModel>,
    },
}

/// The Appearance menu's panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppearancePanel {
    /// Whether the reader is signed in: the form posts to the web tier, and has **Apply**.
    pub signed_in: bool,
    /// The page to return to.
    pub returnto: String,
    /// What the panel says without JavaScript, to a reader not signed in.
    pub nojs: Option<String>,
    /// The controls.
    pub controls: Vec<AppearanceControl>,
    /// **Apply**, for a signed-in reader.
    pub apply: Option<String>,
}

fn select_model(
    m: &Messages,
    key: &str,
    options: &[(String, String)],
    current: &str,
) -> AppearanceControl {
    AppearanceControl::Select {
        id: format!("ts-appearance-{}", key.replace('.', "-")),
        key: key.to_string(),
        label: m.get(&format!("ts-pref-{}", key.replace('.', "-"))),
        options: options
            .iter()
            .map(|(value, label)| OptionModel {
                value: value.clone(),
                label: label.clone(),
                selected: current == value,
            })
            .collect(),
    }
}

/// The panel's model.
#[must_use]
pub fn panel_model(
    m: &Messages,
    theme: &Theme,
    options: Option<&Map<String, Value>>,
    returnto: &str,
) -> AppearancePanel {
    let signed_in = options.is_some();
    let mut controls = Vec::new();
    for (key, choices, _) in KEYS {
        let label_key = format!("ts-pref-{}", key.replace('.', "-"));
        let current = value_of(options, key);
        if ICON_KEYS.contains(key) {
            controls.push(AppearanceControl::Icons {
                id: format!("ts-appearance-{}", key.replace('.', "-")),
                key: (*key).to_string(),
                legend: m.get(&label_key),
                options: choices
                    .iter()
                    .map(|c| IconOption {
                        value: (*c).to_string(),
                        label: m.get(&format!("{label_key}-{c}")),
                        checked: current == *c,
                        icon_html: icon(key, c),
                    })
                    .collect(),
            });
            continue;
        }
        let opts: Vec<(String, String)> = choices
            .iter()
            .map(|c| ((*c).to_string(), m.get(&format!("{label_key}-{c}"))))
            .collect();
        controls.push(select_model(m, key, &opts, current));
    }
    for (key, sheet) in SHEET_KEYS {
        let opts = sheet_options(m, theme, sheet);
        if opts.len() > 1 {
            controls.push(select_model(
                m,
                key,
                &opts,
                &sheet_value(theme, options, key, sheet),
            ));
        }
    }
    AppearancePanel {
        signed_in,
        returnto: returnto.to_string(),
        nojs: (!signed_in).then(|| m.get("ts-appearance-nojs")),
        controls,
        apply: signed_in.then(|| m.get("ts-appearance-apply")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel's model (19 §5.10): three icon groups and the vision select for every
    /// reader, the current choices checked, **Apply** and the account form when signed in,
    /// the no-script note otherwise; icons are SVG (0097 A4).
    #[test]
    fn the_panel_is_a_model() {
        let m = Messages::for_language("en");
        let theme = Theme::shipped();
        let anon = panel_model(&m, &theme, None, "Main Page");
        assert!(!anon.signed_in && anon.apply.is_none() && anon.nojs.is_some());
        let icons = anon
            .controls
            .iter()
            .filter(|c| matches!(c, AppearanceControl::Icons { .. }))
            .count();
        assert_eq!(icons, 3);
        let mut o = Map::new();
        o.insert("ui.mode".into(), json!("dark"));
        let signed = panel_model(&m, &theme, Some(&o), "Item:Q6");
        assert!(signed.signed_in && signed.apply.is_some() && signed.nojs.is_none());
        let AppearanceControl::Icons { key, options, .. } = &signed.controls[0] else {
            panic!("the mode is an icon group");
        };
        assert_eq!(key, "ui.mode");
        let dark = options.iter().find(|o| o.value == "dark").unwrap();
        assert!(dark.checked && dark.icon_html.starts_with("<svg"));
        assert_eq!(options.iter().filter(|o| o.checked).count(), 1);
    }

    #[test]
    fn the_keys_match_the_registry() {
        let reg: toml::Table =
            toml::from_str(include_str!("../../../docs/registry/preferences.toml")).unwrap();
        let prefs = reg["preference"].as_array().unwrap();
        for (key, choices, default) in KEYS {
            let p = prefs
                .iter()
                .find(|p| p["key"].as_str() == Some(key))
                .unwrap_or_else(|| panic!("{key} is registered"));
            assert_eq!(p["group"].as_str(), Some("appearance"), "{key}");
            assert_eq!(p["browser"].as_bool(), Some(true), "{key}");
            let reg_choices: Vec<&str> = p["choices"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(toml::Value::as_str)
                .collect();
            assert_eq!(&reg_choices, choices, "{key}");
            assert_eq!(p["default"].as_str(), Some(*default), "{key}");
        }
    }

    #[test]
    fn attributes() {
        let (class, attrs) = html_attributes(None);
        assert_eq!(class, "skin-theme-clientpref-os");
        assert!(attrs.is_empty());
        let o: Map<String, Value> = serde_json::from_value(
            json!({"ui.mode": "dark", "ui.contrast": "more", "ui.text-size": "large"}),
        )
        .unwrap();
        let (class, attrs) = html_attributes(Some(&o));
        assert_eq!(class, "skin-theme-clientpref-night");
        assert_eq!(
            attrs,
            " data-appearance=\"account\" data-mode=\"dark\" data-contrast=\"more\" data-text-size=\"large\""
        );
        let o: Map<String, Value> = serde_json::from_value(json!({"ui.vision": "rg"})).unwrap();
        assert!(html_attributes(Some(&o)).1.ends_with(" data-vision=\"rg\""));
        let o: Map<String, Value> = serde_json::from_value(json!({"ui.vision": "xy"})).unwrap();
        assert!(!html_attributes(Some(&o)).1.contains("data-vision"));
        let junk: Map<String, Value> =
            serde_json::from_value(json!({"ui.mode": "\"><script>", "ui.text-size": 3})).unwrap();
        let (class, attrs) = html_attributes(Some(&junk));
        assert_eq!(class, "skin-theme-clientpref-os");
        assert_eq!(
            attrs,
            " data-appearance=\"account\" data-text-size=\"medium\""
        );
    }

    #[test]
    fn the_data_block_cannot_close_its_element() {
        let o: Map<String, Value> = Map::new();
        let t = Theme::shipped();
        let d = data(&t, Some(&o), Some(&["</script>".to_string()]));
        assert!(!d.contains("</"));
        assert!(data(&t, None, None).contains("\"signedIn\":false"));
        let v: Value = serde_json::from_str(&data(&t, Some(&o), None)).unwrap();
        assert_eq!(v["choices"]["ui.palette"], json!(["encyclopedia"]));
        assert_eq!(v["values"]["ui.vision"], "none");
        assert_eq!(v["values"]["ui.palette"], "");
    }

    #[test]
    fn the_panel_offers_what_the_wiki_lets_readers_choose() {
        let m = Messages::for_language("en");
        let o: Map<String, Value> = serde_json::from_value(
            json!({"ui.palette": "encyclopedia", "ui.typeset": "nope", "ui.mode": "dark"}),
        )
        .unwrap();
        let skins = crate::skin::Skins::load(None);
        let html = skins
            .get("plain")
            .unwrap()
            .render_str(
                "{% import \"appearance.html\" as a %}{{ a.panel(p) }}",
                &m,
                json!({ "p": panel_model(&m, &Theme::shipped(), Some(&o), "Main Page") }),
            )
            .unwrap();
        assert!(html.contains("<select class=\"cdx-select\" id=\"ts-appearance-ui-palette\" name=\"ui.palette\" data-key=\"ui.palette\"><option value=\"\">Parchment (default)</option>"));
        assert!(html.contains("<option value=\"\" selected>Source (default)</option>"));
        assert!(html.contains("<option value=\"encyclopedia\" selected>"));
        assert!(
            html.contains("<option value=\"\" selected>"),
            "an unknown type set is the wiki's"
        );
        assert!(html.contains("<option value=\"opendyslexic\">"));
        // Mode, contrast and text size are icon buttons, one chosen, named by label.
        assert!(html.contains("<fieldset class=\"ts-choice-group\" id=\"ts-appearance-ui-mode\"><legend class=\"ts-choice-group__legend\">Colour mode</legend>"));
        assert!(html.contains("<label class=\"ts-choice-group__option\" title=\"Dark\"><input class=\"ts-choice-group__input\" type=\"radio\" name=\"ui.mode\" value=\"dark\" data-key=\"ui.mode\" checked>"));
        assert!(html.contains("value=\"medium\" data-key=\"ui.text-size\" checked>"));
        assert!(
            !html.contains("id=\"ts-appearance-ui-mode\" name="),
            "no mode select"
        );
        // The colour-vision alternate is a select, none first.
        assert!(html.contains("<select class=\"cdx-select\" id=\"ts-appearance-ui-vision\" name=\"ui.vision\" data-key=\"ui.vision\"><option value=\"none\" selected>"));
    }

    #[test]
    fn a_retired_palette_reads_as_its_alternate() {
        let o: Map<String, Value> =
            serde_json::from_value(json!({"ui.palette": "parchment-rg", "ui.mode": "dark"}))
                .unwrap();
        let r = resolve(Some(&o)).unwrap();
        assert_eq!(r["ui.palette"], "parchment");
        assert_eq!(r["ui.vision"], "rg");
        assert_eq!(r["ui.mode"], "dark");
        let o: Map<String, Value> =
            serde_json::from_value(json!({"ui.palette": "parchment-rg", "ui.vision": "by"}))
                .unwrap();
        assert_eq!(
            resolve(Some(&o)).unwrap()["ui.vision"],
            "by",
            "the reader's own wins"
        );
        let o: Map<String, Value> =
            serde_json::from_value(json!({"ui.palette": "encyclopedia"})).unwrap();
        assert!(resolve(Some(&o)).is_none());
        assert!(resolve(None).is_none());
    }

    #[test]
    fn the_alternates_match_the_registry() {
        let (_, choices, _) = KEYS.iter().find(|(k, _, _)| *k == "ui.vision").unwrap();
        let mut keys = crate::theme::alternate_keys();
        keys.insert(0, "none".into());
        assert_eq!(choices.to_vec(), keys);
    }
}
