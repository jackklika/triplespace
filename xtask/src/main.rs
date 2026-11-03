//! Repository tasks, run as `cargo xtask <command>` (alias in `.cargo/config.toml`).
//!
//! - `deps`: checks the workspace dependency graph against the crate map, `docs/registry/crates.toml`
//!   (chapter 22 §2.1; 0005 §2), and the rules of 22 §1.2 (dependencies point only downward; the
//!   core is pure; the site's crates reach no store).
//! - `wasm`: builds every crate the map marks `wasm` (22 §1.2 rule 7), skipping the ones that do
//!   not exist yet.
//! - `manifest`: writes the component manifest of ADR 0077 §15 ([`manifest`]).
//! - `skin-parity`: the check that Worksheet is Plain with stylesheets ([`parity`]).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

mod manifest;
mod parity;

/// The crate map: every crate, its layer, what it may depend on, and the `pure` and `wasm`
/// marks (22 §2.1). `docs/architecture/crates.py` generates the chapter's table from it.
const CRATE_MAP: &str = "docs/registry/crates.toml";

/// Dependencies a pure crate may not have (0005 §3 rule 2, 0033 §3–4).
const IMPURE_DEPS: &[&str] = &[
    "tokio",
    "sqlx",
    "tokio-postgres",
    "reqwest",
    "axum",
    "hyper",
];

/// The site's crates (0005 §3 rule 10): they reach the instance only through its API.
const SITE_CRATES: &[&str] = &["triplespace-ui", "triplespace-client", "triplespace-web"];

/// Clients of the instance's stores; no crate the site's crates reach may depend on one
/// (0005 §3 rule 10). `redis` is not here: the response cache's Valkey client holds
/// public API responses only.
const STORE_CLIENTS: &[&str] = &[
    "tokio-postgres",
    "deadpool-postgres",
    "postgres",
    "sqlx",
    "opensearch",
    "elasticsearch",
];

/// Workspace crates the site's crates may not reach, whatever their dependencies: the
/// API crates, and those that open the instance's stores (0005 §3 rule 10).
const STORE_CRATES: &[&str] = &[
    "triplespace-db",
    "triplespace-projections",
    "triplespace-accounts",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let root = workspace_root()?;
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("deps") => deps(&root),
        Some("wasm") => wasm(&root),
        Some("manifest") => manifest::run(&root, args),
        Some("skin-parity") => parity::run(&root, args),
        Some(other) => bail!("unknown task `{other}`; tasks: deps, wasm, manifest, skin-parity"),
        None => bail!("usage: cargo xtask <deps|wasm|manifest|skin-parity>"),
    }
}

fn workspace_root() -> Result<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("xtask is not inside the workspace"))
}

// ---------------------------------------------------------------------------------------
// cargo metadata
// ---------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
    manifest_path: PathBuf,
}

impl Package {
    /// Vendored code (a tree with a `vendor.toml`, 0077 §15) is a path dependency and so a
    /// workspace member, but it is not a crate of the crate map.
    fn is_vendored(&self) -> bool {
        self.manifest_path
            .parent()
            .is_some_and(|d| d.join("vendor.toml").is_file())
    }
}

#[derive(Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

fn metadata(root: &Path) -> Result<Metadata> {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .context("running cargo metadata")?;
    if !out.status.success() {
        bail!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).context("parsing cargo metadata")
}

// ---------------------------------------------------------------------------------------
// The crate map (docs/registry/crates.toml)
// ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Layer {
    Substrate,
    Wikibase,
    Ingest,
    Triplespace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AnyRule {
    /// "lower": anything in the same or a lower layer.
    Lower,
    /// "core": any substrate or Wikibase crate, plus the named ones.
    Core,
}

#[derive(Debug, Deserialize)]
struct CrateMap {
    #[serde(rename = "crate")]
    crates: Vec<CrateEntry>,
}

