//! The search-document builder (0014 §7): one document of the `entities` index from a
//! resolved entity, built as a pure function so that the indexer and the audit tooling
//! produce the same bytes.
//!
//! **Visibility (0056 §8, §6 Rule 1).** A document carries `read_groups`, the sorted group
//! names of its target's visibility set, empty for a public target; every query filters on
//! it, so a document is returned only to a principal that satisfies every group. The set
//! is an input here: the model has no ACLs, and the projection that calls this computes
//! the set from the enclosure chain (0056 §14). The document is built from the entity
//! and the counts alone, so its visibility is its target's: nothing from another target
//! is copied in, and `statement_keywords` carries IDs, not labels. The field is always
//! written, so a document can never be mistaken for public by its absence.
//!
//! **Moderation (0014 §7).** A hidden, deleted or suppressed entity has no document; that
//! is the caller's decision, made before this function is reached.

use std::collections::{BTreeMap, BTreeSet};

use scatter_normalize::{KeyedRegistry, domain, keyword};
use serde::Serialize;

use crate::entity::Entity;
use crate::id::{EntityId, IdForm};
use crate::key::Canon;
use crate::statement::{Rank, SnakKind};
use crate::value::DataValue;

/// What the indexer knows beyond the entity itself.
pub struct DocumentContext<'a> {
    /// The MediaWiki namespace number of the entity's page (0008 §1).
    pub namespace: i64,
    /// Distinct referrers in `view.entity_ref`, resolved to canonical IDs (0014 §7).
    pub incoming_links: u64,
    /// The resolved view's version, which the index uses as `version_type: external`.
    pub resolved_version: u64,
    /// The visibility set of the entity (0056 §2): the groups a principal must all be in
    /// to read it. Empty for a public entity. A resolved entity merges the terms of every
    /// member of its cluster (0004 §4), so this is the union of the members' sets (0056 §6,
    /// Rule 1); the projection computes it, this function only carries it.
    pub read_groups: BTreeSet<String>,
    /// Properties left out of `statement_keywords` (registry configuration, 0014 §7).
    pub excluded_properties: &'a [EntityId],
    /// The canonical IDs of the resolved view, for `statement_keywords`.
    pub canon: &'a dyn Canon,
    /// The keyed types, for `key`, `key_ulabel` and `key_parents`.
    pub keyed: &'a KeyedRegistry,
}

/// One document of the `entities` index (0014 §7). Field names are the mapping's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntityDocument {
    /// The canonical entity ID, prefixed as everywhere (0017 §4).
    pub id: String,
    /// The Wikibase entity type.
    #[serde(rename = "type")]
    pub entity_type: String,
    /// The provider code of a foreign entity; `None` for local and keyed entities.
    pub provider: Option<String>,
    /// The MediaWiki namespace number.
    pub namespace: i64,
    /// Per language: the label first, then the aliases, so a prefix match finds either.
    pub labels: BTreeMap<String, Vec<String>>,
    /// Every label and alias in every language.
    pub labels_all: Vec<String>,
    /// Per language.
    pub descriptions: BTreeMap<String, String>,
    /// Per language.
    pub aliases: BTreeMap<String, Vec<String>>,
    /// A keyed entity's bare key, in its A-label form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// A keyed entity's key in its display form.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_ulabel: Option<String>,
    /// The key and every parent by removing labels from the left (a domain's DNS
    /// hierarchy, 0009 §5); the key alone for other keyed types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_parents: Option<Vec<String>>,
    /// `P31=Q5`-style tokens for entity-valued and `external-id` main snaks, with
    /// canonical IDs, for `haswbstatement:`. Deprecated statements are left out.
    pub statement_keywords: Vec<String>,
    /// Sitelinks on the entity.
    pub sitelink_count: u64,
    /// Statements on the entity.
    pub statement_count: u64,
    /// From the context.
    pub incoming_links: u64,
    /// From the context.
    pub resolved_version: u64,
    /// The visibility set, sorted (0056 §8). Always present; empty means public.
    pub read_groups: Vec<String>,
}

impl EntityDocument {
    /// Whether every principal may read this document.
    #[must_use]
    pub fn is_public(&self) -> bool {
        self.read_groups.is_empty()
    }
}

