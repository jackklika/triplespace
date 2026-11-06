//! Entity pages (0010 §2; 0003): an item, property, foreign entity or keyed entity
//! (`Domain:wikipedia.org`), read from the API in the viewer's name.
//!
//! A page is made of regions, and `action=render` serves each region alone (0057 §8):
//!
//! | Region | Content |
//! |---|---|
//! | `terms` | The description and aliases under the title |
//! | `statements/{P}` | One statement group, also for a property the entity does not use |
//! | `statements` | The Statements tab: its groups, its references and "Where this comes from" |
//! | `identifiers` | The Identifiers tab's groups and references |
//! | `sitelinks` | The Sitelinks tab, grouped by host |
//! | (none) | The Statements tab, as `statements` |
//!
//! A group's footnote numbers are the tab's (`notes`), so `statements/{P}` numbers them as
//! the whole tab would; the editor swaps in the whole tab after a save, so that a new
//! reference renumbers every group after it.
//!
//! Each statement group takes the shape `scatter-wikibase-shape` gives it (0003 §3):
//! Single, Timeline, Series, Table (or a numbered list, or a Matrix), Chips or List, with
//! best values leading, other and deprecated values folded, shared qualifiers stated once
//! and references as endnotes, each numbered once for the tab (§4–5). `shapes` draws them; `chart` draws the
//! Series chart and the Timeline axis as SVG.
//!
//! One page costs three API calls in parallel with the frame's (`wbgetentities` for the
//! entity, its provenance, and `siteinfo`), then one `wbgetentities` per 50 entities it
//! refers to, for their labels and data types.

mod chart;
pub mod diff;
pub mod history;
pub mod model;
mod notes;
mod values;
mod view;

use std::collections::{BTreeMap, BTreeSet};

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::statement::{Snak, SnakKind};
use scatter_wikibase_model::value::DataValue;
use serde_json::Value;
use triplespace_client::{CacheInfo, ClientError, Provenance, SiteInfo};

use scatter_wikibase_shape::Roles;

pub use notes::Endnotes;

use crate::Site;
use crate::codex::MessageKind;
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, ModerationTabs, Peer};

/// The namespaces whose pages are entities, by prefix (`namespaces.toml`: 120, 122,
/// 210, 212, 216).
const ENTITY_NAMESPACES: &[&str] = &["Item:", "Property:", "Domain:", "Keyword:", "Notation:"];

