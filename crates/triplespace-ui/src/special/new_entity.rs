//! `Special:NewItem` and `Special:NewProperty` (0047 §5; architecture 21 §3.1), and their
//! Wikibase aliases `CreateItem` and `CreateProperty`: the targets of the **New** menu
//! (0010 §2), as server-rendered forms that need no JavaScript.
//!
//! - **Parameters** are Wikibase's: `lang`, `label`, `description`, `aliases` (separated
//!   by `|`) and, for a property, `datatype`; the subpage is `label/description`. A query
//!   parameter wins over the subpage, as in Wikibase.
//! - **`statement={property}:{value}`**, repeatable, is Triplespace's: each is shown as a
//!   ticked box and added with the new entity (0029 §3's "Create an item with this DOI").
//!   A value is taken as its data type's value only where that needs no parsing (string
//!   types and entity IDs); a time, quantity, coordinate or monolingual text waits for
//!   `wbparsevalue`, and is listed as left out.
//! - **The write** is one `wbeditentity` with `new=`, through the same handler as the API
//!   (0034 §1.5), so permissions, filters and rate limits are the API's. Success is a `303`
//!   to the new entity's page (post, redirect, get); a refusal shows the form again with
//!   what was typed and the API's reason; an expired token gets a fresh one.
//! - **Who:** a signed-in viewer, since the form carries their CSRF token; `NewProperty`
//!   also needs `property-create` (0016 §2), which the API enforces too.

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::value::{DataType, ValueType};
use serde_json::{Map, Value, json};
use triplespace_client::{CacheInfo, ClientError, Incoming, SiteInfo};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::{Lookup, entity_href};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::{self, Messages};
use crate::moderation::{GateModel, link_to};
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{ControlModel, FieldRow, OptionModel};

/// Which form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `Special:NewItem`.
    Item,
    /// `Special:NewProperty`.
    Property,
}

impl Kind {
    /// The form a special page's name asks for: its own name or Wikibase's alias.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        match name {
            "NewItem" | "CreateItem" => Some(Self::Item),
            "NewProperty" | "CreateProperty" => Some(Self::Property),
            _ => None,
        }
    }

    /// The canonical title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Item => "Special:NewItem",
            Self::Property => "Special:NewProperty",
        }
    }

    /// `wbeditentity`'s `new=`.
    const fn api(self) -> &'static str {
        match self {
            Self::Item => "item",
            Self::Property => "property",
        }
    }

    /// The form's own message: `ts-newitem-{suffix}` or `ts-newproperty-{suffix}`.
    fn key(self, suffix: &str) -> String {
        format!("ts-new{}-{suffix}", self.api())
    }
}

/// The order data types are shown in, by `NewProperty` and `Special:ListDatatypes`: the
/// common ones first. It only orders: which types are offered is what the API registers
/// (`general.wikibase-propertytypes`), and a registered type not named here follows these,
/// by ID.
const DISPLAY_ORDER: &[&str] = &[
    "wikibase-item",
    "wikibase-property",
    "string",
    "external-id",
    "url",
    "quantity",
    "time",
    "globe-coordinate",
    "monolingualtext",
    "commonsMedia",
    "geo-shape",
    "tabular-data",
    "wikibase-domain",
];

/// The data types the repository registers, as the API reports them, in display order:
/// what `NewProperty` offers and `Special:ListDatatypes` lists, so the two agree with each
/// other and with what Wikibase clients read from `siteinfo`.
#[must_use]
pub fn offered(site: &SiteInfo) -> Vec<String> {
    let mut ids: Vec<String> = site
        .property_types
        .iter()
        .map(|(id, _)| id.clone())
        .collect();
    ids.sort_by_key(|id| {
        (
            DISPLAY_ORDER
                .iter()
                .position(|d| d == id)
                .unwrap_or(DISPLAY_ORDER.len()),
            id.clone(),
        )
    });
    ids
}

/// The data type a new property gets when the request names none: the API's own
/// default.
const DEFAULT_DATATYPE: &str = "string";

/// A data type's name in the interface, or its ID where the interface has none.
#[must_use]
pub fn datatype_name(m: &Messages, id: &str) -> String {
    let key = format!("ts-datatype-{}", id.to_ascii_lowercase());
    if m.has(&key) {
        m.get(&key)
    } else {
        id.to_string()
    }
}

