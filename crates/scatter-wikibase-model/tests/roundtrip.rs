//! The contract test of 0005 §3 rule 8 and 0006 §2: every Wikibase JSON snapshot in
//! `docs/samples/wikibase-1.43` and `docs/samples/wikidata` round-trips through the model.
//!
//! - The storage form is a fixed point: parse → canonical → parse → canonical is stable.
//! - The storage form loses nothing but the derived fields: the original, with `hash`,
//!   `numeric-id` and page metadata removed, equals the canonical output as a JSON value.
//! - The hash guard finds no mismatch: every snak and reference hash upstream sent is the
//!   one [`Hasher`] recomputes, so all of them are dropped from the storage form.
//! - The Wikibase form reproduces the original exactly: `numeric-id` and every hash
//!   recomputed, page metadata carried.

use std::path::PathBuf;

use scatter_wikibase_model::{Entity, Hasher, json};
use serde_json::Value;

fn snapshots() -> Vec<(String, Value)> {
    let samples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/samples");
    let mut out = Vec::new();
    for set in ["wikibase-1.43", "wikidata"] {
        let dir = samples.join(set);
        for entry in std::fs::read_dir(&dir).expect("samples dir") {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let text = std::fs::read_to_string(&path).unwrap();
                let v: Value = serde_json::from_str(&text).unwrap();
                if v.get("entities").is_some() {
                    out.push((path.file_name().unwrap().to_string_lossy().into_owned(), v));
                }
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no entity snapshots found in {}",
        samples.display()
    );
    out
}

/// Removes the derived fields and page metadata from a Wikibase entity JSON value.
fn strip_derived(v: &Value) -> Value {
    fn walk(v: &Value, top: bool) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.iter()
                    .filter(|(k, _)| k.as_str() != "hash" && k.as_str() != "numeric-id")
                    .filter(|(k, _)| {
                        !top || !matches!(
                            k.as_str(),
                            "pageid" | "ns" | "title" | "lastrevid" | "modified"
                        )
                    })
                    .map(|(k, v)| (k.clone(), walk(v, false)))
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.iter().map(|v| walk(v, false)).collect()),
            other => other.clone(),
        }
    }
    walk(v, true)
}

#[test]
fn every_snapshot_round_trips() {
    for (name, response) in snapshots() {
        for (id, original) in response["entities"].as_object().unwrap() {
            let mut parsed = Entity::from_value(original.clone())
                .unwrap_or_else(|e| panic!("{name}: {id}: {e}"));
            assert_eq!(parsed.entity.id.as_str(), id, "{name}");

            // The guard (0006 §2): every hash upstream sent is recomputed and matches, so
            // none is kept.
            let hasher = Hasher::local();
            let mismatches = hasher.reconcile(&mut parsed.entity);
            assert!(
                mismatches.is_empty(),
                "{name}: {id}: hash mismatches {:?}",
                mismatches.by_kind()
            );
            let stored_hashes = parsed
                .entity
                .all_statements()
                .flat_map(|s| {
                    s.snaks()
                        .filter_map(|k| k.hash.as_deref())
                        .chain(s.references.iter().filter_map(|r| r.hash.as_deref()))
                })
                .count();
            assert_eq!(stored_hashes, 0, "{name}: every matching hash is dropped");

            // Storage form: a fixed point that loses nothing but the derived fields.
            let canonical = parsed.entity.to_canonical_json();
            assert!(
                !canonical.contains("\"hash\""),
                "{name}: the storage form carries no recomputable hash"
            );
            let reparsed =
                Entity::from_json(&canonical).unwrap_or_else(|e| panic!("{name}: reparse: {e}"));
            assert_eq!(
                reparsed.entity, parsed.entity,
                "{name}: model differs after round trip"
            );
            assert!(
                reparsed.page.is_none(),
                "{name}: the storage form carries no page metadata"
            );
            assert_eq!(
                reparsed.entity.to_canonical_json(),
                canonical,
                "{name}: not a fixed point"
            );
            let expected = strip_derived(original);
            let stored: Value = serde_json::from_str(&canonical).unwrap();
            assert_eq!(stored, expected, "{name}: storage form lost or added data");

            // Wikibase form: the original, exactly, with every hash recomputed.
            let served: Value = serde_json::from_str(&parsed.to_wikibase_json(&hasher)).unwrap();
            assert_eq!(
                &served, original,
                "{name}: Wikibase form differs from the original"
            );

            // Canonical order really is the CBOR order.
            let keys: Vec<&str> = stored
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let mut sorted = keys.clone();
            sorted.sort_by(|a, b| json::key_order(a, b));
            // serde_json's Map is a BTreeMap, so this checks the writer's order indirectly:
            let text_positions: Vec<usize> = sorted
                .iter()
                .map(|k| canonical.find(&format!("\"{k}\":")).unwrap())
                .collect();
            assert!(
                text_positions.windows(2).all(|w| w[0] < w[1]),
                "{name}: keys are not in CBOR order"
            );
        }
    }
}

