//! Wikibase's own hashes, recomputed (0006 §2, as amended 2026-09-28).
//!
//! Wikibase names things by hashes of PHP serializations: a snak's `hash` is the SHA-1 of
//! its pre-7.4 `Serializable` form (`SnakObject::getSerializationForHash()`), a
//! reference's `hash` is the SHA-1 of its snaks' hashes sorted and joined with `|`
//! (`MapValueHasher`), and the RDF names a value node `v:{hash}` by the MD5 of the value's
//! serialization (`DataValueObject::getHash()`). The JSON drops the first two at ingest
//! and recomputes them on output, and the RDF needs the third, so this module reproduces
//! the serializations byte for byte. The crate's private `php` module has the PHP encodings.
//!
//! **Mirrored data.** Wikidata hashes `Q42`; Triplespace stores the same entity value as
//! `WDQ42` (0017 §1). A [`Hasher`] built with [`Hasher::mirrored_from`] writes IDs under
//! that provider in the bare form the source wrote, so that a recomputed hash equals
//! the source's. [`Hasher::local`] hashes IDs as stored.
//!
//! **The guard.** [`Hasher::reconcile`] recomputes every snak and reference hash of an
//! entity, drops the ones that match upstream's, keeps the ones that differ in place, and
//! counts the mismatches by value type. On output ([`crate::json::Form::Wikibase`]) a kept
//! hash is emitted as stored and an absent one is recomputed, so what the instance serves
//! equals what upstream served either way.
//!
//! What is verified: every snak hash, the reference hash and every value-node hash in
//! `docs/samples/wikibase-1.43` and `docs/samples/wikidata`, in this crate's tests.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use md5::Digest as _;

use crate::entity::Entity;
use crate::id::{EntityId, IdForm};
use crate::php;
use crate::statement::{Rank, Reference, Snak, SnakKind, Statement};
use crate::value::DataValue;

/// Computes Wikibase's hashes over the model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hasher {
    /// The provider whose entities the data came from, if any: its IDs are hashed in the
    /// bare form the source wrote them (`WDQ42` as `Q42`).
    source: Option<String>,
}

/// The PHP classes an entity ID is serialized under, by Wikibase `entity-type`.
fn entity_id_class(entity_type: &str) -> &'static str {
    match entity_type {
        "item" => "Wikibase\\DataModel\\Entity\\ItemId",
        "property" => "Wikibase\\DataModel\\Entity\\NumericPropertyId",
        "lexeme" => "Wikibase\\Lexeme\\Domain\\Model\\LexemeId",
        "form" => "Wikibase\\Lexeme\\Domain\\Model\\FormId",
        "sense" => "Wikibase\\Lexeme\\Domain\\Model\\SenseId",
        "mediainfo" => "Wikibase\\MediaInfo\\DataModel\\MediaInfoId",
        // Triplespace's own types (domain, keyword, notation, …) have no PHP class and no
        // upstream hash to match; the abstract base names them deterministically.
        _ => "Wikibase\\DataModel\\Entity\\EntityId",
    }
}

const STRING_VALUE: &str = "DataValues\\StringValue";
const UNKNOWN_VALUE: &str = "DataValues\\UnknownValue";
const MONOLINGUAL_TEXT_VALUE: &str = "DataValues\\MonolingualTextValue";
const TIME_VALUE: &str = "DataValues\\TimeValue";
const GLOBE_COORDINATE_VALUE: &str = "DataValues\\Geo\\Values\\GlobeCoordinateValue";
const DECIMAL_VALUE: &str = "DataValues\\DecimalValue";
const QUANTITY_VALUE: &str = "DataValues\\QuantityValue";
const UNBOUNDED_QUANTITY_VALUE: &str = "DataValues\\UnboundedQuantityValue";
const ENTITY_ID_VALUE: &str = "Wikibase\\DataModel\\Entity\\EntityIdValue";
const ENTITY_SCHEMA_VALUE: &str = "EntitySchema\\Wikibase\\DataValues\\EntitySchemaValue";
const ENTITY_SCHEMA_ID: &str = "EntitySchema\\Domain\\Model\\EntitySchemaId";
const VALUE_SNAK: &str = "Wikibase\\DataModel\\Snak\\PropertyValueSnak";
const SOME_VALUE_SNAK: &str = "Wikibase\\DataModel\\Snak\\PropertySomeValueSnak";
const NO_VALUE_SNAK: &str = "Wikibase\\DataModel\\Snak\\PropertyNoValueSnak";