/// Whether a title is an entity page's.
#[must_use]
pub fn is_entity_title(title: &str) -> bool {
    ENTITY_NAMESPACES
        .iter()
        .any(|p| title.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// The entity type an ID's shape implies; `item` where it implies none.
#[must_use]
pub fn entity_type_of(id: &str) -> String {
    EntityId::parse(id)
        .ok()
        .and_then(|e| e.implied_entity_type().map(str::to_string))
        .unwrap_or_else(|| "item".to_string())
}

/// The page title of an entity (0017 §1; `namespaces.toml`).
#[must_use]
pub fn page_title(id: &str, entity_type: &str) -> String {
    // A keyed entity's namespace is its type's; a source's entity (`mhc:Q1`, 0078 §2)
    // is in its entity type's namespace, as a foreign one is.
    if let Some((t, key)) = id.split_once(':') {
        match t {
            "domain" => return format!("Domain:{key}"),
            "keyword" => return format!("Keyword:{key}"),
            "notation" => return format!("Notation:{key}"),
            _ => {}
        }
    }
    match entity_type {
        "property" => format!("Property:{id}"),
        _ => format!("Item:{id}"),
    }
}

/// The URL of an entity's page.
#[must_use]
pub fn entity_href(id: &str, entity_type: &str) -> String {
    format!("/wiki/{}", title_url(&page_title(id, entity_type)))
}

/// The local or foreign ID a concept IRI names: the tenant's own concept base, or a
/// provider's IRI template (`http://www.wikidata.org/entity/{upstream_id}` → `WDQ…`).
#[must_use]
pub fn unit_id(iri: &str, site: &SiteInfo) -> Option<String> {
    if !site.concept_base.is_empty()
        && let Some(id) = iri.strip_prefix(&site.concept_base)
        && EntityId::parse(id).is_ok()
    {
        return Some(id.to_string());
    }
    for p in &site.providers {
        for t in &p.types {
            let Some((head, tail)) = t.iri.split_once("{upstream_id}") else {
                continue;
            };
            let Some(upstream) = iri.strip_prefix(head).and_then(|r| r.strip_suffix(tail)) else {
                continue;
            };
            let Some(rest) = upstream.strip_prefix(&t.upstream_prefix) else {
                continue;
            };
            let id = format!("{}{}{rest}", p.code, t.code);
            if !rest.is_empty() && EntityId::parse(&id).is_ok() {
                return Some(id);
            }
        }
    }
    None
}

/// The labels and data types of the entities a page refers to.
#[derive(Debug, Clone, Default)]
pub struct Lookup {
    /// Each entity's label in the reader's term chain, with its language.
    labels: BTreeMap<String, (String, String)>,
    /// Each property's data type.
    datatypes: BTreeMap<String, String>,
}

impl Lookup {
    /// From `wbgetentities` results, choosing each label along `chain`.
    #[must_use]
    pub fn from_entities(found: &BTreeMap<String, Value>, chain: &[String]) -> Self {
        let mut out = Self::default();
        for (id, v) in found {
            if let Some(labels) = v["labels"].as_object()
                && let Some((lang, text)) = chain.iter().find_map(|l| {
                    labels
                        .get(l)
                        .and_then(|t| t["value"].as_str())
                        .map(|t| (l.clone(), t.to_string()))
                })
            {
                out.labels.insert(id.clone(), (text, lang));
            }
            if let Some(dt) = v["datatype"].as_str() {
                out.datatypes.insert(id.clone(), dt.to_string());
            }
        }
        out
    }

    /// Adds the page's own entity, which a statement may name (an item that is an
    /// instance of itself), so it shows with its label rather than its ID.
    pub fn add_self(&mut self, entity: &Entity, chain: &[String]) {
        if let Some((lang, text)) = chain
            .iter()
            .find_map(|l| entity.labels.get(l).map(|t| (l.clone(), t.clone())))
        {
            self.labels
                .insert(entity.id.as_str().to_string(), (text, lang));
        }
    }

    /// An entity's label and its language.
    #[must_use]
    pub fn label(&self, id: &str) -> Option<(&str, &str)> {
        self.labels.get(id).map(|(t, l)| (t.as_str(), l.as_str()))
    }

    /// A property's data type.
    #[must_use]
    pub fn datatype(&self, id: &str) -> Option<&str> {
        self.datatypes.get(id).map(String::as_str)
    }
}

/// What the views render from.
pub struct Render<'a> {
    /// The interface messages.
    pub m: &'a Messages,
    /// The site.
    pub site: &'a SiteInfo,
    /// Labels and data types of what the entity refers to.
    pub lookup: &'a Lookup,
    /// The entity.
    pub entity: &'a Entity,
    /// Where it comes from, if the API said.
    pub provenance: Option<&'a Provenance>,
    /// The role map shape detection reads (0003 §7).
    pub roles: &'a Roles,
    /// Whether the viewer may edit here: signed in, editing offered, and a local entity
    /// (a mirrored one is corrected through overrides, which come later, 0003 §8).
    pub editable: bool,
    /// The references a tab cites, numbered once each across its groups (0003 §5).
    pub notes: Endnotes,
}

/// The role map: Wikidata's roles in their mirrored form for each provider that copies
/// Wikidata's properties (`WDP585`). A local property plays a role only where the
/// instance maps it, which the API does not report yet, so local groups take the shapes
/// that need no role (Table, Chips, List).
#[must_use]
pub fn site_roles(site: &SiteInfo) -> Roles {
    let mut roles = Roles::empty();
    for p in &site.providers {
        if p.slug == "wikidata" {
            roles = roles.with_mirror(&p.code);
        }
    }
    roles
}

/// The IDs a page needs labels for: properties and entity values in statements,
/// qualifiers and references, units and globes, and sitelink badges.
fn referenced(entity: &Entity, site: &SiteInfo) -> Vec<String> {
    let mut ids = BTreeSet::new();
    let snak = |s: &Snak, ids: &mut BTreeSet<String>| {
        ids.insert(s.property.as_str().to_string());
        if let SnakKind::Value(v) = &s.kind {
            match v {
                DataValue::EntityId(e) => {
                    ids.insert(e.id.as_str().to_string());
                }
                DataValue::Quantity(q) if q.unit != "1" => {
                    if let Some(id) = unit_id(&q.unit, site) {
                        ids.insert(id);
                    }
                }
                DataValue::GlobeCoordinate(g) => {
                    if let Some(id) = unit_id(&g.globe, site) {
                        ids.insert(id);
                    }
                }
                _ => {}
            }
        }
    };
    for st in entity.all_statements() {
        for s in st.snaks() {
            snak(s, &mut ids);
        }
    }
    for link in entity.sitelinks.values() {
        for b in &link.badges {
            ids.insert(b.as_str().to_string());
        }
    }
    ids.remove(entity.id.as_str());
    ids.into_iter().collect()
}

/// The tab a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabName {
    Statements,
    Identifiers,
    Sitelinks,
    Labels,
    History,
}

