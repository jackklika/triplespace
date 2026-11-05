//! The resolved view of one entity (0002 §3; 0004 §8): what the source graphs say about
//! it, reconciled by the default rules, with the local graph's corrections applied.
//!
//! The crate is pure and knows no registry: the contributions arrive in **graph order**,
//! the local graph first and then the mirrors in the instance's provider order (0004 §4),
//! and the corrections arrive as data. The rules:
//!
//! | What | Rule |
//! |---|---|
//! | Statements, references, aliases | Union. Statements with the same key (main snak and qualifiers, 0004 §8) fuse into one; references are combined, duplicates removed by hash |
//! | Labels, descriptions, rank | The first graph in order that says anything wins |
//! | Sitelinks | Union by site; the first graph in order wins a site |
//! | Corrections | A rank override applies to the fused statement; a suppression removes the member statement it names, and the fused statement once every member is suppressed |
//!
//! Clusters are not handled here yet: every ID is taken as its own canonical ID, which is
//! the [`Identity`] canon. When `scatter-identity` exists, the canon becomes a parameter.
//!
//! [`resolve`] also says, per fused statement, which graphs assert it, so that the
//! `identifier` and `statement_assertion` tables can be written from the same pass.

#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use scatter_wikibase_model::entity::Entity;
use scatter_wikibase_model::hash::Hasher;
use scatter_wikibase_model::id::StatementId;
use scatter_wikibase_model::key::{Identity, statement_key};
use scatter_wikibase_model::statement::{Rank, Statement};

/// One graph's state for the entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    /// The graph name: `local`, `mirror/wikidata`, …
    pub graph: String,
    /// The state the graph asserts. For a local graph that only adds to a foreign
    /// subject this is the partial state the adds built.
    pub entity: Entity,
}

/// A local correction of one statement (0002 §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Correction {
    /// Show the statement with this rank.
    Rank(Rank),
    /// Hide the member statement.
    Suppress,
}

/// One statement of the resolved view and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedStatement {
    /// The statement key (0004 §8).
    pub key: String,
    /// The graphs that assert a member, in graph order.
    pub graphs: Vec<String>,
    /// The members' own IDs, in graph order, where they have one.
    pub members: Vec<StatementId>,
}

/// The resolved view of one entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// The resolved state. Its ID is the first contribution's.
    pub entity: Entity,
    /// Per statement in `entity`, in the same order, its key and provenance.
    pub statements: Vec<ResolvedStatement>,
    /// The graphs that contributed, in order.
    pub graphs: Vec<String>,
}

impl Resolved {
    /// The canonical JSON of the resolved state, in the storage form.
    #[must_use]
    pub fn canonical_json(&self) -> String {
        self.entity.to_canonical_json()
    }

    /// Whether the resolved state is exactly one contribution's state, so that a reader
    /// may take the record itself rather than a stored copy (0013 §5.1).
    #[must_use]
    pub fn equals(&self, contribution: &Entity) -> bool {
        self.equals_given(&self.canonical_json(), contribution)
    }

    /// [`Self::equals`], given this state's canonical JSON, which a caller that needs it
    /// anyway has already built: one serialization instead of two. A structural comparison
    /// would be cheaper still but is not exact: a statement ID compares without case and is
    /// written as given, and `-0.0 == 0.0` serializes two ways.
    #[must_use]
    pub fn equals_given(&self, canonical_json: &str, contribution: &Entity) -> bool {
        debug_assert_eq!(canonical_json, self.canonical_json());
        canonical_json == contribution.to_canonical_json()
    }
}

