//! `Special:WikiSettings`, `Special:InstanceSettings` and `Special:Tenancy` (0085 §4;
//! 19 §2.8): a tenant's settings at its base, the instance's at the farm base, built from
//! `GET /settings/schema` and `GET /settings` with no code per setting.
//!
//! The page is one section per registry group, reached by the links at its top; each
//! section is a form of its own and saves with one `PATCH /settings`, so a section's
//! anchor (`#tenancy`) is a place on the page with or without JavaScript. Each row shows
//! the label and help, the value with a control drawn from its type, an origin chip and
//! **Reset**. A viewer who may not change a setting sees its value without a control.
//!
//! `Special:WikiSettings` adds a read-only **Tenancy** section: the instance's policy as
//! it applies to the wiki. `Special:InstanceSettings`, served only at the farm base, adds
//! the **Tenancy** switches and, unless `config.template` is `none`, **Defaults for
//! wikis**: every tenant setting with its registry default, the instance's default, its
//! lock under `config.template = locks`, and how many wikis inherit the default.
//!
//! A save that works answers `303` to the section with `?saved=`; one the API refuses
//! shows the page again with the reader's input and every reason beside its row.

use std::collections::{BTreeMap, BTreeSet};

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use serde_json::{Map, Value, json};
use triplespace_client::settings::{Refusal, Resolved, Saved, Schema, SettingDef, Settings};
use triplespace_client::{CacheInfo, ClientError, Incoming};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, ChipModel, Field, FieldModel, Message, MessageKind, MessageModel,
    Weight,
};
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{self, Control, ControlModel, Kind, ResetModel, SettingRow};

/// Which page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// `Special:WikiSettings`.
    Wiki,
    /// `Special:InstanceSettings`.
    Instance,
}

impl Scope {
    fn title(self) -> &'static str {
        match self {
            Self::Wiki => "Special:WikiSettings",
            Self::Instance => "Special:InstanceSettings",
        }
    }

    fn instance(self) -> bool {
        self == Self::Instance
    }
}

/// The section of the tenancy policy.
pub const TENANCY: &str = "tenancy";
/// The Visibility section's anchor (19 §6.3).
pub const VISIBILITY: &str = "visibility";
/// The section of the instance's defaults for wikis.
pub const DEFAULTS: &str = "defaults";

/// The text of a registry entry's Markdown description, for the help shown while its
/// message is missing: links reduced to their text, code marks dropped.
fn plain(markdown: &str) -> String {
    let mut out = String::new();
    let mut rest = markdown;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let link = after.find("](").and_then(|close| {
            after[close + 2..]
                .find(')')
                .map(|end| (close, close + 2 + end))
        });
        if let Some((close, end)) = link {
            out.push_str(&after[..close]);
            rest = &after[end + 1..];
        } else {
            out.push('[');
            rest = after;
        }
    }
    out.push_str(rest);
    out.replace('`', "")
}

/// A setting's label: its message, or its key where the message is missing (a setting an
/// instance registered).
fn label(m: &Messages, def: &SettingDef) -> String {
    if m.has(&def.label) {
        m.get(&def.label)
    } else {
        def.key.clone()
    }
}

/// A setting's help: its message, or the registry's description.
fn help(m: &Messages, def: &SettingDef) -> Option<String> {
    if m.has(&def.help) {
        Some(m.get(&def.help))
    } else {
        def.meaning.as_deref().map(plain)
    }
}

/// A group's label.
fn group_label(m: &Messages, id: &str, key: &str) -> String {
    if m.has(key) {
        m.get(key)
    } else {
        id.to_string()
    }
}

/// What the page shows in place of an absent value: the registry's description of what
/// applies, or `none`.
fn absent(m: &Messages, def: &SettingDef) -> String {
    def.default_text
        .as_deref()
        .map_or_else(|| m.get("ts-settings-none"), plain)
}

/// What came back with a form: the reader's input and the API's reasons.
#[derive(Debug, Default)]
struct Returned<'a> {
    /// The section the form was.
    section: String,
    /// The form.
    form: Option<&'a Params>,
    /// The reasons, by key; the empty key for the section as a whole.
    errors: BTreeMap<String, Vec<String>>,
}

impl Returned<'_> {
    /// The form the reader sent, if it was this section's.
    fn sent(&self, section: &str) -> Option<&BTreeMap<String, String>> {
        if self.section == section {
            self.form.map(|f| &f.map)
        } else {
            None
        }
    }

    /// A text field of this section's form, if it was sent.
    fn text(&self, section: &str, field: &str) -> Option<&str> {
        self.sent(section)?.get(field).map(String::as_str)
    }
}

/// Everything a page draws from the API.
struct Loaded {
    schema: Schema,
    settings: Settings,
    token: Option<String>,
    cookies: Vec<HeaderValue>,
    /// The wiki's visibility and the groups it may be restricted to, for the Visibility
    /// section (09 §4.7; 19 §6.3); `None` at the instance's pages or where the API does
    /// not serve `/acl`.
    visibility: Option<(triplespace_client::TenantVisibility, Vec<String>)>,
}

/// Whether the viewer may write settings at all: they may change one that needs only
/// `ts-config`.
fn may_configure(schema: &Schema, settings: &Settings) -> bool {
    schema
        .settings
        .iter()
        .filter(|d| d.right.is_none())
        .any(|d| settings.settings.get(&d.key).is_some_and(|r| r.can_change))
}

/// The settings that name a palette or type set (0086 §4) offered as a select of the
/// wiki's palettes and type sets.
fn offer_themes(defs: &mut [SettingDef], theme: &crate::theme::Theme) {
    for d in defs {
        let sheet = match d.ty.as_str() {
            "palette" => crate::theme::Sheet::Palette,
            "typeset" => crate::theme::Sheet::Typeset,
            _ => continue,
        };
        d.ty = "choice".into();
        d.choices = theme.offered(sheet);
    }
}

