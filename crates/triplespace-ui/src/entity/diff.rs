//! The diff page and old revisions of an entity (0010 §5–6; 19 §3.7–3.8): MediaWiki's
//! addresses on the entity's title — `?diff={revid}` for one change, `?oldid={a}&diff={b}`
//! to compare two local revisions, `?oldid={revid}` to read the entity as it then was —
//! and `?diff={partition}:{offset}` for a record that has no local revision ID, a mirror
//! sync.
//!
//! **A diff is drawn as statements.** Terms and sitelinks are keyed lists with the value
//! before and after; each property the change touched is a group whose statements are
//! marked added, removed or changed, with the unchanged ones kept for context; a changed
//! statement shows both sides. Values are drawn by the entity page's own renderer, so a
//! linked entity shows its label. **Alternative views:** `view=changeset` shows the
//! record's operation as stored. **"About this edit"** lists the graph, the actor, the
//! time, the comment, the size change, the revision and the record's coordinates.
//!
//! **A sync** has no statement diff until the provider's deltas are kept (18 §5.1): the
//! page shows the upstream versions on either side and the stored summary of what
//! changed, per property, language and site.
//!
//! **An old revision** (`oldid=`) is the entity page's statements tab drawn from the
//! local graph's state after that revision, with a banner saying so and linking to the
//! current page and the history. Only local revisions have old states (19 §3.5).
//!
//! What waits for later work: the JSON view, "What this changes" (the effect block,
//! 18 §5.1), the checkpoint and inclusion proof, Undo, and compare controls on the
//! History tab.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::{Rank, Statement};
use serde::Serialize;
use serde_json::Value;
use triplespace_client::revisions::{EntityDiff, RevisionFacts};
use triplespace_client::{Change, ClientError, Comparison, Provenance, SiteInfo};

use super::history::{Choice, Switch};
use super::model::{self, Link, Qualifier, StatementsTab};
use super::{Lookup, Render, page_title, referenced, site_roles};
use crate::Site;
use crate::codex::{Chip, ChipKind, ChipModel, MessageKind, MessageModel};
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context};

/// What the query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// `diff={revid}`: one local change.
    Change(u64),
    /// `diff={partition}:{offset}`: one record.
    Record(u64, u64),
    /// `oldid={a}&diff={b}`: two local revisions.
    Compare(u64, u64),
    /// `oldid={revid}`: the entity as it then was.
    Old(u64),
}

impl Ask {
    /// From the query; `None` when it asks for none of these.
    #[must_use]
    pub fn of(query: &BTreeMap<String, String>) -> Option<Self> {
        let oldid = query
            .get("oldid")
            .and_then(|v| v.trim().parse::<u64>().ok());
        let diff = query.get("diff").map(|d| d.trim());
        match (oldid, diff) {
            (Some(a), Some(b)) if b.parse::<u64>().is_ok() => {
                Some(Self::Compare(a, b.parse().ok()?))
            }
            (_, Some(d)) if d.parse::<u64>().is_ok() => Some(Self::Change(d.parse().ok()?)),
            (_, Some(d)) => {
                let (p, o) = d.split_once(':')?;
                Some(Self::Record(p.parse().ok()?, o.parse().ok()?))
            }
            (Some(a), None) => Some(Self::Old(a)),
            (None, None) => None,
        }
    }
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `entity/diff.html` and `entity/old.html` draw.
// ---------------------------------------------------------------------------------------

/// A revision's line in the header: time, actor, comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevisionLine {
    /// The link to the revision, when it has an ID.
    pub revision: Option<Choice>,
    /// ISO 8601.
    pub datetime: String,
    /// The time as shown.
    pub time: String,
    /// The actor's contributions, when known.
    pub actor: Option<Choice>,
    /// The comment, when there is one.
    pub comment: Option<String>,
}