/// Resolves the contributions, in graph order, under the corrections. `None` when there
/// are no contributions.
#[must_use]
pub fn resolve(
    contributions: &[Contribution],
    corrections: &BTreeMap<StatementId, Correction>,
) -> Option<Resolved> {
    let first = contributions.first()?;
    let mut entity = Entity::new(first.entity.id.clone(), first.entity.entity_type.clone());
    entity.datatype = contributions.iter().find_map(|c| c.entity.datatype.clone());
    let hasher = Hasher::local();

    merge_terms(&mut entity, contributions);

    // Statements: fuse by key across graphs, in the order first seen.
    let mut fused: Vec<(Statement, ResolvedStatement, bool)> = Vec::new();
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    for c in contributions {
        for s in c.entity.all_statements() {
            let suppressed =
                s.id.as_ref()
                    .is_some_and(|id| corrections.get(id) == Some(&Correction::Suppress));
            let key = statement_key(s, &Identity);
            if let Some(&i) = index.get(&key) {
                let (base, prov, all_suppressed) = &mut fused[i];
                prov.graphs.push(c.graph.clone());
                if let Some(id) = &s.id {
                    prov.members.push(id.clone());
                }
                *all_suppressed &= suppressed;
                if !suppressed {
                    merge_references(base, s, &hasher);
                    if base.id.is_none() {
                        base.id.clone_from(&s.id);
                    }
                }
            } else {
                index.insert(key.clone(), fused.len());
                let mut base = s.clone();
                dedupe_references(&mut base, &hasher);
                fused.push((
                    base,
                    ResolvedStatement {
                        key,
                        graphs: vec![c.graph.clone()],
                        members: s.id.iter().cloned().collect(),
                    },
                    suppressed,
                ));
            }
        }
    }
    let mut statements = Vec::new();
    for (mut s, prov, all_suppressed) in fused {
        if all_suppressed {
            continue;
        }
        // A rank override on any member applies to the fused statement; the first
        // graph's rank otherwise, which is the local graph's when it asserts the statement.
        if let Some(Correction::Rank(r)) = prov
            .members
            .iter()
            .find_map(|m| corrections.get(m))
            .filter(|c| matches!(c, Correction::Rank(_)))
        {
            s.rank = *r;
        }
        entity
            .statements
            .entry(s.mainsnak.property.clone())
            .or_default()
            .push(s);
        statements.push(prov);
    }
    // `statements` is in fused order; `entity.statements` groups by property. Re-derive
    // the provenance list in the entity's own order so the two line up.
    let by_key: BTreeMap<String, ResolvedStatement> =
        statements.into_iter().map(|p| (p.key.clone(), p)).collect();
    let ordered: Vec<ResolvedStatement> = entity
        .all_statements()
        .map(|s| by_key[&statement_key(s, &Identity)].clone())
        .collect();

    Some(Resolved {
        entity,
        statements: ordered,
        graphs: contributions.iter().map(|c| c.graph.clone()).collect(),
    })
}

/// Terms: the first graph wins a language; aliases are a union in order; the first graph
/// wins a sitelink's site.
fn merge_terms(entity: &mut Entity, contributions: &[Contribution]) {
    for c in contributions {
        for (lang, text) in &c.entity.labels {
            entity
                .labels
                .entry(lang.clone())
                .or_insert_with(|| text.clone());
        }
        for (lang, text) in &c.entity.descriptions {
            entity
                .descriptions
                .entry(lang.clone())
                .or_insert_with(|| text.clone());
        }
        for (lang, values) in &c.entity.aliases {
            let list = entity.aliases.entry(lang.clone()).or_default();
            for v in values {
                if !list.contains(v) {
                    list.push(v.clone());
                }
            }
        }
        for (site, link) in &c.entity.sitelinks {
            entity
                .sitelinks
                .entry(site.clone())
                .or_insert_with(|| link.clone());
        }
    }
}

fn reference_hashes(s: &Statement, hasher: &Hasher) -> BTreeSet<String> {
    s.references
        .iter()
        .map(|r| r.hash.clone().unwrap_or_else(|| hasher.reference(r)))
        .collect()
}

fn dedupe_references(s: &mut Statement, hasher: &Hasher) {
    let mut seen = BTreeSet::new();
    s.references
        .retain(|r| seen.insert(r.hash.clone().unwrap_or_else(|| hasher.reference(r))));
}