async fn load(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    scope: Scope,
) -> Result<Loaded, ClientError> {
    let client = site.client();
    let (schema, settings) = tokio::join!(
        client.settings_schema(incoming, scope.instance()),
        client.settings(incoming, scope.instance())
    );
    let (schema, settings) = (schema?, settings?);
    cx.inputs.push(schema.cache);
    cx.inputs.push(settings.cache);
    let (mut token, mut cookies) = (None, Vec::new());
    if may_configure(&schema.value, &settings.value) {
        let t = client.token(incoming, "csrf").await?;
        // A page holding a token is the viewer's alone.
        cx.inputs.push(CacheInfo::default());
        token = Some(t.value.value);
        cookies = t.set_cookies;
    }
    let mut schema = schema.value;
    offer_themes(&mut schema.settings, &cx.theme);
    // Only for a viewer who may configure: the ACL route's answer is the viewer's alone,
    // and a reader's page stays public (0057 §6).
    let visibility = if scope == Scope::Wiki && token.is_some() && cx.features.has(Feature::Protect)
    {
        let (v, g) = tokio::join!(
            client.tenant_visibility(incoming, &cx.site.tenant),
            client.groups(incoming)
        );
        match (v, g) {
            (Ok(v), Ok(g)) => {
                cx.inputs.push(v.cache);
                cx.inputs.push(g.cache);
                Some((
                    v.value,
                    g.value
                        .into_iter()
                        .filter(|g| !g.implicit && g.name != "*")
                        .map(|g| g.name)
                        .collect(),
                ))
            }
            // A reader of a private wiki, or an API without the route: no section.
            _ => None,
        }
    } else {
        None
    };
    Ok(Loaded {
        schema,
        settings: settings.value,
        token,
        cookies,
        visibility,
    })
}

fn value_or_unset(m: &Messages, v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => m.get("ts-settings-unset"),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// `Special:WikiSettings` and `Special:InstanceSettings` as Plain's
/// `special/settings.html` draws them (19 §5.10): the read-only notice and the sections.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsPage {
    /// Said to a viewer who may change nothing.
    pub read_only: Option<MessageModel>,
    /// One per registry group, then Visibility, Tenancy and Defaults for wikis.
    pub sections: Vec<SettingsSection>,
}

/// One section: its heading, notices and, where the viewer may change it, its form.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsSection {
    /// The anchor.
    pub id: String,
    /// The heading.
    pub title: String,
    /// A paragraph under the heading.
    pub intro: Option<String>,
    /// The notice after a save.
    pub saved: Option<MessageModel>,
    /// What the API refused about the section as a whole.
    pub errors: Vec<MessageModel>,
    /// The form around the body, where the viewer may change the section.
    pub form: Option<SectionForm>,
    /// The rows.
    pub body: SectionBody,
}

/// A section's form: where it posts, the fields that say what it is, the summary and
/// **Save**.
#[derive(Debug, Clone, Serialize)]
pub struct SectionForm {
    /// The `action`.
    pub action: String,
    /// The section.
    pub section: String,
    /// The settings' base revision.
    pub base: u64,
    /// The CSRF token.
    pub token: Option<String>,
    /// The keys the form edits, space-separated.
    pub keys: String,
    /// The summary field.
    pub summary: FieldModel,
    /// **Save**.
    pub save: ButtonModel,
}

/// What a section holds.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SectionBody {
    /// Rows of settings (`forms.html`'s `setting`).
    Rows {
        /// The rows.
        rows: Vec<SettingRow>,
    },
    /// The tenancy switches, and on a wiki what the instance locks and caps.
    Switches {
        /// The switches.
        switches: Vec<SwitchRow>,
        /// Headed paragraphs after them.
        facts: Vec<Fact>,
    },
    /// Who may read the wiki, and the form that changes it.
    Visibility {
        /// Who may read it now.
        now: String,
        /// The form, for a viewer holding a token.
        form: Option<Box<VisibilityForm>>,
    },
}

/// One tenancy switch.
#[derive(Debug, Clone, Serialize)]
pub struct SwitchRow {
    /// The row's element ID.
    pub row_id: String,
    /// The label.
    pub label: String,
    /// The control the label is for; `None` draws the label as text.
    pub label_for: Option<String>,
    /// The switch's name.
    pub key: String,
    /// What the form refused about it.
    pub errors: Vec<MessageModel>,
    /// The control.
    pub control: Option<ControlModel>,
    /// What the switch allows, after the control.
    pub allowed: Option<String>,
    /// The value shown, where it is not edited here.
    pub shown: Option<String>,
}

/// A heading and its paragraph.
#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    /// The heading.
    pub heading: String,
    /// The paragraph.
    pub text: String,
}

/// The Visibility form (09 §4.7).
#[derive(Debug, Clone, Serialize)]
pub struct VisibilityForm {
    /// The `action`.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The fieldset's legend.
    pub legend: String,
    /// Anyone, accounts on this wiki, a group.
    pub options: Vec<ControlModel>,
    /// The group's label.
    pub group_label: String,
    /// The group's select.
    pub group: ControlModel,
    /// The group's help.
    pub group_help: String,
    /// The warning before the reason.
    pub warning: MessageModel,
    /// The reason.
    pub reason: FieldModel,
    /// **Save**.
    pub save: ButtonModel,
}

fn messages(kind: MessageKind, texts: Option<&Vec<String>>) -> Vec<MessageModel> {
    texts.map_or_else(Vec::new, |e| {
        e.iter().map(|t| Message::block(kind, t).model()).collect()
    })
}

fn saved_model(m: &Messages, saved: Option<&String>, section: &str) -> Option<MessageModel> {
    saved
        .is_some_and(|s| s == section)
        .then(|| Message::block(MessageKind::Success, &m.get("ts-settings-saved")).model())
}