/// A keyed list's row: a term or a sitelink before and after.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyedRow {
    /// The language or the site.
    pub key: String,
    /// The values' language, where the key is one that differs from the page's.
    pub lang: Option<String>,
    /// Before.
    pub before: Option<String>,
    /// The before cell's class: `removed`, `changed`, `same` or `empty`.
    pub before_class: &'static str,
    /// After.
    pub after: Option<String>,
    /// The after cell's class: `added`, `changed`, `same` or `empty`.
    pub after_class: &'static str,
}

/// A keyed list: labels, descriptions, aliases or sitelinks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyedTable {
    /// The section's heading.
    pub heading: String,
    /// The rows.
    pub rows: Vec<KeyedRow>,
}

/// A reference as the diff draws it: its snak groups.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct References {
    /// The accordion's title.
    pub title: String,
    /// Each reference's snaks.
    pub list: Vec<Vec<Qualifier>>,
}

/// A statement as the diff draws it: the value, the rank, qualifiers and references.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffStatement {
    /// The main value.
    pub main: model::Value,
    /// The rank chip, for a preferred or deprecated statement.
    pub rank: Option<ChipModel>,
    /// The qualifiers.
    pub qualifiers: Vec<Qualifier>,
    /// The references, when there are any.
    pub references: Option<References>,
}

/// A statement's row in a group: its fate and both sides.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaimRow {
    /// The fate as said: Added, Removed, Changed, Unchanged.
    pub mark: String,
    /// The fate's class: `added`, `removed`, `changed` or `unchanged`.
    pub class: &'static str,
    /// Before.
    pub before: Option<DiffStatement>,
    /// The before cell's class.
    pub before_class: &'static str,
    /// After.
    pub after: Option<DiffStatement>,
    /// The after cell's class.
    pub after_class: &'static str,
}

/// A property's statements in the diff.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClaimsGroup {
    /// The property.
    pub property: Link,
    /// The rows.
    pub rows: Vec<ClaimRow>,
}

/// A sync summary's key: a property, or a section and its key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SummaryKey {
    /// A property.
    Property {
        /// The link.
        link: Link,
    },
    /// "Labels: en".
    Text {
        /// The text.
        text: String,
    },
}

/// A sync summary's row: the counts per key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryRow {
    /// The key.
    pub key: SummaryKey,
    /// Added.
    pub added: u64,
    /// Removed.
    pub removed: u64,
    /// Changed.
    pub changed: u64,
}

/// The diff's content.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Main {
    /// The record's operation as stored.
    Changeset {
        /// Pretty JSON.
        json: String,
    },
    /// A sync: the upstream versions and the stored summary.
    Sync {
        /// "Synced from version A to B".
        line: String,
        /// The summary's rows, when the sync stored any.
        rows: Vec<SummaryRow>,
        /// What the page says when it stored none.
        none: String,
    },
    /// The statement diff.
    Diff {
        /// What the page says when nothing changed.
        nothing: Option<String>,
        /// Labels, descriptions, aliases, sitelinks.
        tables: Vec<KeyedTable>,
        /// The statement groups.
        claims: Vec<ClaimsGroup>,
    },
    /// Nothing.
    None,
}

/// "About this edit"'s value: text, left-to-right text, or a record's coordinates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AboutValue {
    /// Text.
    Text {
        /// The text.
        text: String,
    },
    /// Text kept left-to-right: a size.
    Ltr {
        /// The text.
        text: String,
    },
    /// Monospace, left-to-right: the record's coordinates.
    Mono {
        /// The text.
        text: String,
    },
}

/// "About this edit".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct About {
    /// The rows: the graph, the actor, the time, the comment, the size change, the
    /// revision and the record's coordinates.
    pub rows: Vec<(String, AboutValue)>,
}

/// The two revisions of a comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Revisions {
    /// Before.
    pub before: RevisionLine,
    /// After.
    pub after: RevisionLine,
}