fn sha1_hex(data: &str) -> String {
    hex(&sha1::Sha1::digest(data.as_bytes()))
}

fn md5_hex(data: &str) -> String {
    hex(&md5::Md5::digest(data.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

impl Hasher {
    /// Hashes IDs as stored: for the tenant's own data.
    #[must_use]
    pub fn local() -> Self {
        Self { source: None }
    }

    /// Hashes IDs under the given two-letter provider code in the bare form that provider
    /// wrote them, so that a recomputed hash equals the provider's own.
    #[must_use]
    pub fn mirrored_from(provider_code: &str) -> Self {
        Self {
            source: Some(provider_code.to_ascii_uppercase()),
        }
    }

    /// An ID as the source wrote it.
    fn source_form<'a>(&self, id: &'a EntityId) -> &'a str {
        let s = id.as_str();
        match &self.source {
            Some(code) if id.form() == IdForm::Foreign && s.starts_with(code.as_str()) => {
                &s[code.len()..]
            }
            _ => s,
        }
    }

    // --- serializations -------------------------------------------------------------

    /// `getSerializationForHash()` of a data value: the pre-7.4 `Serializable` form.
    #[must_use]
    pub fn value_serialization(&self, value: &DataValue) -> String {
        match value {
            DataValue::String(s) => php::class_wrapper(STRING_VALUE, s),
            // The EntitySchema extension's value is not an `EntityIdValue`, though its
            // JSON looks like one: an `EntitySchemaValue` (a `DataValueObject`) whose
            // `serialize()` is PHP's `serialize()` of a plain `EntitySchemaId` object with
            // one private `$id` (0006 §2). Checked against Wikidata's own snak hashes.
            DataValue::EntityId(e) if e.entity_type == "entity-schema" => {
                let mut data = format!("O:{}:\"{ENTITY_SCHEMA_ID}\":1:{{", ENTITY_SCHEMA_ID.len());
                php::serialize_str(&mut data, &format!("\0{ENTITY_SCHEMA_ID}\0id"));
                php::serialize_str(&mut data, self.source_form(&e.id));
                data.push('}');
                php::class_wrapper(ENTITY_SCHEMA_VALUE, &data)
            }
            DataValue::EntityId(e) => {
                let inner =
                    php::class_wrapper(entity_id_class(&e.entity_type), self.source_form(&e.id));
                php::class_wrapper(ENTITY_ID_VALUE, &inner)
            }
            DataValue::MonolingualText(m) => {
                let mut data = String::from("a:2:{i:0;");
                php::serialize_str(&mut data, &m.language);
                data.push_str("i:1;");
                php::serialize_str(&mut data, &m.text);
                data.push('}');
                php::class_wrapper(MONOLINGUAL_TEXT_VALUE, &data)
            }
            DataValue::Time(t) => {
                // json_encode(array_values(getArrayValue()))
                let mut data = String::from("[");
                php::json_str(&mut data, &t.time);
                let _ = write!(
                    data,
                    ",{},{},{},{},",
                    t.timezone, t.before, t.after, t.precision
                );
                php::json_str(&mut data, &t.calendarmodel);
                data.push(']');
                php::class_wrapper(TIME_VALUE, &data)
            }
            DataValue::GlobeCoordinate(g) => {
                // json_encode([latitude, longitude, null, precision, globe])
                let mut data = String::from("[");
                php::json_float(&mut data, g.latitude);
                data.push(',');
                php::json_float(&mut data, g.longitude);
                data.push_str(",null,");
                match g.precision {
                    Some(p) => php::json_float(&mut data, p),
                    None => data.push_str("null"),
                }
                data.push(',');
                php::json_str(&mut data, &g.globe);
                data.push(']');
                php::class_wrapper(GLOBE_COORDINATE_VALUE, &data)
            }
            DataValue::Quantity(q) => {
                let decimal = |s: &str| {
                    let mut d = String::new();
                    php::serialize_str(&mut d, s);
                    php::class_wrapper(DECIMAL_VALUE, &d)
                };
                let mut unit = String::new();
                php::serialize_str(&mut unit, &q.unit);
                if let (Some(upper), Some(lower)) = (&q.upper_bound, &q.lower_bound) {
                    let data = format!(
                        "a:4:{{i:0;{}i:1;{unit}i:2;{}i:3;{}}}",
                        decimal(&q.amount),
                        decimal(upper),
                        decimal(lower)
                    );
                    php::class_wrapper(QUANTITY_VALUE, &data)
                } else {
                    let data = format!("a:2:{{i:0;{}i:1;{unit}}}", decimal(&q.amount));
                    php::class_wrapper(UNBOUNDED_QUANTITY_VALUE, &data)
                }
            }
            DataValue::Unknown { value, .. } => {
                let mut data = String::new();
                php::serialize_json(&mut data, value);
                php::class_wrapper(UNKNOWN_VALUE, &data)
            }
        }
    }

    /// `getSerializationForHash()` of a snak.
    #[must_use]
    pub fn snak_serialization(&self, snak: &Snak) -> String {
        let property = self.source_form(&snak.property);
        match &snak.kind {
            SnakKind::Value(v) => {
                let mut inner = String::from("a:2:{i:0;");
                php::serialize_str(&mut inner, property);
                inner.push_str("i:1;");
                inner.push_str(&self.value_serialization(v));
                inner.push('}');
                php::class_wrapper(VALUE_SNAK, &inner)
            }
            SnakKind::SomeValue => php::class_wrapper(SOME_VALUE_SNAK, property),
            SnakKind::NoValue => php::class_wrapper(NO_VALUE_SNAK, property),
        }
    }

    // --- hashes -----------------------------------------------------------------------

    /// A snak's `hash`: SHA-1 of [`Self::snak_serialization`].
    #[must_use]
    pub fn snak(&self, snak: &Snak) -> String {
        sha1_hex(&self.snak_serialization(snak))
    }

    /// `DataValue::getHash()`, the value-node name in the RDF (`v:{hash}`): MD5 of the
    /// value's serialization, except that a globe coordinate hashes
    /// `latitude|longitude|precision|globe` with PHP's `(string)$float`.
    #[must_use]
    pub fn value(&self, value: &DataValue) -> String {
        if let DataValue::GlobeCoordinate(g) = value {
            let mut s = String::new();
            php::float_to_string(&mut s, g.latitude);
            s.push('|');
            php::float_to_string(&mut s, g.longitude);
            s.push('|');
            if let Some(p) = g.precision {
                php::float_to_string(&mut s, p);
            }
            s.push('|');
            s.push_str(&g.globe);
            return md5_hex(&s);
        }
        md5_hex(&self.value_serialization(value))
    }

    /// `SnakList::getHash()`: the SHA-1 of the distinct snak hashes, sorted and joined
    /// with `|`. A `SnakList` holds each hash once, so a repeated snak counts once.
    fn snak_list<'a>(&self, snaks: impl Iterator<Item = &'a Snak>) -> String {
        let mut hashes: Vec<String> = snaks.map(|s| self.snak(s)).collect();
        hashes.sort_by(|a, b| php_sort_cmp(a, b));
        hashes.dedup();
        sha1_hex(&hashes.join("|"))
    }

    /// A reference's `hash`: `SnakList::getHash()` over its snaks, the SHA-1 of the
    /// distinct snak hashes sorted and joined with `|`.
    #[must_use]
    pub fn reference(&self, reference: &Reference) -> String {
        self.snak_list(reference.snaks.values().flatten())
    }

    /// `Statement::getHash()`: not in the JSON, but what Wikibase compares statements by.
    #[must_use]
    pub fn statement(&self, statement: &Statement) -> String {
        let main_and_qualifiers = sha1_hex(&format!(
            "{}{}",
            self.snak(&statement.mainsnak),
            self.snak_list(statement.qualifiers.values().flatten())
        ));
        let rank = match statement.rank {
            Rank::Deprecated => 0,
            Rank::Normal => 1,
            Rank::Preferred => 2,
        };
        let references: Vec<String> = {
            let mut hs: Vec<String> = statement
                .references
                .iter()
                .map(|r| self.reference(r))
                .collect();
            hs.sort_by(|a, b| php_sort_cmp(a, b));
            hs
        };
        sha1_hex(&format!(
            "{main_and_qualifiers}|{rank}|{}",
            sha1_hex(&references.join("|"))
        ))
    }

    // --- the guard --------------------------------------------------------------------

    /// Recomputes every snak and reference hash the entity carries, drops those that equal
    /// upstream's, keeps those that differ in place, and counts the mismatches (0006 §2).
    /// Snaks and references with no stored hash are left alone.
    pub fn reconcile(&self, entity: &mut Entity) -> HashMismatches {
        let mut report = HashMismatches::default();
        for statements in entity.statements.values_mut() {
            for statement in statements {
                self.reconcile_snak(&mut statement.mainsnak, &mut report);
                for snak in statement.qualifiers.values_mut().flatten() {
                    self.reconcile_snak(snak, &mut report);
                }
                for reference in &mut statement.references {
                    for snak in reference.snaks.values_mut().flatten() {
                        self.reconcile_snak(snak, &mut report);
                    }
                    if let Some(stored) = &reference.hash {
                        if *stored == self.reference(reference) {
                            reference.hash = None;
                        } else {
                            report.count("reference");
                        }
                    }
                }
            }
        }
        report
    }

    fn reconcile_snak(&self, snak: &mut Snak, report: &mut HashMismatches) {
        if let Some(stored) = &snak.hash {
            if *stored == self.snak(snak) {
                snak.hash = None;
            } else {
                report.count(snak_kind_key(snak));
            }
        }
    }
}

