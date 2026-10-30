//! Skins (0097 §8–§10; 19 §5.10): the site's HTML presentation apart from the binary.
//!
//! A skin is a directory with a manifest, `skin.toml`, and what the manifest names:
//! MiniJinja templates under `templates/`, messages under `i18n/`, and the stylesheets
//! and files a skin that extends Plain adds. Two ship embedded in the binary from
//! `skins/` in the repository, **Plain** and **Worksheet**; the operator installs others
//! as directories under `server.skins` or `web.skins`, and one of the same name replaces
//! a shipped one.
//!
//! - **The manifest** gives the name, version, licence and requirements: for a skin that
//!   stands alone, the template contract and the Codex token major it is written for;
//!   for one that extends Plain, Plain's version. A skin whose requirements this binary
//!   does not meet is not loaded; the reason is kept for `Special:Version`.
//! - **The engine is MiniJinja**: templates are parsed once when the skin is loaded,
//!   rendered with autoescape on and [`crate::html::esc`] as the escaper, with no access
//!   to files, the network or the environment. A template that names a variable the
//!   view model lacks fails, rather than rendering nothing in its place.
//! - **A skin that extends Plain** has Plain's templates and adds stylesheets, so its
//!   pages are Plain's DOM with its stylesheets linked after Plain's and before the
//!   theme's (§5.9).
//!
//! The template contract, [`TEMPLATE_CONTRACT`], is versioned on its own and written in
//! `docs/api/skin-contract.md` once a skin that stands alone renders every route; until
//! then it is major 0 (0097 §16).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use minijinja::value::{Object, Value};
use minijinja::{AutoEscape, Environment, State, UndefinedBehavior};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::i18n::Messages;

/// The template contract this binary offers (0097 §9): the templates, the view models and
/// the obligations of 19 §5.10. Major 0 until a skin that stands alone renders every
/// route.
pub const TEMPLATE_CONTRACT: &str = "0.1.0";

/// The `@wikimedia/codex-design-tokens` release the binary pins (19 §5.9), whose major a
/// skin that stands alone names; `ui/package.json` is the authority, and a test holds
/// the two together.
pub const CODEX_TOKENS: &str = "2.7.0";

/// The skins the repository ships, embedded in both binaries (0097 §8).
#[derive(rust_embed::Embed)]
#[folder = "../../skins"]
struct Shipped;

/// The default skin, `skins.default`'s default: Plain with Codex's and the site's
/// stylesheets.
pub const DEFAULT: &str = "worksheet";

/// What `skin.toml` says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The name, which is the directory's: lower-case letters, digits and hyphens.
    pub name: String,
    /// The skin's version: of its DOM, for a skin that stands alone, and of its
    /// stylesheets for one that extends Plain.
    pub version: String,
    /// The licence, an SPDX expression.
    #[serde(default)]
    pub licence: String,
    /// The skin this one extends, by name: it has that skin's templates and DOM and adds
    /// stylesheets and the files they use, and nothing else.
    #[serde(default)]
    pub extends: Option<String>,
    /// The stylesheets a page links, in order, before the theme's: `@site` for the
    /// stylesheet the build writes from `ui/src/site.css` (with its right-to-left
    /// variant), or a file in the skin's directory.
    #[serde(default)]
    pub stylesheets: Vec<String>,
    /// What the skin needs of the binary.
    #[serde(default)]
    pub requires: Requires,
}

/// A manifest's requirements, each a Cargo-style version requirement.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    /// The template contract a skin that stands alone is written for.
    #[serde(default)]
    pub template_contract: Option<String>,
    /// The Codex token major it is written for.
    #[serde(default)]
    pub codex_tokens: Option<u64>,
    /// Plain's version, for a skin that extends it.
    #[serde(default)]
    pub plain: Option<String>,
}

/// A loaded skin.
pub struct Skin {
    manifest: Manifest,
    /// Where it came from: `shipped`, or the installed directory.
    origin: String,
    /// Every file of the skin but the manifest and the templates, by path within the
    /// skin's directory.
    files: BTreeMap<String, Vec<u8>>,
    /// The template environment: this skin's own, or its parent's.
    env: Arc<Environment<'static>>,
    /// The chain of skins whose stylesheets a page links: the root first.
    stylesheets: Vec<(String, String)>,
    /// The hash of everything, for asset URLs.
    hash: String,
}

impl std::fmt::Debug for Skin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Skin")
            .field("name", &self.manifest.name)
            .field("version", &self.manifest.version)
            .field("origin", &self.origin)
            .finish_non_exhaustive()
    }
}

