//! `Special:Preferences` (0085 §5; 19 §2.6) and the Appearance menu's form (0085 §6).
//!
//! The page is the signed-in reader's preferences, one section per group of
//! `preferences.toml`, each a form that saves with one `PATCH /account/preferences`;
//! every key has **Reset**, and **Restore all defaults** resets them all. A reader who is
//! not logged in is offered **Log in** and pointed to the Appearance menu, which keeps
//! their choices in the browser.
//!
//! The Appearance menu's form posts here with `appearance=1`. For a signed-in reader the
//! web tier saves the keys that changed and answers `303` back to the page the reader was
//! on. The form carries no token, since it is drawn on every page, including public
//! ones: the web tier accepts it only from the site's own pages (`Origin` or
//! `Sec-Fetch-Site`), and then asks the API for the reader's token itself.

use std::collections::{BTreeMap, BTreeSet};

use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use serde_json::{Map, Value, json};
use triplespace_client::settings::{Pref, Preferences, Saved};
use triplespace_client::{CacheInfo, ClientError, Incoming};

use crate::Site;
use crate::codex::{Action, Button, ButtonModel, Message, MessageKind, MessageModel, Weight};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{self, Control, Kind, ResetModel, SettingRow};
use crate::theme::Theme;

const TITLE: &str = "Special:Preferences";

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/preferences.html` draws.
// ---------------------------------------------------------------------------------------

/// One section's form: a group of `preferences.toml`, saved together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PrefSection {
    /// The group's ID, the section's anchor.
    pub id: String,
    /// Its heading.
    pub title: String,
    /// That it was saved.
    pub saved: Option<MessageModel>,
    /// The Appearance section's note.
    pub note: Option<String>,
    /// What the form refused about the section as a whole.
    pub errors: Vec<MessageModel>,
    /// The CSRF token.
    pub token: String,
    /// The keys it holds, space-separated.
    pub keys: String,
    /// The rows.
    pub rows: Vec<SettingRow>,
    /// **Save**.
    pub save: ButtonModel,
}

/// **Restore all defaults**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RestoreForm {
    /// The CSRF token.
    pub token: String,
    /// What it does.
    pub intro: String,
    /// The button.
    pub button: ButtonModel,
}

/// The page for a reader who is not logged in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AnonymousPrefs {
    /// That preferences need an account.
    pub notice: MessageModel,
    /// **Log in**, where the wiki offers it.
    pub login: Option<Choice>,
    /// That the Appearance menu keeps choices in the browser.
    pub appearance: String,
}

/// `Special:Preferences`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PreferencesPage {
    /// The page for a reader who is not logged in, in place of the rest.
    pub anonymous: Option<AnonymousPrefs>,
    /// That every default was restored.
    pub restored: Option<MessageModel>,
    /// The sections.
    pub sections: Vec<PrefSection>,
    /// **Restore all defaults**.
    pub restore: Option<RestoreForm>,
}

/// The palette and type set are chosen from what the wiki lets readers choose (0086 §4),
/// as the Appearance menu offers them: a select whose empty choice is the wiki's own.
fn sheet_pref(
    m: &Messages,
    theme: &Theme,
    key: &str,
    p: &Pref,
) -> Option<(Pref, Vec<(String, String)>)> {
    let (_, sheet) = crate::appearance::SHEET_KEYS
        .iter()
        .find(|(k, _)| *k == key)?;
    let options = crate::appearance::sheet_options(m, theme, *sheet);
    let mut p = p.clone();
    p.ty = "choice".into();
    p.choices = options.iter().skip(1).map(|(v, _)| v.clone()).collect();
    if p.value
        .as_ref()
        .and_then(Value::as_str)
        .is_some_and(|v| !p.choices.iter().any(|c| c == v))
    {
        p.value = None;
    }
    Some((p, options))
}

/// How a preference type is edited.
fn kind_of(ty: &str) -> Kind {
    match ty {
        "boolean" => Kind::Boolean,
        "choice" => Kind::Choice,
        "integer" => Kind::Integer,
        "language-list" => Kind::Lines,
        "json" => Kind::Json,
        _ => Kind::Line,
    }
}

fn label(m: &Messages, key: &str, p: &Pref) -> String {
    if m.has(&p.label) {
        m.get(&p.label)
    } else {
        key.to_string()
    }
}

/// What came back with a form.
#[derive(Debug, Default)]
struct Returned<'a> {
    section: String,
    form: Option<&'a Params>,
    errors: BTreeMap<String, Vec<String>>,
}

/// One preference's row, as a model.
fn row_model(m: &Messages, theme: &Theme, key: &str, p: &Pref, back: &Returned<'_>) -> SettingRow {
    let sheet = sheet_pref(m, theme, key, p);
    let p = sheet.as_ref().map_or(p, |(p, _)| p);
    let kind = kind_of(&p.ty);
    let id = format!("ts-p-{key}");
    let help_key = format!("{}-help", p.label);
    let help_id = format!("{id}-help");
    let name = format!("v.{key}");
    let label_text = label(m, key, p);
    let has_help = m.has(&help_key);
    let typed;
    let value = match (back.section == p.group, back.form) {
        (true, Some(f)) => match (kind, f.map.get(&name)) {
            (Kind::Boolean, raw) => {
                typed = Value::Bool(raw.is_some());
                Some(&typed)
            }
            (_, Some(raw)) => {
                typed = Value::String(raw.clone());
                Some(&typed)
            }
            (_, None) => p.value.as_ref(),
        },
        _ => p.value.as_ref(),
    };
    let choice_label = |c: &str| {
        if let Some((_, options)) = &sheet {
            return options
                .iter()
                .find(|(v, _)| v == c)
                .map_or_else(|| c.to_string(), |(_, l)| l.clone());
        }
        let k = format!("{}-{}", p.label, c.replace(' ', "-").to_lowercase());
        if m.has(&k) { m.get(&k) } else { c.to_string() }
    };
    let empty = sheet.as_ref().map(|(_, options)| options[0].1.clone());
    let control = Control {
        id: &id,
        name: &name,
        kind,
        choices: &p.choices,
        value,
        empty_choice: empty
            .as_deref()
            .or_else(|| p.value.is_none().then_some("—")),
        described_by: has_help.then_some(help_id.as_str()),
        choice_label: Some(&choice_label),
    }
    .model();
    SettingRow {
        row_id: format!("pref-{key}"),
        key: key.to_string(),
        label: label_text.clone(),
        label_for: Some(id.clone()),
        chips: Vec::new(),
        help_id: Some(help_id.clone()),
        help: has_help.then(|| m.get(&help_key)),
        errors: back
            .errors
            .get(key)
            .map(|e| {
                e.iter()
                    .map(|t| Message::block(MessageKind::Error, t).model())
                    .collect()
            })
            .unwrap_or_default(),
        control: Some(control),
        shown: None,
        reset: p.own.then(|| ResetModel {
            value: key.to_string(),
            aria_label: m.with("ts-settings-reset-label", &[&label_text]),
            label: m.get("ts-settings-reset"),
        }),
        lock: None,
        shown_chip: None,
        note: None,
    }
}

/// The page's model, signed in.
fn body_model(
    m: &Messages,
    theme: &Theme,
    prefs: &Preferences,
    token: &str,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> PreferencesPage {
    let errors = |e: Option<&Vec<String>>| -> Vec<MessageModel> {
        e.map(|e| {
            e.iter()
                .map(|t| Message::block(MessageKind::Error, t).model())
                .collect()
        })
        .unwrap_or_default()
    };
    let mut sections = Vec::new();
    for g in &prefs.groups {
        let keys: Vec<(&String, &Pref)> = prefs
            .preferences
            .iter()
            .filter(|(_, p)| p.group == g.id)
            .collect();
        if keys.is_empty() {
            continue;
        }
        let names: Vec<&str> = keys.iter().map(|(k, _)| k.as_str()).collect();
        sections.push(PrefSection {
            id: g.id.clone(),
            title: if m.has(&g.label) {
                m.get(&g.label)
            } else {
                g.id.clone()
            },
            saved: saved.is_some_and(|s| *s == g.id).then(|| {
                Message::block(MessageKind::Success, &m.get("ts-preferences-saved")).model()
            }),
            note: (g.id == "appearance").then(|| m.get("ts-preferences-appearance-note")),
            errors: if back.section == g.id {
                errors(back.errors.get(""))
            } else {
                Vec::new()
            },
            token: token.to_string(),
            keys: names.join(" "),
            rows: keys
                .iter()
                .map(|(k, p)| row_model(m, theme, k, p, back))
                .collect(),
            save: Button {
                label: &m.get("ts-preferences-save"),
                kind: "submit",
                action: Action::Progressive,
                weight: Weight::Primary,
            }
            .model(),
        });
    }
    PreferencesPage {
        anonymous: None,
        restored: saved.is_some_and(|s| s == "restore").then(|| {
            Message::block(MessageKind::Success, &m.get("ts-preferences-restored")).model()
        }),
        sections,
        restore: Some(RestoreForm {
            token: token.to_string(),
            intro: m.get("ts-preferences-restore-intro"),
            button: Button {
                label: &m.get("ts-preferences-restore"),
                kind: "submit",
                action: Action::Destructive,
                weight: Weight::Normal,
            }
            .model(),
        }),
    }
}

/// The page for a reader who is not logged in.
fn anonymous(cx: &Context, headers: &HeaderMap) -> Response {
    let m = &cx.m;
    let login_href = format!("/wiki/Special:UserLogin?returnto={}", title_url(TITLE));
    let offered = cx.features.has(Feature::Login);
    let model = PreferencesPage {
        anonymous: Some(AnonymousPrefs {
            notice: Message::block(MessageKind::Notice, &m.get("ts-preferences-anon")).model(),
            login: offered.then(|| Choice {
                href: login_href.clone(),
                label: m.get("ts-login"),
                current: false,
            }),
            appearance: m.get("ts-preferences-anon-appearance"),
        }),
        restored: None,
        sections: Vec::new(),
        restore: None,
    };
    let body = cx.draw("special/preferences.html", model);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-preferences-title"),
        returnto: Some(TITLE.to_string()),
        body,
        ..Page::default()
    })
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Preferences) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok(cx)
}

async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &Returned<'_>,
    saved: Option<&String>,
    status: StatusCode,
) -> Response {
    let client = site.client();
    let (prefs, token) = tokio::join!(client.preferences(incoming), client.token(incoming, "csrf"));
    let (prefs, token) = match (prefs, token) {
        (Ok(p), Ok(t)) => (p, t),
        (Err(ClientError::Api { code, .. }), _) if code == "notloggedin" => {
            return anonymous(cx, headers);
        }
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(cx, headers, &e),
    };
    let model = body_model(
        &cx.m,
        &cx.theme,
        &prefs.value,
        &token.value.value,
        back,
        saved,
    );
    // The page's tabs are the frame's: one per section.
    let tabs = model
        .sections
        .iter()
        .map(|s| Tab {
            label: s.title.clone(),
            href: format!("#{}", s.id),
            current: false,
        })
        .collect();
    let html = cx.draw("special/preferences.html", model);
    let page = pages::respond(cx, headers, status, |m| Page {
        title: m.get("ts-preferences-title"),
        returnto: Some(TITLE.to_string()),
        tabs,
        body: html,
        ..Page::default()
    });
    pages::with_cookies(page, &token.set_cookies)
}

/// `GET Special:Preferences`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if cx.user.is_none() {
        return anonymous(&cx, headers);
    }
    render(
        site,
        &mut cx,
        headers,
        &incoming,
        &Returned::default(),
        params.map.get("saved"),
        StatusCode::OK,
    )
    .await
}

/// The body of a section's save.
fn patch_of(
    form: &Params,
    prefs: &Preferences,
) -> (Map<String, Value>, BTreeMap<String, Vec<String>>) {
    let f = &form.map;
    let mut body = Map::new();
    let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(t) = f.get("token") {
        body.insert("token".into(), json!(t));
    }
    if f.contains_key("reset_all") {
        body.insert("reset_all".into(), json!(true));
        return (body, errors);
    }
    if let Some(k) = f.get("reset") {
        body.insert("reset".into(), json!([k]));
        return (body, errors);
    }
    let keys: BTreeSet<&str> = f
        .get("keys")
        .map_or_else(BTreeSet::new, |k| k.split_whitespace().collect());
    let mut set = Map::new();
    let mut reset = Vec::new();
    for key in keys {
        let Some(p) = prefs.preferences.get(key) else {
            continue;
        };
        // The palette's and type set's empty choice is the wiki's own: it resets.
        if crate::appearance::SHEET_KEYS.iter().any(|(k, _)| *k == key)
            && f.get(&format!("v.{key}")).is_some_and(String::is_empty)
        {
            if p.own {
                reset.push(key.to_string());
            }
            continue;
        }
        let kind = kind_of(&p.ty);
        match controls::parse(kind, f.get(&format!("v.{key}")).map(String::as_str)) {
            Err(e) => errors.entry(key.to_string()).or_default().push(e),
            Ok(None) => {}
            Ok(Some(v)) => {
                if p.value.as_ref() != Some(&v) {
                    set.insert(key.to_string(), v);
                }
            }
        }
    }
    if !set.is_empty() {
        body.insert("set".into(), Value::Object(set));
    }
    if !reset.is_empty() {
        body.insert("reset".into(), json!(reset));
    }
    (body, errors)
}

/// Whether a form came from the site's own pages: the browser's `Origin` names this host,
/// or `Sec-Fetch-Site` says the request is same-origin.
fn same_origin(headers: &HeaderMap, host: Option<&str>) -> bool {
    if let Some(site) = headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        return site == "same-origin";
    }
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    match (origin, host) {
        (Some(o), Some(h)) => {
            let o = o.split_once("://").map_or(o, |(_, rest)| rest);
            o.eq_ignore_ascii_case(h)
        }
        _ => false,
    }
}

/// The page to return to from the Appearance menu: a title, never an address elsewhere.
fn returnto(form: &BTreeMap<String, String>, mainpage: &str) -> String {
    let t = form.get("returnto").map_or("", |v| v.trim()).to_string();
    if t.is_empty() || t.contains("://") || t.starts_with('/') {
        mainpage.to_string()
    } else {
        t
    }
}

/// `POST Special:Preferences&appearance=1`: the Appearance menu without JavaScript.
async fn post_appearance(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    cx: &Context,
    form: &Params,
) -> Response {
    let back = format!(
        "/wiki/{}",
        title_url(&returnto(&form.map, &cx.site.mainpage))
    );
    if cx.user.is_none() {
        // Nothing can be kept for a reader who is not logged in and has no script.
        return pages::see_other(&back, &[]);
    }
    if !same_origin(headers, incoming.host()) {
        return (
            StatusCode::FORBIDDEN,
            [(header::CACHE_CONTROL, "no-store")],
            "Forbidden\n",
        )
            .into_response();
    }
    let current = cx.prefs.clone().unwrap_or_default();
    let mut set = Map::new();
    let mut reset = Vec::new();
    for (key, choices, _) in crate::appearance::KEYS {
        if let Some(v) = form.map.get(*key)
            && choices.contains(&v.as_str())
            && current.get(*key).and_then(Value::as_str) != Some(v.as_str())
        {
            set.insert((*key).to_string(), json!(v));
        }
    }
    for (key, sheet) in crate::appearance::SHEET_KEYS {
        let Some(v) = form.map.get(key) else { continue };
        let now = current.get(key).and_then(Value::as_str).unwrap_or_default();
        if v.is_empty() {
            if !now.is_empty() {
                reset.push(key);
            }
        } else if v != now && cx.theme.choices(sheet).iter().any(|o| o.name == *v) {
            set.insert(key.to_string(), json!(v));
        }
    }
    if set.is_empty() && reset.is_empty() {
        return pages::see_other(&back, &[]);
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let body = json!({"set": set, "reset": reset, "token": token.value.value});
    match site.client().save_preferences(incoming, &body).await {
        Ok(Saved::Ok(_)) => pages::see_other(&back, &token.set_cookies),
        Ok(Saved::Refused(r)) => {
            let e = ClientError::Api {
                code: r.first().map(|x| x.code.clone()).unwrap_or_default(),
                info: r.first().map(|x| x.message.clone()).unwrap_or_default(),
            };
            pages::api_failed(cx, headers, &e)
        }
        Err(e) => pages::api_failed(cx, headers, &e),
    }
}

/// `POST Special:Preferences`: saves a section, resets a key or all of them, or takes the
/// Appearance menu's form.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if query.contains_key("appearance") || form.map.contains_key("appearance") {
        return post_appearance(site, headers, &incoming, &cx, form).await;
    }
    if cx.user.is_none() {
        return anonymous(&cx, headers);
    }
    let section = form.map.get("section").cloned().unwrap_or_default();
    let prefs = match site.client().preferences(&incoming).await {
        Ok(p) => p.value,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let (body, errors) = patch_of(form, &prefs);
    let back_to = |saved: bool| {
        let anchor = if section == "restore" {
            String::new()
        } else {
            format!("#{section}")
        };
        format!(
            "/wiki/{}{}{anchor}",
            title_url(TITLE),
            if saved {
                format!("?saved={section}")
            } else {
                String::new()
            }
        )
    };
    if !errors.is_empty() {
        let back = Returned {
            section: section.clone(),
            form: Some(form),
            errors,
        };
        return render(
            site,
            &mut cx,
            headers,
            &incoming,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    if !["set", "reset", "reset_all"]
        .iter()
        .any(|k| body.contains_key(*k))
    {
        return pages::see_other(&back_to(false), &[]);
    }
    match site
        .client()
        .save_preferences(&incoming, &Value::Object(body))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to(true), &[]),
        Ok(Saved::Refused(refusals)) => {
            let mut errors: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for r in refusals {
                let text = match r.code.as_str() {
                    "badtoken" => cx.m.get("ts-login-expired"),
                    "permissiondenied" => cx.m.get("ts-preferences-denied"),
                    _ => r.message,
                };
                // The API names the key in its message; the form shows it at the top.
                errors.entry(String::new()).or_default().push(text);
            }
            let back = Returned {
                section: section.clone(),
                form: Some(form),
                errors,
            };
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preference's row (19 §5.10): its control from its type, help where the label has
    /// some, **Reset** only for the reader's own value, and the reader's text back after a
    /// refusal, with the row's errors.
    #[test]
    fn a_row_is_a_model() {
        let m = Messages::for_language("en");
        let theme = Theme::shipped();
        let p = Pref {
            value: Some(json!(50)),
            own: false,
            ty: "integer".into(),
            group: "history".into(),
            label: "ts-pref-rc-limit".into(),
            ..Pref::default()
        };
        let r = row_model(&m, &theme, "rc.limit", &p, &Returned::default());
        assert_eq!(
            (r.row_id.as_str(), r.label_for.as_deref()),
            ("pref-rc.limit", Some("ts-p-rc.limit"))
        );
        assert!(r.reset.is_none() && r.errors.is_empty());
        assert!(
            matches!(&r.control, Some(controls::ControlModel::Input { value, .. }) if value == "50")
        );
        let own = Pref {
            own: true,
            ..p.clone()
        };
        assert_eq!(
            row_model(&m, &theme, "rc.limit", &own, &Returned::default())
                .reset
                .unwrap()
                .value,
            "rc.limit"
        );
        let mut sent = Params::default();
        sent.map.insert("v.rc.limit".into(), "twenty".into());
        let back = Returned {
            section: "history".into(),
            form: Some(&sent),
            errors: BTreeMap::from([("rc.limit".to_string(), vec!["not a number".to_string()])]),
        };
        let r = row_model(&m, &theme, "rc.limit", &p, &back);
        assert_eq!(r.errors.len(), 1);
        assert!(
            matches!(&r.control, Some(controls::ControlModel::Input { value, .. }) if value == "twenty")
        );
    }

    #[test]
    fn same_origin_checks() {
        let mut h = HeaderMap::new();
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert(header::ORIGIN, "https://librarybase.org".parse().unwrap());
        assert!(same_origin(&h, Some("librarybase.org")));
        h.insert(header::ORIGIN, "https://evil.example".parse().unwrap());
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(!same_origin(&h, Some("librarybase.org")));
        h.insert("sec-fetch-site", "same-origin".parse().unwrap());
        assert!(same_origin(&h, Some("librarybase.org")));
    }

    #[test]
    fn returnto_stays_on_the_site() {
        let f = |v: &str| BTreeMap::from([("returnto".to_string(), v.to_string())]);
        assert_eq!(returnto(&f("Item:Q6"), "Project:Home"), "Item:Q6");
        assert_eq!(
            returnto(&f("https://evil.example/"), "Project:Home"),
            "Project:Home"
        );
        assert_eq!(
            returnto(&f("//evil.example"), "Project:Home"),
            "Project:Home"
        );
    }
}
