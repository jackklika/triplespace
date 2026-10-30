//! `cargo xtask skin-parity`: the parity harness of the skin migration (0097 §16;
//! 19 §5.10), run as a release build with timing rounds, and its report.
//!
//! The harness itself is `crates/triplespace-ui/tests/parity.rs`: every case of the
//! corpus rendered by both renderers and compared as a browser reads it. This task runs
//! it, writes `target/skin-parity/report.tsv` (one row per case: route, scenario, equal
//! or not, the first differing node, and each renderer's median and 95th-percentile
//! time), and prints the summary: the cases that differ, and per group whether the
//! skin's time is within the budget of the built-in renderer's. The built-in renderer
//! against itself reads within 1% at the median and within a few percent at the 95th
//! percentile over 100 rounds, and well outside that under 20, so fewer rounds show
//! differences but cannot judge the budget.
//!
//! ```text
//! cargo xtask skin-parity                # 100 rounds, release build
//! cargo xtask skin-parity --rounds 10    # quicker, too noisy for the budget
//! cargo xtask skin-parity --debug        # a debug build, quicker to compile
//! ```

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// The budget (0097 §16): the skin's median and 95th percentile within this fraction of
/// the built-in renderer's.
const BUDGET: f64 = 0.05;

pub fn run(root: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let mut rounds = 100u32;
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
    cmd.args(["--", "every_case_renders_equal_through_both_renderers"])
        .env("SKIN_PARITY_ROUNDS", rounds.to_string())
        .env("SKIN_PARITY_REPORT", &report)
        .current_dir(root);
    let status = cmd.status().context("running the parity harness")?;
    let text = std::fs::read_to_string(&report)
        .with_context(|| format!("the harness wrote no report at {}", report.display()))?;
    summarize(&text);
    println!("report: {}", report.display());
    if !status.success() {
        bail!("a case that must be equal differs");
    }
    Ok(())
}

/// A row of the report.
struct Row<'a> {
    case: &'a str,
    group: &'a str,
    equal: bool,
    builtin: (f64, f64),
    skin: (f64, f64),
    difference: &'a str,
}

fn rows(text: &str) -> Vec<Row<'_>> {
    let mut lines = text.lines();
    let Some(head) = lines.next() else {
        return Vec::new();
    };
    let cols: Vec<&str> = head.split('\t').collect();
    let col = |name: &str| cols.iter().position(|c| *c == name);
    let (Some(case), Some(group), Some(equal), Some(bm), Some(bp), Some(sm), Some(sp), Some(diff)) = (
        col("case"),
        col("group"),
        col("equal"),
        col("builtin_median_us"),
        col("builtin_p95_us"),
        col("skin_median_us"),
        col("skin_p95_us"),
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
                builtin: (num(bm), num(bp)),
                skin: (num(sm), num(sp)),
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
    // Per group: the skin's time against the built-in's, median and p95, summed over
    // the group's cases.
    let mut groups: BTreeMap<u32, (f64, f64, f64, f64)> = BTreeMap::new();
    for r in &rows {
        let g = r.group.parse().unwrap_or(0);
        let e = groups.entry(g).or_default();
        e.0 += r.builtin.0;
        e.1 += r.builtin.1;
        e.2 += r.skin.0;
        e.3 += r.skin.1;
    }
    println!("group  builtin median/p95 (ms)  skin median/p95 (ms)  skin vs builtin");
    for (g, (bm, bp, sm, sp)) in &groups {
        let ratio = |s: f64, b: f64| if b > 0.0 { s / b - 1.0 } else { 0.0 };
        let (rm, rp) = (ratio(*sm, *bm), ratio(*sp, *bp));
        let verdict = if rm <= BUDGET && rp <= BUDGET {
            "within budget"
        } else {
            "OVER BUDGET"
        };
        println!(
            "{g:>5}  {:>10.2} / {:<10.2}  {:>9.2} / {:<9.2}  {:+.1}% / {:+.1}%  {verdict}",
            bm / 1000.0,
            bp / 1000.0,
            sm / 1000.0,
            sp / 1000.0,
            rm * 100.0,
            rp * 100.0
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_is_read_by_column_name() {
        let text = "case\tset\tgroup\tviewer\tpath\tstatus\tequal\tbuiltin_median_us\tbuiltin_p95_us\tskin_median_us\tskin_p95_us\tdifference\n\
                    a\tfixture\t3\tanonymous\t/x\t200\tequal\t100\t120\t101\t125\t\n\
                    b\tfixture\t3\tanonymous\t/y\t200\tdiffers\t100\t120\t200\t250\tat html[0]\n";
        let r = rows(text);
        assert_eq!(r.len(), 2);
        assert!(r[0].equal && !r[1].equal);
        assert_eq!(r[1].skin, (200.0, 250.0));
        assert_eq!(r[1].difference, "at html[0]");
    }
}