/// The key a snak's mismatch is counted under: its value type, or its snak type for a
/// snak with no value.
fn snak_kind_key(snak: &Snak) -> &str {
    match &snak.kind {
        SnakKind::Value(v) => v.value_type_name(),
        SnakKind::SomeValue => "somevalue",
        SnakKind::NoValue => "novalue",
    }
}

/// PHP's `sort()` on strings compares two *numeric* strings as numbers. A hex hash is
/// numeric only when it is all digits, or digits around one `e`; the odds are below one
/// in a hundred million per hash, but the order is Wikibase's, so it is reproduced.
fn php_sort_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    fn numeric(s: &str) -> Option<f64> {
        let (mantissa, exponent) = s.split_once(['e', 'E']).unwrap_or((s, ""));
        let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
        if digits(mantissa) && (exponent.is_empty() || digits(exponent)) {
            s.parse().ok()
        } else {
            None
        }
    }
    match (numeric(a), numeric(b)) {
        (Some(x), Some(y)) => x.total_cmp(&y).then_with(|| a.cmp(b)),
        _ => a.cmp(b),
    }
}

/// Mismatches between upstream's hashes and the recomputation, by value type (0006 §2;
/// 0011 §6.3): `string`, `time`, …, `somevalue`, `novalue`, and `reference`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HashMismatches {
    counts: BTreeMap<String, u64>,
}