/// What the form holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Input {
    lang: String,
    label: String,
    description: String,
    aliases: String,
    datatype: String,
    /// The data types offered ([`offered`]).
    datatypes: Vec<String>,
    statements: Vec<String>,
}

impl Input {
    /// From a request: the parameters, and for a `GET` the subpage (`label/description`).
    fn of(m: &Messages, datatypes: Vec<String>, title: Option<&str>, params: &Params) -> Self {
        let mut parts = title
            .and_then(|t| t.split_once('/'))
            .map(|(_, rest)| rest.splitn(2, '/').map(str::to_string).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter();
        let (sub_label, sub_description) = (parts.next(), parts.next());
        let get = |k: &str| params.map.get(k).map(|v| v.trim().to_string());
        Self {
            lang: get("lang")
                .filter(|l| !l.is_empty())
                .unwrap_or_else(|| m.lang().to_string()),
            label: get("label").or(sub_label).unwrap_or_default(),
            description: get("description").or(sub_description).unwrap_or_default(),
            aliases: get("aliases").unwrap_or_default(),
            datatype: get("datatype")
                .filter(|d| datatypes.contains(d))
                .unwrap_or_else(|| DEFAULT_DATATYPE.to_string()),
            datatypes,
            statements: params
                .all("statement")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        }
    }

    /// The aliases, split on `|`, trimmed, without empty ones or repeats.
    fn alias_list(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for a in self.aliases.split('|').map(str::trim) {
            if !a.is_empty() && !out.iter().any(|x| x == a) {
                out.push(a.to_string());
            }
        }
        out
    }
}

/// A `statement=` parameter, read.
#[derive(Debug, Clone, PartialEq)]
struct Prefill {
    /// The parameter as given.
    raw: String,
    /// The property, its label, and the value as typed.
    property: String,
    label: Option<String>,
    value: String,
    /// The statement's Wikibase JSON, or why it is left out (a message, expanded).
    statement: Result<Value, String>,
}

/// The entity type an ID's form implies, where it is an ID and implies one.
fn implied_type(id: &str) -> Option<String> {
    EntityId::parse(id)
        .ok()?
        .implied_entity_type()
        .map(str::to_string)
}

/// The `datavalue` for `value` of a property of type `datatype`, or why there is none.
fn datavalue(m: &Messages, datatype: &str, value: &str, raw: &str) -> Result<Value, String> {
    let dt = DataType::parse(datatype);
    match dt.value_type() {
        Some(ValueType::String) => Ok(json!({"type": "string", "value": value})),
        Some(ValueType::EntityId) => {
            let want = dt.entity_type().unwrap_or("item");
            // A keyed type's value may be given without its prefix: `example.org` for
            // `domain:example.org` (0009 §2).
            let keyed = matches!(want, "domain" | "keyword" | "notation")
                .then(|| format!("{want}:{value}"));
            let id = std::iter::once(value)
                .chain(keyed.as_deref())
                .find(|c| implied_type(c).as_deref() == Some(want))
                .ok_or_else(|| {
                    m.with(
                        "ts-new-statement-bad-value",
                        &[value, &datatype_name(m, datatype)],
                    )
                })?;
            Ok(json!({"type": "wikibase-entityid", "value": {"entity-type": want, "id": id}}))
        }
        _ => Err(m.with(
            "ts-new-statement-unsupported",
            &[&datatype_name(m, datatype), raw],
        )),
    }
}

/// Reads the `statement=` parameters, with each property's label and data type from the
/// API.
async fn prefills(
    site: &Site,
    cx: &mut Context,
    incoming: &Incoming,
    raw: &[String],
) -> Result<Vec<Prefill>, ClientError> {
    let parsed: Vec<(String, Option<(String, String)>)> = raw
        .iter()
        .map(|r| {
            let pv = r.split_once(':').and_then(|(p, v)| {
                let (p, v) = (p.trim(), v.trim());
                (implied_type(p).as_deref() == Some("property") && !v.is_empty())
                    .then(|| (p.to_string(), v.to_string()))
            });
            (r.clone(), pv)
        })
        .collect();
    let mut ids: Vec<String> = parsed
        .iter()
        .filter_map(|(_, pv)| pv.as_ref().map(|(p, _)| p.clone()))
        .collect();
    ids.sort();
    ids.dedup();
    let lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        let languages = cx.m.term_chain().join("|");
        let f = site
            .client()
            .entities(incoming, &ids, "labels|datatype", &languages)
            .await?;
        cx.inputs.push(f.cache);
        Lookup::from_entities(&f.value.found, &cx.m.term_chain())
    };
    let m = &cx.m;
    Ok(parsed
        .into_iter()
        .map(|(raw, pv)| {
            let Some((property, value)) = pv else {
                return Prefill {
                    statement: Err(m.with("ts-new-statement-malformed", &[&raw])),
                    raw,
                    property: String::new(),
                    label: None,
                    value: String::new(),
                };
            };
            let statement = match lookup.datatype(&property) {
                None => Err(m.with("ts-new-statement-no-property", &[&property, &raw])),
                Some(dt) => datavalue(m, dt, &value, &raw).map(|dv| {
                    json!({
                        "mainsnak": {"snaktype": "value", "property": property, "datavalue": dv},
                        "type": "statement",
                        "rank": "normal",
                    })
                }),
            };
            Prefill {
                label: lookup.label(&property).map(|(t, _)| t.to_string()),
                raw,
                property,
                value,
                statement,
            }
        })
        .collect())
}