fn merge_references(base: &mut Statement, other: &Statement, hasher: &Hasher) {
    let mut have = reference_hashes(base, hasher);
    for r in &other.references {
        let h = r.hash.clone().unwrap_or_else(|| hasher.reference(r));
        if have.insert(h) {
            base.references.push(r.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scatter_wikibase_model::id::EntityId;
    use serde_json::json;

    fn entity(v: serde_json::Value) -> Entity {
        Entity::from_value(v).unwrap().entity
    }

    fn statement(
        p: &str,
        v: &str,
        id: Option<&str>,
        rank: &str,
        refs: &[&str],
    ) -> serde_json::Value {
        let mut s = json!({"mainsnak": {"snaktype": "value", "property": p,
            "datavalue": {"value": v, "type": "string"}, "datatype": "string"},
            "type": "statement", "rank": rank,
            "references": refs.iter().map(|r| json!({"snaks": {"P9": [{"snaktype": "value", "property": "P9",
                "datavalue": {"value": r, "type": "string"}, "datatype": "string"}]}, "snaks-order": ["P9"]})).collect::<Vec<_>>()});
        if let Some(id) = id {
            s["id"] = json!(id);
        }
        s
    }

    const M1: &str = "domain:example.org$00000000-0000-0000-0000-000000000001";
    const M2: &str = "domain:example.org$00000000-0000-0000-0000-000000000002";
    const L1: &str = "domain:example.org$00000000-0000-0000-0000-00000000000A";

    fn mirror() -> Entity {
        entity(json!({"type": "domain", "id": "domain:example.org",
            "labels": {"en": {"language": "en", "value": "example.org"}, "de": {"language": "de", "value": "example.org (de)"}},
            "aliases": {"en": [{"language": "en", "value": "example"}]},
            "claims": {"P1": [statement("P1", "a", Some(M1), "normal", &["r1"]),
                              statement("P1", "b", Some(M2), "normal", &[])]}}))
    }

    fn local() -> Entity {
        entity(json!({"type": "domain", "id": "domain:example.org",
            "labels": {"en": {"language": "en", "value": "Example"}},
            "aliases": {"en": [{"language": "en", "value": "ex"}, {"language": "en", "value": "example"}]},
            "claims": {"P1": [statement("P1", "a", Some(L1), "preferred", &["r1", "r2"])],
                       "P2": [statement("P2", "c", None, "normal", &[])]}}))
    }

    #[test]
    fn local_wins_terms_and_rank_and_statements_fuse() {
        let contributions = [
            Contribution {
                graph: "local".into(),
                entity: local(),
            },
            Contribution {
                graph: "mirror/internetdomains".into(),
                entity: mirror(),
            },
        ];
        let r = resolve(&contributions, &BTreeMap::new()).unwrap();
        let e = &r.entity;
        assert_eq!(e.labels["en"], "Example");
        assert_eq!(e.labels["de"], "example.org (de)");
        assert_eq!(e.aliases["en"], vec!["ex", "example"]);
        let p1 = &e.statements[&EntityId::parse("P1").unwrap()];
        assert_eq!(p1.len(), 2, "a fused with a, b kept");
        assert_eq!(
            p1[0].id.as_ref().unwrap().as_str(),
            L1,
            "the ID comes from the first graph"
        );
        assert_eq!(p1[0].rank, Rank::Preferred);
        assert_eq!(p1[0].references.len(), 2, "r1 deduplicated, r2 added");
        assert_eq!(r.statements.len(), 3);
        assert_eq!(
            r.statements[0].graphs,
            vec!["local", "mirror/internetdomains"]
        );
        assert_eq!(r.statements[1].graphs, vec!["mirror/internetdomains"]);
        assert_eq!(r.statements[2].graphs, vec!["local"]);
        assert_eq!(r.graphs, vec!["local", "mirror/internetdomains"]);
        assert!(!r.equals(&mirror()) && !r.equals(&local()));
    }

    #[test]
    fn a_single_source_resolves_to_itself() {
        let m = mirror();
        let r = resolve(
            &[Contribution {
                graph: "mirror/internetdomains".into(),
                entity: m.clone(),
            }],
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(r.equals(&m));
        assert_eq!(r.canonical_json(), m.to_canonical_json());
        assert!(resolve(&[], &BTreeMap::new()).is_none());
    }

    #[test]
    fn corrections_apply() {
        let mut corrections = BTreeMap::new();
        corrections.insert(
            StatementId::parse(M2).unwrap(),
            Correction::Rank(Rank::Deprecated),
        );
        corrections.insert(StatementId::parse(M1).unwrap(), Correction::Suppress);
        let r = resolve(
            &[Contribution {
                graph: "mirror/x".into(),
                entity: mirror(),
            }],
            &corrections,
        )
        .unwrap();
        let p1 = &r.entity.statements[&EntityId::parse("P1").unwrap()];
        assert_eq!(p1.len(), 1, "the suppressed statement is gone");
        assert_eq!(p1[0].rank, Rank::Deprecated);
        // Suppressing one member of a fused statement leaves the other member's.
        let both = [
            Contribution {
                graph: "local".into(),
                entity: local(),
            },
            Contribution {
                graph: "mirror/x".into(),
                entity: mirror(),
            },
        ];
        let r = resolve(&both, &corrections).unwrap();
        let p1 = &r.entity.statements[&EntityId::parse("P1").unwrap()];
        assert_eq!(p1.len(), 2);
        assert_eq!(p1[0].id.as_ref().unwrap().as_str(), L1);
        assert_eq!(
            p1[0].references.len(),
            2,
            "the suppressed member's references are not merged"
        );
        assert_eq!(p1[1].rank, Rank::Deprecated);
        assert_eq!(
            r.statements[0].members.len(),
            2,
            "provenance still lists the suppressed member"
        );
    }
}