/// Builds the document for a resolved entity.
#[must_use]
pub fn entity_document(entity: &Entity, ctx: &DocumentContext<'_>) -> EntityDocument {
    let mut labels: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (lang, label) in &entity.labels {
        labels.entry(lang.clone()).or_default().push(label.clone());
    }
    for (lang, aliases) in &entity.aliases {
        labels
            .entry(lang.clone())
            .or_default()
            .extend(aliases.iter().cloned());
    }
    let labels_all: Vec<String> = labels.values().flatten().cloned().collect();

    let (key, key_ulabel, key_parents) = keyed_fields(&entity.id, ctx.keyed);

    let mut statement_keywords = Vec::new();
    for statement in entity.all_statements() {
        if statement.rank == Rank::Deprecated {
            continue;
        }
        let property = ctx.canon.canonical_id(&statement.mainsnak.property);
        if ctx.excluded_properties.contains(&property)
            || ctx
                .excluded_properties
                .contains(&statement.mainsnak.property)
        {
            continue;
        }
        let SnakKind::Value(value) = &statement.mainsnak.kind else {
            continue;
        };
        let token = match value {
            DataValue::EntityId(e) => Some(ctx.canon.canonical_id(&e.id).to_string()),
            DataValue::String(s)
                if statement
                    .mainsnak
                    .datatype
                    .as_ref()
                    .is_some_and(|d| *d == crate::value::DataType::ExternalId) =>
            {
                Some(s.clone())
            }
            _ => None,
        };
        if let Some(token) = token {
            statement_keywords.push(format!("{property}={token}"));
        }
    }
    statement_keywords.sort();
    statement_keywords.dedup();

    EntityDocument {
        id: entity.id.to_string(),
        entity_type: entity.entity_type.name().to_string(),
        provider: match entity.id.form() {
            IdForm::Foreign => Some(entity.id.as_str()[..2].to_string()),
            IdForm::Local | IdForm::Keyed => None,
        },
        namespace: ctx.namespace,
        labels,
        labels_all,
        descriptions: entity.descriptions.clone(),
        aliases: entity.aliases.clone(),
        key,
        key_ulabel,
        key_parents,
        statement_keywords,
        sitelink_count: entity.sitelinks.len() as u64,
        statement_count: entity.all_statements().count() as u64,
        incoming_links: ctx.incoming_links,
        resolved_version: ctx.resolved_version,
        read_groups: ctx.read_groups.iter().cloned().collect(),
    }
}