/// The links beside one change: the previous change, and comparing with it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Nav {
    /// The previous change.
    pub previous: Choice,
    /// The comparison with it.
    pub compare: Choice,
}

/// The diff page's body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffModel {
    /// One change's revision line.
    pub head: Option<RevisionLine>,
    /// One change's links to the previous.
    pub nav: Option<Nav>,
    /// A comparison's two revisions.
    pub revisions: Option<Revisions>,
    /// The view switch, for a change.
    pub switch: Option<Switch>,
    /// The content.
    pub main: Main,
    /// "About this edit", for a change.
    pub about: Option<About>,
}

/// An old revision's body: the banner, the links and the statements tab as it then was.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OldModel {
    /// The banner.
    pub banner: MessageModel,
    /// The current page.
    pub current: Choice,
    /// The history.
    pub history: Choice,
    /// The statements tab's heading.
    pub heading: String,
    /// The statements tab.
    pub tab: StatementsTab,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn revision_line_model(m: &Messages, facts: &RevisionFacts, title: &str) -> RevisionLine {
    RevisionLine {
        revision: facts.revid.map(|r| {
            choice(
                format!("/wiki/{}?oldid={r}", title_url(title)),
                m.with("ts-diff-revision", &[&r.to_string()]),
            )
        }),
        datetime: facts.time.clone(),
        time: facts
            .time
            .replace('T', " ")
            .trim_end_matches('Z')
            .to_string(),
        actor: facts.actor.as_ref().map(|a| {
            choice(
                format!("/wiki/Special:Contributions/{}", title_url(a)),
                a.clone(),
            )
        }),
        comment: facts.comment.clone().filter(|c| !c.is_empty()),
    }
}

fn keyed_table_model(
    m: &Messages,
    heading: String,
    rows: &[(String, Option<String>, Option<String>, bool)],
) -> Option<KeyedTable> {
    if rows.is_empty() {
        return None;
    }
    let rows = rows
        .iter()
        .map(|(key, before, after, is_lang)| {
            let lang = is_lang
                .then(|| key.as_str())
                .filter(|l| !l.is_empty() && *l != m.lang())
                .map(str::to_string);
            let before_class = match (before, after) {
                (None, _) => "empty",
                (Some(_), None) => "removed",
                (Some(b), Some(a)) if b != a => "changed",
                _ => "same",
            };
            let after_class = match (before, after) {
                (_, None) => "empty",
                (None, Some(_)) => "added",
                (Some(b), Some(a)) if b != a => "changed",
                _ => "same",
            };
            KeyedRow {
                key: key.clone(),
                lang,
                before: before.clone(),
                before_class,
                after: after.clone(),
                after_class,
            }
        })
        .collect();
    Some(KeyedTable { heading, rows })
}

fn statement_model(r: &Render<'_>, st: &Statement) -> DiffStatement {
    let rank = match st.rank {
        Rank::Normal => None,
        Rank::Preferred => Some(Chip::new(ChipKind::Rank, &r.m.get("ts-rank-preferred")).model()),
        Rank::Deprecated => Some(Chip::new(ChipKind::Rank, &r.m.get("ts-rank-deprecated")).model()),
    };
    DiffStatement {
        main: r.snak_model(&st.mainsnak),
        rank,
        qualifiers: r.snak_groups_model(&st.qualifiers),
        references: (!st.references.is_empty()).then(|| References {
            title: r
                .m
                .with("ts-references", &[&st.references.len().to_string()]),
            list: st
                .references
                .iter()
                .map(|rf| r.snak_groups_model(&rf.snaks))
                .collect(),
        }),
    }
}

