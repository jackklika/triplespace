//! The classifier audit (0003 §10, step 4): runs shape detection over a Wikidata sample in
//! the dump's shape and reports how often each shape occurs, how the groups fall around
//! each threshold, and how the counts move when a threshold moves (0003 Q1).
//!
//! ```text
//! cargo run --release -p scatter-wikibase-shape --example audit -- \
//!     docs/samples/wikidata/big/wikidata-20261004-sample-50000.json.gz > audit.md
//! ```
//!
//! The input is `latest-all.json`'s shape, plain or gzip: a JSON array with one entity
//! per line. Only items are classified; properties are skipped. Roles are Wikidata's.

// A report generator: long, linear functions that print tables read better whole.
#![allow(clippy::too_many_lines, clippy::many_single_char_names)]

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read};

use scatter_wikibase_model::Entity;
use scatter_wikibase_model::statement::{Rank, Statement};
use scatter_wikibase_shape::{Role, Roles, Shape, Thresholds, classify};

/// A count per key, printed in key order.
type Counts<K> = BTreeMap<K, u64>;

fn bump<K: Ord>(m: &mut Counts<K>, k: K) {
    *m.entry(k).or_default() += 1;
}

/// Coverage in tenths: 0 (none) … 10 (all).
fn tenth(c: f64) -> u8 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let t = (c * 10.0).floor() as u8;
    t.min(10)
}

fn size_bucket(n: usize) -> &'static str {
    match n {
        0 => "0",
        1 => "1",
        2 => "2",
        3 => "3",
        4..=5 => "4–5",
        6..=10 => "6–10",
        11..=25 => "11–25",
        26..=100 => "26–100",
        _ => "> 100",
    }
}

const SIZES: [&str; 9] = [
    "0", "1", "2", "3", "4–5", "6–10", "11–25", "26–100", "> 100",
];

fn current(st: &[Statement]) -> Vec<&Statement> {
    st.iter().filter(|s| s.rank != Rank::Deprecated).collect()
}

fn cov(cur: &[&Statement], has: impl Fn(&str) -> bool) -> f64 {
    if cur.is_empty() {
        return 0.0;
    }
    #[allow(clippy::cast_precision_loss)]
    let r = cur
        .iter()
        .filter(|s| s.qualifiers.keys().any(|k| has(k.as_str())))
        .count() as f64
        / cur.len() as f64;
    r
}

/// The largest coverage of a single qualifier key whose values differ somewhere (a key
/// with one value everywhere is hoisted, not a column).
fn max_key_cov(cur: &[&Statement]) -> f64 {
    let mut keys: Vec<&str> = Vec::new();
    for s in cur {
        for k in s.qualifiers.keys() {
            if !keys.contains(&k.as_str()) {
                keys.push(k.as_str());
            }
        }
    }
    keys.iter()
        .filter(|k| {
            let vals: Vec<_> = cur
                .iter()
                .map(|s| {
                    s.qualifiers
                        .iter()
                        .find(|(q, _)| q.as_str() == **k)
                        .map(|(_, v)| {
                            v.iter()
                                .map(|x| format!("{:?}", x.kind))
                                .collect::<Vec<_>>()
                        })
                })
                .collect();
            !(cur.len() >= 2 && vals.iter().all(|v| v.is_some() && *v == vals[0]))
        })
        .map(|k| cov(cur, |q| q == *k))
        .fold(0.0, f64::max)
}

struct Variant {
    name: String,
    t: Thresholds,
    shapes: Counts<Shape>,
}

fn variants() -> Vec<Variant> {
    let d = Thresholds::default();
    let mut v = vec![("ADR defaults (0003 §3)".to_string(), d)];
    for c in [0.5, 0.6, 0.7, 0.9, 1.0] {
        v.push((
            format!("timeline coverage {c}"),
            Thresholds {
                timeline_coverage: c,
                ..d
            },
        ));
    }
    for c in [0.5, 0.6, 0.7, 0.9, 1.0] {
        v.push((
            format!("series coverage {c}"),
            Thresholds {
                series_coverage: c,
                ..d
            },
        ));
    }
    for m in [2, 3, 5, 6] {
        v.push((format!("series min {m}"), Thresholds { series_min: m, ..d }));
    }
    for c in [0.5, 0.6, 0.7, 0.9, 1.0] {
        v.push((
            format!("table coverage {c}"),
            Thresholds {
                table_coverage: c,
                ..d
            },
        ));
    }
    for m in [2, 4, 5] {
        v.push((format!("table min {m}"), Thresholds { table_min: m, ..d }));
    }
    v.into_iter()
        .map(|(name, t)| Variant {
            name,
            t,
            shapes: Counts::new(),
        })
        .collect()
}