fn submit_model(label: &str) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action: Action::Progressive,
        weight: Weight::Primary,
    }
    .model()
}

fn section_errors(back: &Returned<'_>, section: &str) -> Vec<MessageModel> {
    if back.section == section {
        messages(MessageKind::Error, back.errors.get(""))
    } else {
        Vec::new()
    }
}

/// A section's form, as `form_open` and `form_close` draw it.
fn form_model(
    m: &Messages,
    scope: Scope,
    section: &str,
    loaded: &Loaded,
    keys: &[&str],
    back: &Returned<'_>,
) -> SectionForm {
    let summary = format!("ts-summary-{section}");
    SectionForm {
        action: format!("/w/index.php?title={}", title_url(scope.title())),
        section: section.to_string(),
        base: loaded.settings.base,
        token: loaded.token.clone(),
        keys: keys.join(" "),
        summary: Field {
            id: &summary,
            name: "summary",
            kind: "text",
            value: back.text(section, "summary").unwrap_or_default(),
            label: &m.get("ts-settings-summary"),
            description: None,
            help: None,
            required: false,
            autocomplete: None,
        }
        .model(),
        save: submit_model(&m.get("ts-settings-save")),
    }
}

fn origin_chips(m: &Messages, scope: Scope, r: &Resolved) -> Vec<ChipModel> {
    let (class, text) = match (r.origin.as_str(), scope) {
        ("locked", _) => ("locked", m.get("ts-settings-origin-locked")),
        ("tenant", _) => ("own", m.get("ts-settings-origin-tenant")),
        ("instance", Scope::Wiki) => ("inherited", m.get("ts-settings-origin-instance")),
        ("instance", Scope::Instance) => ("own", m.get("ts-settings-origin-instance-own")),
        _ => ("default", m.get("ts-settings-origin-registry")),
    };
    let mut chips = vec![origin(class, text)];
    if let Some(c) = &r.capped {
        chips.push(origin(
            "capped",
            m.with(
                "ts-settings-origin-capped",
                &[&controls::as_text(Kind::Line, c)],
            ),
        ));
    }
    chips
}

fn origin(class: &str, text: String) -> ChipModel {
    ChipModel {
        classes: format!("ts-chip ts-setting__origin ts-setting__origin--{class}"),
        text,
        title: None,
    }
}

fn reset_model(m: &Messages, value: &str, label: &str) -> ResetModel {
    ResetModel {
        value: value.to_string(),
        aria_label: m.with("ts-settings-reset-label", &[label]),
        label: m.get("ts-settings-reset"),
    }
}

/// One setting's row.
fn row_model(
    m: &Messages,
    scope: Scope,
    def: &SettingDef,
    r: &Resolved,
    editable: bool,
    back: &Returned<'_>,
) -> SettingRow {
    let id = format!("ts-s-{}", def.key);
    let help_id = format!("{id}-help");
    let kind = Kind::of(&def.ty);
    let name = format!("v.{}", def.key);
    let label_text = label(m, def);
    let help_text = help(m, def);
    let edits = editable && kind != Kind::ReadOnly;
    let (mut control, mut shown, mut reset) = (None, None, None);
    if edits {
        let typed;
        let value = match back.sent(&def.group) {
            Some(f) if kind == Kind::Boolean => {
                typed = Value::Bool(f.contains_key(&name));
                Some(&typed)
            }
            Some(f) if f.contains_key(&name) => {
                typed = Value::String(f[&name].clone());
                Some(&typed)
            }
            _ => r.value.as_ref(),
        };
        let choice_label = |c: &str| {
            let key = format!("{}-{}", def.label, c);
            if m.has(&key) {
                m.get(&key)
            } else {
                c.to_string()
            }
        };
        control = Some(
            Control {
                id: &id,
                name: &name,
                kind,
                choices: &def.choices,
                value,
                empty_choice: (r.value.is_none()).then_some("—"),
                described_by: help_text.as_ref().map(|_| help_id.as_str()),
                choice_label: Some(&choice_label),
            }
            .model(),
        );
        let own = match scope {
            Scope::Wiki => r.origin == "tenant",
            Scope::Instance => r.origin == "instance",
        };
        reset = own.then(|| reset_model(m, &def.key, &label_text));
    } else {
        shown = Some(match &r.value {
            None => absent(m, def),
            Some(v) => controls::shown(m, kind, Some(v)),
        });
    }
    SettingRow {
        row_id: format!("setting-{}", def.key),
        key: def.key.clone(),
        label: label_text,
        label_for: edits.then(|| id.clone()),
        chips: origin_chips(m, scope, r),
        help_id: Some(help_id),
        help: help_text,
        errors: messages(MessageKind::Error, back.errors.get(&def.key)),
        control,
        shown,
        reset,
        lock: None,
        shown_chip: None,
        note: r
            .unresolved_default
            .then(|| m.get("ts-settings-unresolved-default")),
    }
}