/// One `[[crate]]` entry; the other fields (summary, contents, depends_note) are the
/// documentation's and are not read here.
#[derive(Debug, Deserialize)]
struct CrateEntry {
    names: Vec<String>,
    layer: Layer,
    #[serde(default)]
    depends: Vec<String>,
    #[serde(default)]
    depends_any: Option<AnyRule>,
    #[serde(default)]
    pure: bool,
    #[serde(default)]
    wasm: bool,
}

#[derive(Debug)]
struct TableRow {
    layer: Layer,
    /// Crates the entry names (internal and external).
    named: BTreeSet<String>,
    /// The entry said "any …": the layer rule applies instead of the named list.
    any: Option<AnyRule>,
    /// 22 §1.2 rule 2.
    pure: bool,
    /// 22 §1.2 rule 7.
    wasm: bool,
}

/// Reads the crate map: one row per crate, keyed by name.
fn crate_table(root: &Path) -> Result<BTreeMap<String, TableRow>> {
    let text = std::fs::read_to_string(root.join(CRATE_MAP))
        .with_context(|| format!("reading {CRATE_MAP}"))?;
    let map: CrateMap = toml::from_str(&text).with_context(|| format!("parsing {CRATE_MAP}"))?;
    let mut rows = BTreeMap::new();
    for entry in map.crates {
        let named: BTreeSet<String> = entry.depends.iter().cloned().collect();
        for name in entry.names {
            if rows
                .insert(
                    name.clone(),
                    TableRow {
                        layer: entry.layer,
                        named: named.clone(),
                        any: entry.depends_any,
                        pure: entry.pure,
                        wasm: entry.wasm,
                    },
                )
                .is_some()
            {
                bail!("{CRATE_MAP}: `{name}` is listed twice");
            }
        }
    }
    if rows.is_empty() {
        bail!("{CRATE_MAP}: no crates");
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------------------
// cargo xtask deps
// ---------------------------------------------------------------------------------------

fn deps(root: &Path) -> Result<()> {
    let table = crate_table(root)?;
    let meta = metadata(root)?;
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id) && !p.is_vendored())
        .map(|p| p.name.as_str())
        .collect();

    let mut problems = Vec::new();
    for pkg in meta
        .packages
        .iter()
        .filter(|p| members.contains(p.name.as_str()))
    {
        let name = pkg.name.as_str();
        if name == "xtask" {
            continue;
        }
        let Some(row) = table.get(name) else {
            problems.push(format!(
                "`{name}` is in the workspace but not in the crate map (docs/registry/crates.toml)"
            ));
            continue;
        };
        for dep in pkg
            .dependencies
            .iter()
            .filter(|d| d.kind.as_deref() != Some("dev"))
        {
            let dep_name = dep.name.as_str();
            let internal = members.contains(dep_name) && dep_name != "xtask";

            // Rule 2: the core is pure.
            if row.pure && IMPURE_DEPS.contains(&dep_name) {
                problems.push(format!(
                    "`{name}` is a pure crate (22 §1.2 rule 2) but depends on `{dep_name}`"
                ));
            }
            if !internal {
                continue;
            }

            // Rule 1: dependencies point only downward.
            if name.starts_with("scatter-") && dep_name.starts_with("triplespace-") {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`: no scatter-* crate may depend on a triplespace-* crate (0005 §3 rule 1)"
                ));
                continue;
            }
            let Some(dep_row) = table.get(dep_name) else {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`, which is not in the crate map (docs/registry/crates.toml)"
                ));
                continue;
            };
            if dep_row.layer > row.layer {
                problems.push(format!(
                    "`{name}` ({:?}) depends on `{dep_name}` ({:?}): dependencies point only downward (0005 §3 rule 1)",
                    row.layer, dep_row.layer
                ));
                continue;
            }

            // The row's own "Depends on" cell.
            let allowed = row.named.contains(dep_name)
                || match row.any {
                    Some(AnyRule::Lower) => true,
                    Some(AnyRule::Core) => dep_row.layer <= Layer::Wikibase,
                    None => false,
                };
            if !allowed {
                problems.push(format!(
                    "`{name}` depends on `{dep_name}`, which its crates.toml entry does not list (listed: {})",
                    if row.named.is_empty() {
                        "—".to_string()
                    } else {
                        row.named.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", ")
                    }
                ));
            }
        }
    }

    problems.extend(site_reaches_no_store(&meta, &members));

    let checked = members.len().saturating_sub(1);
    if problems.is_empty() {
        println!(
            "deps: {checked} workspace crates match the crate map ({} entries)",
            table.len()
        );
        Ok(())
    } else {
        for p in &problems {
            eprintln!("error: {p}");
        }
        bail!("{} dependency problem(s)", problems.len())
    }
}