#[derive(Default)]
struct Audit {
    items: u64,
    unparsed: u64,
    statements: u64,
    groups: u64,
    shape_groups: Counts<Shape>,
    shape_statements: Counts<Shape>,
    shape_by_size: BTreeMap<Shape, Counts<&'static str>>,
    shape_by_datatype: BTreeMap<String, Counts<Shape>>,
    sizes: Counts<&'static str>,
    timeline_cov: Counts<u8>,
    series_cov: Counts<u8>,
    series_cov_n4: Counts<u8>,
    table_cov: Counts<u8>,
    columns: Counts<usize>,
    extra_keys: Counts<usize>,
    tables: u64,
    matrix: u64,
    hoisted_groups: u64,
    large_groups: u64,
    best_groups: u64,
    deprecated_groups: u64,
    shared_reference: u64,
    with_references: u64,
    unsourced_values: u64,
    current_values: u64,
    examples: BTreeMap<Shape, Vec<String>>,
    properties: BTreeMap<Shape, Counts<String>>,
    one_column: Counts<String>,
}

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: audit <sample.json[.gz]>");
    let file = std::fs::File::open(&path).expect("open the sample");
    let gz = std::path::Path::new(&path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gz"));
    let reader: Box<dyn Read> = if gz {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let roles = Roles::wikidata();
    let mut a = Audit::default();
    let mut vs = variants();
    let defaults = Thresholds::default();
    for line in BufReader::with_capacity(1 << 20, reader).lines() {
        let line = line.expect("read a line");
        let line = line.trim_end().trim_end_matches(',');
        if line.is_empty() || line == "[" || line == "]" {
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            a.unparsed += 1;
            continue;
        };
        if v["type"] != "item" {
            continue;
        }
        let id = v["id"].as_str().unwrap_or_default().to_string();
        let Ok(parsed) = Entity::from_value(v) else {
            a.unparsed += 1;
            continue;
        };
        a.items += 1;
        for (p, st) in &parsed.entity.statements {
            a.groups += 1;
            a.statements += st.len() as u64;
            let g = classify(st, None, &roles, &defaults);
            let cur = current(st);
            let n = cur.len();
            let dt = st
                .iter()
                .find_map(|s| s.mainsnak.datatype.as_ref().map(|d| d.id().to_string()))
                .unwrap_or_else(|| "?".into());
            bump(&mut a.shape_groups, g.shape);
            *a.shape_statements.entry(g.shape).or_default() += st.len() as u64;
            bump(a.shape_by_size.entry(g.shape).or_default(), size_bucket(n));
            bump(a.shape_by_datatype.entry(dt.clone()).or_default(), g.shape);
            bump(&mut a.sizes, size_bucket(n));
            let ex = a.examples.entry(g.shape).or_default();
            if ex.len() < 6 && n >= 2 {
                ex.push(format!("{id} {} (n = {n})", p.as_str()));
            }
            if n >= 2 {
                let c = cov(&cur, |k| roles.is(k, Role::TimeStart));
                if c > 0.0 {
                    bump(&mut a.timeline_cov, tenth(c));
                }
            }
            if dt == "quantity" && n >= 2 {
                let c = cov(&cur, |k| roles.is(k, Role::TimePoint));
                if c > 0.0 {
                    bump(&mut a.series_cov, tenth(c));
                    if n >= 4 {
                        bump(&mut a.series_cov_n4, tenth(c));
                    }
                }
            }
            if n >= 3 && !matches!(g.shape, Shape::Timeline | Shape::Series) {
                let c = max_key_cov(&cur);
                if c > 0.0 {
                    bump(&mut a.table_cov, tenth(c));
                }
            }
            if n >= 2 {
                bump(
                    a.properties.entry(g.shape).or_default(),
                    p.as_str().to_string(),
                );
            }
            if g.shape == Shape::Table {
                a.tables += 1;
                if g.columns.len() == 1 {
                    bump(
                        &mut a.one_column,
                        format!("{} by {}", p.as_str(), g.columns[0].as_str()),
                    );
                }
                bump(&mut a.columns, g.columns.len());
                bump(&mut a.extra_keys, g.extra.len());
                if g.matrix {
                    a.matrix += 1;
                }
            }
            if !g.hoisted.is_empty() {
                a.hoisted_groups += 1;
            }
            if g.inline.is_some() {
                a.large_groups += 1;
            }
            if !g.best.is_empty() {
                a.best_groups += 1;
            }
            if !g.deprecated.is_empty() {
                a.deprecated_groups += 1;
            }
            if g.footnotes.shared {
                a.shared_reference += 1;
            }
            if !g.footnotes.notes.is_empty() {
                a.with_references += 1;
            }
            a.unsourced_values += g.footnotes.unsourced as u64;
            a.current_values += n as u64;
            for v in &mut vs {
                bump(&mut v.shapes, classify(st, None, &roles, &v.t).shape);
            }
        }
    }
    print!("{}", report(&path, &a, &vs));
}

fn pct(x: u64, of: u64) -> String {
    if of == 0 {
        return "—".into();
    }
    #[allow(clippy::cast_precision_loss)]
    let p = 100.0 * x as f64 / of as f64;
    format!("{p:.1}%")
}

fn tenths_table(out: &mut String, title: &str, m: &Counts<u8>) {
    let total: u64 = m.values().sum();
    let _ = writeln!(out, "| {title} | Groups | Share | At or above |");
    let _ = writeln!(out, "|---|---:|---:|---:|");
    let mut above = total;
    for t in 0..=10u8 {
        let c = m.get(&t).copied().unwrap_or(0);
        let label = if t == 10 {
            "1.0".to_string()
        } else {
            format!("{:.1}–{:.1}", f64::from(t) / 10.0, f64::from(t + 1) / 10.0)
        };
        let _ = writeln!(
            out,
            "| {label} | {c} | {} | {} |",
            pct(c, total),
            pct(above, total)
        );
        above -= c;
    }
    out.push('\n');
}

fn report(path: &str, a: &Audit, vs: &[Variant]) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "# Shape audit\n");
    let _ = writeln!(o, "- Sample: `{path}`");
    let _ = writeln!(
        o,
        "- {} items, {} statement groups, {} statements; {} lines not parsed\n",
        a.items, a.groups, a.statements, a.unparsed
    );

    let _ = writeln!(o, "## Shapes at the ADR's thresholds\n");
    let _ = writeln!(
        o,
        "| Shape | Groups | Share of groups | Statements | Share of statements |"
    );
    let _ = writeln!(o, "|---|---:|---:|---:|---:|");
    for s in Shape::ALL {
        let g = a.shape_groups.get(&s).copied().unwrap_or(0);
        let st = a.shape_statements.get(&s).copied().unwrap_or(0);
        let _ = writeln!(
            o,
            "| {} | {g} | {} | {st} | {} |",
            s.name(),
            pct(g, a.groups),
            pct(st, a.statements)
        );
    }
    let multi: u64 = a.groups
        - a.shape_groups.get(&Shape::Single).copied().unwrap_or(0)
        - a.shape_groups.get(&Shape::Empty).copied().unwrap_or(0)
        - a.shape_groups
            .get(&Shape::NoCurrentValue)
            .copied()
            .unwrap_or(0);
    let _ = writeln!(
        o,
        "\nGroups with two or more current values: {multi}. Of those:\n"
    );
    for s in [
        Shape::Timeline,
        Shape::Series,
        Shape::Table,
        Shape::Chips,
        Shape::List,
    ] {
        let g = a.shape_groups.get(&s).copied().unwrap_or(0);
        let _ = writeln!(o, "- {}: {g} ({})", s.name(), pct(g, multi));
    }

    let _ = writeln!(o, "\n## Group sizes (current values)\n");
    let _ = write!(o, "| Shape |");
    for s in SIZES {
        let _ = write!(o, " {s} |");
    }
    let _ = write!(o, "\n|---|");
    for _ in SIZES {
        let _ = write!(o, "---:|");
    }
    o.push('\n');
    for s in Shape::ALL {
        let Some(m) = a.shape_by_size.get(&s) else {
            continue;
        };
        let _ = write!(o, "| {} |", s.name());
        for z in SIZES {
            let _ = write!(o, " {} |", m.get(z).copied().unwrap_or(0));
        }
        o.push('\n');
    }
    let _ = write!(o, "| all |");
    for z in SIZES {
        let _ = write!(o, " {} |", a.sizes.get(z).copied().unwrap_or(0));
    }
    let _ = writeln!(
        o,
        "\n\nLarge groups (more than 25 values): {} ({} of groups).\n",
        a.large_groups,
        pct(a.large_groups, a.groups)
    );

    let _ = writeln!(o, "## Around the thresholds\n");
    let _ = writeln!(
        o,
        "### Timeline: `time-start` coverage, groups of two or more with any start time\n"
    );
    tenths_table(&mut o, "Coverage", &a.timeline_cov);
    let _ = writeln!(
        o,
        "### Series: `time-point` coverage, quantity groups of two or more with any point in time\n"
    );
    tenths_table(&mut o, "Coverage", &a.series_cov);
    let _ = writeln!(o, "The same, groups of four or more:\n");
    tenths_table(&mut o, "Coverage", &a.series_cov_n4);
    let _ = writeln!(
        o,
        "### Table: the best coverage of a qualifier key that varies, groups of three or more not already Timeline or Series\n"
    );
    tenths_table(&mut o, "Coverage", &a.table_cov);

    let _ = writeln!(o, "### Tables\n");
    let _ = writeln!(
        o,
        "- Tables: {}; with a Matrix view: {} ({})",
        a.tables,
        a.matrix,
        pct(a.matrix, a.tables)
    );
    let _ = writeln!(
        o,
        "- Columns per table: {}",
        a.columns
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(
        o,
        "- Extra (under-row) keys per table: {}\n",
        a.extra_keys
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let _ = writeln!(o, "## Ranks, hoisting and references\n");
    let _ = writeln!(
        o,
        "- Groups with a best value: {} ({})",
        a.best_groups,
        pct(a.best_groups, a.groups)
    );
    let _ = writeln!(
        o,
        "- Groups with deprecated values: {} ({})",
        a.deprecated_groups,
        pct(a.deprecated_groups, a.groups)
    );
    let _ = writeln!(
        o,
        "- Groups with a qualifier hoisted to \"Same for all N\": {} ({})",
        a.hoisted_groups,
        pct(a.hoisted_groups, a.groups)
    );
    let _ = writeln!(
        o,
        "- Groups with references: {}; with one reference cited by all values: {} ({} of groups of two or more)",
        a.with_references,
        a.shared_reference,
        pct(a.shared_reference, multi)
    );
    let _ = writeln!(
        o,
        "- Current values with no source: {} of {} ({})\n",
        a.unsourced_values,
        a.current_values,
        pct(a.unsourced_values, a.current_values)
    );

    let _ = writeln!(o, "## Shapes by data type\n");
    let _ = writeln!(
        o,
        "| Data type | Groups | single | timeline | series | table | chips | list | no current |"
    );
    let _ = writeln!(o, "|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    let mut dts: Vec<_> = a.shape_by_datatype.iter().collect();
    dts.sort_by_key(|(_, m)| std::cmp::Reverse(m.values().sum::<u64>()));
    for (dt, m) in dts {
        let total: u64 = m.values().sum();
        let g = |s| m.get(&s).copied().unwrap_or(0);
        let _ = writeln!(
            o,
            "| {dt} | {total} | {} | {} | {} | {} | {} | {} | {} |",
            g(Shape::Single),
            g(Shape::Timeline),
            g(Shape::Series),
            g(Shape::Table),
            g(Shape::Chips),
            g(Shape::List),
            g(Shape::NoCurrentValue)
        );
    }

    let _ = writeln!(o, "\n## Sensitivity: shape counts as one threshold moves\n");
    let shown = [
        Shape::Timeline,
        Shape::Series,
        Shape::Table,
        Shape::Chips,
        Shape::List,
    ];
    let _ = write!(o, "| Variant |");
    for s in shown {
        let _ = write!(o, " {} |", s.name());
    }
    let _ = write!(o, "\n|---|");
    for _ in shown {
        let _ = write!(o, "---:|");
    }
    o.push('\n');
    for v in vs {
        let _ = write!(o, "| {} |", v.name);
        for s in shown {
            let _ = write!(o, " {} |", v.shapes.get(&s).copied().unwrap_or(0));
        }
        o.push('\n');
    }

    let top = |m: &Counts<String>, k: usize| {
        let mut v: Vec<_> = m.iter().collect();
        v.sort_by(|x, y| y.1.cmp(x.1).then(x.0.cmp(y.0)));
        v.into_iter()
            .take(k)
            .map(|(p, c)| format!("{p} ({c})"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(
        o,
        "\n## The properties behind each shape (groups of two or more)\n"
    );
    for s in [
        Shape::Timeline,
        Shape::Series,
        Shape::Table,
        Shape::Chips,
        Shape::List,
    ] {
        if let Some(m) = a.properties.get(&s) {
            let _ = writeln!(o, "- {}: {}", s.name(), top(m, 12));
        }
    }
    let _ = writeln!(
        o,
        "\nOne-column tables, by property and column: {}",
        top(&a.one_column, 12)
    );
    let _ = writeln!(o, "\n## Examples (first groups of two or more found)\n");
    for (s, ex) in &a.examples {
        let _ = writeln!(o, "- {}: {}", s.name(), ex.join("; "));
    }
    o
}