/// The keys of a group the viewer may change.
fn editable_keys<'a>(loaded: &'a Loaded, defs: &[&'a SettingDef]) -> BTreeSet<&'a str> {
    defs.iter()
        .filter(|d| {
            loaded.token.is_some()
                && loaded
                    .settings
                    .settings
                    .get(&d.key)
                    .is_some_and(|r| r.can_change)
        })
        .map(|d| d.key.as_str())
        .collect()
}

/// One registry group's section.
fn group_model(
    m: &Messages,
    scope: Scope,
    loaded: &Loaded,
    group: &str,
    group_title: &str,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> SettingsSection {
    let defs: Vec<&SettingDef> = loaded
        .schema
        .settings
        .iter()
        .filter(|d| d.group == group)
        .collect();
    let editable = editable_keys(loaded, &defs);
    let keys: Vec<&str> = editable.iter().copied().collect();
    let rows = defs
        .iter()
        .filter_map(|def| {
            let r = loaded.settings.settings.get(&def.key)?;
            Some(row_model(
                m,
                scope,
                def,
                r,
                editable.contains(def.key.as_str()),
                back,
            ))
        })
        .collect();
    SettingsSection {
        id: group.to_string(),
        title: group_title.to_string(),
        intro: None,
        saved: saved_model(m, saved, group),
        errors: section_errors(back, group),
        form: (!keys.is_empty()).then(|| form_model(m, scope, group, loaded, &keys, back)),
        body: SectionBody::Rows { rows },
    }
}

/// The Visibility section (09 §4.7; 0056 §3; 19 §6.3): who may read the wiki at all.
/// *Anyone*; *Accounts on this wiki* (`user`); *A group*. The tenant ACL behind it needs
/// `ts-config`, which the page's token stands for; without it the section only reports.
fn visibility_model(
    m: &Messages,
    cx: &Context,
    loaded: &Loaded,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> Option<SettingsSection> {
    let (vis, groups) = loaded.visibility.as_ref()?;
    let current = match vis.read_group.as_deref() {
        None => "anyone",
        Some("user") => "user",
        Some(_) => "group",
    };
    let chosen = back.text(VISIBILITY, "visibility").unwrap_or(current);
    let group_now = back
        .text(VISIBILITY, "group")
        .map(str::to_string)
        .or_else(|| vis.read_group.clone().filter(|g| g != "user"))
        .unwrap_or_default();
    let now = m.with(
        "ts-settings-visibility-now",
        &[&match vis.read_group.as_deref() {
            None => m.get("ts-settings-visibility-anyone"),
            Some("user") => m.get("ts-settings-visibility-user"),
            Some(g) => m.with("ts-settings-visibility-a-group", &[g]),
        }],
    );
    let form = loaded.token.as_deref().map(|token| {
        let group_value = Value::String(group_now);
        Box::new(VisibilityForm {
            action: format!("/wiki/{}", title_url(Scope::Wiki.title())),
            token: token.to_string(),
            legend: m.get("ts-settings-visibility-who"),
            options: [
                ("anyone", "ts-settings-visibility-anyone"),
                ("user", "ts-settings-visibility-user"),
                ("group", "ts-settings-visibility-group"),
            ]
            .into_iter()
            .map(|(value, key)| ControlModel::Radio {
                id: Some(format!("ts-vis-{value}")),
                name: "visibility".into(),
                value: value.into(),
                checked: chosen == value,
                label: m.get(key),
                described_by: None,
            })
            .collect(),
            group_label: m.get("ts-settings-visibility-which-group"),
            group: Control {
                id: "ts-vis-group-name",
                name: "group",
                kind: Kind::Choice,
                choices: groups,
                value: Some(&group_value),
                empty_choice: Some(""),
                described_by: Some("ts-vis-group-help"),
                choice_label: None,
            }
            .model(),
            group_help: m.with(
                "ts-settings-visibility-group-help",
                &[&cx.groups.join(", ")],
            ),
            warning: Message::block(
                MessageKind::Warning,
                &m.get("ts-settings-visibility-warning"),
            )
            .model(),
            reason: Field {
                id: "ts-vis-reason",
                name: "reason",
                kind: "text",
                value: back.text(VISIBILITY, "reason").unwrap_or_default(),
                label: &m.get("ts-settings-visibility-reason"),
                description: None,
                help: None,
                required: false,
                autocomplete: None,
            }
            .model(),
            save: submit_model(&m.get("ts-settings-save")),
        })
    });
    Some(SettingsSection {
        id: VISIBILITY.into(),
        title: m.get("ts-settings-group-visibility"),
        intro: None,
        saved: saved_model(m, saved, VISIBILITY),
        errors: section_errors(back, VISIBILITY),
        form: None,
        body: SectionBody::Visibility { now, form },
    })
}

/// What a wiki's Tenancy section says the instance locks and caps.
fn tenancy_facts(m: &Messages, schema: &Schema) -> Vec<Fact> {
    let mut facts = Vec::new();
    facts.push(Fact {
        heading: m.get("ts-settings-tenancy-locked"),
        text: if schema.locked.is_empty() || schema.template != "locks" {
            m.get("ts-settings-tenancy-none-locked")
        } else {
            schema
                .locked
                .iter()
                .map(|k| k.strip_prefix("site:").unwrap_or(k).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        },
    });
    facts.push(Fact {
        heading: m.get("ts-settings-tenancy-ceilings"),
        text: if schema.ceilings.is_empty() {
            m.get("ts-settings-tenancy-no-ceilings")
        } else {
            schema
                .ceilings
                .iter()
                .map(|(k, v)| format!("{k} ≤ {}", value_or_unset(m, Some(v))))
                .collect::<Vec<_>>()
                .join(", ")
        },
    });
    facts
}

/// The tenancy policy: read-only on a wiki, the switches' form at the farm base.
fn tenancy_model(
    m: &Messages,
    scope: Scope,
    loaded: &Loaded,
    back: &Returned<'_>,
    saved: Option<&String>,
) -> SettingsSection {
    let schema = &loaded.schema;
    let editable = scope == Scope::Instance
        && loaded.token.is_some()
        && may_configure(schema, &loaded.settings);
    let switches = schema
        .tenancy
        .iter()
        .map(|(name, sw)| {
            let id = format!("ts-t-{name}");
            let field = format!("t.{name}");
            let label_key = format!("ts-tenancy-{}", name.replace('.', "-"));
            let (mut control, mut allowed, mut shown) = (None, None, None);
            if editable {
                let typed;
                let value = match back.text(TENANCY, &field) {
                    Some(raw) => {
                        typed = Value::String(raw.to_string());
                        Some(&typed)
                    }
                    _ => sw.value.as_ref(),
                };
                let kind = if sw.allowed.choices.is_empty() {
                    Kind::Line
                } else {
                    Kind::Choice
                };
                control = Some(
                    Control {
                        id: &id,
                        name: &field,
                        kind,
                        choices: &sw.allowed.choices,
                        value,
                        empty_choice: sw.value.is_none().then_some("—"),
                        described_by: None,
                        choice_label: None,
                    }
                    .model(),
                );
                allowed.clone_from(&sw.allowed.text);
            } else {
                shown = Some(value_or_unset(m, sw.value.as_ref()));
            }
            SwitchRow {
                row_id: format!("switch-{name}"),
                label: if m.has(&label_key) {
                    m.get(&label_key)
                } else {
                    name.clone()
                },
                label_for: editable.then_some(id),
                key: name.clone(),
                errors: messages(MessageKind::Error, back.errors.get(name)),
                control,
                allowed,
                shown,
            }
        })
        .collect();
    let facts = if scope == Scope::Wiki {
        tenancy_facts(m, schema)
    } else {
        Vec::new()
    };
    let keys: Vec<&str> = schema.tenancy.keys().map(String::as_str).collect();
    SettingsSection {
        id: TENANCY.into(),
        title: m.get("ts-settings-group-tenancy"),
        intro: Some(m.get(match scope {
            Scope::Wiki => "ts-settings-tenancy-wiki",
            Scope::Instance => "ts-settings-tenancy-instance",
        })),
        saved: saved_model(m, saved, TENANCY),
        errors: section_errors(back, TENANCY),
        form: editable.then(|| form_model(m, scope, TENANCY, loaded, &keys, back)),
        body: SectionBody::Switches { switches, facts },
    }
}

/// A default's control: a boolean is a select of on, off and no default, since a
/// checkbox cannot say "none"; other types as their own control.
fn default_control_model(
    m: &Messages,
    def: &SettingDef,
    value: Option<&Value>,
    id: &str,
    name: &str,
) -> ControlModel {
    let kind = Kind::of(&def.ty);
    let no_default = m.get("ts-settings-no-default");
    if kind == Kind::Boolean {
        let choices = vec!["true".to_string(), "false".to_string()];
        let as_choice = value
            .and_then(Value::as_bool)
            .map(|b| Value::String(b.to_string()));
        let label = |c: &str| {
            if c == "true" {
                m.get("ts-settings-on")
            } else {
                m.get("ts-settings-off")
            }
        };
        return Control {
            id,
            name,
            kind: Kind::Choice,
            choices: &choices,
            value: as_choice.as_ref().or(value.filter(|v| v.is_string())),
            empty_choice: Some(&no_default),
            described_by: None,
            choice_label: Some(&label),
        }
        .model();
    }
    Control {
        id,
        name,
        kind,
        choices: &def.choices,
        value,
        empty_choice: (kind == Kind::Choice).then_some(no_default.as_str()),
        described_by: None,
        choice_label: None,
    }
    .model()
}

/// The instance's defaults for wikis, with their locks (0085 §4).
fn defaults_model(
    m: &Messages,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    back: &Returned<'_>,
    saved: Option<&String>,
) -> SettingsSection {
    let schema = &loaded.schema;
    let locks = schema.template == "locks";
    let editable = loaded.token.is_some() && may_configure(schema, &loaded.settings);
    let keys: Vec<&str> = tenant_defs
        .iter()
        .filter(|d| Kind::of(&d.ty) != Kind::ReadOnly)
        .map(|d| d.key.as_str())
        .collect();
    let rows = tenant_defs
        .iter()
        .filter_map(|def| {
            let ts = schema.tenant_settings.get(&def.key)?;
            let kind = Kind::of(&def.ty);
            let id = format!("ts-d-{}", def.key);
            let name = format!("d.{}", def.key);
            let label_text = label(m, def);
            let edits = editable && kind != Kind::ReadOnly;
            let (mut control, mut shown, mut reset, mut lock, mut shown_chip) =
                (None, None, None, None, None);
            if edits {
                let typed;
                let value = match back.text(DEFAULTS, &name) {
                    Some(raw) => {
                        typed = Value::String(raw.to_string());
                        Some(&typed)
                    }
                    _ => ts.instance_default.as_ref(),
                };
                control = Some(default_control_model(m, def, value, &id, &name));
                reset = ts
                    .instance_default
                    .is_some()
                    .then(|| reset_model(m, &format!("d.{}", def.key), &label_text));
                if locks {
                    let checked = match back.sent(DEFAULTS) {
                        Some(f) => f.contains_key(&format!("l.{}", def.key)),
                        None => ts.locked,
                    };
                    lock = Some(ControlModel::Checkbox {
                        id: Some(format!("ts-l-{}", def.key)),
                        name: format!("l.{}", def.key),
                        value: "1".into(),
                        checked,
                        label: Some(m.get("ts-settings-lock")),
                        note: None,
                        disabled: false,
                        described_by: None,
                    });
                }
            } else {
                shown = Some(ts.instance_default.as_ref().map_or_else(
                    || m.get("ts-settings-no-default"),
                    |v| controls::shown(m, kind, Some(v)),
                ));
                shown_chip = ts
                    .locked
                    .then(|| origin("locked", m.get("ts-settings-origin-locked")));
            }
            Some(SettingRow {
                row_id: format!("default-{}", def.key),
                key: def.key.clone(),
                label: label_text,
                label_for: edits.then_some(id),
                chips: Vec::new(),
                help_id: None,
                help: Some(
                    m.with(
                        "ts-settings-registry-default",
                        &[&ts
                            .registry_default
                            .as_ref()
                            .map_or_else(|| absent(m, def), |v| controls::shown(m, kind, Some(v)))],
                    ),
                ),
                errors: messages(MessageKind::Error, back.errors.get(&def.key)),
                control,
                shown,
                reset,
                lock,
                shown_chip,
                note: ts
                    .inheriting
                    .map(|n| m.with("ts-settings-inheriting", &[&n.to_string()])),
            })
        })
        .collect();
    SettingsSection {
        id: DEFAULTS.into(),
        title: m.get("ts-settings-group-defaults"),
        intro: Some(m.get(if locks {
            "ts-settings-defaults-intro-locks"
        } else {
            "ts-settings-defaults-intro"
        })),
        saved: saved_model(m, saved, DEFAULTS),
        errors: section_errors(back, DEFAULTS),
        form: editable.then(|| form_model(m, Scope::Instance, DEFAULTS, loaded, &keys, back)),
        body: SectionBody::Rows { rows },
    }
}

/// The page's model and its section links.
fn page_model(
    cx: &Context,
    scope: Scope,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    back: &Returned<'_>,
    saved: Option<&String>,
) -> (SettingsPage, Vec<Tab>) {
    let m = &cx.m;
    let mut tabs = Vec::new();
    let mut sections = Vec::new();
    let mut tab = |label: String, id: &str| {
        tabs.push(Tab {
            label,
            href: format!("#{id}"),
            current: false,
        });
    };
    for g in &loaded.schema.groups {
        let title = group_label(m, &g.id, &g.label);
        tab(title.clone(), &g.id);
        sections.push(group_model(m, scope, loaded, &g.id, &title, back, saved));
    }
    if let Some(v) = visibility_model(m, cx, loaded, back, saved) {
        tab(m.get("ts-settings-group-visibility"), VISIBILITY);
        sections.push(v);
    }
    tab(m.get("ts-settings-group-tenancy"), TENANCY);
    sections.push(tenancy_model(m, scope, loaded, back, saved));
    if scope == Scope::Instance
        && loaded.schema.template != "none"
        && !loaded.schema.template.is_empty()
    {
        tab(m.get("ts-settings-group-defaults"), DEFAULTS);
        sections.push(defaults_model(m, loaded, tenant_defs, back, saved));
    }
    let read_only = loaded
        .token
        .is_none()
        .then(|| Message::block(MessageKind::Notice, &m.get("ts-settings-read-only")).model());
    (
        SettingsPage {
            read_only,
            sections,
        },
        tabs,
    )
}

/// The tenant settings' definitions, for **Defaults for wikis**: the tenant-scope schema,
/// read at the farm base, where the farm base is also the primary tenant's base.
async fn tenant_defs(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    scope: Scope,
    loaded: &Loaded,
) -> Result<Vec<SettingDef>, ClientError> {
    if scope != Scope::Instance || loaded.schema.template == "none" {
        return Ok(Vec::new());
    }
    let s = site.client().settings_schema(incoming, false).await?;
    cx.inputs.push(s.cache);
    let mut defs = s.value.settings;
    offer_themes(&mut defs, &cx.theme);
    Ok(defs)
}

/// The context, or the response that ends the request.
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    scope: Scope,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    let served = cx.features.has(Feature::Settings) && (scope == Scope::Wiki || cx.site.at_farm);
    if !served {
        return Err(pages::no_page(&cx, headers, scope.title().to_string()));
    }
    Ok(cx)
}

#[allow(clippy::too_many_arguments)]
async fn render_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    scope: Scope,
    back: &Returned<'_>,
    saved: Option<&String>,
    status: StatusCode,
) -> Response {
    let loaded = match load(site, cx, incoming, scope).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let defs = match tenant_defs(site, cx, incoming, scope, &loaded).await {
        Ok(d) => d,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let (model, tabs) = page_model(cx, scope, &loaded, &defs, back, saved);
    let html = cx.draw("special/settings.html", &model);
    let page = pages::respond(cx, headers, status, |m| Page {
        title: m.get(match scope {
            Scope::Wiki => "ts-wikisettings-title",
            Scope::Instance => "ts-instancesettings-title",
        }),
        returnto: Some(scope.title().to_string()),
        tabs,
        body: html,
        ..Page::default()
    });
    pages::with_cookies(page, &loaded.cookies)
}

/// `GET Special:WikiSettings` or `Special:InstanceSettings`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    scope: Scope,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, scope).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    render_page(
        site,
        &mut cx,
        headers,
        &incoming,
        scope,
        &Returned::default(),
        params.map.get("saved"),
        StatusCode::OK,
    )
    .await
}

/// `GET Special:Tenancy`: to the Tenancy section of the page this base serves (0085 §4).
pub async fn tenancy(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Settings) {
        return pages::no_page(&cx, headers, "Special:Tenancy".into());
    }
    let target = if cx.site.at_farm {
        Scope::Instance
    } else {
        Scope::Wiki
    };
    let mut r = pages::redirect_to(&cx, target.title());
    if let Some(loc) = r
        .headers()
        .get(axum::http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        && let Ok(v) = HeaderValue::from_str(&format!("{loc}#{TENANCY}"))
    {
        r.headers_mut().insert(axum::http::header::LOCATION, v);
    }
    r
}

type Errors = BTreeMap<String, Vec<String>>;

/// The tenancy switches the reader changed.
fn tenancy_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
) -> Map<String, Value> {
    let mut tenancy = Map::new();
    for name in keys {
        let Some(sw) = loaded.schema.tenancy.get(*name) else {
            continue;
        };
        let raw = f
            .get(&format!("t.{name}"))
            .map(|s| s.trim())
            .unwrap_or_default();
        if !raw.is_empty() && sw.value.as_ref().and_then(Value::as_str) != Some(raw) {
            tenancy.insert((*name).to_string(), json!(raw));
        }
    }
    tenancy
}

