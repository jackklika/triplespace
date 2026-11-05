//! The fixtures of 0003 §10, from the Wikidata snapshots in `docs/samples/wikidata`: Los
//! Angeles' population (Series) and head of government (Timeline), Pluto's classes (best
//! and deprecated values), Astana's names and capital status, and manganese's ionic
//! radii (a Table with a Matrix view).

use scatter_wikibase_model::Entity;
use scatter_wikibase_model::entity::Entity as Model;
use scatter_wikibase_model::id::EntityId;
use scatter_wikibase_model::statement::Statement;
use scatter_wikibase_shape::{Group, Roles, Shape, Thresholds, classify};

fn entity(q: &str) -> Model {
    let path = format!(
        "{}/../../docs/samples/wikidata/wikidata-{q}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    Entity::from_value(v["entities"][q].clone()).unwrap().entity
}

fn group<'e>(e: &'e Model, p: &str) -> &'e [Statement] {
    e.statements
        .iter()
        .find(|(k, _)| k.as_str() == p)
        .map_or_else(
            || panic!("{} has no {p}", e.id.as_str()),
            |(_, v)| v.as_slice(),
        )
}

fn classify_wd(e: &Model, p: &str) -> Group {
    classify(
        group(e, p),
        None,
        &Roles::wikidata(),
        &Thresholds::default(),
    )
}

#[test]
fn los_angeles_population_is_a_series() {
    let e = entity("Q65");
    let g = classify_wd(&e, "P1082");
    assert_eq!(g.shape, Shape::Series);
    assert_eq!(g.n(), 11);
    assert_eq!(g.best.len(), 1);
    assert!(g.folded.is_empty(), "a Series keeps its values in place");
    // Newest first by point in time.
    let years: Vec<i64> = g
        .order
        .iter()
        .filter_map(|&i| {
            group(&e, "P1082")[i]
                .qualifiers
                .iter()
                .find(|(k, _)| k.as_str() == "P585")
                .and_then(|(_, v)| scatter_wikibase_shape::order::snak_time(&v[0]))
                .map(|t| t.0)
        })
        .collect();
    let mut sorted = years.clone();
    sorted.sort_unstable_by(|a, b| b.cmp(a));
    assert_eq!(years, sorted);
}

#[test]
fn los_angeles_head_of_government_is_a_timeline_led_by_the_best_value() {
    let e = entity("Q65");
    let g = classify_wd(&e, "P6");
    assert_eq!(g.shape, Shape::Timeline);
    assert_eq!(g.order[0], g.best[0], "the best value leads");
}

#[test]
fn without_roles_there_is_no_timeline_or_series() {
    let e = entity("Q65");
    let g = classify(
        group(&e, "P6"),
        None,
        &Roles::empty(),
        &Thresholds::default(),
    );
    assert_ne!(g.shape, Shape::Timeline);
    let g = classify(
        group(&e, "P1082"),
        None,
        &Roles::empty(),
        &Thresholds::default(),
    );
    assert_ne!(g.shape, Shape::Series);
}

#[test]
fn pluto_folds_its_deprecated_class_and_leads_with_the_best() {
    let e = entity("Q339");
    let g = classify_wd(&e, "P31");
    assert_eq!(g.deprecated.len(), 1, "planet, deprecated");
    assert_eq!(g.n(), 6);
    assert_eq!(g.best.len(), 1);
    assert_eq!(g.order[0], g.best[0]);
    assert_eq!(
        g.folded.len(),
        5,
        "the other values fold under \"5 other values\""
    );
    assert!(matches!(g.shape, Shape::List | Shape::Chips));
}

#[test]
fn astana_names_and_leaders_are_timelines() {
    let e = entity("Q1520");
    for p in ["P1448", "P6", "P17"] {
        assert_eq!(classify_wd(&e, p).shape, Shape::Timeline, "{p}");
    }
    let names = classify_wd(&e, "P1448");
    assert_eq!(names.n(), 28);
    assert_eq!(names.inline, Some(10), "more than 25 values show 10 inline");
    assert_eq!(
        &names.order[..3]
            .iter()
            .filter(|i| names.best.contains(i))
            .count(),
        &3,
        "three best values lead"
    );
}