/// `wbeditentity`'s `data` for the form.
fn edit_data(kind: Kind, input: &Input, prefills: &[Prefill]) -> Value {
    let mut data = Map::new();
    let lang = input.lang.as_str();
    if !input.label.is_empty() {
        data.insert(
            "labels".into(),
            json!({lang: {"language": lang, "value": input.label}}),
        );
    }
    if !input.description.is_empty() {
        data.insert(
            "descriptions".into(),
            json!({lang: {"language": lang, "value": input.description}}),
        );
    }
    let aliases = input.alias_list();
    if !aliases.is_empty() {
        let list: Vec<Value> = aliases
            .iter()
            .map(|a| json!({"language": lang, "value": a}))
            .collect();
        data.insert("aliases".into(), json!({lang: list}));
    }
    let claims: Vec<Value> = prefills
        .iter()
        .filter_map(|p| p.statement.as_ref().ok().cloned())
        .collect();
    if !claims.is_empty() {
        data.insert("claims".into(), Value::Array(claims));
    }
    if kind == Kind::Property {
        data.insert("datatype".into(), json!(input.datatype));
    }
    Value::Object(data)
}

/// `Special:NewItem` or `Special:NewProperty` as Plain's `special/newentity.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NewEntityPage {
    /// For a reader, the way to log in; for a user without the right, why not.
    pub gate: Option<GateModel>,
    /// The form.
    pub form: Option<NewEntityForm>,
}

/// The form.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NewEntityForm {
    /// Notices above it: a refusal, an expired token.
    pub notices: Vec<MessageModel>,
    /// What the form makes.
    pub intro: String,
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// Language, label, description, aliases.
    pub fields: Vec<FieldModel>,
    /// A property's data type.
    pub datatype: Option<FieldRow>,
    /// The prefilled statements that can be added.
    pub statements: Option<StatementBoxes>,
    /// Those that cannot, and why.
    pub left_out: Vec<MessageModel>,
    /// **Create**.
    pub submit: ButtonModel,
}

/// The prefilled statements, each a ticked box.
#[derive(Debug, Clone, serde::Serialize)]
pub struct StatementBoxes {
    /// "Statements".
    pub legend: String,
    /// What ticking does.
    pub help: String,
    /// One box each.
    pub boxes: Vec<ControlModel>,
}

fn field_model(m: &Messages, name: &str, value: &str, key: &str, help: Option<&str>) -> FieldModel {
    FieldModel {
        id: format!("ts-new-{name}"),
        name: name.into(),
        kind: "text".into(),
        value: value.into(),
        label: m.get(key),
        description: None,
        help: help.map(|k| m.get(k)),
        required: false,
        autocomplete: Some("off".into()),
    }
}