/// A default as posted: a boolean comes as `true`, `false` or nothing.
fn default_value(kind: Kind, raw: Option<&str>) -> Result<Option<Value>, String> {
    match (kind, raw.map(str::trim)) {
        (_, None | Some("")) => Ok(None),
        (Kind::Boolean, Some("true")) => Ok(Some(Value::Bool(true))),
        (Kind::Boolean, Some("false")) => Ok(Some(Value::Bool(false))),
        _ => controls::parse(kind, raw),
    }
}

/// The defaults and locks the reader changed.
fn defaults_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
    errors: &mut Errors,
) -> (Map<String, Value>, Map<String, Value>) {
    let (mut defaults, mut locks) = (Map::new(), Map::new());
    if let Some(key) = f.get("reset").and_then(|r| r.strip_prefix("d.")) {
        defaults.insert(key.to_string(), Value::Null);
        return (defaults, locks);
    }
    for key in keys {
        let Some(def) = tenant_defs.iter().find(|d| d.key == *key) else {
            continue;
        };
        let ts = loaded
            .schema
            .tenant_settings
            .get(*key)
            .cloned()
            .unwrap_or_default();
        let kind = Kind::of(&def.ty);
        match default_value(kind, f.get(&format!("d.{key}")).map(String::as_str)) {
            Err(e) => errors.entry((*key).to_string()).or_default().push(e),
            Ok(None) => {
                if ts.instance_default.is_some() && kind != Kind::Lines {
                    defaults.insert((*key).to_string(), Value::Null);
                }
            }
            Ok(Some(v)) => {
                if ts.instance_default.as_ref() != Some(&v) {
                    defaults.insert((*key).to_string(), v);
                }
            }
        }
        if loaded.schema.template == "locks" {
            let lock = f.contains_key(&format!("l.{key}"));
            if lock != ts.locked {
                locks.insert((*key).to_string(), Value::Bool(lock));
            }
        }
    }
    (defaults, locks)
}

