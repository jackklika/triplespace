//! The entity page's view models (19 §5.10; 0097 §9): what the skin's `entity/*.html`
//! templates draw. Everything is computed here, from the entity, its provenance, the
//! labels looked up and the shapes `scatter-wikibase-shape` picks (0003 §3–5); the
//! templates place and draw it. A field named `*_html` is HTML the binary built: the
//! Series chart and the Timeline axis, which are SVG drawn by [`super::chart`]
//! (0097 A4). Everything else is text the templates escape.
//!
//! The models are built by [`super::Render`], region by region, with the footnote
//! numbers of the tab they belong to ([`super::Endnotes`]).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use scatter_wikibase_model::id::{EntityId, IdForm};
use scatter_wikibase_model::sites::SiteRegistry;
use scatter_wikibase_model::statement::{Snak, SnakGroups, SnakKind, Statement};
use scatter_wikibase_model::value::{
    DataType, DataValue, EntityIdValue, GlobeCoordinate, Quantity, Time,
};
use scatter_wikibase_shape::order::{amount, cmp_cells, cmp_snaks, snak_time};
use scatter_wikibase_shape::{Group, Role, Shape, Thresholds, classify};
use serde::Serialize;

use super::{Render, chart, values};
use crate::codex::{Align, Chip, ChipKind, ChipModel, TableModel};
use crate::frame::title_url;
use crate::html::linkable;

// ---------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------

/// A link to an entity's page: by its label, or its ID where it has no label the reader
/// can read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Link {
    /// The page.
    pub href: String,
    /// The ID, the link's `title` where it has a label.
    pub id: String,
    /// The label.
    pub label: Option<String>,
    /// The label's language, where it differs from the page's.
    pub lang: Option<String>,
}

/// A data value (wikibase-compat §3.3), as a snak says it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Value {
    /// "unknown value" or "no value".
    Special {
        /// The text.
        text: String,
    },
    /// A URL, linked.
    Url {
        /// The URL.
        href: String,
    },
    /// An external identifier.
    ExternalId {
        /// The identifier.
        text: String,
    },
    /// A file on Commons, linked.
    Media {
        /// The file's page.
        href: String,
        /// The file name.
        text: String,
    },
    /// Code: a formula or notation, left to right, or an unknown value's JSON.
    Code {
        /// The text.
        text: String,
        /// Whether it is written left to right whatever the page.
        ltr: bool,
    },
    /// Text in its own direction.
    Text {
        /// The text.
        text: String,
    },
    /// An entity, linked.
    Entity {
        /// The link.
        link: Link,
    },
    /// Monolingual text, with its language.
    Monolingual {
        /// The language.
        lang: String,
        /// The text.
        text: String,
    },
    /// A quantity.
    Quantity {
        /// The amount, grouped.
        amount: String,
        /// `±x` where the bounds are symmetric.
        plus: Option<String>,
        /// `[low, high]` where they are not.
        bounds: Option<(String, String)>,
        /// The unit, where it is not 1.
        unit: Option<Unit>,
    },
    /// A point in time.
    Time {
        /// The date, in the reader's language.
        text: String,
        /// Whether it is in the Julian calendar.
        julian: bool,
    },
    /// Coordinates.
    Coordinate {
        /// Degrees, minutes and seconds.
        text: String,
        /// The globe, where it is not Earth.
        globe: Option<Unit>,
    },
    /// Text as it is.
    Plain {
        /// The text.
        text: String,
    },
}

/// A quantity's unit, or a coordinate's globe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Unit {
    /// An entity, linked.
    Entity {
        /// The link.
        link: Link,
    },
    /// An IRI, linked.
    External {
        /// The IRI.
        href: String,
        /// Its last segment.
        text: String,
    },
    /// Text.
    Plain {
        /// The text.
        text: String,
    },
}

/// A message whose parameters are values or links: the message's text in parts, each
/// parameter isolated (`<bdi>`) by the template, so that a date in a fallback language
/// keeps its order in a right-to-left page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rich {
    /// The parts, in order.
    pub parts: Vec<Part>,
}

/// One part of [`Rich`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Part {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// A value.
    Value {
        /// The value.
        value: Value,
    },
    /// An entity link.
    Link {
        /// The link.
        link: Link,
    },
    /// Links to the groups of a tab, by property label, joined by commas.
    Groups {
        /// Each group: its anchor, the property's ID, and its label.
        groups: Vec<(String, String)>,
    },
}

// ---------------------------------------------------------------------------------------
// Statements
// ---------------------------------------------------------------------------------------

/// A footnote marker: a link to endnote `n`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Mark {
    /// The number.
    pub n: usize,
    /// The accessible name.
    pub label: String,
}

/// A qualifier, or a reference's snak group: a property and its values.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Qualifier {
    /// The property.
    pub property: Link,
    /// The values.
    pub values: Vec<Value>,
}

/// A statement as a shape draws it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatementModel {
    /// The statement ID, the element's `id`.
    pub id: Option<String>,
    /// The main value.
    pub main: Value,
    /// The "Best value" badge.
    pub badge: Option<ChipModel>,
    /// The chips for where it comes from.
    pub chips: Vec<ChipModel>,
    /// Its footnote markers.
    pub marks: Vec<Mark>,
    /// Its qualifiers, but for the ones the shape draws elsewhere.
    pub qualifiers: Vec<Qualifier>,
    /// When a Timeline row held.
    pub period: Option<Rich>,
    /// A numbered list's ordinal.
    pub ordinal: Option<u64>,
    /// A deprecated value's reason.
    pub reason: Option<Value>,
}

/// Values past the large-group limit, under "Show N more values", or the other values
/// under "N other values".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fold {
    /// The summary.
    pub summary: String,
    /// The statements.
    pub items: Vec<StatementModel>,
}

/// A cell of a group's grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GridCell {
    /// Whether it is a row heading.
    pub header: bool,
    /// The alignment class.
    pub align_class: Option<String>,
    /// Whether it repeats the row above and is visually hidden.
    pub hidden: bool,
    /// The content.
    pub content: CellContent,
}

/// What a grid cell holds.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum CellContent {
    /// Nothing.
    Empty,
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// A property link.
    Link {
        /// The link.
        link: Link,
    },
    /// A value.
    Value {
        /// The value.
        value: Value,
    },
    /// A message with parameters.
    Rich {
        /// The message.
        rich: Rich,
    },
    /// Footnote markers.
    Marks {
        /// The markers.
        marks: Vec<Mark>,
    },
    /// A statement's value with what goes beside it.
    Statement {
        /// The main value.
        main: Value,
        /// Whether the main value is wrapped as the statement's main.
        span: bool,
        /// The "Best value" badge.
        badge: Option<ChipModel>,
        /// The chips.
        chips: Vec<ChipModel>,
        /// The qualifiers.
        qualifiers: Vec<Qualifier>,
        /// The footnote markers.
        marks: Vec<Mark>,
    },
}

/// A row of a group's grid.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GridRow {
    /// The statement ID.
    pub id: Option<String>,
    /// Whether it is a best value.
    pub best: bool,
    /// The cells.
    pub cells: Vec<GridCell>,
}

/// A table in Codex's markup, for a Series, a Table or a Matrix.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Grid {
    /// The caption, for assistive technology.
    pub caption: String,
    /// The column headings.
    pub head: Vec<GridCell>,
    /// The rows.
    pub rows: Vec<GridRow>,
}