impl Skin {
    /// The manifest.
    #[must_use]
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// The version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.manifest.version
    }

    /// `shipped`, or the directory the skin was installed from.
    #[must_use]
    pub fn origin(&self) -> &str {
        &self.origin
    }

    /// The name and version together, `worksheet 0.1.0`: part of the build ID of a page
    /// the skin drew (0097 §15).
    #[must_use]
    pub fn build(&self) -> String {
        format!("{} {}", self.manifest.name, self.manifest.version)
    }

    /// The stylesheets a page links, in order, before the theme's: each as the skin it
    /// belongs to and the entry (`@site` or a file).
    #[must_use]
    pub fn stylesheets(&self) -> &[(String, String)] {
        &self.stylesheets
    }

    /// A file of the skin, by path within its directory.
    #[must_use]
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// The URL of a file of the skin, content-hashed: `/ui/skins/{name}/{hash}/{path}`.
    #[must_use]
    pub fn asset_url(&self, path: &str) -> Option<String> {
        self.files
            .contains_key(path)
            .then(|| format!("/ui/skins/{}/{}/{path}", self.manifest.name, self.hash))
    }

    /// The hash of the skin's contents.
    #[must_use]
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Renders a template of the contract with a view model and the page's messages.
    ///
    /// # Errors
    ///
    /// When the template is missing, or fails: a variable the view model lacks, a
    /// message key the bundle lacks.
    pub fn render(
        &self,
        template: &str,
        m: &Messages,
        model: impl serde::Serialize,
    ) -> Result<String, minijinja::Error> {
        let t = self.env.get_template(template)?;
        let msg = Value::from_object(Msg(m.clone()));
        let model = Value::from(minijinja::value::Serde(&model));
        t.render(minijinja::context! { msg, ..model })
    }
}

/// `msg(key, …params)`: a message in the page's language, through the wiki's Interface
/// overrides (0097 §10). A key the bundle lacks is an error, as it is in the built-in
/// rendering's tests.
#[derive(Debug)]
struct Msg(Messages);

impl Object for Msg {
    fn call(
        self: &Arc<Self>,
        _state: &mut State<'_, '_>,
        args: &[Value],
    ) -> Result<Value, minijinja::Error> {
        let Some(key) = args.first().and_then(Value::as_str) else {
            return Err(minijinja::Error::new(
                minijinja::ErrorKind::InvalidOperation,
                "msg() takes a message key",
            ));
        };
        let params: Vec<String> = args[1..].iter().map(ToString::to_string).collect();
        let refs: Vec<&str> = params.iter().map(String::as_str).collect();
        Ok(Value::from(self.0.with(key, &refs)))
    }
}

/// The formatter: [`crate::html::esc`] for anything not marked safe, nothing for `none`.
fn format(
    out: &mut minijinja::Output,
    state: &mut State,
    value: &Value,
) -> Result<(), minijinja::Error> {
    if value.is_none() {
        return Ok(());
    }
    if value.is_safe() || !matches!(state.auto_escape(), AutoEscape::Html) {
        return write!(out, "{value}").map_err(minijinja::Error::from);
    }
    let text = match value.as_str() {
        Some(s) => crate::html::esc(s),
        None => crate::html::esc(&value.to_string()),
    };
    out.write_str(&text).map_err(minijinja::Error::from)
}

/// A skin that could not be loaded, and why: shown on `Special:Version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The name, from the directory.
    pub name: String,
    /// Where it came from.
    pub origin: String,
    /// Why.
    pub reason: String,
}

/// The skins this binary has: those loaded, by name, and those refused.
#[derive(Debug, Default)]
pub struct Skins {
    loaded: BTreeMap<String, Arc<Skin>>,
    refused: Vec<Refused>,
}

/// A skin's files as read, before loading.
#[derive(Debug, Default)]
struct Raw {
    name: String,
    origin: String,
    files: BTreeMap<String, Vec<u8>>,
}

impl Skins {
    /// The shipped skins, then those installed in `dir`, read once; an installed skin
    /// replaces a shipped one of the same name. A skin that extends another is loaded
    /// after it, whichever directory each came from.
    #[must_use]
    pub fn load(dir: Option<&Path>) -> Self {
        let mut raw: BTreeMap<String, Raw> = BTreeMap::new();
        for path in Shipped::iter() {
            let Some((name, rest)) = path.split_once('/') else {
                continue;
            };
            let Some(f) = Shipped::get(&path) else {
                continue;
            };
            raw.entry(name.to_string())
                .or_insert_with(|| Raw {
                    name: name.to_string(),
                    origin: "shipped".into(),
                    ..Raw::default()
                })
                .files
                .insert(rest.to_string(), f.data.into_owned());
        }
        if let Some(dir) = dir {
            for r in read_dir(dir) {
                raw.insert(r.name.clone(), r);
            }
        }
        let mut skins = Self::default();
        skins.load_all(raw);
        skins
    }