impl TabName {
    /// `?tab=` picks a tab; `?action=history` is the History tab, MediaWiki's address for
    /// a history.
    fn of(query: &BTreeMap<String, String>) -> Self {
        if query.get("action").is_some_and(|a| a == "history") {
            return Self::History;
        }
        match query.get("tab").map(String::as_str) {
            Some("identifiers") => Self::Identifiers,
            Some("sitelinks") => Self::Sitelinks,
            Some("labels") => Self::Labels,
            _ => Self::Statements,
        }
    }
}

fn tabs(m: &Messages, title: &str, current: TabName, items: bool, history: bool) -> Vec<Tab> {
    let base = format!("/wiki/{}", title_url(title));
    let tab = |name: TabName, key: &str, href: String| Tab {
        label: m.get(key),
        href,
        current: name == current,
    };
    let mut v = vec![
        tab(TabName::Statements, "ts-tab-statements", base.clone()),
        tab(
            TabName::Identifiers,
            "ts-tab-identifiers",
            format!("{base}?tab=identifiers"),
        ),
    ];
    // Only items have sitelinks (wikibase-compat §3.1).
    if items {
        v.push(tab(
            TabName::Sitelinks,
            "ts-tab-sitelinks",
            format!("{base}?tab=sitelinks"),
        ));
    }
    v.push(tab(
        TabName::Labels,
        "ts-tab-labels",
        format!("{base}?tab=labels"),
    ));
    // History joins when the API serves it and the site has built it (Feature::History).
    if history {
        v.push(tab(
            TabName::History,
            "ts-tab-history",
            format!("{base}?action=history"),
        ));
    }
    // Links here and Discussion close the row, as on a document page (19 §1.3). An
    // entity's title is canonical (`Item:Q42`), so no site name is needed for its talk.
    v.push(crate::document::links_here_tab(m, title));
    if let Some(talk) = crate::document::talk_tab(m, "", title) {
        v.push(talk);
    }
    v
}

/// The overflow menu (19 §1.3): **Protect…** and **Delete…** for their holders.
fn actions(m: &Messages, title: &str, offered: ModerationTabs) -> Vec<(String, String)> {
    let base = format!("/wiki/{}", title_url(title));
    let mut v = Vec::new();
    if offered.protect {
        v.push((m.get("ts-action-protect"), format!("{base}?action=protect")));
    }
    if offered.delete {
        v.push((m.get("ts-action-delete"), format!("{base}?action=delete")));
    }
    v
}

/// A not-found page for an entity title: the frame, a 404, and the API's reason where it
/// gave one.
fn missing(cx: &Context, headers: &HeaderMap, title: &str) -> Response {
    let title = title.to_string();
    pages::respond(cx, headers, StatusCode::NOT_FOUND, move |m| Page {
        returnto: Some(title.clone()),
        body: cx.message(
            MessageKind::Warning,
            &m.with("ts-entity-missing", &[&title]),
        ),
        title,
        ..Page::default()
    })
}