#[test]
fn q8_specifics() {
    let (_, response) = snapshots()
        .into_iter()
        .find(|(n, _)| n == "wikibase-Q8.json")
        .expect("Q8 snapshot");
    let parsed = Entity::from_value(response["entities"]["Q8"].clone()).unwrap();
    let e = &parsed.entity;
    assert_eq!(parsed.page.as_ref().unwrap().lastrevid, Some(26));
    assert_eq!(e.labels["en"], "Douglas Adams (test)");
    assert_eq!(e.aliases["en"], vec!["DNA"]);
    assert_eq!(e.sitelinks["testwiki"].badges[0].as_str(), "Q2");
    let p1 = &e.statements[&"P1".parse::<scatter_wikibase_model::EntityId>().unwrap()];
    assert_eq!(p1[0].qualifiers.keys().next().unwrap().as_str(), "P4");
    assert_eq!(
        p1[0].references[0]
            .snaks
            .keys()
            .map(scatter_wikibase_model::EntityId::as_str)
            .collect::<Vec<_>>(),
        ["P8", "P4"]
    );
    assert_eq!(
        p1[0].references[0].hash.as_deref(),
        Some("eaf0a11b92f297234266b31f9331c3ebcfe09c1e"),
        "as parsed, before the guard runs"
    );
    assert_eq!(
        Hasher::local().reference(&p1[0].references[0]),
        "eaf0a11b92f297234266b31f9331c3ebcfe09c1e"
    );
    // A mirrored copy hashes the same once its IDs are read in the source's form.
    let mirrored: Entity = serde_json::from_str(
        &e.to_canonical_json()
            .replace("\"P", "\"WDP")
            .replace("\"Q", "\"WDQ"),
    )
    .map(|p: scatter_wikibase_model::ParsedEntity| p.entity)
    .unwrap();
    let wd = Hasher::mirrored_from("WD");
    let hashes = |e: &Entity, h: &Hasher| {
        let mut v: Vec<String> = e.all_statements().map(|s| h.statement(s)).collect();
        v.sort();
        v
    };
    assert_eq!(hashes(e, &Hasher::local()), hashes(&mirrored, &wd));
    assert_ne!(
        hashes(e, &Hasher::local()),
        hashes(&mirrored, &Hasher::local())
    );
    // Every value type in the contract's table appears and is typed, not kept as Unknown.
    let unknown = e
        .all_statements()
        .flat_map(scatter_wikibase_model::Statement::snaks)
        .filter_map(|s| s.data_value())
        .filter(|v| matches!(v, scatter_wikibase_model::DataValue::Unknown { .. }))
        .count();
    assert_eq!(unknown, 0);
    let canonical = e.to_canonical_json();
    assert!(!canonical.contains("numeric-id"));
    assert!(!canonical.contains("\"pageid\""));
    assert!(
        canonical.starts_with(r#"{"id":"Q8","type":"item","#),
        "{}",
        &canonical[..40]
    );
}