fn claims_model(r: &Render<'_>, diff: &EntityDiff) -> Vec<ClaimsGroup> {
    let m = r.m;
    diff.claims
        .iter()
        .map(|g| ClaimsGroup {
            property: r.property_link_model(&g.property),
            rows: g
                .statements
                .iter()
                .map(|st| {
                    let (label, class, before, after) = match st.change.as_str() {
                        "added" => (
                            "ts-diff-added",
                            "added",
                            None,
                            parse_statement(st.after.as_ref()),
                        ),
                        "removed" => (
                            "ts-diff-removed",
                            "removed",
                            parse_statement(st.before.as_ref()),
                            None,
                        ),
                        "changed" => (
                            "ts-diff-changed",
                            "changed",
                            parse_statement(st.before.as_ref()),
                            parse_statement(st.after.as_ref()),
                        ),
                        _ => {
                            let same = parse_statement(st.statement.as_ref());
                            ("ts-diff-unchanged", "unchanged", same.clone(), same)
                        }
                    };
                    let (before_class, after_class) = match class {
                        "added" => ("empty", "added"),
                        "removed" => ("removed", "empty"),
                        "changed" => ("changed", "changed"),
                        _ => ("same", "same"),
                    };
                    ClaimRow {
                        mark: m.get(label),
                        class,
                        before: before.as_ref().map(|st| statement_model(r, st)),
                        before_class,
                        after: after.as_ref().map(|st| statement_model(r, st)),
                        after_class,
                    }
                })
                .collect(),
        })
        .collect()
}

/// The whole diff: terms, aliases, sitelinks, then the statement groups.
fn diff_model(r: &Render<'_>, diff: &EntityDiff) -> Main {
    let m = r.m;
    if diff.is_empty() {
        return Main::Diff {
            nothing: Some(m.get("ts-diff-nothing")),
            tables: Vec::new(),
            claims: Vec::new(),
        };
    }
    let terms = |changes: &[triplespace_client::revisions::TermChange]| {
        changes
            .iter()
            .map(|t| (t.language.clone(), t.before.clone(), t.after.clone(), true))
            .collect::<Vec<_>>()
    };
    let aliases: Vec<_> = diff
        .aliases
        .iter()
        .map(|a| {
            let mut before: Vec<&str> = a.kept.iter().map(String::as_str).collect();
            before.extend(a.removed.iter().map(String::as_str));
            let mut after: Vec<&str> = a.kept.iter().map(String::as_str).collect();
            after.extend(a.added.iter().map(String::as_str));
            (
                a.language.clone(),
                (!before.is_empty()).then(|| before.join(" | ")),
                (!after.is_empty()).then(|| after.join(" | ")),
                true,
            )
        })
        .collect();
    let sitelinks: Vec<_> = diff
        .sitelinks
        .iter()
        .map(|l| {
            (
                l.site.clone(),
                sitelink_text(l.before.as_ref()),
                sitelink_text(l.after.as_ref()),
                false,
            )
        })
        .collect();
    let tables = [
        keyed_table_model(m, m.get("ts-diff-labels"), &terms(&diff.labels)),
        keyed_table_model(m, m.get("ts-diff-descriptions"), &terms(&diff.descriptions)),
        keyed_table_model(m, m.get("ts-diff-aliases"), &aliases),
        keyed_table_model(m, m.get("ts-diff-sitelinks"), &sitelinks),
    ]
    .into_iter()
    .flatten()
    .collect();
    Main::Diff {
        nothing: None,
        tables,
        claims: claims_model(r, diff),
    }
}