/// A grid, with its rows past the large-group limit under "Show N more values".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SplitGrid {
    /// The grid.
    pub grid: Grid,
    /// The rest, folded.
    pub more: Option<MoreGrid>,
}

/// The folded rest of [`SplitGrid`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoreGrid {
    /// The summary.
    pub summary: String,
    /// The grid.
    pub grid: Grid,
}

/// A view switch between two panels (0003 §9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Views {
    /// The property, which names the radio group.
    pub property: String,
    /// The legend.
    pub legend: String,
    /// The first option's label, chosen.
    pub a: String,
    /// The second option's label.
    pub b: String,
}

/// A group's body, by shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "shape", rename_all = "kebab-case")]
pub enum Body {
    /// No statements.
    Empty {
        /// What it says.
        text: String,
    },
    /// Only deprecated statements.
    NoCurrentValue {
        /// What it says.
        text: String,
    },
    /// One statement.
    Single {
        /// The statement.
        statement: StatementModel,
    },
    /// A list, or chips.
    List {
        /// Chips rather than a list.
        chips: bool,
        /// The values shown.
        visible: Vec<StatementModel>,
        /// The rest past the large-group limit.
        more: Option<Fold>,
        /// The other values.
        folded: Option<Fold>,
    },
    /// A timeline.
    Timeline {
        /// The axis, SVG.
        axis_html: String,
        /// The rows.
        visible: Vec<StatementModel>,
        /// The rest past the large-group limit.
        more: Option<Fold>,
    },
    /// A series.
    Series {
        /// The headline value.
        headline: Headline,
        /// The chart, SVG, where two or more values have a point in time.
        chart_html: Option<String>,
        /// The table.
        table: SplitGrid,
        /// The switch between chart and table, where there is a chart.
        views: Option<Views>,
    },
    /// A table.
    Table {
        /// The table.
        table: SplitGrid,
        /// The matrix view, where 0003 §3 allows one.
        matrix: Option<Grid>,
        /// The switch between table and matrix, where there is a matrix.
        views: Option<Views>,
    },
    /// A list numbered by the series ordinal.
    Numbered {
        /// The values shown.
        visible: Vec<StatementModel>,
        /// The rest past the large-group limit.
        more: Option<Fold>,
    },
}

/// A Series' headline value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Headline {
    /// The value.
    pub main: Value,
    /// "in 2024", where it has a point in time.
    pub when: Option<Rich>,
    /// The "Best value" badge.
    pub badge: Option<ChipModel>,
}

/// A group's deprecated values.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Deprecated {
    /// Open where there is no current value.
    pub open: bool,
    /// The summary.
    pub summary: String,
    /// The values.
    pub items: Vec<StatementModel>,
}

/// A group's footer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Footer {
    /// "Same for all N" and the shared qualifiers.
    pub hoisted: Option<Hoisted>,
    /// "Cited by all N" and the marker.
    pub shared: Option<Shared>,
    /// "N values have no source".
    pub unsourced: Option<String>,
}

/// The qualifiers every value of a group shares.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Hoisted {
    /// The title.
    pub title: String,
    /// The qualifiers.
    pub qualifiers: Vec<Qualifier>,
}

/// The reference every value of a group cites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Shared {
    /// The text.
    pub text: String,
    /// The marker.
    pub mark: Mark,
}

/// The edit button of a group, the terms, or a tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Edit {
    /// The accessible name.
    pub label: String,
    /// The text.
    pub text: String,
}

/// A statement group in its shape (`statements/{P}`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupModel {
    /// The property ID.
    pub pid: String,
    /// The shape's name.
    pub shape: String,
    /// The property, linked.
    pub property: Link,
    /// "N values", for more than one.
    pub count: Option<String>,
    /// The edit button, for a viewer who may edit.
    pub edit: Option<Edit>,
    /// The body.
    pub body: Body,
    /// The deprecated values.
    pub deprecated: Option<Deprecated>,
    /// The footer.
    pub footer: Option<Footer>,
}

// ---------------------------------------------------------------------------------------
// Tabs and regions
// ---------------------------------------------------------------------------------------

/// A tab's endnotes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EndnotesModel {
    /// The title.
    pub title: String,
    /// The notes, in number order.
    pub notes: Vec<NoteModel>,
}

/// One endnote.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteModel {
    /// The reference's snaks.
    pub snaks: Vec<Qualifier>,
    /// "Cited for …", naming the groups.
    pub cited: Rich,
}

/// "Where this comes from".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sources {
    /// The title.
    pub title: String,
    /// What is said where the API gave no provenance.
    pub unknown: Option<String>,
    /// Each graph holding a record for the entity.
    pub graphs: Vec<SourceGraph>,
    /// What happens if the upstream deletes it.
    pub retention: Option<String>,
    /// The JSON link's target.
    pub json_href: String,
    /// The JSON link's text.
    pub json_text: String,
}

/// One graph of [`Sources`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceGraph {
    /// The chip.
    pub chip: ChipModel,
    /// The name.
    pub name: String,
    /// "N statements".
    pub statements: String,
    /// "Synced …", with the upstream revision where there is one.
    pub synced: Option<Synced>,
}

/// A graph's sync line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Synced {
    /// "Synced on …".
    pub text: String,
    /// The upstream revision, linked where the API says where.
    pub upstream: Option<Upstream>,
}

/// An upstream revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Upstream {
    /// The text.
    pub text: String,
    /// The page upstream.
    pub href: Option<String>,
}

/// The Statements tab.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatementsTab {
    /// The hidden heading.
    pub heading: String,
    /// The groups.
    pub groups: Vec<GroupModel>,
    /// What an empty tab says.
    pub empty: Option<String>,
    /// **Add statement**, for a viewer who may edit.
    pub add: Option<String>,
    /// The endnotes, where anything is cited.
    pub endnotes: Option<EndnotesModel>,
    /// "Where this comes from".
    pub sources: Sources,
}

/// The Identifiers tab.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct IdentifiersTab {
    /// The groups.
    pub groups: Vec<GroupModel>,
    /// What an empty tab says.
    pub empty: Option<String>,
    /// **Add statement**, for a viewer who may edit.
    pub add: Option<String>,
    /// The endnotes, where anything is cited.
    pub endnotes: Option<EndnotesModel>,
}

/// The `terms` region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Terms {
    /// The description, with its language where it differs from the page's.
    pub description: Option<(String, Option<String>)>,
    /// What is said where there is none.
    pub no_description: Option<String>,
    /// The aliases.
    pub aliases: Option<Aliases>,
    /// The edit button.
    pub edit: Option<Edit>,
}

/// The aliases under the title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Aliases {
    /// "Also known as".
    pub label: String,
    /// Their language, where it differs from the page's.
    pub lang: Option<String>,
    /// The aliases.
    pub list: Vec<String>,
}

/// The `sitelinks` region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SitelinksModel {
    /// What is said where there are none.
    pub empty: Option<String>,
    /// The hosts.
    pub hosts: Vec<Host>,
}

/// One host's sitelinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Host {
    /// The Domain page.
    pub href: String,
    /// The chip.
    pub chip: ChipModel,
    /// The links.
    pub items: Vec<Sitelink>,
}