/// Serves an entity title: the page with a tab, or with `action=render` a region.
#[allow(clippy::too_many_lines)]
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: String,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let client = site.client();
    let one = [title.clone()];
    let (cx, fetched, provenance) = tokio::join!(
        pages::context(site, &incoming, query),
        client.entities(&incoming, &one, "", ""),
        client.provenance(&incoming, &title),
    );
    let mut cx = match cx {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let render = query.get("action").is_some_and(|a| a == "render");
    let fetched = match fetched {
        Ok(f) => f,
        Err(ClientError::Api { code, .. })
            if code == "invalid-entity-id" || code == "no-such-entity" =>
        {
            return if render {
                pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new)
            } else {
                missing(&cx, headers, &title)
            };
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(fetched.cache.clone());
    let visibility = fetched.value.visibility.values().next().cloned();
    let Some((_, mut json)) = fetched.value.found.into_iter().next() else {
        if render {
            return pages::fragment(&cx, headers, StatusCode::NOT_FOUND, String::new);
        }
        // A deleted entity shows its deletion (19 §6.3) before it is a 404.
        if let Some(r) =
            crate::moderation::deleted(site, &mut cx, headers, &incoming, &title, None).await
        {
            return r;
        }
        return missing(&cx, headers, &title);
    };
    let redirected = json
        .as_object_mut()
        .and_then(|m| m.remove("redirects"))
        .is_some();
    let parsed = match Entity::from_value(json) {
        Ok(p) => p,
        Err(e) => {
            return pages::api_failed(
                &cx,
                headers,
                &ClientError::Unexpected(format!("the entity's JSON: {e}")),
            );
        }
    };
    let entity = parsed.entity;
    // The API answers an alias (`Item:XDQ9`, a key-mapped provider's own ID) with the
    // entity it stands for; the page lives at that entity's own title (05 §4.1).
    let own = page_title(entity.id.as_str(), entity.entity_type.name());
    if redirected && own != title {
        return pages::redirect_to_with(&cx, &own, query);
    }
    let provenance = match provenance {
        Ok(p) => {
            cx.inputs.push(p.cache);
            p.value
        }
        Err(e) => {
            eprintln!("triplespace-ui: provenance of {title}: {e}");
            cx.inputs.push(CacheInfo::default());
            None
        }
    };
    // A diff, a comparison or an old revision (19 §3.7–3.8): MediaWiki's `diff=` and
    // `oldid=` on the entity's title.
    if !render
        && cx.features.has(Feature::History)
        && let Some(ask) = diff::Ask::of(query)
    {
        return diff::serve(
            site,
            cx,
            &incoming,
            headers,
            &entity,
            provenance.as_ref(),
            &ask,
            query,
        )
        .await;
    }
    let chain = cx.m.term_chain();
    let ids = referenced(&entity, &cx.site);
    let mut lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        match client
            .entities(&incoming, &ids, "labels|datatype", &chain.join("|"))
            .await
        {
            Ok(f) => {
                cx.inputs.push(f.cache);
                Lookup::from_entities(&f.value.found, &chain)
            }
            Err(e) => {
                eprintln!("triplespace-ui: labels for {title}: {e}");
                cx.inputs.push(CacheInfo::default());
                Lookup::default()
            }
        }
    };
    lookup.add_self(&entity, &chain);
    let history_tab =
        !render && TabName::of(query) == TabName::History && cx.features.has(Feature::History);
    let history = if history_tab {
        let ask = history::Ask::of(query);
        match client
            .history(&incoming, entity.id.as_str(), &ask.query(cx.m.lang()))
            .await
        {
            Ok(f) => {
                cx.inputs.push(f.cache);
                f.value.map(|page| (ask, page))
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    } else {
        None
    };
    let roles = site_roles(&cx.site);
    let editable =
        cx.user.is_some() && cx.features.has(Feature::Edit) && entity.id.form() == IdForm::Local;
    let r = Render {
        m: &cx.m,
        site: &cx.site,
        lookup: &lookup,
        entity: &entity,
        provenance: provenance.as_ref(),
        roles: &roles,
        editable,
        notes: Endnotes::default(),
    };
    if render {
        region(&r, &cx, headers, query)
    } else {
        let watch = crate::watch::control(
            site,
            &cx,
            &incoming,
            &page_title(entity.id.as_str(), entity.entity_type.name()),
        )
        .await;
        page(
            &r,
            &cx,
            headers,
            query,
            history.as_ref(),
            visibility.as_ref(),
            watch,
        )
    }
}

/// A part of the entity page the skin draws (19 §5.10): a template of `entity/` and the
/// model it takes.
pub(super) enum Part {
    Statements,
    Identifiers,
    Terms,
    Sitelinks,
    Labels,
    Identity(Option<model::Restricted>),
    GroupAlone(String),
}

/// What each template of `entity/` takes.
#[derive(serde::Serialize)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
enum Ctx {
    Statements {
        heading: String,
        tab: model::StatementsTab,
    },
    Identifiers {
        tab: model::IdentifiersTab,
    },
    Terms {
        terms: model::Terms,
    },
    Sitelinks {
        sitelinks: model::SitelinksModel,
    },
    Labels {
        table: crate::codex::TableModel,
    },
    Identity {
        identity: model::Identity,
    },
    Group {
        group: model::GroupModel,
    },
}

/// Draws a part of the page through the skin ([`Context::draw`]; group 3 switched on
/// 2026-10-30).
pub(super) fn draw(r: &Render<'_>, cx: &Context, part: &Part) -> String {
    let (template, model) = match part {
        Part::Statements => {
            let tab = r.statements_tab_model();
            (
                "entity/statements.html",
                Ctx::Statements {
                    heading: tab.heading.clone(),
                    tab,
                },
            )
        }
        Part::Identifiers => (
            "entity/identifiers.html",
            Ctx::Identifiers {
                tab: r.identifiers_model(),
            },
        ),
        Part::Terms => (
            "entity/terms.html",
            Ctx::Terms {
                terms: r.terms_model(),
            },
        ),
        Part::Sitelinks => (
            "entity/sitelinks.html",
            Ctx::Sitelinks {
                sitelinks: r.sitelinks_model(),
            },
        ),
        Part::Labels => (
            "entity/labels.html",
            Ctx::Labels {
                table: r.labels_model(),
            },
        ),
        Part::Identity(restricted) => (
            "entity/identity.html",
            Ctx::Identity {
                identity: r.identity_model(restricted.clone()),
            },
        ),
        Part::GroupAlone(p) => (
            "entity/group.html",
            Ctx::Group {
                group: r.group_alone_model(p),
            },
        ),
    };
    cx.draw(template, model)
}

/// `action=render`: one region, or the Statements tab without the frame (0057 §8).
fn region(
    r: &Render<'_>,
    cx: &Context,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
) -> Response {
    if headers
        .get("x-triplespace-ui-build")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|b| b != crate::assets::build_id())
    {
        return pages::build_skew();
    }
    let part = match query.get("region").map(String::as_str) {
        None | Some("statements") => Part::Statements,
        Some("terms") => Part::Terms,
        Some("identifiers") => Part::Identifiers,
        Some("sitelinks") => Part::Sitelinks,
        Some(region) => match region.strip_prefix("statements/") {
            Some(p) if EntityId::parse(p).is_ok() => Part::GroupAlone(p.to_string()),
            _ => return pages::unknown_region(region),
        },
    };
    let html = draw(r, cx, &part);
    pages::fragment(cx, headers, StatusCode::OK, move || html)
}

/// The data the editing components start from (0034 §4): a JSON block beside the
/// content, which the browser reads only when it has script. `</` is escaped so the
/// block cannot end early.
fn edit_data(r: &Render<'_>, cx: &Context, title: &str) -> String {
    let data = serde_json::json!({
        "id": r.entity.id.as_str(),
        "type": r.entity.entity_type.name(),
        "title": title,
        "lang": cx.m.lang(),
        "dir": cx.m.dir(),
        "build": crate::assets::build_id(),
        "conceptBase": cx.site.concept_base,
        "messages": cx.m.with_prefix("ts-edit-"),
    });
    format!(
        "<script type=\"application/json\" id=\"ts-edit-data\">{}</script>",
        data.to_string().replace("</", "<\\/")
    )
}

/// The page in the frame, with the tab the request asks for.
fn page(
    r: &Render<'_>,
    cx: &Context,
    headers: &HeaderMap,
    query: &BTreeMap<String, String>,
    history: Option<&(history::Ask, triplespace_client::ActivityPage)>,
    visibility: Option<&triplespace_client::Visibility>,
    watch: Option<crate::frame::WatchControl>,
) -> Response {
    let entity = r.entity;
    let canonical_title = page_title(entity.id.as_str(), entity.entity_type.name());
    let has_history = cx.features.has(Feature::History);
    let tab = match TabName::of(query) {
        // The address of a feature the site does not offer shows the statements.
        TabName::History if !has_history => TabName::Statements,
        t => t,
    };
    let mut body = match tab {
        // The Protect form is its own page (`action=protect`, `moderation`); the entity
        // page never shows it as current.
        TabName::Statements => draw(r, cx, &Part::Statements),
        TabName::Identifiers => draw(r, cx, &Part::Identifiers),
        TabName::Sitelinks => draw(r, cx, &Part::Sitelinks),
        TabName::Labels => draw(r, cx, &Part::Labels),
        TabName::History => match history {
            Some((ask, page)) => history::body(cx, &canonical_title, ask, r.provenance, page),
            None => cx.message(MessageKind::Warning, &cx.m.get("ts-history-none")),
        },
    };
    if r.editable {
        body.push_str(&edit_data(r, cx, &canonical_title));
    }
    let title_lang = r
        .label()
        .map(|(_, l)| l.to_string())
        .filter(|l| l != cx.m.lang());
    let tabs = tabs(
        &cx.m,
        &canonical_title,
        tab,
        entity.entity_type.name() == "item",
        has_history,
    );
    let actions = actions(&cx.m, &canonical_title, cx.moderation_tabs());
    let restricted = visibility.map(|v| model::Restricted {
        tip: crate::frame::restricted_tip(&cx.m, &v.groups, &v.sets),
        text: cx.m.get("ts-restricted"),
    });
    let (display, identity, terms, doc_title) = (
        r.display_title(),
        draw(r, cx, &Part::Identity(restricted)),
        draw(r, cx, &Part::Terms),
        r.document_title(),
    );
    pages::respond(cx, headers, StatusCode::OK, move |_| Page {
        title: display,
        title_lang,
        doc_title: Some(doc_title),
        returnto: Some(canonical_title),
        identity_html: Some(identity),
        subtitle_html: Some(terms),
        tabs,
        actions,
        body,
        watch,
        ..Page::default()
    })
}

/// Timing of the entity page's parts, for the skin migration's budget: run by the
/// `profile_entity_rendering` test. Prints to stderr.
#[doc(hidden)]
pub fn profile(r: &Render<'_>, skin: &crate::skin::Skin, rounds: u32) {
    use std::time::Instant;
    let time = |f: &dyn Fn()| {
        let t = Instant::now();
        for _ in 0..rounds {
            f();
        }
        t.elapsed().as_micros() / u128::from(rounds)
    };
    let model = time(&|| {
        let _ = r.statements_tab_model();
    });
    let tab = r.statements_tab_model();
    let convert = time(&|| {
        let _ = minijinja::value::Value::from(minijinja::value::Serde(&tab));
    });
    let render = time(&|| {
        let _ = skin.render(
            "entity/statements.html",
            r.m,
            Ctx::Statements {
                heading: tab.heading.clone(),
                tab: tab.clone(),
            },
        );
    });
    eprintln!(
        "statements tab: model {model} us; serde->Value {convert} us; clone+convert+render {render} us"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::{ProviderInfo, ProviderType};

    fn site() -> SiteInfo {
        SiteInfo {
            concept_base: "https://librarybase.org/entity/".into(),
            providers: vec![ProviderInfo {
                code: "WD".into(),
                slug: "wikidata".into(),
                name: "Wikidata".into(),
                types: vec![ProviderType {
                    code: "Q".into(),
                    entity_type: "item".into(),
                    iri: "http://www.wikidata.org/entity/{upstream_id}".into(),
                    upstream_prefix: "Q".into(),
                    namespace: Some("Item".into()),
                }],
                chip: None,
            }],
            ..SiteInfo::default()
        }
    }

    #[test]
    fn titles_and_links() {
        assert!(is_entity_title("Item:Q6"));
        assert!(is_entity_title("Domain:wikipedia.org"));
        assert!(!is_entity_title("Item:"));
        assert!(!is_entity_title("Main Page"));
        assert_eq!(page_title("P12", "property"), "Property:P12");
        assert_eq!(page_title("WDQ65", "item"), "Item:WDQ65");
        assert_eq!(
            page_title("domain:wikipedia.org", "domain"),
            "Domain:wikipedia.org"
        );
        assert_eq!(entity_href("Q6", "item"), "/wiki/Item:Q6");
        assert_eq!(entity_type_of("WDP31"), "property");
        assert_eq!(entity_type_of("domain:x.org"), "domain");
    }

    #[test]
    fn units_resolve_to_local_and_foreign_ids() {
        let s = site();
        assert_eq!(
            unit_id("https://librarybase.org/entity/Q9", &s).as_deref(),
            Some("Q9")
        );
        assert_eq!(
            unit_id("http://www.wikidata.org/entity/Q11573", &s).as_deref(),
            Some("WDQ11573")
        );
        assert_eq!(unit_id("http://example.org/unit/metre", &s), None);
    }

    #[test]
    fn labels_follow_the_chain() {
        let found: BTreeMap<String, Value> = [(
            "Q1".to_string(),
            serde_json::json!({"labels": {"en": {"value": "one"}, "mul": {"value": "uno"}}, "datatype": "string"}),
        )]
        .into();
        let l = Lookup::from_entities(&found, &["de".into(), "mul".into(), "en".into()]);
        assert_eq!(l.label("Q1"), Some(("uno", "mul")));
        assert_eq!(l.datatype("Q1"), Some("string"));
    }
}