impl HashMismatches {
    fn count(&mut self, kind: &str) {
        *self.counts.entry(kind.to_string()).or_default() += 1;
    }

    /// Mismatches by kind, in key order.
    #[must_use]
    pub fn by_kind(&self) -> &BTreeMap<String, u64> {
        &self.counts
    }

    /// All mismatches.
    #[must_use]
    pub fn total(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Whether every hash matched.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{EntityIdValue, GlobeCoordinate, MonolingualText, Quantity, Time};

    fn snak(json: &str) -> Snak {
        serde_json::from_str(json).unwrap()
    }

    /// An EntitySchema value hashes as the EntitySchema extension serializes it (0006 §2),
    /// not as an `EntityIdValue`: two qualifiers on Wikidata properties, with the hashes
    /// Wikidata wrote for them in the 2026-10-26 dump, as stored here and as mirrored.
    #[test]
    fn entity_schema_values_hash_as_the_extension_serializes_them() {
        for (property, schema, wikidata) in [
            ("P12861", "E13", "992aaa1ed52a6b8f0aa31f65774ce65826aa1301"),
            ("P12886", "E1", "cf92ee168427e91beb5eff58971a5e0f6d1382ca"),
        ] {
            let json = |p: &str, e: &str| {
                format!(
                    r#"{{"snaktype":"value","property":"{p}","datavalue":{{"value":{{"id":"{e}","entity-type":"entity-schema"}},"type":"wikibase-entityid"}},"datatype":"entity-schema"}}"#
                )
            };
            assert_eq!(
                Hasher::local().snak(&snak(&json(property, schema))),
                wikidata,
                "{property} {schema}"
            );
            assert_eq!(
                Hasher::mirrored_from("WD").snak(&snak(&json(
                    &format!("WD{property}"),
                    &format!("WD{schema}")
                ))),
                wikidata,
                "{property} {schema}, mirrored"
            );
        }
    }

    #[test]
    fn serializations_are_the_php_forms() {
        let h = Hasher::local();
        let s = snak(
            r#"{"snaktype":"value","property":"P1","datavalue":{"value":"Douglas Noël Adams","type":"string"},"datatype":"string"}"#,
        );
        assert_eq!(
            h.snak_serialization(&s),
            "C:41:\"Wikibase\\DataModel\\Snak\\PropertyValueSnak\":77:{a:2:{i:0;s:2:\"P1\";i:1;C:22:\"DataValues\\StringValue\":19:{Douglas Noël Adams}}}"
        );
        assert_eq!(h.snak(&s), "7c8cb8226a0845691900e67b6b69d41e6e2baf44");
        let n = snak(r#"{"snaktype":"novalue","property":"P2"}"#);
        assert_eq!(
            h.snak_serialization(&n),
            "C:43:\"Wikibase\\DataModel\\Snak\\PropertyNoValueSnak\":2:{P2}"
        );
        assert_eq!(h.snak(&n), "64baea7b9db49c64f5daf05ddf5162faa28d5e86");
        assert_eq!(
            h.snak(&snak(r#"{"snaktype":"somevalue","property":"P4"}"#)),
            "ede91cc55952400386a2401405bb09e446b1867b"
        );
    }

    #[test]
    fn value_node_hashes_match_the_rdf_dump() {
        // v: nodes in docs/samples/wikibase-1.43/wikibase-rdf-full-dump.ttl.
        let h = Hasher::local();
        let time = DataValue::Time(Time {
            time: "+1952-03-11T00:00:00Z".into(),
            timezone: 0,
            before: 0,
            after: 0,
            precision: 11,
            calendarmodel: "http://www.wikidata.org/entity/Q1985727".into(),
        });
        assert_eq!(h.value(&time), "426df9023763f08b066f4478480f44cd");
        let quantity = DataValue::Quantity(Quantity {
            amount: "+1.96".into(),
            unit: "http://localhost:8080/entity/Q3".into(),
            upper_bound: Some("+1.97".into()),
            lower_bound: Some("+1.95".into()),
        });
        assert_eq!(h.value(&quantity), "ee7a3b3b3cad181ed729f7c25d9e92d2");
        let unbounded = DataValue::Quantity(Quantity {
            amount: "+42".into(),
            unit: "1".into(),
            upper_bound: None,
            lower_bound: None,
        });
        assert_eq!(h.value(&unbounded), "a168135f766cf673e04ee1850837047d");
        let globe = DataValue::GlobeCoordinate(GlobeCoordinate {
            latitude: 51.5,
            longitude: -0.1275,
            altitude: None,
            precision: Some(0.0001),
            globe: "http://www.wikidata.org/entity/Q2".into(),
        });
        assert_eq!(h.value(&globe), "abfb822748a314cd9bb1132c1c731a39");
        // Not in the dump, but the serializations are the ones PHP produces.
        let mono = DataValue::MonolingualText(MonolingualText {
            text: "Douglas Adams".into(),
            language: "en".into(),
        });
        assert_eq!(
            h.value_serialization(&mono),
            "C:31:\"DataValues\\MonolingualTextValue\":44:{a:2:{i:0;s:2:\"en\";i:1;s:13:\"Douglas Adams\";}}"
        );
        assert_eq!(h.value(&mono), "1e6977489205086925920c704cb85e51");
    }

    #[test]
    fn mirrored_ids_hash_in_the_source_form() {
        let local = Hasher::local();
        let wd = Hasher::mirrored_from("wd");
        let value = |id: &str| {
            DataValue::EntityId(EntityIdValue {
                entity_type: "item".into(),
                id: EntityId::parse(id).unwrap(),
            })
        };
        assert_eq!(
            wd.value_serialization(&value("WDQ3")),
            local.value_serialization(&value("Q3"))
        );
        assert_ne!(
            local.value_serialization(&value("WDQ3")),
            local.value_serialization(&value("Q3"))
        );
        // Another provider's ID inside Wikidata's data is left as written.
        assert_eq!(
            wd.value_serialization(&value("LBQ3")),
            local.value_serialization(&value("LBQ3"))
        );
        // Properties too.
        let s = snak(r#"{"snaktype":"novalue","property":"WDP2"}"#);
        assert_eq!(wd.snak(&s), "64baea7b9db49c64f5daf05ddf5162faa28d5e86");
        let m = DataValue::EntityId(EntityIdValue {
            entity_type: "mediainfo".into(),
            id: EntityId::parse("M7").unwrap(),
        });
        assert!(
            local
                .value_serialization(&m)
                .contains("Wikibase\\MediaInfo\\DataModel\\MediaInfoId")
        );
    }

    #[test]
    fn reference_and_statement_hashes() {
        let h = Hasher::local();
        let s: Statement = serde_json::from_str(
            r#"{"mainsnak":{"snaktype":"novalue","property":"P2"},"rank":"preferred",
                "references":[{"snaks":{"P8":[{"snaktype":"value","property":"P8","datavalue":{"value":"https://example.org/source","type":"string"}}],
                                        "P4":[{"snaktype":"value","property":"P4","datavalue":{"value":{"time":"+1952-03-11T00:00:00Z","timezone":0,"before":0,"after":0,"precision":11,"calendarmodel":"http://www.wikidata.org/entity/Q1985727"},"type":"time"}}]},
                               "snaks-order":["P8","P4"]}]}"#,
        )
        .unwrap();
        let r = &s.references[0];
        assert_eq!(h.reference(r), "eaf0a11b92f297234266b31f9331c3ebcfe09c1e");
        // Order and repetition do not change a reference hash.
        let mut reordered = r.clone();
        reordered.snaks.reverse();
        assert_eq!(h.reference(&reordered), h.reference(r));
        let mut repeated = r.clone();
        let p8 = EntityId::parse("P8").unwrap();
        let dup = repeated.snaks[&p8][0].clone();
        repeated.snaks.get_mut(&p8).unwrap().push(dup);
        assert_eq!(h.reference(&repeated), h.reference(r));
        // Statement::getHash(): sha1( sha1(main . qualifiers) | rank | references ).
        let want = sha1_hex(&format!(
            "{}|2|{}",
            sha1_hex(&format!(
                "64baea7b9db49c64f5daf05ddf5162faa28d5e86{}",
                sha1_hex("")
            )),
            sha1_hex("eaf0a11b92f297234266b31f9331c3ebcfe09c1e")
        ));
        assert_eq!(h.statement(&s), want);
    }

    #[test]
    fn php_sort_order() {
        use std::cmp::Ordering::*;
        assert_eq!(php_sort_cmp("0f5e", "8431"), Less);
        assert_eq!(
            php_sort_cmp("9", "10"),
            Less,
            "numeric strings compare as numbers"
        );
        assert_eq!(php_sort_cmp("9e1", "100"), Less);
        assert_eq!(php_sort_cmp("9a", "10"), Greater, "not numeric: bytewise");
    }

    #[test]
    fn reconcile_drops_matching_hashes_and_keeps_the_rest() {
        let mut e: Entity = serde_json::from_str(
            r#"{"type":"item","id":"Q1","claims":{"P2":[{"mainsnak":{"snaktype":"novalue","property":"P2","hash":"64baea7b9db49c64f5daf05ddf5162faa28d5e86"},"rank":"normal",
              "qualifiers":{"P4":[{"snaktype":"somevalue","property":"P4","hash":"not-what-wikibase-computes"}]},"qualifiers-order":["P4"],
              "references":[{"hash":"stale","snaks":{"P2":[{"snaktype":"novalue","property":"P2"}]},"snaks-order":["P2"]}]}]}}"#,
        )
        .map(|p: crate::entity::ParsedEntity| p.entity)
        .unwrap();
        let report = Hasher::local().reconcile(&mut e);
        assert_eq!(report.total(), 2);
        assert_eq!(report.by_kind()["somevalue"], 1);
        assert_eq!(report.by_kind()["reference"], 1);
        let s = &e.statements[&EntityId::parse("P2").unwrap()][0];
        assert_eq!(s.mainsnak.hash, None, "matched: dropped");
        assert_eq!(
            s.qualifiers
                .values()
                .flatten()
                .next()
                .unwrap()
                .hash
                .as_deref(),
            Some("not-what-wikibase-computes"),
            "kept in place"
        );
        assert_eq!(s.references[0].hash.as_deref(), Some("stale"));
        assert_eq!(
            s.references[0]
                .snaks
                .values()
                .flatten()
                .next()
                .unwrap()
                .hash,
            None,
            "no stored hash: left alone"
        );
        // A second pass finds the same mismatches, and nothing else.
        assert_eq!(Hasher::local().reconcile(&mut e), report);
    }
}