/// One sitelink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sitelink {
    /// The page, where it can be linked.
    pub href: Option<String>,
    /// The title's language, where it differs from the page's.
    pub lang: Option<String>,
    /// The title.
    pub title: String,
    /// The badges.
    pub badges: Vec<Link>,
}

/// The identity line (0010 §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    /// The chip for the ID or namespace.
    pub chip: ChipModel,
    /// A keyed entity's key, after the chip.
    pub key: Option<String>,
    /// The kind of page.
    pub kind: String,
    /// "Adopted from …".
    pub adopted: Option<String>,
    /// The concept IRI, and how it is shown.
    pub iri: Option<(String, String)>,
    /// The **Restricted** pill (19 §1.3).
    pub restricted: Option<Restricted>,
}

/// The **Restricted** pill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Restricted {
    /// The tooltip.
    pub tip: String,
    /// The text.
    pub text: String,
}

// ---------------------------------------------------------------------------------------
// Building
// ---------------------------------------------------------------------------------------

/// The Gregorian and Julian calendar models.
const JULIAN: &str = "Q1985786";

/// The IRI of Earth, the default globe.
const EARTH: &str = "/Q2";

/// One group being drawn.
struct Card<'s> {
    property: &'s str,
    st: &'s [Statement],
    g: Group,
    /// The tab's number for each of the group's footnotes (`g.footnotes.notes`).
    nums: Vec<usize>,
}

impl Card<'_> {
    fn best(&self, i: usize) -> bool {
        self.g.best.contains(&i)
    }

    fn hoisted(&self, k: &EntityId) -> bool {
        self.g.hoisted.contains(k)
    }
}

fn is_quantity(snak: Option<&Snak>) -> bool {
    matches!(
        snak.map(|s| &s.kind),
        Some(SnakKind::Value(DataValue::Quantity(_)))
    )
}

fn align_of(snak: Option<&Snak>) -> Option<String> {
    let a = if is_quantity(snak) {
        Align::Number
    } else {
        Align::Start
    };
    a.class().map(str::to_string)
}

fn same(a: Option<&Snak>, b: Option<&Snak>) -> bool {
    a.is_some() && b.is_some() && cmp_cells(a, b) == Ordering::Equal
}

fn id_of(st: &Statement) -> Option<String> {
    st.id.as_ref().map(|i| i.as_str().to_string())
}

/// The language attribute of a text, where it differs from the page's.
fn lang_of(text_lang: &str, page_lang: &str) -> Option<String> {
    if text_lang.is_empty() || text_lang == page_lang {
        None
    } else {
        Some(text_lang.to_string())
    }
}

