//! `cargo xtask skin-parity`: the skins' parity check (0097 §16, A12; 19 §5.10), that
//! Worksheet is Plain with the site's stylesheets and nothing else, with its report.
//!
//! The check itself is `crates/triplespace-ui/tests/parity.rs`: every case of the corpus
//! rendered with Worksheet and with Plain, Worksheet's stylesheet links taken out, and
//! compared as a browser reads it. This task runs it, writes
//! `target/skin-parity/report.tsv` (one row per case: route, scenario, equal or not, the
//! first differing node, and each skin's median and 95th-percentile time), and prints
//! the summary: the cases that differ, and each migration group's render time. Until the
//! built-in renderer was removed (A12) it compared that renderer with the skin and judged
//! the skin's time against a budget; the times are now a watch, not a gate.
//!
//! ```text
//! cargo xtask skin-parity                # 1 round, release build
//! cargo xtask skin-parity --rounds 100   # timing worth comparing between runs
//! cargo xtask skin-parity --debug        # a debug build, quicker to compile
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run(root: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let mut rounds = 1u32;
    let mut release = true;
    let mut args = args.peekable();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--rounds" => {
                rounds = args
                    .next()
                    .and_then(|r| r.parse().ok())
                    .context("--rounds takes a number")?;
            }
            "--debug" => release = false,
            other => bail!("unknown argument `{other}`; arguments: --rounds N, --debug"),
        }
    }
    let report = root.join("target/skin-parity/report.tsv");
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["test", "-p", "triplespace-ui", "--test", "parity"]);
    if release {
        cmd.arg("--release");
    }
    cmd.args(["--", "worksheet_is_plain_with_its_stylesheets"])
        .env("SKIN_PARITY_ROUNDS", rounds.to_string())
        .env("SKIN_PARITY_REPORT", &report)
        .current_dir(root);
    let status = cmd.status().context("running the parity harness")?;
    let text = std::fs::read_to_string(&report)
        .with_context(|| format!("the harness wrote no report at {}", report.display()))?;
    summarize(&text);
    println!("report: {}", report.display());
    if !status.success() {
        bail!("Worksheet adds more than its stylesheets to Plain");
    }
    Ok(())
}

/// A row of the report.
struct Row<'a> {
    case: &'a str,
    group: &'a str,
    equal: bool,
    worksheet: (f64, f64),
    plain: (f64, f64),
    difference: &'a str,
}

fn rows(text: &str) -> Vec<Row<'_>> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    let cols: Vec<&str> = head.split('\t').collect();
    let col = |name: &str| cols.iter().position(|c| *c == name);
    let (Some(case), Some(group), Some(equal), Some(wm), Some(wp), Some(pm), Some(pp), Some(diff)) = (
        col("case"),
        col("group"),
        col("equal"),
        col("worksheet_median_us"),
        col("worksheet_p95_us"),
        col("plain_median_us"),
        col("plain_p95_us"),
        col("difference"),
    ) else {
        return Vec::new();
    };
    lines
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0);
            Some(Row {
                case: f.get(case)?,
                group: f.get(group)?,
                equal: f.get(equal) == Some(&"equal"),
                worksheet: (num(wm), num(wp)),
                plain: (num(pm), num(pp)),
                difference: f.get(diff).copied().unwrap_or_default(),
            })
        })
        .collect()
}

fn summarize(text: &str) {
    let rows = rows(text);
    let equal = rows.iter().filter(|r| r.equal).count();
    println!("skin-parity: {equal} of {} cases equal", rows.len());
    for r in rows.iter().filter(|r| !r.equal) {
        println!(
            "  differs: {} (group {}): {}",
            r.case, r.group, r.difference
        );
    }
    // Per group: each skin's time, median and p95, summed over the group's cases.
    let mut groups: BTreeMap<u32, (f64, f64, f64, f64)> = BTreeMap::new();
    for r in &rows {
        let g = r.group.parse().unwrap_or(0);
        let e = groups.entry(g).or_default();
        e.0 += r.worksheet.0;
        e.1 += r.worksheet.1;
        e.2 += r.plain.0;
        e.3 += r.plain.1;
    }
    println!("group  Worksheet median/p95 (ms)  Plain median/p95 (ms)");
    for (g, (wm, wp, pm, pp)) in &groups {
        println!(
            "{g:>5}  {:>12.2} / {:<12.2}  {:>9.2} / {:<9.2}",
            wm / 1000.0,
            wp / 1000.0,
            pm / 1000.0,
            pp / 1000.0,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_read_by_column_name() {
        let text = "case\tset\tgroup\tviewer\tpath\tstatus\tequal\tworksheet_median_us\tworksheet_p95_us\tplain_median_us\tplain_p95_us\tdifference\n\
                    a\tfixture\t3\tanonymous\t/x\t200\tequal\t100\t120\t101\t125\t\n\
                    b\tfixture\t3\tanonymous\t/y\t200\tdiffers\t100\t120\t200\t250\tat html[0]\n";
        let r = rows(text);
        assert_eq!(r.len(), 2);
        assert!(r[0].equal && !r[1].equal);
        assert_eq!(r[1].plain, (200.0, 250.0));
        assert_eq!(r[1].difference, "at html[0]");
    }
}