fn about_model(m: &Messages, site: &SiteInfo, change: &Change) -> About {
    let text = |t: String| AboutValue::Text { text: t };
    let mut rows = Vec::new();
    let graph = match change.source.strip_prefix("mirror/") {
        Some(slug) => site
            .providers
            .iter()
            .find(|p| p.slug == slug)
            .map_or_else(|| change.source.clone(), |p| p.name.clone()),
        None => m.get("ts-history-made-here"),
    };
    rows.push((m.get("ts-diff-about-graph"), text(graph)));
    if let Some(r) = change.revision.revid {
        rows.push((m.get("ts-diff-about-revision"), text(r.to_string())));
    }
    if let Some(op) = &change.revision.op {
        rows.push((m.get("ts-diff-about-operation"), text(op.clone())));
    }
    if let (Some(size), Some(delta)) = (change.size, change.delta) {
        let sign = if delta >= 0 { "+" } else { "\u{2212}" };
        rows.push((
            m.get("ts-diff-about-size"),
            AboutValue::Ltr {
                text: format!("{size} ({sign}{})", delta.abs()),
            },
        ));
    } else if let Some(size) = change.size {
        rows.push((
            m.get("ts-diff-about-size"),
            AboutValue::Ltr {
                text: size.to_string(),
            },
        ));
    }
    rows.push((
        m.get("ts-diff-about-record"),
        AboutValue::Mono {
            text: format!("{}:{}", change.revision.partition, change.revision.offset),
        },
    ));
    if !change.revision.tags.is_empty() {
        rows.push((
            m.get("ts-diff-about-tags"),
            text(change.revision.tags.join(", ")),
        ));
    }
    About { rows }
}

fn summary_rows(m: &Messages, r: &Render<'_>, changes: &Value) -> Vec<SummaryRow> {
    let mut rows = Vec::new();
    for (section, is_property) in [
        ("labels", false),
        ("descriptions", false),
        ("aliases", false),
        ("sitelinks", false),
        ("claims", true),
    ] {
        let Some(map) = changes.get(section).and_then(Value::as_object) else {
            continue;
        };
        for (key, counts) in map {
            let n = |k: &str| counts.get(k).and_then(Value::as_u64).unwrap_or(0);
            rows.push(SummaryRow {
                key: if is_property {
                    SummaryKey::Property {
                        link: r.property_link_model(key),
                    }
                } else {
                    SummaryKey::Text {
                        text: format!("{}: {key}", m.get(&format!("ts-diff-{section}"))),
                    }
                },
                added: n("added"),
                removed: n("removed"),
                changed: n("changed"),
            });
        }
    }
    rows
}

fn view_switch_model(
    m: &Messages,
    title: &str,
    query: &BTreeMap<String, String>,
    current: &str,
) -> Switch {
    let mut base = format!("/wiki/{}?", title_url(title));
    for key in ["oldid", "diff"] {
        if let Some(v) = query.get(key) {
            let _ = write!(base, "{key}={v}&");
        }
    }
    Switch {
        label: m.get("ts-diff-views-label"),
        items: [
            ("statements", "ts-diff-view-statements"),
            ("changeset", "ts-diff-view-changeset"),
        ]
        .into_iter()
        .map(|(view, msg)| Choice {
            href: format!("{base}view={view}"),
            label: m.get(msg),
            current: view == current,
        })
        .collect(),
    }
}

/// The upstream version a sync names: the revision ID, or the version.
fn upstream_text(v: Option<&Value>) -> String {
    v.and_then(|u| {
        u.get("revid")
            .and_then(Value::as_u64)
            .map(|n| n.to_string())
            .or_else(|| u.get("version").and_then(Value::as_str).map(str::to_string))
    })
    .unwrap_or_else(|| "–".into())
}