/// `key`, `key_ulabel` and `key_parents` for a keyed ID; `None`s otherwise.
fn keyed_fields(
    id: &EntityId,
    keyed: &KeyedRegistry,
) -> (Option<String>, Option<String>, Option<Vec<String>>) {
    let Some((type_name, key)) = id.keyed_parts() else {
        return (None, None, None);
    };
    let ulabel = keyed
        .by_name(type_name)
        .and_then(|t| t.derived_label(key))
        .unwrap_or_else(|| {
            if type_name == "keyword" {
                keyword::display(key)
            } else {
                key.to_string()
            }
        });
    let parents = if type_name == "domain" {
        std::iter::once(key)
            .chain(domain::parents(key))
            .map(str::to_string)
            .collect()
    } else {
        vec![key.to_string()]
    };
    (Some(key.to_string()), Some(ulabel), Some(parents))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Identity;

    fn ctx<'a>(read_groups: &[&str], excluded: &'a [EntityId]) -> DocumentContext<'a> {
        DocumentContext {
            namespace: 120,
            incoming_links: 3,
            resolved_version: 26,
            read_groups: read_groups.iter().map(|s| (*s).to_string()).collect(),
            excluded_properties: excluded,
            canon: &Identity,
            keyed: KeyedRegistry::default_registry(),
        }
    }

    fn q8() -> Entity {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../docs/samples/wikibase-1.43/wikibase-Q8.json"
        ))
        .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        Entity::from_value(v["entities"]["Q8"].clone())
            .unwrap()
            .entity
    }

    #[test]
    fn q8_document() {
        let e = q8();
        let d = entity_document(&e, &ctx(&[], &[]));
        assert_eq!(d.id, "Q8");
        assert_eq!(d.entity_type, "item");
        assert_eq!(d.provider, None);
        assert_eq!(d.labels["en"], vec!["Douglas Adams (test)", "DNA"]);
        assert!(d.labels_all.contains(&"DNA".to_string()));
        assert_eq!(d.sitelink_count, 1);
        assert_eq!(d.statement_count, e.all_statements().count() as u64);
        assert_eq!((d.incoming_links, d.resolved_version), (3, 26));
        // Entity-valued and external-id main snaks, canonical IDs, sorted.
        assert_eq!(d.statement_keywords, vec!["P2=Q3", "P3=P1", "P9=n80076765"]);
        assert!(d.is_public());
        let json = serde_json::to_value(&d).unwrap();
        assert_eq!(json["read_groups"], serde_json::json!([]));
        assert_eq!(json["type"], "item");
        assert!(json.get("key").is_none());
        // An excluded property drops its tokens.
        let excluded = [EntityId::parse("P2").unwrap()];
        let d = entity_document(&e, &ctx(&[], &excluded));
        assert_eq!(d.statement_keywords, vec!["P3=P1", "P9=n80076765"]);
    }

    #[test]
    fn read_groups_are_sorted_and_always_written() {
        let d = entity_document(&q8(), &ctx(&["staff", "board", "staff"], &[]));
        assert_eq!(d.read_groups, vec!["board", "staff"]);
        assert!(!d.is_public());
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains(r#""read_groups":["board","staff"]"#));
    }

    #[test]
    fn keyed_and_foreign_entities() {
        let d: Entity = serde_json::from_str(
            r#"{"type":"domain","id":"domain:en.wikipedia.org","labels":{"mul":{"language":"mul","value":"en.wikipedia.org"}}}"#,
        )
        .map(|p: crate::entity::ParsedEntity| p.entity)
        .unwrap();
        let doc = entity_document(&d, &ctx(&[], &[]));
        assert_eq!(doc.key.as_deref(), Some("en.wikipedia.org"));
        assert_eq!(doc.key_ulabel.as_deref(), Some("en.wikipedia.org"));
        assert_eq!(
            doc.key_parents.as_deref(),
            Some(
                &[
                    "en.wikipedia.org".to_string(),
                    "wikipedia.org".into(),
                    "org".into()
                ][..]
            )
        );
        let k: Entity =
            serde_json::from_str(r#"{"type":"keyword","id":"keyword:machine-learning"}"#)
                .map(|p: crate::entity::ParsedEntity| p.entity)
                .unwrap();
        let doc = entity_document(&k, &ctx(&[], &[]));
        assert_eq!(
            doc.key_parents.as_deref(),
            Some(&["machine-learning".to_string()][..])
        );
        assert_eq!(
            doc.key_ulabel.as_deref(),
            Some(&*keyword::display("machine-learning"))
        );
        let w: Entity = serde_json::from_str(r#"{"type":"item","id":"WDQ42"}"#)
            .map(|p: crate::entity::ParsedEntity| p.entity)
            .unwrap();
        let doc = entity_document(&w, &ctx(&[], &[]));
        assert_eq!(doc.provider.as_deref(), Some("WD"));
        assert!(doc.key.is_none());
    }

    #[test]
    fn deprecated_statements_make_no_keywords() {
        let e: Entity = serde_json::from_str(
            r#"{"type":"item","id":"Q1","claims":{"P2":[
                {"mainsnak":{"snaktype":"value","property":"P2","datatype":"wikibase-item","datavalue":{"value":{"entity-type":"item","id":"Q3"},"type":"wikibase-entityid"}},"rank":"deprecated"},
                {"mainsnak":{"snaktype":"value","property":"P2","datatype":"wikibase-item","datavalue":{"value":{"entity-type":"item","id":"Q4"},"type":"wikibase-entityid"}},"rank":"normal"},
                {"mainsnak":{"snaktype":"somevalue","property":"P2"},"rank":"normal"}]}}"#,
        )
        .map(|p: crate::entity::ParsedEntity| p.entity)
        .unwrap();
        let doc = entity_document(&e, &ctx(&[], &[]));
        assert_eq!(doc.statement_keywords, vec!["P2=Q4"]);
        assert_eq!(doc.statement_count, 3);
    }
}