#[test]
fn manganese_ionic_radii_are_a_table_with_a_matrix() {
    let e = entity("Q731");
    let g = classify_wd(&e, "P10685");
    assert_eq!(g.shape, Shape::Table);
    let cols: Vec<&str> = g.columns.iter().map(EntityId::as_str).collect();
    assert_eq!(
        cols.len(),
        2,
        "charge number and coordination number: {cols:?}"
    );
    assert!(cols.contains(&"P10764") && cols.contains(&"P10795"));
    assert!(
        g.matrix,
        "two columns, few distinct values, no repeated pair"
    );
    // Rows sorted by the first column, then the second.
    let st = group(&e, "P10685");
    let firsts: Vec<_> = g
        .order
        .iter()
        .map(|&i| Group::cell(st, i, &g.columns[0]).cloned())
        .collect();
    for w in firsts.windows(2) {
        assert_ne!(
            scatter_wikibase_shape::order::cmp_cells(w[0].as_ref(), w[1].as_ref()),
            std::cmp::Ordering::Greater
        );
    }
}

#[test]
fn manganese_identifiers_are_chips_and_large() {
    let e = entity("Q731");
    let g = classify_wd(&e, "P2877");
    assert_eq!(g.shape, Shape::Chips);
    assert_eq!(g.n(), 97);
    assert_eq!(g.inline, Some(10));
    // Oxidation states are quantities without qualifiers: a List, best values first.
    let ox = classify_wd(&e, "P1121");
    assert_eq!(ox.shape, Shape::List);
    assert_eq!(ox.best.len(), 5);
    assert!(ox.order[..5].iter().all(|i| ox.best.contains(i)));
}

#[test]
fn every_group_of_the_snapshots_classifies_and_keeps_every_value() {
    for q in ["Q65", "Q339", "Q731", "Q1520"] {
        let e = entity(q);
        for (p, st) in &e.statements {
            let g = classify(st, None, &Roles::wikidata(), &Thresholds::default());
            let mut all: Vec<usize> = g.order.iter().chain(&g.deprecated).copied().collect();
            all.sort_unstable();
            assert_eq!(
                all,
                (0..st.len()).collect::<Vec<_>>(),
                "{q} {p}: nothing is lost (0003 §1.5)"
            );
            assert_eq!(g.footnotes.rows.len(), st.len());
            for &(s, r) in &g.footnotes.notes {
                assert!(
                    st[s].references.get(r).is_some(),
                    "{q} {p}: footnotes point at references"
                );
            }
        }
    }
}

#[test]
fn an_ordinal_only_table_is_a_numbered_list_in_numeric_order() {
    // An article's authors, by series ordinal: one column, P1545, strings "1" … "12".
    let statements: Vec<Statement> = (1..=12)
        .rev()
        .map(|n| {
            let json = serde_json::json!({
                "mainsnak": {"snaktype": "value", "property": "P2093", "datatype": "string",
                    "datavalue": {"value": format!("Author {n}"), "type": "string"}},
                "type": "statement", "rank": "normal",
                "qualifiers": {"P1545": [{"snaktype": "value", "property": "P1545", "datatype": "string",
                    "datavalue": {"value": n.to_string(), "type": "string"}}]},
                "qualifiers-order": ["P1545"],
                "id": format!("Q1$00000000-0000-0000-0000-{n:012}")
            });
            let entity = serde_json::json!({"type": "item", "id": "Q1", "claims": {"P2093": [json]}});
            Entity::from_value(entity).unwrap().entity.statements.into_values().next().unwrap().remove(0)
        })
        .collect();
    let g = classify(
        &statements,
        None,
        &Roles::wikidata(),
        &Thresholds::default(),
    );
    assert_eq!(g.shape, Shape::Table);
    assert!(g.numbered);
    let first = Group::cell(&statements, g.order[0], &g.columns[0]).unwrap();
    let last = Group::cell(&statements, *g.order.last().unwrap(), &g.columns[0]).unwrap();
    assert_eq!(
        format!("{:?}", first.kind),
        format!(
            "{:?}",
            statements[11].qualifiers.values().next().unwrap()[0].kind
        ),
        "1 first"
    );
    assert_eq!(
        format!("{:?}", last.kind),
        format!(
            "{:?}",
            statements[0].qualifiers.values().next().unwrap()[0].kind
        ),
        "12 last, not between 1 and 2"
    );
    // Without the role, the same group is an ordinary Table.
    let plain = classify(&statements, None, &Roles::empty(), &Thresholds::default());
    assert!(!plain.numbered);
}