impl Render<'_> {
    // ---- values ----

    /// A snak's value, or what it says instead of one.
    #[must_use]
    pub fn snak_model(&self, snak: &Snak) -> Value {
        match &snak.kind {
            SnakKind::Value(v) => self.value_model(v, snak.datatype.as_ref()),
            SnakKind::SomeValue => Value::Special {
                text: self.m.get("ts-value-somevalue"),
            },
            SnakKind::NoValue => Value::Special {
                text: self.m.get("ts-value-novalue"),
            },
        }
    }

    /// A data value, by its data type where that matters and by its value type
    /// otherwise.
    #[must_use]
    pub fn value_model(&self, value: &DataValue, datatype: Option<&DataType>) -> Value {
        match value {
            DataValue::String(s) => match datatype.map(DataType::id) {
                Some("url") if linkable(s) => Value::Url { href: s.clone() },
                Some("external-id") => Value::ExternalId { text: s.clone() },
                Some("commonsMedia") => Value::Media {
                    href: format!("https://commons.wikimedia.org/wiki/File:{}", title_url(s)),
                    text: s.clone(),
                },
                Some("math" | "musical-notation") => Value::Code {
                    text: s.clone(),
                    ltr: true,
                },
                _ => Value::Text { text: s.clone() },
            },
            DataValue::EntityId(e) => Value::Entity {
                link: self.entity_link_model(e),
            },
            DataValue::MonolingualText(t) => Value::Monolingual {
                lang: t.language.clone(),
                text: t.text.clone(),
            },
            DataValue::Quantity(q) => self.quantity_model(q),
            DataValue::Time(t) => self.time_model(t),
            DataValue::GlobeCoordinate(g) => self.coordinate_model(g),
            DataValue::Unknown { value, .. } => Value::Code {
                text: value.to_string(),
                ltr: false,
            },
            #[allow(unreachable_patterns)]
            _ => Value::Plain {
                text: String::new(),
            },
        }
    }

    /// A link to an entity's page.
    #[must_use]
    pub fn entity_link_model(&self, e: &EntityIdValue) -> Link {
        self.id_link_model(e.id.as_str(), &e.entity_type)
    }

    /// A link to an entity by ID and entity type.
    #[must_use]
    pub fn id_link_model(&self, id: &str, entity_type: &str) -> Link {
        let href = super::entity_href(id, entity_type);
        match self.lookup.label(id) {
            Some((text, lang)) => Link {
                href,
                id: id.to_string(),
                label: Some(text.to_string()),
                lang: lang_of(lang, self.m.lang()),
            },
            None => Link {
                href,
                id: id.to_string(),
                label: None,
                lang: None,
            },
        }
    }

    pub(super) fn property_link_model(&self, property: &str) -> Link {
        self.id_link_model(property, "property")
    }

    fn quantity_model(&self, q: &Quantity) -> Value {
        let mut plus = None;
        let mut bounds = None;
        if let (Some(up), Some(low)) = (&q.upper_bound, &q.lower_bound)
            && let (Some(amount), Some(upper), Some(lower)) =
                (values::num(&q.amount), values::num(up), values::num(low))
        {
            let p = upper - amount;
            let minus = amount - lower;
            if (p - minus).abs() < 1e-9 * amount.abs().max(1.0) {
                if p > 0.0 {
                    plus = Some(values::group(&values::trim_float(p)));
                }
            } else {
                bounds = Some((values::group(low), values::group(up)));
            }
        }
        let unit = (q.unit != "1").then(|| self.unit_model(&q.unit));
        Value::Quantity {
            amount: values::group(&q.amount),
            plus,
            bounds,
            unit,
        }
    }

    fn unit_model(&self, iri: &str) -> Unit {
        if let Some(id) = super::unit_id(iri, self.site) {
            return Unit::Entity {
                link: self.id_link_model(&id, &super::entity_type_of(&id)),
            };
        }
        let tail = iri.rsplit('/').next().unwrap_or(iri).to_string();
        if linkable(iri) {
            Unit::External {
                href: iri.to_string(),
                text: tail,
            }
        } else {
            Unit::Plain { text: tail }
        }
    }

    fn time_model(&self, t: &Time) -> Value {
        match values::time_text(self.m, t) {
            Some(text) => Value::Time {
                text,
                julian: t.calendarmodel.ends_with(JULIAN),
            },
            None => Value::Plain {
                text: t.time.clone(),
            },
        }
    }

    fn coordinate_model(&self, g: &GlobeCoordinate) -> Value {
        let text = values::coordinate_text(g);
        let globe = (!g.globe.ends_with(EARTH)).then(|| {
            super::unit_id(&g.globe, self.site).map_or_else(
                || Unit::Plain {
                    text: g.globe.rsplit('/').next().unwrap_or("").to_string(),
                },
                |id| Unit::Entity {
                    link: self.id_link_model(&id, "item"),
                },
            )
        });
        Value::Coordinate { text, globe }
    }

    /// A message whose parameters are values or links.
    fn rich(&self, key: &str, params: &[Part]) -> Rich {
        let marks: Vec<String> = (0..params.len())
            .map(|i| format!("\u{E000}{i}\u{E001}"))
            .collect();
        let refs: Vec<&str> = marks.iter().map(String::as_str).collect();
        let text = self.m.with(key, &refs);
        let mut parts = Vec::new();
        let mut rest = text.as_str();
        while let Some(start) = rest.find('\u{E000}') {
            let Some(end) = rest[start..].find('\u{E001}') else {
                break;
            };
            if start > 0 {
                parts.push(Part::Text {
                    text: rest[..start].to_string(),
                });
            }
            if let Ok(i) = rest[start + '\u{E000}'.len_utf8()..start + end].parse::<usize>()
                && let Some(p) = params.get(i)
            {
                parts.push(p.clone());
            }
            rest = &rest[start + end + '\u{E001}'.len_utf8()..];
        }
        if !rest.is_empty() {
            parts.push(Part::Text {
                text: rest.to_string(),
            });
        }
        Rich { parts }
    }

    // ---- statements ----

    fn label_of(&self, property: &str) -> String {
        self.lookup
            .label(property)
            .map_or_else(|| property.to_string(), |(t, _)| t.to_string())
    }

    fn mark(&self, n: usize) -> Mark {
        Mark {
            n,
            label: self.m.with("ts-footnote-label", &[&n.to_string()]),
        }
    }

    fn badge_model(&self, c: &Card, i: usize) -> Option<ChipModel> {
        c.best(i).then(|| {
            let (text, tip) = (self.m.get("ts-best-value"), self.m.get("ts-best-value-tip"));
            Chip {
                kind: ChipKind::Best,
                text: &text,
                title: Some(&tip),
            }
            .model()
        })
    }

    fn marks_model(&self, c: &Card, i: usize) -> Vec<Mark> {
        c.g.footnotes.rows[i]
            .iter()
            .map(|&k| self.mark(c.nums[k]))
            .collect()
    }

    /// The chips for where a statement comes from (see `statement_chips`).
    pub(super) fn statement_chips_model(&self, st: &Statement) -> Vec<ChipModel> {
        let mut out = Vec::new();
        let (Some(p), Some(sid)) = (self.provenance, st.id.as_ref()) else {
            return out;
        };
        if let (Some(dominant), Some(src)) = (&p.dominant, p.statements.get(sid.as_str()))
            && !src.graphs.iter().any(|g| g == dominant)
        {
            for g in &src.graphs {
                out.push(self.graph_chip_model(g));
            }
        }
        if p.corrections.contains_key(sid.as_str()) {
            out.push(Chip::new(ChipKind::Corrected, &self.m.get("ts-chip-corrected")).model());
        }
        out
    }

    pub(super) fn graph_chip_model(&self, graph: &str) -> ChipModel {
        if graph == "local" {
            return Chip::new(ChipKind::Local, &self.m.get("ts-chip-local")).model();
        }
        if let Some(slug) = graph.strip_prefix("mirror/")
            && let Some(p) = self.site.providers.iter().find(|p| p.slug == slug)
        {
            return Chip::new(ChipKind::Provider(p.code.clone()), &p.code).model();
        }
        if let Some(name) = graph.strip_prefix("source/") {
            return Chip::new(ChipKind::Namespace, name).model();
        }
        Chip::new(ChipKind::Namespace, graph).model()
    }

    /// A snak group's rows: a qualifier's or a reference's.
    pub(super) fn snak_groups_model(&self, groups: &SnakGroups) -> Vec<Qualifier> {
        groups
            .iter()
            .map(|(p, snaks)| Qualifier {
                property: self.property_link_model(p.as_str()),
                values: snaks.iter().map(|v| self.snak_model(v)).collect(),
            })
            .collect()
    }

    fn qualifiers_model(&self, st: &Statement, skip: &dyn Fn(&EntityId) -> bool) -> Vec<Qualifier> {
        st.qualifiers
            .iter()
            .filter(|(k, _)| !skip(k))
            .map(|(p, snaks)| Qualifier {
                property: self.property_link_model(p.as_str()),
                values: snaks.iter().map(|v| self.snak_model(v)).collect(),
            })
            .collect()
    }

    fn statement_model(
        &self,
        c: &Card,
        i: usize,
        skip: &dyn Fn(&EntityId) -> bool,
    ) -> StatementModel {
        let st = &c.st[i];
        StatementModel {
            id: id_of(st),
            main: self.snak_model(&st.mainsnak),
            badge: self.badge_model(c, i),
            chips: self.statement_chips_model(st),
            marks: self.marks_model(c, i),
            qualifiers: self.qualifiers_model(st, skip),
            period: None,
            ordinal: None,
            reason: None,
        }
    }

    fn role_of<'s>(&self, st: &'s Statement, role: Role) -> Option<&'s Snak> {
        st.qualifiers
            .iter()
            .find(|(k, _)| self.roles.is(k.as_str(), role))
            .and_then(|(_, v)| v.first())
    }

    /// Values `idx`, built by `build`; past the large-group limit, the rest under "Show
    /// N more values".
    fn split_at<T>(
        &self,
        c: &Card,
        idx: &[usize],
        build: &dyn Fn(&[usize]) -> T,
    ) -> (T, Option<(String, T)>) {
        match c.g.inline {
            Some(k) if idx.len() > k => {
                let n = (idx.len() - k).to_string();
                (
                    build(&idx[..k]),
                    Some((self.m.with("ts-show-more", &[&n]), build(&idx[k..]))),
                )
            }
            _ => (build(idx), None),
        }
    }

    fn list_body(&self, c: &Card, chips: bool) -> Body {
        let skip = |k: &EntityId| c.hoisted(k);
        let build = |idx: &[usize]| -> Vec<StatementModel> {
            idx.iter()
                .map(|&i| {
                    if chips {
                        let st = &c.st[i];
                        StatementModel {
                            id: id_of(st),
                            main: self.snak_model(&st.mainsnak),
                            badge: self.badge_model(c, i),
                            chips: self.statement_chips_model(st),
                            marks: self.marks_model(c, i),
                            qualifiers: Vec::new(),
                            period: None,
                            ordinal: None,
                            reason: None,
                        }
                    } else {
                        self.statement_model(c, i, &skip)
                    }
                })
                .collect()
        };
        let visible: Vec<usize> =
            c.g.order
                .iter()
                .copied()
                .filter(|i| !c.g.folded.contains(i))
                .collect();
        let (visible, more) = self.split_at(c, &visible, &build);
        let folded = (!c.g.folded.is_empty()).then(|| {
            let n = c.g.folded.len().to_string();
            Fold {
                summary: self.m.with("ts-fold-others", &[&n]),
                items: build(&c.g.folded),
            }
        });
        Body::List {
            chips,
            visible,
            more: more.map(|(summary, items)| Fold { summary, items }),
            folded,
        }
    }

    fn period_model(&self, st: &Statement) -> Option<Rich> {
        let (a, b) = (
            self.role_of(st, Role::TimeStart),
            self.role_of(st, Role::TimeEnd),
        );
        let value = |s: &Snak| Part::Value {
            value: self.snak_model(s),
        };
        Some(match (a, b) {
            (Some(a), Some(b)) => self.rich("ts-period-range", &[value(a), value(b)]),
            (Some(a), None) => self.rich("ts-period-since", &[value(a)]),
            (None, Some(b)) => self.rich("ts-period-until", &[value(b)]),
            (None, None) => return None,
        })
    }

    fn timeline_body(&self, c: &Card) -> Body {
        let year = |s: Option<&Snak>| s.and_then(snak_time).map(chart::year_of);
        let rows: Vec<_> =
            c.g.order
                .iter()
                .map(|&i| {
                    (
                        year(self.role_of(&c.st[i], Role::TimeStart)),
                        year(self.role_of(&c.st[i], Role::TimeEnd)),
                        c.best(i),
                    )
                })
                .collect();
        let skip = |k: &EntityId| {
            c.hoisted(k)
                || self.roles.is(k.as_str(), Role::TimeStart)
                || self.roles.is(k.as_str(), Role::TimeEnd)
        };
        let build = |idx: &[usize]| -> Vec<StatementModel> {
            idx.iter()
                .map(|&i| {
                    let mut s = self.statement_model(c, i, &skip);
                    s.period = self.period_model(&c.st[i]);
                    s
                })
                .collect()
        };
        let (visible, more) = self.split_at(c, &c.g.order, &build);
        Body::Timeline {
            axis_html: chart::timeline(&rows),
            visible,
            more: more.map(|(summary, items)| Fold { summary, items }),
        }
    }

    fn views_model(&self, c: &Card, a: &str, b: &str) -> Views {
        Views {
            property: c.property.to_string(),
            legend: self.m.get("ts-views-label"),
            a: self.m.get(a),
            b: self.m.get(b),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn series_body(&self, c: &Card) -> Body {
        let point = |i: usize| self.role_of(&c.st[i], Role::TimePoint);
        let head = c.g.best.first().copied().unwrap_or(c.g.order[0]);
        let headline = Headline {
            main: self.snak_model(&c.st[head].mainsnak),
            when: point(head).map(|t| {
                self.rich(
                    "ts-series-in",
                    &[Part::Value {
                        value: self.snak_model(t),
                    }],
                )
            }),
            badge: self.badge_model(c, head),
        };
        let label = self.label_of(c.property);
        let chart = self.series_svg(c, &label);
        let time_key =
            c.g.order
                .iter()
                .find_map(|&i| point(i))
                .map(|t| t.property.clone());
        let skip = |k: &EntityId| c.hoisted(k) || self.roles.is(k.as_str(), Role::TimePoint);
        let marked = c.g.footnotes.rows.iter().any(|r| !r.is_empty());
        let mut head_row = vec![
            GridCell {
                header: false,
                align_class: None,
                hidden: false,
                content: time_key
                    .as_ref()
                    .map_or(CellContent::Empty, |k| CellContent::Link {
                        link: self.property_link_model(k.as_str()),
                    }),
            },
            GridCell {
                header: false,
                align_class: Align::Number.class().map(str::to_string),
                hidden: false,
                content: CellContent::Text {
                    text: label.clone(),
                },
            },
        ];
        if marked {
            head_row.push(GridCell {
                header: false,
                align_class: None,
                hidden: false,
                content: CellContent::Text {
                    text: self.m.get("ts-column-sources"),
                },
            });
        }
        let rows = |idx: &[usize]| -> Vec<GridRow> {
            idx.iter()
                .map(|&i| {
                    let st = &c.st[i];
                    let mut cells = vec![
                        GridCell {
                            header: true,
                            align_class: None,
                            hidden: false,
                            content: point(i).map_or(CellContent::Empty, |t| CellContent::Value {
                                value: self.snak_model(t),
                            }),
                        },
                        GridCell {
                            header: false,
                            align_class: Align::Number.class().map(str::to_string),
                            hidden: false,
                            content: CellContent::Statement {
                                main: self.snak_model(&st.mainsnak),
                                span: false,
                                badge: self.badge_model(c, i),
                                chips: self.statement_chips_model(st),
                                qualifiers: self.qualifiers_model(st, &skip),
                                marks: Vec::new(),
                            },
                        },
                    ];
                    if marked {
                        cells.push(GridCell {
                            header: false,
                            align_class: None,
                            hidden: false,
                            content: CellContent::Marks {
                                marks: self.marks_model(c, i),
                            },
                        });
                    }
                    GridRow {
                        id: id_of(st),
                        best: c.best(i),
                        cells,
                    }
                })
                .collect()
        };
        let grid = |idx: &[usize]| Grid {
            caption: label.clone(),
            head: head_row.clone(),
            rows: rows(idx),
        };
        let (first, more) = self.split_at(c, &c.g.order, &grid);
        let table = SplitGrid {
            grid: first,
            more: more.map(|(summary, grid)| MoreGrid { summary, grid }),
        };
        let views = chart
            .is_some()
            .then(|| self.views_model(c, "ts-view-chart", "ts-view-table"));
        Body::Series {
            headline,
            chart_html: chart,
            table,
            views,
        }
    }

    /// The Series chart: the values with a point in time, oldest first; none where fewer
    /// than two have one.
    fn series_svg(&self, c: &Card, label: &str) -> Option<String> {
        let mut points: Vec<(f64, f64)> =
            c.g.order
                .iter()
                .filter_map(|&i| {
                    let t = self
                        .role_of(&c.st[i], Role::TimePoint)
                        .and_then(snak_time)?;
                    let SnakKind::Value(DataValue::Quantity(q)) = &c.st[i].mainsnak.kind else {
                        return None;
                    };
                    Some((chart::year_of(t), amount(&q.amount)?))
                })
                .collect();
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (first, last) = (points.first()?, points.last()?);
        let name = self.m.with(
            "ts-chart-label",
            &[
                label,
                &format!("{:.0}", first.0.floor()),
                &format!("{:.0}", last.0.floor()),
            ],
        );
        let svg = chart::series(&points, &name, |v| values::group(&format!("{v}")));
        (!svg.is_empty()).then_some(svg)
    }

    #[allow(clippy::too_many_lines)]
    fn table_body(&self, c: &Card) -> Body {
        let label = self.label_of(c.property);
        let skip = |k: &EntityId| c.hoisted(k) || c.g.columns.contains(k);
        let value_align = align_of(c.g.order.first().map(|&i| &c.st[i].mainsnak));
        let mut head = vec![GridCell {
            header: false,
            align_class: value_align.clone(),
            hidden: false,
            content: CellContent::Text {
                text: label.clone(),
            },
        }];
        for k in &c.g.columns {
            let first = c.g.order.iter().find_map(|&i| Group::cell(c.st, i, k));
            head.push(GridCell {
                header: false,
                align_class: align_of(first),
                hidden: false,
                content: CellContent::Link {
                    link: self.property_link_model(k.as_str()),
                },
            });
        }
        let marked = c.g.footnotes.rows.iter().any(|r| !r.is_empty());
        if marked {
            head.push(GridCell {
                header: false,
                align_class: None,
                hidden: false,
                content: CellContent::Text {
                    text: self.m.get("ts-column-sources"),
                },
            });
        }
        let rows = |idx: &[usize]| -> Vec<GridRow> {
            let mut prev: Option<usize> = None;
            idx.iter()
                .map(|&i| {
                    let st = &c.st[i];
                    let mut cells = vec![GridCell {
                        header: false,
                        align_class: value_align.clone(),
                        hidden: false,
                        content: CellContent::Statement {
                            main: self.snak_model(&st.mainsnak),
                            span: true,
                            badge: self.badge_model(c, i),
                            chips: self.statement_chips_model(st),
                            qualifiers: self.qualifiers_model(st, &skip),
                            marks: Vec::new(),
                        },
                    }];
                    let mut run = prev.is_some();
                    for k in &c.g.columns {
                        let cell = Group::cell(c.st, i, k);
                        run = run && same(cell, prev.and_then(|j| Group::cell(c.st, j, k)));
                        cells.push(GridCell {
                            header: false,
                            align_class: align_of(cell),
                            hidden: run,
                            content: cell.map_or(CellContent::Empty, |s| CellContent::Value {
                                value: self.snak_model(s),
                            }),
                        });
                    }
                    if marked {
                        cells.push(GridCell {
                            header: false,
                            align_class: None,
                            hidden: false,
                            content: CellContent::Marks {
                                marks: self.marks_model(c, i),
                            },
                        });
                    }
                    prev = Some(i);
                    GridRow {
                        id: id_of(st),
                        best: c.best(i),
                        cells,
                    }
                })
                .collect()
        };
        let grid = |idx: &[usize]| Grid {
            caption: label.clone(),
            head: head.clone(),
            rows: rows(idx),
        };
        let (first, more) = self.split_at(c, &c.g.order, &grid);
        let table = SplitGrid {
            grid: first,
            more: more.map(|(summary, grid)| MoreGrid { summary, grid }),
        };
        let matrix = (c.g.matrix && c.g.columns.len() == 2).then(|| self.matrix_grid(c, &label));
        let views = matrix
            .is_some()
            .then(|| self.views_model(c, "ts-view-table", "ts-view-matrix"));
        Body::Table {
            table,
            matrix,
            views,
        }
    }

    fn matrix_grid(&self, c: &Card, label: &str) -> Grid {
        let (ka, kb) = (&c.g.columns[0], &c.g.columns[1]);
        let distinct = |k: &EntityId| {
            let mut v: Vec<&Snak> =
                c.g.order
                    .iter()
                    .filter_map(|&i| Group::cell(c.st, i, k))
                    .collect();
            v.sort_by(|a, b| cmp_snaks(a, b));
            v.dedup_by(|a, b| cmp_snaks(a, b) == Ordering::Equal);
            v
        };
        let (down, across) = (distinct(ka), distinct(kb));
        let value_align = align_of(c.g.order.first().map(|&i| &c.st[i].mainsnak));
        let corner = self.rich(
            "ts-matrix-corner",
            &[
                Part::Link {
                    link: self.property_link_model(ka.as_str()),
                },
                Part::Link {
                    link: self.property_link_model(kb.as_str()),
                },
            ],
        );
        let mut head = vec![GridCell {
            header: false,
            align_class: None,
            hidden: false,
            content: CellContent::Rich { rich: corner },
        }];
        head.extend(across.iter().map(|b| GridCell {
            header: false,
            align_class: align_of(Some(b)),
            hidden: false,
            content: CellContent::Value {
                value: self.snak_model(b),
            },
        }));
        let none = Value::Special {
            text: self.m.get("ts-value-novalue"),
        };
        let rows: Vec<GridRow> = down
            .iter()
            .map(|a| {
                let mut cells = vec![GridCell {
                    header: true,
                    align_class: align_of(Some(a)),
                    hidden: false,
                    content: CellContent::Value {
                        value: self.snak_model(a),
                    },
                }];
                for b in &across {
                    let hit = c.g.order.iter().copied().find(|&i| {
                        same(Group::cell(c.st, i, ka), Some(a))
                            && same(Group::cell(c.st, i, kb), Some(b))
                    });
                    let content = hit.map_or_else(
                        || CellContent::Value {
                            value: none.clone(),
                        },
                        |i| CellContent::Statement {
                            main: self.snak_model(&c.st[i].mainsnak),
                            span: false,
                            badge: self.badge_model(c, i),
                            chips: Vec::new(),
                            qualifiers: Vec::new(),
                            marks: self.marks_model(c, i),
                        },
                    );
                    cells.push(GridCell {
                        header: false,
                        align_class: value_align.clone(),
                        hidden: false,
                        content,
                    });
                }
                GridRow {
                    id: None,
                    best: false,
                    cells,
                }
            })
            .collect();
        Grid {
            caption: label.to_string(),
            head,
            rows,
        }
    }

    fn numbered_body(&self, c: &Card) -> Body {
        let key = &c.g.columns[0];
        let skip = |k: &EntityId| c.hoisted(k) || k == key;
        let build = |idx: &[usize]| -> Vec<StatementModel> {
            idx.iter()
                .map(|&i| {
                    let mut s = self.statement_model(c, i, &skip);
                    s.ordinal = Group::cell(c.st, i, key).and_then(|s| match &s.kind {
                        SnakKind::Value(DataValue::String(t)) => t.trim().parse::<u64>().ok(),
                        _ => None,
                    });
                    s
                })
                .collect()
        };
        let (visible, more) = self.split_at(c, &c.g.order, &build);
        Body::Numbered {
            visible,
            more: more.map(|(summary, items)| Fold { summary, items }),
        }
    }

    fn deprecated_model(&self, c: &Card) -> Option<Deprecated> {
        if c.g.deprecated.is_empty() {
            return None;
        }
        let skip = |k: &EntityId| self.roles.is(k.as_str(), Role::DeprecationReason);
        let items =
            c.g.deprecated
                .iter()
                .map(|&i| {
                    let st = &c.st[i];
                    let mut nums: Vec<usize> = Vec::new();
                    for r in &st.references {
                        let n = self.notes.number(r, c.property);
                        if !nums.contains(&n) {
                            nums.push(n);
                        }
                    }
                    StatementModel {
                        id: id_of(st),
                        main: self.snak_model(&st.mainsnak),
                        badge: None,
                        chips: self.statement_chips_model(st),
                        marks: nums.into_iter().map(|n| self.mark(n)).collect(),
                        qualifiers: self.qualifiers_model(st, &skip),
                        period: None,
                        ordinal: None,
                        reason: self
                            .role_of(st, Role::DeprecationReason)
                            .map(|r| self.snak_model(r)),
                    }
                })
                .collect();
        let n = c.g.deprecated.len().to_string();
        Some(Deprecated {
            open: c.g.shape == Shape::NoCurrentValue,
            summary: self.m.with("ts-fold-deprecated", &[&n]),
            items,
        })
    }

    fn footer_model(&self, c: &Card) -> Option<Footer> {
        let count = c.g.n().to_string();
        let hoisted = match (c.g.hoisted.is_empty(), c.g.order.first()) {
            (false, Some(&first)) => {
                let keep = |k: &EntityId| !c.hoisted(k);
                Some(Hoisted {
                    title: self.m.with("ts-same-for-all", &[&count]),
                    qualifiers: self.qualifiers_model(&c.st[first], &keep),
                })
            }
            _ => None,
        };
        let notes = &c.g.footnotes;
        let shared = notes.shared.then(|| Shared {
            text: self.m.with("ts-cited-by-all", &[&count]),
            mark: self.mark(c.nums[0]),
        });
        let unsourced = (notes.unsourced > 0 && notes.unsourced < c.g.n())
            .then(|| self.m.with("ts-unsourced", &[&notes.unsourced.to_string()]));
        if hoisted.is_none() && shared.is_none() && unsourced.is_none() {
            return None;
        }
        Some(Footer {
            hoisted,
            shared,
            unsourced,
        })
    }

    /// The `statements/{P}` region: one statement group in its shape, or the empty group
    /// for a property the entity does not use.
    #[must_use]
    pub fn group_model(&self, property: &str) -> GroupModel {
        let st = self
            .entity
            .statements
            .iter()
            .find(|(p, _)| p.as_str() == property)
            .map_or(&[][..], |(_, v)| v.as_slice());
        let dt = self.datatype_of(property, st);
        let g = classify(st, dt.as_deref(), self.roles, &Thresholds::default());
        let nums = self.cite_m(property, st, &g);
        let c = Card {
            property,
            st,
            g,
            nums,
        };
        let body = match c.g.shape {
            Shape::Empty => Body::Empty {
                text: self.m.get("ts-group-empty"),
            },
            Shape::NoCurrentValue => Body::NoCurrentValue {
                text: self.m.get("ts-no-current-value"),
            },
            Shape::Single => {
                let i = c.g.order[0];
                let skip = |k: &EntityId| c.hoisted(k);
                Body::Single {
                    statement: self.statement_model(&c, i, &skip),
                }
            }
            Shape::List => self.list_body(&c, false),
            Shape::Chips => self.list_body(&c, true),
            Shape::Timeline => self.timeline_body(&c),
            Shape::Series => self.series_body(&c),
            Shape::Table if c.g.numbered => self.numbered_body(&c),
            Shape::Table => self.table_body(&c),
        };
        GroupModel {
            pid: property.to_string(),
            shape: c.g.shape.name().to_string(),
            property: self.property_link_model(property),
            count: (st.len() > 1).then(|| self.m.with("ts-values-count", &[&st.len().to_string()])),
            edit: self.editable.then(|| Edit {
                label: self
                    .m
                    .with("ts-edit-group-label", &[&self.label_of(property)]),
                text: self.m.get("ts-edit-group"),
            }),
            body,
            deprecated: self.deprecated_model(&c),
            footer: self.footer_model(&c),
        }
    }

    /// Numbers a group's references in the tab's endnotes, in the order the group shows
    /// them: its footnotes, then its deprecated values' references. Returns the numbers
    /// of its footnotes.
    fn cite_m(&self, property: &str, st: &[Statement], g: &Group) -> Vec<usize> {
        let nums = g
            .footnotes
            .notes
            .iter()
            .map(|&(si, ri)| self.notes.number(&st[si].references[ri], property))
            .collect();
        for &i in &g.deprecated {
            for r in &st[i].references {
                self.notes.number(r, property);
            }
        }
        nums
    }

    /// Numbers the references of the groups of `properties`, a tab's in its order, so
    /// that a group drawn alone carries the numbers the whole tab would give it.
    pub(super) fn prime_m(&self, properties: &[&str]) {
        self.notes.clear();
        for &p in properties {
            let st = self
                .entity
                .statements
                .iter()
                .find(|(q, _)| q.as_str() == p)
                .map_or(&[][..], |(_, v)| v.as_slice());
            let dt = self.datatype_of(p, st);
            let g = classify(st, dt.as_deref(), self.roles, &Thresholds::default());
            self.cite_m(p, st, &g);
        }
    }

    /// The `statements/{P}` region alone, its footnotes numbered as on its tab.
    #[must_use]
    pub fn group_alone_model(&self, property: &str) -> GroupModel {
        let st = self
            .entity
            .statements
            .iter()
            .find(|(p, _)| p.as_str() == property)
            .map_or(&[][..], |(_, v)| v.as_slice());
        let props = self.tab_properties(self.is_identifier(property, st));
        self.prime_m(&props);
        self.group_model(property)
    }

    /// The tab's endnotes: each reference its groups cite, once, with the groups that
    /// cite it. None where nothing is cited.
    fn endnotes_model(&self) -> Option<EndnotesModel> {
        let notes = self.notes.all();
        if notes.is_empty() {
            return None;
        }
        Some(EndnotesModel {
            title: self.m.get("ts-endnotes-title"),
            notes: notes
                .iter()
                .map(|note| NoteModel {
                    snaks: self.snak_groups_model(&note.reference.snaks),
                    cited: self.rich(
                        "ts-endnotes-cited-for",
                        &[Part::Groups {
                            groups: note
                                .cited_by
                                .iter()
                                .map(|p| (p.clone(), self.label_of(p)))
                                .collect(),
                        }],
                    ),
                })
                .collect(),
        })
    }

    /// The groups of one tab, with its references numbered across them.
    fn groups_model(&self, identifiers: bool) -> Vec<GroupModel> {
        let props = self.tab_properties(identifiers);
        self.prime_m(&props);
        props.into_iter().map(|p| self.group_model(p)).collect()
    }

    fn add_statement_text(&self) -> Option<String> {
        self.editable.then(|| self.m.get("ts-edit-add-statement"))
    }

    /// The Statements tab.
    #[must_use]
    pub fn statements_tab_model(&self) -> StatementsTab {
        let groups = self.groups_model(false);
        StatementsTab {
            heading: self.m.get("ts-tab-statements"),
            empty: groups.is_empty().then(|| self.m.get("ts-statements-none")),
            groups,
            add: self.add_statement_text(),
            endnotes: self.endnotes_model(),
            sources: self.sources_model(),
        }
    }

    /// The `identifiers` region.
    #[must_use]
    pub fn identifiers_model(&self) -> IdentifiersTab {
        let groups = self.groups_model(true);
        IdentifiersTab {
            empty: groups.is_empty().then(|| self.m.get("ts-identifiers-none")),
            groups,
            add: self.add_statement_text(),
            endnotes: self.endnotes_model(),
        }
    }

    /// The `terms` region.
    #[must_use]
    pub fn terms_model(&self) -> Terms {
        let description = self
            .pick(&self.entity.descriptions)
            .map(|(d, l)| (d.to_string(), lang_of(l, self.m.lang())));
        let aliases = self
            .m
            .term_chain()
            .into_iter()
            .find_map(|l| self.entity.aliases.get_key_value(&l))
            .filter(|(_, v)| !v.is_empty())
            .map(|(l, list)| Aliases {
                label: self.m.get("ts-terms-aliases"),
                lang: lang_of(l, self.m.lang()),
                list: list.clone(),
            });
        Terms {
            no_description: description
                .is_none()
                .then(|| self.m.get("ts-terms-no-description")),
            description,
            aliases,
            edit: self.editable.then(|| Edit {
                label: self.m.get("ts-edit-terms-label"),
                text: self.m.get("ts-edit-terms"),
            }),
        }
    }

    /// The `sitelinks` region.
    #[must_use]
    pub fn sitelinks_model(&self) -> SitelinksModel {
        let registry = SiteRegistry::default_registry();
        let mut by_host: BTreeMap<String, Vec<Sitelink>> = BTreeMap::new();
        for (key, l) in &self.entity.sitelinks {
            let site = registry.by_id(&l.site).or_else(|| registry.by_id(key));
            let host = site.map_or_else(|| l.site.clone(), |s| s.host.clone());
            let href = registry
                .sitelink_url(l)
                .or_else(|| l.url.clone())
                .filter(|u| linkable(u));
            let lang = site.and_then(|s| s.language.clone()).unwrap_or_default();
            by_host.entry(host).or_default().push(Sitelink {
                href,
                lang: lang_of(&lang, self.m.lang()),
                title: l.title.clone(),
                badges: l
                    .badges
                    .iter()
                    .map(|b| self.id_link_model(b.as_str(), "item"))
                    .collect(),
            });
        }
        SitelinksModel {
            empty: by_host.is_empty().then(|| self.m.get("ts-sitelinks-none")),
            hosts: by_host
                .into_iter()
                .map(|(host, items)| Host {
                    href: format!("/wiki/{}", title_url(&format!("Domain:{host}"))),
                    chip: Chip::new(ChipKind::Namespace, &host).model(),
                    items,
                })
                .collect(),
        }
    }

    /// The Labels tab.
    #[must_use]
    pub fn labels_model(&self) -> TableModel {
        use crate::codex::{CellModel, ColumnModel};
        use crate::inline::{Inline, join, text};
        let e = self.entity;
        let mut langs: Vec<String> = e
            .labels
            .keys()
            .chain(e.descriptions.keys())
            .chain(e.aliases.keys())
            .cloned()
            .collect();
        langs.sort();
        langs.dedup();
        let chain = self.m.term_chain();
        langs.sort_by_key(|l| chain.iter().position(|c| c == l).unwrap_or(usize::MAX));
        let rows = langs
            .iter()
            .map(|l| {
                let term = |content: Vec<Inline>| CellModel {
                    lang: Some(l.clone()),
                    ..CellModel::of(content)
                };
                vec![
                    CellModel::heading(vec![Inline::Mono { text: l.clone() }]),
                    term(
                        e.labels
                            .get(l)
                            .map(|t| text(t.as_str()))
                            .unwrap_or_default(),
                    ),
                    term(
                        e.descriptions
                            .get(l)
                            .map(|t| text(t.as_str()))
                            .unwrap_or_default(),
                    ),
                    term(
                        e.aliases
                            .get(l)
                            .map(|a| join(a.iter().map(|x| text(x.as_str())).collect(), " · "))
                            .unwrap_or_default(),
                    ),
                ]
            })
            .collect();
        let col = |k: &str| ColumnModel::new(self.m.get(k), Align::Start);
        TableModel {
            header: None,
            caption: self.m.get("ts-labels-caption"),
            columns: vec![
                col("ts-labels-language"),
                col("ts-labels-label"),
                col("ts-labels-description"),
                col("ts-labels-aliases"),
            ],
            rows,
            empty: self.m.get("ts-labels-none"),
        }
    }

    /// "Where this comes from" (0003 §6).
    #[must_use]
    pub fn sources_model(&self) -> Sources {
        let mut graphs = Vec::new();
        let mut retention = None;
        if let Some(p) = self.provenance {
            for g in &p.graphs {
                let name = if g.graph == "local" {
                    self.m.get("ts-sources-local")
                } else if let Some(slug) = g.graph.strip_prefix("mirror/") {
                    self.site
                        .providers
                        .iter()
                        .find(|x| x.slug == slug)
                        .map_or_else(
                            || slug.to_string(),
                            |x| self.m.with("ts-sources-mirror", &[&x.name]),
                        )
                } else {
                    g.graph.clone()
                };
                let synced = g.synced_at.as_ref().map(|at| Synced {
                    text: self
                        .m
                        .with("ts-sources-synced", &[&values::date(self.m, at)]),
                    upstream: g.upstream_version.as_ref().map(|v| Upstream {
                        text: self.m.with("ts-sources-upstream", &[v]),
                        href: g.upstream_url.clone().filter(|u| linkable(u)),
                    }),
                });
                graphs.push(SourceGraph {
                    chip: self.graph_chip_model(&g.graph),
                    name,
                    statements: self
                        .m
                        .with("ts-sources-statements", &[&g.statements.to_string()]),
                    synced,
                });
            }
            retention = p
                .retention
                .as_ref()
                .map(|r| self.m.with("ts-sources-retention", &[r]));
        }
        Sources {
            title: self.m.get("ts-sources-title"),
            unknown: self
                .provenance
                .is_none()
                .then(|| self.m.get("ts-sources-unknown")),
            graphs,
            retention,
            json_href: format!(
                "/wiki/Special:EntityData/{}.json",
                title_url(self.entity.id.as_str())
            ),
            json_text: self.m.get("ts-sources-json"),
        }
    }

    /// The identity line (0010 §2).
    #[must_use]
    pub fn identity_model(&self, restricted: Option<Restricted>) -> Identity {
        let id = &self.entity.id;
        let et = self.entity.entity_type.name();
        let (chip, key, kind) = match id.form() {
            IdForm::Local => {
                let kind = match (et, &self.entity.datatype) {
                    ("property", Some(dt)) => self
                        .m
                        .with("ts-kind-property-datatype", &[&self.datatype_name(dt.id())]),
                    ("property", None) => self.m.get("ts-kind-property"),
                    _ => self.m.get("ts-kind-item"),
                };
                (Chip::new(ChipKind::Local, id.as_str()).model(), None, kind)
            }
            IdForm::Foreign => {
                let code = &id.as_str()[..2];
                let name = self.provider_name(code);
                let key = match et {
                    "item" => "ts-kind-foreign-item",
                    "property" => "ts-kind-foreign-property",
                    _ => "ts-kind-foreign-entity",
                };
                (
                    Chip::new(ChipKind::Provider(code.to_string()), id.as_str()).model(),
                    None,
                    self.m.with(key, &[&name]),
                )
            }
            IdForm::Keyed
                if let Some(source) = self
                    .provenance
                    .filter(|p| p.minted_by.kind == "source")
                    .and_then(|p| p.minted_by.source.as_deref()) =>
            {
                let key = match et {
                    "item" => "ts-kind-source-item",
                    "property" => "ts-kind-source-property",
                    _ => "ts-kind-source-entity",
                };
                (
                    Chip::new(ChipKind::Namespace, id.as_str()).model(),
                    None,
                    self.m.with(key, &[source]),
                )
            }
            IdForm::Keyed => {
                let key = match et {
                    "domain" => "ts-kind-domain",
                    "keyword" => "ts-kind-keyword",
                    "notation" => "ts-kind-notation",
                    _ => "ts-kind-entity",
                };
                let k = self.m.get(key);
                let name = id.keyed_parts().map_or("", |(_, key)| key);
                (
                    Chip::new(ChipKind::Namespace, &k).model(),
                    Some(name.to_string()),
                    self.m.get("ts-kind-keyed"),
                )
            }
        };
        let iri = self.concept_iri().filter(|i| linkable(i)).map(|iri| {
            let shown = iri
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .trim_start_matches("www.")
                .to_string();
            (iri, shown)
        });
        Identity {
            chip,
            key,
            kind,
            adopted: self
                .provenance
                .and_then(|p| p.minted_by.adopted_from.as_deref())
                .map(|from| self.m.with("ts-adopted-from", &[from])),
            iri,
            restricted,
        }
    }
}

/// A Codex message's model, for the empty-page messages the entity routes draw.
#[must_use]
pub fn message_model(kind: crate::codex::MessageKind, text: &str) -> crate::codex::MessageModel {
    crate::codex::Message::block(kind, text).model()
}