/// The diff page's model.
#[allow(clippy::too_many_arguments)]
fn diff_page_model(
    r: &Render<'_>,
    site: &SiteInfo,
    title: &str,
    query: &BTreeMap<String, String>,
    view: &str,
    change: Option<&Change>,
    comparison: Option<&Comparison>,
    diff: Option<&EntityDiff>,
) -> DiffModel {
    let m = r.m;
    let mut model = DiffModel {
        head: None,
        nav: None,
        revisions: None,
        switch: None,
        main: Main::None,
        about: change.map(|c| about_model(m, site, c)),
    };
    match (change, comparison) {
        (Some(c), _) => {
            model.head = Some(revision_line_model(m, &c.revision, title));
            let prev = c.previous.as_ref().and_then(|p| p["revid"].as_u64());
            if let (Some(p), Some(rv)) = (prev, c.revision.revid) {
                model.nav = Some(Nav {
                    previous: choice(
                        format!("/wiki/{}?diff={p}", title_url(title)),
                        m.get("ts-diff-previous"),
                    ),
                    compare: choice(
                        format!("/wiki/{}?oldid={p}&diff={rv}", title_url(title)),
                        m.get("ts-diff-compare-previous"),
                    ),
                });
            }
        }
        (None, Some(c)) => {
            model.revisions = Some(Revisions {
                before: revision_line_model(m, &c.from, title),
                after: revision_line_model(m, &c.to, title),
            });
        }
        (None, None) => {}
    }
    model.main = match (change, comparison, view) {
        (Some(c), _, "changeset") => {
            model.switch = Some(view_switch_model(m, title, query, view));
            Main::Changeset {
                json: serde_json::to_string_pretty(c.operation.as_ref().unwrap_or(&Value::Null))
                    .unwrap_or_default(),
            }
        }
        (Some(c), _, _) if c.kind == "sync" => Main::Sync {
            line: m.with(
                "ts-diff-sync-line",
                &[
                    &upstream_text(c.revision.prev_upstream.as_ref()),
                    &upstream_text(c.revision.upstream.as_ref()),
                ],
            ),
            rows: match &c.changes {
                Some(ch) if !ch.is_null() => summary_rows(m, r, ch),
                _ => Vec::new(),
            },
            none: m.get("ts-diff-sync-no-summary"),
        },
        (Some(_), _, _) | (None, Some(_), _) => {
            if change.is_some() {
                model.switch = Some(view_switch_model(m, title, query, "statements"));
            }
            diff.map_or(Main::None, |d| diff_model(r, d))
        }
        (None, None, _) => Main::None,
    };
    model
}

