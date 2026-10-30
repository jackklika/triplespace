//! The built frontend (0034 §8): `ui/dist`, which `npm run build` in `ui/` writes, embedded
//! in the binary in a release build and read from disk in a debug one. Its manifest names
//! the hashed files a page links to.
//!
//! A build without `ui/dist` (no Node on the machine, as in the Rust CI job) still
//! compiles and serves pages; they link no stylesheet or script, which a page reads
//! without.

use std::borrow::Cow;
use std::sync::LazyLock;

use sha2::{Digest as _, Sha256};

#[derive(rust_embed::Embed)]
#[folder = "../../ui/dist"]
#[allow_missing = true]
struct Dist;

/// The entries of the manifest the site links to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entries {
    /// The stylesheet, left to right.
    pub css: Option<String>,
    /// The stylesheet, right to left.
    pub css_rtl: Option<String>,
    /// The script.
    pub js: Option<String>,
    /// The blocking script in `<head>` that applies a reader's stored appearance before
    /// the first paint (0085 §6).
    pub head_js: Option<String>,
}

fn entry(manifest: &serde_json::Value, src: &str) -> Option<String> {
    manifest
        .get(src)
        .and_then(|e| e.get("file"))
        .and_then(serde_json::Value::as_str)
        .map(|f| format!("/ui/assets/{f}"))
}

static MANIFEST: LazyLock<(Entries, String)> = LazyLock::new(|| {
    let raw = Dist::get("manifest.json").map(|f| f.data.into_owned());
    let manifest: serde_json::Value = raw
        .as_deref()
        .and_then(|b| serde_json::from_slice(b).ok())
        .unwrap_or(serde_json::Value::Null);
    let entries = Entries {
        css: entry(&manifest, "src/site.css"),
        css_rtl: entry(&manifest, "src/site-rtl.css"),
        js: entry(&manifest, "src/main.js"),
        head_js: entry(&manifest, "src/head.js"),
    };
    // The build ID: the crate's version and the manifest's hash, so that a new frontend is
    // a new build even when the Rust is unchanged (0057 §8).
    let mut h = Sha256::new();
    h.update(env!("CARGO_PKG_VERSION").as_bytes());
    h.update(raw.as_deref().unwrap_or_default());
    let id = crate::hex(&h.finalize()[..6]);
    (entries, format!("{}-{id}", env!("CARGO_PKG_VERSION")))
});

/// The manifest's entries.
#[must_use]
pub fn entries() -> &'static Entries {
    &MANIFEST.0
}

/// The `@font-face` rules of a shipped type set's typefaces (0086 §3), built from
/// `ui/src/typesets/{name}.css`, whose `url()`s name the hashed font files under
/// `/ui/assets/`; `None` for a type set the build has none for.
#[must_use]
pub fn typeset_fonts(name: &str) -> Option<String> {
    built_css("src/typesets/", name)
}

/// The `@font-face` rules of a fallback layer (0093 §3), built from
/// `ui/src/fallbacks/{name}.css`, which `ui/scripts/fallbacks.js` writes from the registry;
/// `None` for a layer the build has none for.
#[must_use]
pub fn fallback_fonts(name: &str) -> Option<String> {
    built_css("src/fallbacks/", name)
}

/// A built stylesheet of the build's `@font-face` rules, by its source directory and name.
fn built_css(dir: &str, name: &str) -> Option<String> {
    static FONTS: LazyLock<std::collections::BTreeMap<String, String>> = LazyLock::new(|| {
        let manifest: serde_json::Value = Dist::get("manifest.json")
            .and_then(|f| serde_json::from_slice(&f.data).ok())
            .unwrap_or(serde_json::Value::Null);
        let Some(entries) = manifest.as_object() else {
            return std::collections::BTreeMap::new();
        };
        entries
            .iter()
            .filter(|(src, _)| {
                src.starts_with("src/typesets/") || src.starts_with("src/fallbacks/")
            })
            .filter_map(|(src, e)| {
                let src = src.strip_suffix(".css")?;
                let file = e.get("file")?.as_str()?;
                let css = String::from_utf8(Dist::get(file)?.data.into_owned()).ok()?;
                Some((src.to_string(), css))
            })
            .collect()
    });
    FONTS.get(&format!("{dir}{name}")).cloned()
}

/// The build ID, which a page's `ETag` and the components' build check carry.
#[must_use]
pub fn build_id() -> &'static str {
    &MANIFEST.1
}

/// A file of the build, by its path under `/ui/assets/`, with its content type.
#[must_use]
pub fn get(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    if path == "manifest.json" || path.contains("..") {
        return None;
    }
    let file = Dist::get(path)?;
    Some((file.data, content_type(path)))
}

/// The content type of a file, by its extension.
#[must_use]
pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or_default() {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "svg" => "image/svg+xml",
        "json" => "application/json",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_types_and_refusals() {
        assert_eq!(content_type("site-abc.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("x.woff2"), "font/woff2");
        assert!(get("manifest.json").is_none());
        assert!(get("../Cargo.toml").is_none());
        assert!(build_id().starts_with(env!("CARGO_PKG_VERSION")));
    }
}