/// The settings of a group section the reader changed.
fn group_patch(
    f: &BTreeMap<String, String>,
    keys: &[&str],
    loaded: &Loaded,
    errors: &mut Errors,
) -> Map<String, Value> {
    let mut set = Map::new();
    for key in keys {
        let Some(def) = loaded.schema.settings.iter().find(|d| d.key == *key) else {
            continue;
        };
        let kind = Kind::of(&def.ty);
        let current = loaded
            .settings
            .settings
            .get(*key)
            .and_then(|r| r.value.clone());
        match controls::parse(kind, f.get(&format!("v.{key}")).map(String::as_str)) {
            Err(e) => errors.entry((*key).to_string()).or_default().push(e),
            Ok(None) => {}
            Ok(Some(v)) => {
                // An absent value compares equal to the empty list, or the unticked box,
                // that stands for it on the page.
                let unchanged = current.as_ref() == Some(&v)
                    || (current.is_none() && (v == json!([]) || v == json!(false)));
                if !unchanged {
                    set.insert((*key).to_string(), v);
                }
            }
        }
    }
    set
}

/// The body of a section's save, and the reasons the form itself is not a save.
fn patch_of(
    form: &Params,
    loaded: &Loaded,
    tenant_defs: &[SettingDef],
) -> (Map<String, Value>, Errors) {
    let f = &form.map;
    let section = f.get("section").map_or("", String::as_str);
    let keys: Vec<&str> = f
        .get("keys")
        .map_or_else(Vec::new, |k| k.split_whitespace().collect());
    let mut errors = Errors::new();
    let mut body = Map::new();
    if let Some(t) = f.get("token") {
        body.insert("token".into(), json!(t));
    }
    if let Some(b) = f.get("base").and_then(|b| b.parse::<u64>().ok()) {
        body.insert("base".into(), json!(b));
    }
    if let Some(s) = f.get("summary").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        body.insert("summary".into(), json!(s));
    }
    let mut put = |name: &str, m: Map<String, Value>| {
        if !m.is_empty() {
            body.insert(name.into(), Value::Object(m));
        }
    };
    match section {
        TENANCY => put("tenancy", tenancy_patch(f, &keys, loaded)),
        DEFAULTS => {
            let (defaults, locks) = defaults_patch(f, &keys, loaded, tenant_defs, &mut errors);
            put("defaults", defaults);
            put("locks", locks);
        }
        _ => {
            if let Some(key) = f.get("reset") {
                body.insert("unset".into(), json!([key]));
            } else {
                put("set", group_patch(f, &keys, loaded, &mut errors));
            }
        }
    }
    (body, errors)
}