fn sitelink_text(v: Option<&Value>) -> Option<String> {
    let v = v?;
    let title = v.get("title").and_then(Value::as_str)?;
    let badges: Vec<&str> = v
        .get("badges")
        .and_then(Value::as_array)
        .map(|b| b.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    Some(if badges.is_empty() {
        title.to_string()
    } else {
        format!("{title} ({})", badges.join(", "))
    })
}

fn parse_statement(v: Option<&Value>) -> Option<Statement> {
    serde_json::from_value(v?.clone()).ok()
}

/// The IDs a diff refers to, for the label lookup: both sides' statements.
fn referenced_in(diff: &EntityDiff, entity: &Entity, site: &SiteInfo) -> Vec<String> {
    let mut probe = Entity::new(entity.id.clone(), entity.entity_type.clone());
    for g in &diff.claims {
        let Ok(property) = EntityId::parse(&g.property) else {
            continue;
        };
        let mut sts = Vec::new();
        for st in &g.statements {
            for v in [&st.before, &st.after, &st.statement] {
                if let Some(parsed) = parse_statement(v.as_ref()) {
                    sts.push(parsed);
                }
            }
        }
        probe.statements.insert(property, sts);
    }
    referenced(&probe, site)
}

/// The page's tabs: the entity's, with History current.
fn tabs(m: &Messages, title: &str, items: bool) -> Vec<Tab> {
    super::tabs(m, title, super::TabName::History, items, true)
}

/// The label lookup for a diff's values.
async fn lookup_for(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    entity: &Entity,
    diff: Option<&EntityDiff>,
) -> Result<Lookup, ClientError> {
    let chain = cx.m.term_chain();
    let mut ids = referenced(entity, &cx.site);
    if let Some(d) = diff {
        ids.extend(referenced_in(d, entity, &cx.site));
    }
    ids.sort();
    ids.dedup();
    let mut lookup = if ids.is_empty() {
        Lookup::default()
    } else {
        let f = site
            .client()
            .entities(incoming, &ids, "labels|datatype", &chain.join("|"))
            .await?;
        cx.inputs.push(f.cache);
        Lookup::from_entities(&f.value.found, &chain)
    };
    lookup.add_self(entity, &chain);
    Ok(lookup)
}

/// What a diff address resolved to.
type Found = Option<(Option<Change>, Option<Comparison>)>;

/// Serves a diff, a comparison or an old revision of the entity.
#[allow(clippy::too_many_lines, clippy::too_many_arguments)]
pub async fn serve(
    site: &Site,
    mut cx: Context,
    incoming: &triplespace_client::Incoming,
    headers: &HeaderMap,
    entity: &Entity,
    provenance: Option<&Provenance>,
    ask: &Ask,
    query: &BTreeMap<String, String>,
) -> Response {
    let client = site.client();
    let title = page_title(entity.id.as_str(), entity.entity_type.name());
    let items = entity.entity_type.name() == "item";
    let roles = site_roles(&cx.site);
    let view = query.get("view").map_or("statements", String::as_str);

    // An old revision: the statements tab as it then was.
    if let Ask::Old(revid) = ask {
        let rev = match client.revision(incoming, *revid).await {
            Ok(f) => {
                cx.inputs.push(f.cache);
                f.value
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        };
        let old = rev
            .as_ref()
            .filter(|r| r.title == title)
            .and_then(|r| r.content.as_deref())
            .and_then(|c| serde_json::from_str::<Value>(c).ok())
            .and_then(|v| Entity::from_value(v).ok())
            .map(|p| p.entity);
        let Some(old) = old else {
            return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
                title: title.clone(),
                returnto: Some(title.clone()),
                body: cx.message(
                    MessageKind::Warning,
                    &m.with("ts-oldid-missing", &[&revid.to_string()]),
                ),
                ..Page::default()
            });
        };
        let lookup = match lookup_for(site, &mut cx, incoming, &old, None).await {
            Ok(l) => l,
            Err(e) => return pages::api_failed(&cx, headers, &e),
        };
        let r = Render {
            m: &cx.m,
            site: &cx.site,
            lookup: &lookup,
            entity: &old,
            provenance,
            roles: &roles,
            editable: false,
            notes: super::Endnotes::default(),
        };
        let facts = rev.as_ref().expect("checked above");
        let tab = r.statements_tab_model();
        let body = cx.draw(
            "entity/old.html",
            OldModel {
                banner: model::message_model(
                    MessageKind::Notice,
                    &cx.m.with(
                        "ts-oldid-banner",
                        &[
                            &revid.to_string(),
                            facts.user.as_deref().unwrap_or(""),
                            facts.timestamp.replace('T', " ").trim_end_matches('Z'),
                        ],
                    ),
                ),
                current: choice(
                    format!("/wiki/{}", title_url(&title)),
                    cx.m.get("ts-oldid-current"),
                ),
                history: choice(
                    format!("/wiki/{}?action=history", title_url(&title)),
                    cx.m.get("ts-tab-history"),
                ),
                heading: tab.heading.clone(),
                tab,
            },
        );
        let (display, identity, terms, doc_title) = (
            r.display_title(),
            super::draw(&r, &cx, &super::Part::Identity(None)),
            super::draw(&r, &cx, &super::Part::Terms),
            r.document_title(),
        );
        let tabs = super::tabs(&cx.m, &title, super::TabName::Statements, items, true);
        return pages::respond(&cx, headers, StatusCode::OK, move |_| Page {
            title: display,
            doc_title: Some(doc_title),
            returnto: Some(title),
            identity_html: Some(identity),
            subtitle_html: Some(terms),
            tabs,
            body,
            ..Page::default()
        });
    }

    // A change or a comparison.
    let fetched: Result<Found, ClientError> =
        match ask {
            Ask::Change(revid) => match client.revision(incoming, *revid).await {
                Ok(f) => {
                    cx.inputs.push(f.cache);
                    match f.value.and_then(|r| r.record) {
                        Some((p, o)) => client
                            .change(incoming, entity.id.as_str(), p, o)
                            .await
                            .map(|f| {
                                cx.inputs.push(f.cache);
                                f.value.map(|c| (Some(c), None))
                            }),
                        None => Ok(None),
                    }
                }
                Err(e) => Err(e),
            },
            Ask::Record(p, o) => client
                .change(incoming, entity.id.as_str(), *p, *o)
                .await
                .map(|f| {
                    cx.inputs.push(f.cache);
                    f.value.map(|c| (Some(c), None))
                }),
            Ask::Compare(a, b) => client
                .compare(incoming, entity.id.as_str(), *a, *b)
                .await
                .map(|f| {
                    cx.inputs.push(f.cache);
                    f.value.map(|c| (None, Some(c)))
                }),
            Ask::Old(_) => unreachable!("served above"),
        };
    let found = match fetched {
        Ok(f) => f,
        Err(ClientError::Api { code, info })
            if code == "ts-compare-source" || code == "bad-parameter" =>
        {
            return pages::respond(&cx, headers, StatusCode::BAD_REQUEST, |_| Page {
                title: title.clone(),
                returnto: Some(title.clone()),
                body: cx.message(MessageKind::Error, &info),
                ..Page::default()
            });
        }
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let Some((change, comparison)) = found else {
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: title.clone(),
            returnto: Some(title.clone()),
            body: cx.message(MessageKind::Warning, &m.get("ts-diff-missing")),
            ..Page::default()
        });
    };
    let diff: Option<&EntityDiff> = change
        .as_ref()
        .and_then(|c| c.diff.as_ref())
        .or_else(|| comparison.as_ref().map(|c| &c.diff));
    let lookup = match lookup_for(site, &mut cx, incoming, entity, diff).await {
        Ok(l) => l,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let r = Render {
        m: &cx.m,
        site: &cx.site,
        lookup: &lookup,
        entity,
        provenance,
        roles: &roles,
        editable: false,
        notes: super::Endnotes::default(),
    };
    let m = &cx.m;
    let body = cx.draw(
        "entity/diff.html",
        diff_page_model(
            &r,
            &cx.site,
            &title,
            query,
            view,
            change.as_ref(),
            comparison.as_ref(),
            diff,
        ),
    );
    let (display, identity, terms, doc_title) = (
        r.display_title(),
        super::draw(&r, &cx, &super::Part::Identity(None)),
        super::draw(&r, &cx, &super::Part::Terms),
        r.document_title(),
    );
    let tabs = tabs(m, &title, items);
    pages::respond(&cx, headers, StatusCode::OK, move |_| Page {
        title: display,
        doc_title: Some(doc_title),
        returnto: Some(title),
        identity_html: Some(identity),
        subtitle_html: Some(terms),
        tabs,
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asks_follow_mediawikis_addresses() {
        let q = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect()
        };
        assert_eq!(Ask::of(&q(&[])), None);
        assert_eq!(Ask::of(&q(&[("diff", "907")])), Some(Ask::Change(907)));
        assert_eq!(
            Ask::of(&q(&[("diff", "8194:41")])),
            Some(Ask::Record(8194, 41))
        );
        assert_eq!(
            Ask::of(&q(&[("oldid", "900"), ("diff", "907")])),
            Some(Ask::Compare(900, 907))
        );
        assert_eq!(Ask::of(&q(&[("oldid", "900")])), Some(Ask::Old(900)));
        assert_eq!(
            Ask::of(&q(&[("diff", "prev")])),
            None,
            "MediaWiki's keywords are not served yet"
        );
    }

    #[test]
    fn sitelinks_show_their_badges() {
        assert_eq!(
            sitelink_text(Some(
                &serde_json::json!({"site": "enwiki", "title": "Six", "badges": ["Q17437796"]})
            ))
            .as_deref(),
            Some("Six (Q17437796)")
        );
        assert_eq!(sitelink_text(None), None);
    }
}