fn form_model(
    m: &Messages,
    kind: Kind,
    token: &str,
    input: &Input,
    prefills: &[Prefill],
    notices: &[(MessageKind, String)],
) -> NewEntityForm {
    let mut boxes = Vec::new();
    let mut left_out = Vec::new();
    for (i, p) in prefills.iter().enumerate() {
        match &p.statement {
            Ok(_) => {
                let property = p.label.as_ref().map_or_else(
                    || p.property.clone(),
                    |l| m.with("ts-new-statement-property", &[l, &p.property]),
                );
                boxes.push(ControlModel::Checkbox {
                    id: Some(format!("ts-new-statement-{i}")),
                    name: "statement".into(),
                    value: p.raw.clone(),
                    checked: true,
                    label: Some(m.with("ts-new-statement", &[&property, &p.value])),
                    note: None,
                    disabled: false,
                    described_by: None,
                });
            }
            Err(why) => left_out.push(Message::block(MessageKind::Warning, why).model()),
        }
    }
    NewEntityForm {
        notices: notices
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        intro: m.get(&kind.key("intro")),
        action: format!("/w/index.php?title={}", title_url(kind.title())),
        token: token.to_string(),
        fields: vec![
            field_model(
                m,
                "lang",
                &input.lang,
                "ts-new-lang",
                Some("ts-new-lang-help"),
            ),
            field_model(m, "label", &input.label, "ts-new-label", None),
            field_model(
                m,
                "description",
                &input.description,
                "ts-new-description",
                None,
            ),
            field_model(
                m,
                "aliases",
                &input.aliases,
                "ts-new-aliases",
                Some("ts-new-aliases-help"),
            ),
        ],
        datatype: (kind == Kind::Property).then(|| FieldRow {
            id: "ts-new-datatype".into(),
            label: m.get("ts-new-datatype"),
            class: None,
            control: ControlModel::Select {
                id: Some("ts-new-datatype".into()),
                name: "datatype".into(),
                options: input
                    .datatypes
                    .iter()
                    .map(|dt| OptionModel {
                        value: dt.clone(),
                        label: datatype_name(m, dt),
                        selected: *dt == input.datatype,
                    })
                    .collect(),
                described_by: None,
            },
            help: None,
            help_id: None,
        }),
        statements: (!boxes.is_empty()).then(|| StatementBoxes {
            legend: m.get("ts-new-statements"),
            help: m.get("ts-new-statements-help"),
            boxes,
        }),
        left_out,
        submit: Button {
            label: &m.get(&kind.key("submit")),
            kind: "submit",
            action: Action::Progressive,
            weight: Weight::Primary,
        }
        .model(),
    }
}

/// The page, as the skin draws it.
fn page(
    cx: &Context,
    headers: &HeaderMap,
    kind: Kind,
    status: StatusCode,
    model: &NewEntityPage,
) -> Response {
    let body = cx.draw("special/newentity.html", model);
    pages::respond(cx, headers, status, |m| Page {
        title: m.get(&kind.key("title")),
        returnto: Some(kind.title().to_string()),
        body,
        ..Page::default()
    })
}

/// The form page, with a fresh CSRF token and any notices above it.
async fn form_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    kind: Kind,
    input: &Input,
    notices: &[(MessageKind, String)],
) -> Response {
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    // A page holding the viewer's token is theirs alone.
    cx.inputs.push(token.value.cache);
    cx.inputs.push(CacheInfo::default());
    let prefills = match prefills(site, cx, incoming, &input.statements).await {
        Ok(p) => p,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    let model = NewEntityPage {
        gate: None,
        form: Some(form_model(
            &cx.m,
            kind,
            &token.value.value,
            input,
            &prefills,
            notices,
        )),
    };
    pages::with_cookies(
        page(cx, headers, kind, StatusCode::OK, &model),
        &token.set_cookies,
    )
}

/// The context, or the response that ends the request: the API away; creating not
/// offered (`404`); a viewer not signed in (asked to); or, for a property, one without
/// `property-create` (`403`).
async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    kind: Kind,
    query: &std::collections::BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Create) {
        return Err(pages::no_page(&cx, headers, kind.title().to_string()));
    }
    if cx.user.is_none() {
        let login_href = format!(
            "/w/index.php?title=Special:UserLogin&returnto={}",
            title_url(kind.title())
        );
        let model = NewEntityPage {
            gate: Some(GateModel {
                notice: Message::block(MessageKind::Notice, &cx.m.get("ts-new-login")).model(),
                login: cx
                    .features
                    .has(Feature::Login)
                    .then(|| link_to(login_href, cx.m.get("ts-login-title"))),
            }),
            form: None,
        };
        return Err(page(&cx, headers, kind, StatusCode::OK, &model));
    }
    if kind == Kind::Property {
        match site.client().rights(incoming).await {
            Ok(r) if r.iter().any(|x| x == "property-create") => {}
            Ok(_) => {
                let text = cx.m.get("ts-newproperty-denied");
                let model = NewEntityPage {
                    gate: Some(GateModel::notice(MessageKind::Error, &text)),
                    form: None,
                };
                return Err(page(&cx, headers, kind, StatusCode::FORBIDDEN, &model));
            }
            Err(e) => return Err(pages::api_failed(&cx, headers, &e)),
        }
    }
    Ok(cx)
}