/// Rule 10: walks every workspace crate a site crate reaches through normal and build
/// dependencies, and reports any that is an API or store crate or depends on a store
/// client.
fn site_reaches_no_store(meta: &Metadata, members: &BTreeSet<&str>) -> Vec<String> {
    let by_name: BTreeMap<&str, &Package> = meta
        .packages
        .iter()
        .filter(|p| members.contains(p.name.as_str()))
        .map(|p| (p.name.as_str(), p))
        .collect();
    let mut problems = Vec::new();
    for site in SITE_CRATES {
        let Some(start) = by_name.get(site) else {
            continue;
        };
        // Breadth first, remembering the path to each crate for the message.
        let mut path_to: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        path_to.insert(start.name.as_str(), vec![start.name.as_str()]);
        let mut queue = vec![*start];
        while let Some(pkg) = queue.pop() {
            let here = path_to[pkg.name.as_str()].clone();
            for dep in pkg
                .dependencies
                .iter()
                .filter(|d| d.kind.as_deref() != Some("dev"))
            {
                let name = dep.name.as_str();
                let via = || {
                    let mut p = here.clone();
                    p.push(name);
                    p.join(" → ")
                };
                if STORE_CLIENTS.contains(&name) {
                    problems.push(format!(
                        "`{site}` reaches the store client `{name}` ({}): the site's crates reach no store (0005 §3 rule 10)",
                        via()
                    ));
                    continue;
                }
                let Some(next) = by_name.get(name) else {
                    continue;
                };
                if STORE_CRATES.contains(&name) || name.starts_with("triplespace-api-") {
                    problems.push(format!(
                        "`{site}` reaches `{name}` ({}): the site's crates reach the instance only through its API (0005 §3 rule 10)",
                        via()
                    ));
                    continue;
                }
                if !path_to.contains_key(name) {
                    let mut p = here.clone();
                    p.push(name);
                    path_to.insert(name, p);
                    queue.push(next);
                }
            }
        }
    }
    problems
}

// ---------------------------------------------------------------------------------------
// cargo xtask wasm
// ---------------------------------------------------------------------------------------

fn wasm(root: &Path) -> Result<()> {
    let table = crate_table(root)?;
    let wasm_crates: Vec<&str> = table
        .iter()
        .filter(|(_, row)| row.wasm)
        .map(|(name, _)| name.as_str())
        .collect();
    let meta = metadata(root)?;
    let members: BTreeSet<&str> = meta
        .packages
        .iter()
        .filter(|p| meta.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .collect();
    let present: Vec<&str> = wasm_crates
        .iter()
        .copied()
        .filter(|c| members.contains(c))
        .collect();
    for absent in wasm_crates.iter().filter(|c| !members.contains(*c)) {
        println!("wasm: `{absent}` not in the workspace yet, skipped");
    }
    if present.is_empty() {
        println!("wasm: nothing to build");
        return Ok(());
    }
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["build", "--target", "wasm32-unknown-unknown"])
        .current_dir(root);
    for c in &present {
        cmd.args(["-p", c]);
    }
    let status = cmd.status().context("running cargo build for wasm32")?;
    if !status.success() {
        bail!("wasm32 build failed for {}", present.join(", "));
    }
    println!("wasm: built {}", present.join(", "));
    Ok(())
}