/// `POST Special:WikiSettings` or `Special:InstanceSettings`: saves one section.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    scope: Scope,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, scope).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let section = form.map.get("section").cloned().unwrap_or_default();
    let loaded = match load(site, &mut cx, &incoming, scope).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    if section == VISIBILITY && scope == Scope::Wiki {
        return post_visibility(site, &mut cx, headers, &incoming, loaded, form).await;
    }
    let defs = match tenant_defs(site, &mut cx, &incoming, scope, &loaded).await {
        Ok(d) => d,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let (body, errors) = patch_of(form, &loaded, &defs);
    let back_to = |saved: bool| {
        format!(
            "/wiki/{}{}#{}",
            title_url(scope.title()),
            if saved {
                format!("?saved={section}")
            } else {
                String::new()
            },
            section
        )
    };
    if !errors.is_empty() {
        let back = Returned {
            section: section.clone(),
            form: Some(form),
            errors,
        };
        return render_page(
            site,
            &mut cx,
            headers,
            &incoming,
            scope,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let changes = ["set", "unset", "defaults", "locks", "tenancy"];
    if !changes.iter().any(|k| body.contains_key(*k)) {
        return pages::see_other(&back_to(false), &loaded.cookies);
    }
    match site
        .client()
        .save_settings(&incoming, scope.instance(), &Value::Object(body))
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to(true), &loaded.cookies),
        Ok(Saved::Refused(refusals)) => {
            let back = Returned {
                section: section.clone(),
                form: Some(form),
                errors: refusal_errors(&cx.m, &refusals),
            };
            render_page(
                site,
                &mut cx,
                headers,
                &incoming,
                scope,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// The Visibility form: `PUT` or `DELETE /acl/tenant/{slug}` (09 §4.7).
async fn post_visibility(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    loaded: Loaded,
    form: &Params,
) -> Response {
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let group = match field("visibility") {
        "user" => Some("user"),
        "group" => Some(field("group").trim()).filter(|g| !g.is_empty()),
        _ => None,
    };
    if field("visibility") == "group" && group.is_none() {
        let back = Returned {
            section: VISIBILITY.into(),
            form: Some(form),
            errors: BTreeMap::from([(
                String::new(),
                vec![cx.m.get("ts-settings-visibility-no-group")],
            )]),
        };
        return render_page(
            site,
            cx,
            headers,
            incoming,
            Scope::Wiki,
            &back,
            None,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }
    let back_to = format!(
        "/wiki/{}?saved={VISIBILITY}#{VISIBILITY}",
        title_url(Scope::Wiki.title())
    );
    match site
        .client()
        .set_tenant_visibility(
            incoming,
            &cx.site.tenant,
            group,
            field("reason").trim(),
            field("token"),
        )
        .await
    {
        Ok(Saved::Ok(_)) => pages::see_other(&back_to, &loaded.cookies),
        Ok(Saved::Refused(refusals)) => {
            let errors = refusals
                .iter()
                .map(|r| match r.code.as_str() {
                    "ts-locked-out" => cx.m.get("ts-protect-locked-out"),
                    "ts-policy" => cx.m.get("ts-protect-policy"),
                    "ts-unknown-group" => cx.m.get("ts-protect-unknown-group"),
                    "badtoken" => cx.m.get("ts-login-expired"),
                    "permissiondenied" => cx.m.get("ts-settings-visibility-denied"),
                    _ => r.message.clone(),
                })
                .collect();
            let back = Returned {
                section: VISIBILITY.into(),
                form: Some(form),
                errors: BTreeMap::from([(String::new(), errors)]),
            };
            render_page(
                site,
                cx,
                headers,
                incoming,
                Scope::Wiki,
                &back,
                None,
                StatusCode::BAD_REQUEST,
            )
            .await
        }
        Err(e) => pages::api_failed(cx, headers, &e),
    }
}

/// The API's reasons by key, with the ones the reader can act on in their own words.
fn refusal_errors(m: &Messages, refusals: &[Refusal]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in refusals {
        let text = match r.code.as_str() {
            "editconflict" => m.get("ts-settings-conflict"),
            "badtoken" => m.get("ts-login-expired"),
            "permissiondenied" => m.get("ts-settings-denied"),
            "ts-locked" => m.get("ts-settings-locked"),
            _ => r.message.clone(),
        };
        out.entry(r.key.clone().unwrap_or_default())
            .or_default()
            .push(text);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_setting_and_preference_has_its_messages() {
        let m = Messages::for_language("en");
        let settings: toml::Table =
            toml::from_str(include_str!("../../../../docs/registry/settings.toml")).unwrap();
        for s in settings["setting"].as_array().unwrap() {
            let key = s["key"].as_str().unwrap();
            let label = format!("ts-setting-{}", key.replace('.', "-"));
            assert!(m.has(&label), "{label}");
            assert!(m.has(&format!("{label}-help")), "{label}-help");
        }
        for g in settings["group"].as_array().unwrap() {
            let key = format!("ts-settings-group-{}", g["id"].as_str().unwrap());
            assert!(m.has(&key), "{key}");
        }
        let prefs: toml::Table =
            toml::from_str(include_str!("../../../../docs/registry/preferences.toml")).unwrap();
        for p in prefs["preference"].as_array().unwrap() {
            if p.get("pattern").and_then(toml::Value::as_bool) == Some(true) {
                continue;
            }
            let key = format!("ts-pref-{}", p["key"].as_str().unwrap().replace('.', "-"));
            assert!(m.has(&key), "{key}");
        }
        for g in prefs["group"].as_array().unwrap() {
            let key = format!("ts-prefs-group-{}", g["id"].as_str().unwrap());
            assert!(m.has(&key), "{key}");
        }
    }

    #[test]
    fn markdown_to_plain_text() {
        assert_eq!(
            plain("The logo in [07](07-actors.md) §2 and `general.logo`"),
            "The logo in 07 §2 and general.logo"
        );
        assert_eq!(plain("a [b] c"), "a [b] c");
    }

    #[test]
    fn a_tenancy_save_sends_only_changed_switches() {
        let loaded = Loaded {
            schema: Schema {
                tenancy: BTreeMap::from([
                    (
                        "config.template".to_string(),
                        triplespace_client::settings::Switch {
                            allowed: triplespace_client::settings::Allowed {
                                choices: vec!["none".into(), "defaults".into(), "locks".into()],
                                text: None,
                            },
                            value: Some(json!("none")),
                        },
                    ),
                    (
                        "wikitext.ceiling".to_string(),
                        triplespace_client::settings::Switch {
                            allowed: triplespace_client::settings::Allowed::default(),
                            value: Some(json!("lua")),
                        },
                    ),
                ]),
                ..Schema::default()
            },
            settings: Settings::default(),
            token: Some("t".into()),
            cookies: Vec::new(),
            visibility: None,
        };
        let form = Params::new(vec![
            ("section".into(), TENANCY.into()),
            ("keys".into(), "config.template wikitext.ceiling".into()),
            ("t.config.template".into(), "defaults".into()),
            ("t.wikitext.ceiling".into(), "lua".into()),
            ("token".into(), "t".into()),
        ]);
        let (body, errors) = patch_of(&form, &loaded, &[]);
        assert!(errors.is_empty());
        assert_eq!(body["tenancy"], json!({"config.template": "defaults"}));
        assert_eq!(body["token"], "t");
    }
}