/// `GET Special:NewItem` and `Special:NewProperty`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    kind: Kind,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, kind, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let input = Input::of(&cx.m, offered(&cx.site), Some(title), params);
    form_page(site, &mut cx, headers, &incoming, kind, &input, &[]).await
}

/// What is wrong with the form as sent, before asking the API.
fn invalid(m: &Messages, kind: Kind, input: &Input) -> Option<String> {
    if i18n::valid_code(&input.lang).is_none() {
        return Some(m.with("ts-new-bad-lang", &[&input.lang]));
    }
    if input.label.is_empty() && input.description.is_empty() && input.alias_list().is_empty() {
        return Some(m.get("ts-new-insufficient"));
    }
    if kind == Kind::Property && !input.datatypes.contains(&input.datatype) {
        return Some(m.get("ts-new-bad-datatype"));
    }
    None
}

/// `POST Special:NewItem` and `Special:NewProperty`: creates the entity.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    kind: Kind,
    query: &std::collections::BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, kind, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let mut input = Input::of(&cx.m, offered(&cx.site), None, form);
    if let Some(l) = i18n::valid_code(&input.lang) {
        input.lang = l;
    }
    if let Some(why) = invalid(&cx.m, kind, &input) {
        let notices = [(MessageKind::Error, why)];
        return form_page(site, &mut cx, headers, &incoming, kind, &input, &notices).await;
    }
    let prefills = match prefills(site, &mut cx, &incoming, &input.statements).await {
        Ok(p) => p,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    if let Some(Err(why)) = prefills.iter().map(|p| &p.statement).find(|s| s.is_err()) {
        let notices = [(MessageKind::Error, why.clone())];
        return form_page(site, &mut cx, headers, &incoming, kind, &input, &notices).await;
    }
    let token = form.map.get("token").map_or("", String::as_str);
    let data = edit_data(kind, &input, &prefills);
    match site
        .client()
        .create_entity(&incoming, kind.api(), &data, token)
        .await
    {
        Ok(c) => pages::see_other(&entity_href(&c.id, &c.entity_type), &[]),
        Err(ClientError::Api { code, info }) => {
            let text = match code.as_str() {
                "badtoken" => {
                    let notices = [(MessageKind::Warning, cx.m.get("ts-new-expired"))];
                    return form_page(site, &mut cx, headers, &incoming, kind, &input, &notices)
                        .await;
                }
                "permissiondenied" if kind == Kind::Property => cx.m.get("ts-newproperty-denied"),
                "permissiondenied" => cx.m.get("ts-permission-denied"),
                _ => info,
            };
            let notices = [(MessageKind::Error, cx.m.with("ts-new-failed", &[&text]))];
            form_page(site, &mut cx, headers, &incoming, kind, &input, &notices).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn names_and_aliases() {
        assert_eq!(Kind::of("NewItem"), Some(Kind::Item));
        assert_eq!(Kind::of("CreateItem"), Some(Kind::Item));
        assert_eq!(Kind::of("NewProperty"), Some(Kind::Property));
        assert_eq!(Kind::of("CreateProperty"), Some(Kind::Property));
        assert_eq!(Kind::of("NewLexeme"), None);
    }

    #[test]
    fn input_from_subpage_and_parameters() {
        let m = Messages::for_language("en");
        let types: Vec<String> = ["string", "time"]
            .iter()
            .map(|t| (*t).to_string())
            .collect();
        let i = Input::of(
            &m,
            types.clone(),
            Some("Special:NewItem/Douglas Adams/English writer/x"),
            &params(&[
                ("description", " author "),
                ("statement", "P12:a"),
                ("statement", "P13:b"),
                ("statement", " "),
            ]),
        );
        assert_eq!(i.lang, "en");
        assert_eq!(i.label, "Douglas Adams", "the subpage gives the label");
        assert_eq!(i.description, "author", "a parameter wins over the subpage");
        assert_eq!(i.statements, vec!["P12:a", "P13:b"]);
        assert_eq!(i.datatype, "string");
        let p = Input::of(
            &m,
            types.clone(),
            None,
            &params(&[
                ("datatype", "time"),
                ("aliases", "a| b ||a|c"),
                ("lang", "de"),
            ]),
        );
        assert_eq!(p.datatype, "time");
        assert_eq!(p.lang, "de");
        assert_eq!(p.alias_list(), vec!["a", "b", "c"]);
        let bad = Input::of(&m, types, None, &params(&[("datatype", "nonsense")]));
        assert_eq!(bad.datatype, "string");
    }

    #[test]
    fn values_that_need_no_parsing() {
        let m = Messages::for_language("en");
        assert_eq!(
            datavalue(&m, "external-id", "10.1000/182", "P5:10.1000/182").unwrap(),
            json!({"type": "string", "value": "10.1000/182"})
        );
        assert_eq!(
            datavalue(&m, "wikibase-item", "Q2", "P1:Q2").unwrap(),
            json!({"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "Q2"}})
        );
        assert!(datavalue(&m, "wikibase-item", "P2", "P1:P2").is_err());
        assert!(datavalue(&m, "wikibase-item", "nonsense", "P1:nonsense").is_err());
        assert_eq!(
            datavalue(&m, "wikibase-domain", "example.org", "P4:example.org").unwrap()["value"]["id"],
            json!("domain:example.org")
        );
        let why = datavalue(&m, "time", "1952", "P3:1952").unwrap_err();
        assert!(why.contains("Point in time"), "{why}");
    }

    #[test]
    fn the_data_sent() {
        let input = Input {
            lang: "en".into(),
            label: "Seven".into(),
            description: String::new(),
            aliases: "7|VII".into(),
            datatype: "external-id".into(),
            datatypes: vec!["external-id".into()],
            statements: vec![],
        };
        let statement = json!({"mainsnak": {"snaktype": "value", "property": "P12"}});
        let prefills = [
            Prefill {
                raw: "P12:x".into(),
                property: "P12".into(),
                label: None,
                value: "x".into(),
                statement: Ok(statement.clone()),
            },
            Prefill {
                raw: "P99:x".into(),
                property: "P99".into(),
                label: None,
                value: "x".into(),
                statement: Err("no".into()),
            },
        ];
        assert_eq!(
            edit_data(Kind::Item, &input, &prefills),
            json!({
                "labels": {"en": {"language": "en", "value": "Seven"}},
                "aliases": {"en": [{"language": "en", "value": "7"}, {"language": "en", "value": "VII"}]},
                "claims": [statement],
            })
        );
        assert_eq!(
            edit_data(Kind::Property, &input, &[])["datatype"],
            json!("external-id")
        );
    }

    #[test]
    fn what_is_refused_before_the_api() {
        let m = Messages::for_language("en");
        let mut input = Input {
            lang: "en".into(),
            datatype: "string".into(),
            ..Input::default()
        };
        assert!(
            invalid(&m, Kind::Item, &input).is_some(),
            "nothing to create"
        );
        input.aliases = "x".into();
        assert!(invalid(&m, Kind::Item, &input).is_none());
        input.lang = "not a language".into();
        assert!(invalid(&m, Kind::Item, &input).is_some());
        // A property's data type must be one the API registers.
        input.lang = "en".into();
        input.datatypes = vec!["string".into()];
        assert!(invalid(&m, Kind::Property, &input).is_none());
        input.datatype = "math".into();
        assert!(invalid(&m, Kind::Property, &input).is_some());
    }

    #[test]
    fn the_api_decides_which_types_are_offered() {
        let site = SiteInfo {
            property_types: [
                ("string", "string"),
                ("edtf", "string"),
                ("wikibase-item", "wikibase-entityid"),
                ("commonsMedia", "string"),
            ]
            .iter()
            .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
            .collect(),
            ..SiteInfo::default()
        };
        // Display order first; a type it does not name after, by ID.
        assert_eq!(
            offered(&site),
            ["wikibase-item", "string", "commonsMedia", "edtf"]
        );
        assert!(offered(&SiteInfo::default()).is_empty());
        let m = Messages::for_language("en");
        assert_eq!(datatype_name(&m, "wikibase-item"), "Item");
        assert_eq!(datatype_name(&m, "edtf"), "edtf");
    }
}