    fn load_all(&mut self, raw: BTreeMap<String, Raw>) {
        // Roots first, then children of what is loaded, until nothing more loads.
        let mut pending: Vec<Raw> = raw.into_values().collect();
        loop {
            let before = pending.len();
            let mut rest = Vec::new();
            for r in pending {
                match self.load_one(&r) {
                    Ok(skin) => {
                        self.loaded.insert(r.name.clone(), Arc::new(skin));
                    }
                    Err(Wait) => rest.push(r),
                    Err(Refuse(reason)) => self.refused.push(Refused {
                        name: r.name.clone(),
                        origin: r.origin.clone(),
                        reason,
                    }),
                }
            }
            pending = rest;
            if pending.is_empty() || pending.len() == before {
                break;
            }
        }
        for r in pending {
            let parent = r
                .manifest()
                .ok()
                .and_then(|m| m.extends)
                .unwrap_or_default();
            self.refused.push(Refused {
                name: r.name,
                origin: r.origin,
                reason: format!("extends `{parent}`, which is not loaded"),
            });
        }
    }

    #[allow(clippy::too_many_lines)]
    fn load_one(&self, r: &Raw) -> Result<Skin, LoadError> {
        let manifest = r.manifest().map_err(Refuse)?;
        if manifest.name != r.name {
            return Err(Refuse(format!(
                "the manifest names `{}`, the directory `{}`",
                manifest.name, r.name
            )));
        }
        if !r
            .name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || r.name.is_empty()
        {
            return Err(Refuse(
                "a skin's name is lower-case letters, digits and hyphens".into(),
            ));
        }
        let version = semver::Version::parse(&manifest.version)
            .map_err(|e| Refuse(format!("version `{}`: {e}", manifest.version)))?;
        let req = |field: &str, text: &str| {
            semver::VersionReq::parse(text)
                .map_err(|e| Refuse(format!("requires.{field} `{text}`: {e}")))
        };
        let mut hasher = Sha256::new();
        for (path, data) in &r.files {
            hasher.update(path.as_bytes());
            hasher.update([0]);
            hasher.update(data);
            hasher.update([0]);
        }
        let hash = crate::hex(&hasher.finalize()[..8]);
        let files: BTreeMap<String, Vec<u8>> = r
            .files
            .iter()
            .filter(|(p, _)| p.as_str() != "skin.toml" && !p.starts_with("templates/"))
            .map(|(p, d)| (p.clone(), d.clone()))
            .collect();
        for s in &manifest.stylesheets {
            if s != "@site" && !files.contains_key(s) {
                return Err(Refuse(format!("stylesheet `{s}` is not in the skin")));
            }
        }
        let _ = version;
        if let Some(parent_name) = &manifest.extends {
            let Some(parent) = self.loaded.get(parent_name) else {
                return Err(Wait);
            };
            if r.files
                .keys()
                .any(|p| p.starts_with("templates/") || p.starts_with("i18n/"))
            {
                return Err(Refuse(format!(
                    "extends `{parent_name}` and so may add stylesheets and files only, not templates or messages"
                )));
            }
            if parent_name != "plain" {
                return Err(Refuse(format!(
                    "extends `{parent_name}`; only Plain can be extended"
                )));
            }
            let want = manifest
                .requires
                .plain
                .as_deref()
                .ok_or_else(|| Refuse("extends `plain` without requires.plain".into()))?;
            let want = req("plain", want)?;
            let have =
                semver::Version::parse(parent.version()).map_err(|e| Refuse(e.to_string()))?;
            if !want.matches(&have) {
                return Err(Refuse(format!(
                    "requires plain {want}; this binary has plain {have}"
                )));
            }
            let mut stylesheets = parent.stylesheets.clone();
            stylesheets.extend(
                manifest
                    .stylesheets
                    .iter()
                    .map(|s| (manifest.name.clone(), s.clone())),
            );
            return Ok(Skin {
                origin: r.origin.clone(),
                files,
                env: parent.env.clone(),
                stylesheets,
                hash,
                manifest,
            });
        }
        // A skin that stands alone: its requirements, then its templates.
        let want = manifest
            .requires
            .template_contract
            .as_deref()
            .ok_or_else(|| {
                Refuse("a skin that stands alone names requires.template_contract".into())
            })?;
        let want = req("template_contract", want)?;
        let have =
            semver::Version::parse(TEMPLATE_CONTRACT).expect("the contract's version parses");
        if !want.matches(&have) {
            return Err(Refuse(format!(
                "requires template contract {want}; this binary offers {have}"
            )));
        }
        let tokens = manifest
            .requires
            .codex_tokens
            .ok_or_else(|| Refuse("a skin that stands alone names requires.codex_tokens".into()))?;
        let have_tokens = semver::Version::parse(CODEX_TOKENS).expect("the token version parses");
        if tokens != have_tokens.major {
            return Err(Refuse(format!(
                "requires Codex tokens major {tokens}; this binary pins {CODEX_TOKENS}"
            )));
        }
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| AutoEscape::Html);
        env.set_formatter(format);
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        let mut templates = 0;
        for (path, data) in &r.files {
            if let Some(name) = path.strip_prefix("templates/") {
                let source = String::from_utf8(data.clone())
                    .map_err(|_| Refuse(format!("template `{name}` is not UTF-8")))?;
                env.add_template_owned(name.to_string(), source)
                    .map_err(|e| Refuse(format!("template `{name}`: {e}")))?;
                templates += 1;
            }
        }
        if templates == 0 {
            return Err(Refuse("a skin that stands alone has templates".into()));
        }
        let stylesheets = manifest
            .stylesheets
            .iter()
            .map(|s| (manifest.name.clone(), s.clone()))
            .collect();
        Ok(Skin {
            origin: r.origin.clone(),
            files,
            env: Arc::new(env),
            stylesheets,
            hash,
            manifest,
        })
    }

    /// A loaded skin, by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<Skin>> {
        self.loaded.get(name)
    }

    /// The loaded skins, by name.
    pub fn loaded(&self) -> impl Iterator<Item = &Arc<Skin>> {
        self.loaded.values()
    }

    /// The skins that could not be loaded.
    #[must_use]
    pub fn refused(&self) -> &[Refused] {
        &self.refused
    }

    /// The skin a page is drawn with (0097 §8): the reader's `useskin=` for one request,
    /// then their preference, then the wiki's default, each only among the skins the
    /// wiki offers (`available`, every loaded skin when the wiki names none) and loaded;
    /// then [`DEFAULT`], then any skin loaded.
    #[must_use]
    pub fn choose(
        &self,
        useskin: Option<&str>,
        preference: Option<&str>,
        default: Option<&str>,
        available: Option<&[String]>,
    ) -> Option<&Arc<Skin>> {
        let offered = |name: &str| available.is_none_or(|a| a.iter().any(|n| n == name));
        [useskin, preference, default, Some(DEFAULT)]
            .into_iter()
            .flatten()
            .filter(|n| offered(n))
            .find_map(|n| self.loaded.get(n))
            .or_else(|| self.loaded.values().next())
    }
}

/// Why a skin did not load now.
enum LoadError {
    /// Its parent is not loaded yet.
    Wait,
    /// It cannot be loaded, and this is why.
    Refuse(String),
}

use LoadError::{Refuse, Wait};

impl Raw {
    fn manifest(&self) -> Result<Manifest, String> {
        let text = self
            .files
            .get("skin.toml")
            .ok_or_else(|| "no skin.toml".to_string())?;
        let text = std::str::from_utf8(text).map_err(|_| "skin.toml is not UTF-8".to_string())?;
        toml::from_str(text).map_err(|e| format!("skin.toml: {}", e.message()))
    }
}

/// The skins installed under a directory: each subdirectory with a `skin.toml`, every
/// regular file under it read.
fn read_dir(dir: &Path) -> Vec<Raw> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        eprintln!(
            "triplespace-ui: the skins directory {} cannot be read",
            dir.display()
        );
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_dir() || !path.join("skin.toml").is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let mut files = BTreeMap::new();
        read_files(&path, &path, &mut files);
        out.push(Raw {
            name,
            origin: path.display().to_string(),
            files,
        });
    }
    out
}

fn read_files(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            read_files(root, &path, out);
        } else if let (Ok(rel), Ok(data)) = (path.strip_prefix(root), std::fs::read(&path)) {
            out.insert(rel.to_string_lossy().replace('\\', "/"), data);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_skins_load() {
        let skins = Skins::load(None);
        assert!(skins.refused().is_empty(), "{:?}", skins.refused());
        let plain = skins.get("plain").expect("plain");
        let worksheet = skins.get("worksheet").expect("worksheet");
        assert!(plain.stylesheets().is_empty());
        assert_eq!(
            worksheet.stylesheets(),
            &[("worksheet".to_string(), "@site".to_string())]
        );
        assert_eq!(worksheet.manifest().extends.as_deref(), Some("plain"));
        assert_eq!(
            skins.choose(None, None, None, None).unwrap().name(),
            "worksheet"
        );
        assert_eq!(
            skins
                .choose(Some("plain"), None, None, None)
                .unwrap()
                .name(),
            "plain"
        );
        // A skin the wiki does not offer is not chosen, and one not loaded falls through.
        assert_eq!(
            skins
                .choose(Some("plain"), None, None, Some(&["worksheet".to_string()]))
                .unwrap()
                .name(),
            "worksheet"
        );
        assert_eq!(
            skins
                .choose(Some("nope"), Some("plain"), None, None)
                .unwrap()
                .name(),
            "plain"
        );
    }

    #[test]
    fn the_codex_token_version_is_the_one_the_build_pins() {
        let package: serde_json::Value =
            serde_json::from_str(include_str!("../../../ui/package.json")).unwrap();
        assert_eq!(
            package["dependencies"]["@wikimedia/codex-design-tokens"].as_str(),
            Some(CODEX_TOKENS)
        );
    }

    fn raw(name: &str, files: &[(&str, &str)]) -> Raw {
        Raw {
            name: name.into(),
            origin: "test".into(),
            files: files
                .iter()
                .map(|(p, d)| ((*p).to_string(), d.as_bytes().to_vec()))
                .collect(),
        }
    }

    #[test]
    fn requirements_are_checked_and_refusals_say_why() {
        let mut skins = Skins::load(None);
        let mut raw_skins = BTreeMap::new();
        raw_skins.insert(
            "old".to_string(),
            raw(
                "old",
                &[
                    ("skin.toml", "name = \"old\"\nversion = \"1.0.0\"\n[requires]\ntemplate_contract = \"7\"\ncodex_tokens = 2\n"),
                    ("templates/frame.html", "x"),
                ],
            ),
        );
        raw_skins.insert(
            "child".to_string(),
            raw(
                "child",
                &[
                    ("skin.toml", "name = \"child\"\nversion = \"0.1.0\"\nextends = \"plain\"\nstylesheets = [\"c.css\"]\n[requires]\nplain = \"0.1\"\n"),
                    ("c.css", "body{}"),
                ],
            ),
        );
        raw_skins.insert(
            "orphan".to_string(),
            raw(
                "orphan",
                &[("skin.toml", "name = \"orphan\"\nversion = \"0.1.0\"\nextends = \"nothing\"\n[requires]\nplain = \"0.1\"\n")],
            ),
        );
        raw_skins.insert(
            "wrongname".to_string(),
            raw(
                "wrongname",
                &[("skin.toml", "name = \"other\"\nversion = \"0.1.0\"\n")],
            ),
        );
        skins.load_all(raw_skins);
        let child = skins.get("child").expect("the child loads");
        assert_eq!(child.stylesheets().len(), 1);
        assert_eq!(
            child.asset_url("c.css").unwrap(),
            format!("/ui/skins/child/{}/c.css", child.hash())
        );
        assert!(child.asset_url("skin.toml").is_none());
        let reasons: BTreeMap<&str, &str> = skins
            .refused()
            .iter()
            .map(|r| (r.name.as_str(), r.reason.as_str()))
            .collect();
        assert!(
            reasons["old"].contains("requires template contract ^7"),
            "{reasons:?}"
        );
        assert!(reasons["orphan"].contains("`nothing`, which is not loaded"));
        assert!(reasons["wrongname"].contains("names `other`"));
    }

    #[test]
    fn templates_escape_with_the_sites_escaper_and_fail_on_missing_variables() {
        let skins = Skins::load(None);
        let mut env = Environment::new();
        env.set_auto_escape_callback(|_| AutoEscape::Html);
        env.set_formatter(format);
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.add_template("t", "{{ a }}|{{ b|safe }}|{{ c }}|{{ msg(\"ts-login\") }}")
            .unwrap();
        let m = Messages::for_language("en");
        let skin = Skin {
            manifest: Manifest::default(),
            origin: String::new(),
            files: BTreeMap::new(),
            env: Arc::new(env),
            stylesheets: Vec::new(),
            hash: String::new(),
        };
        let out = skin
            .render(
                "t",
                &m,
                serde_json::json!({"a": "<'&\">", "b": "<b>", "c": null}),
            )
            .unwrap();
        assert_eq!(out, "&lt;&#39;&amp;&quot;&gt;|<b>||Log in");
        assert!(skin.render("t", &m, serde_json::json!({"a": 1})).is_err());
        let _ = skins;
    }
}
