//! `action=parse` (compat `#mod-parse`; 18 §2.3; 11 §1): a document page, a revision of
//! one, or given text, rendered by the tenant's renderer, in MediaWiki's shape.
//!
//! The page is `page=`, `pageid=` or `oldid=`, or the text is `text=` with `title=` (by
//! default `API`); `redirects` follows a redirect one hop; `pst` applies the pre-save
//! transform to `text=` first and `onlypst` returns only that. `prop` takes `text`,
//! `links`, `templates`, `categories`, `externallinks`, `sections`, `revid`,
//! `displaytitle`, `wikitext`, `parsewarnings`, `properties`, and `langlinks`, `images`
//! and `iwlinks`, which are empty here. The renderer is the built-in one (11 §1.2),
//! after expansion, which is part of wikitext (0042 A19). Entity pages are not
//! parsed: their HTML is the site's.

use std::collections::BTreeSet;

use scatter_wikitext::{Resolver as _, Title};
use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::pages::{self, Page, Site};
use crate::response::{ApiError, ApiResponse};

const DEFAULT_PROPS: &[&str] = &[
    "text",
    "langlinks",
    "categories",
    "links",
    "templates",
    "images",
    "externallinks",
    "sections",
    "revid",
    "displaytitle",
    "iwlinks",
    "properties",
    "parsewarnings",
];

fn missing_title() -> ApiError {
    ApiError::new("missingtitle", "The page you specified doesn't exist.")
}

/// What to parse.
struct Source {
    page: Option<Page>,
    title: Title,
    revid: Option<u64>,
    text: String,
    redirected_from: Option<String>,
}

async fn source(ctx: &Ctx, site: &Site, hidden: &pages::Hidden) -> Result<Source, ApiError> {
    let tenant = ctx.tenant.slug.as_str();
    if let Some(text) = ctx.params.get("text") {
        let name = ctx.params.get("title").unwrap_or("API");
        let title = pages::resolve_title(site, name)
            .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
        return Ok(Source {
            page: pages::by_title(ctx.db(), tenant, hidden, title.namespace, &title.text).await?,
            title,
            revid: None,
            text: text.to_string(),
            redirected_from: None,
        });
    }
    let (mut page, revid) = if let Some(oldid) = ctx.params.get("oldid") {
        let r: u64 = oldid
            .parse()
            .map_err(|_| ApiError::bad_value("oldid", oldid))?;
        let page = pages::by_revid(ctx.db(), tenant, hidden, r)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchrevid", format!("There is no revision with ID {r}."))
            })?;
        (page, Some(r))
    } else if let Some(id) = ctx.params.get("pageid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("pageid", id))?;
        let page = pages::by_id(ctx.db(), tenant, hidden, n)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
            })?;
        (page, None)
    } else if let Some(name) = ctx.params.get("page") {
        if pages::names_entity(site, name) {
            return Err(ApiError::new(
                "ts-entity-page",
                "Entity pages are not parsed; read them with wbgetentities, or the site.",
            ));
        }
        let title = pages::resolve_title(site, name)
            .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
        let page = pages::by_title(ctx.db(), tenant, hidden, title.namespace, &title.text)
            .await?
            .ok_or_else(missing_title)?;
        (page, None)
    } else {
        return Err(ApiError::new(
            "params",
            "One of the parameters \"text\", \"title\", \"page\", \"pageid\" and \"oldid\" is required.",
        ));
    };
    let mut redirected_from = None;
    if revid.is_none() && ctx.params.flag("redirects") && page.is_redirect {
        let target = ctx
            .db()
            .query_opt(
                "SELECT target_page_id FROM view.redirect WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &i64::try_from(page.page_id).unwrap_or(i64::MAX)],
            )
            .await?
            .and_then(|r| r.get::<_, Option<i64>>(0))
            .and_then(|t| u64::try_from(t).ok());
        if let Some(t) = target
            && let Some(p) = pages::by_id(ctx.db(), tenant, hidden, t).await?
        {
            redirected_from = Some(site.prefixed(page.ns, &page.title));
            page = p;
        }
    }
    if crate::modules::revisions::is_discussion(&page.model) {
        return Err(crate::modules::revisions::discussion_content());
    }
    let revid = revid.or(page.latest_revid);
    let text = match revid {
        Some(r) => pages::text_at(ctx.db(), tenant, page.page_id, r).await?,
        None => None,
    }
    .ok_or_else(|| ApiError::new("permissiondenied", "The text of this revision is hidden."))?;
    Ok(Source {
        title: Title::new(page.ns, &page.title),
        page: Some(page),
        revid,
        text,
        redirected_from,
    })
}

/// The signer of a pre-save transform: the caller.
pub(crate) fn signer_name(ctx: &Ctx) -> String {
    ctx.caller
        .identity
        .name
        .clone()
        .or_else(|| ctx.caller.identity.actor_key.clone())
        .unwrap_or_else(|| "127.0.0.1".into())
}

/// Applies the pre-save transform as the caller.
pub(crate) fn pst(ctx: &Ctx, site: &Site, text: &str) -> String {
    let user = site.prefixed(2, "x");
    let talk = site.prefixed(3, "x");
    let name = signer_name(ctx);
    let now = i64::try_from(crate::edit::now_micros() / 1_000_000).unwrap_or(0);
    scatter_wikitext_expand::pre_save_transform(
        text,
        &scatter_wikitext_expand::Signer {
            name: &name,
            user_ns: user.trim_end_matches(":x"),
            user_talk_ns: talk.trim_end_matches(":x"),
            talk: "talk",
            now,
        },
    )
}

/// `displaytitle` as MediaWiki 1.39 writes it by default.
fn display_title(site: &Site, title: &Title) -> String {
    let esc = |s: &str| {
        let mut o = String::new();
        scatter_wikitext::html::escape_text(s, &mut o);
        o
    };
    if title.namespace == 0 {
        return format!(
            "<span class=\"mw-page-title-main\">{}</span>",
            esc(&title.text)
        );
    }
    let prefixed = site.prefixed(title.namespace, &title.text);
    let ns = prefixed
        .strip_suffix(&title.text)
        .map_or("", |p| p.trim_end_matches(':'));
    format!(
        "<span class=\"mw-page-title-namespace\">{}</span><span class=\"mw-page-title-separator\">:</span><span class=\"mw-page-title-main\">{}</span>",
        esc(ns),
        esc(&title.text)
    )
}

/// `action=parse`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let hidden = pages::hidden(ctx).await?;
    let mut src = source(ctx, &site, &hidden).await?;
    let v2 = ctx.formatversion == 2;
    let flag = || if v2 { json!(true) } else { json!("") };
    if ctx.params.get("text").is_some() && (ctx.params.flag("pst") || ctx.params.flag("onlypst")) {
        src.text = pst(ctx, &site, &src.text);
        if ctx.params.flag("onlypst") {
            let text = if v2 {
                json!(src.text)
            } else {
                json!({"*": src.text})
            };
            return Ok(ApiResponse::ok(json!({"parse": {
                "title": site.prefixed(src.title.namespace, &src.title.text),
                "pageid": src.page.as_ref().map_or(0, |p| p.page_id),
                "text": text,
            }})));
        }
    }
    let mut props = ctx.params.list("prop");
    if props.is_empty() {
        props = DEFAULT_PROPS.iter().map(|s| (*s).to_string()).collect();
    }
    let want = |p: &str| props.iter().any(|x| x == p);
    let model = src.page.as_ref().map_or_else(
        || {
            ctx.params
                .get("contentmodel")
                .unwrap_or("wikitext")
                .to_string()
        },
        |p| p.model.clone(),
    );
    let tenant = ctx.tenant.slug.clone();
    let prefixed = site.prefixed(src.title.namespace, &src.title.text);
    let mut out = Map::new();
    out.insert("title".into(), json!(prefixed));
    out.insert(
        "pageid".into(),
        json!(src.page.as_ref().map_or(0, |p| p.page_id)),
    );
    if let Some(from) = &src.redirected_from {
        out.insert("redirects".into(), json!([{"from": from, "to": prefixed}]));
    }
    // Triplespace's: what the site's Restricted pill shows (19 §1.3), for a page.
    if let Some(p) = &src.page {
        let chain = triplespace_projections::acl_read::page_chain(ctx.db(), &tenant, p.page_id)
            .await
            .map_err(ApiError::internal)?;
        if let Some(v) = pages::visibility_of(&chain) {
            out.insert("ts-visibility".into(), v);
        }
    }
    if want("revid") {
        out.insert("revid".into(), json!(src.revid.unwrap_or(0)));
    }

    if model != "wikitext" {
        // Other text models show their text as text.
        let mut html = String::from("<div class=\"mw-parser-output\"><pre>");
        scatter_wikitext::html::escape_text(&src.text, &mut html);
        html.push_str("</pre></div>");
        if want("text") {
            out.insert(
                "text".into(),
                if v2 { json!(html) } else { json!({"*": html}) },
            );
        }
        if want("wikitext") {
            out.insert(
                "wikitext".into(),
                if v2 {
                    json!(src.text)
                } else {
                    json!({"*": src.text})
                },
            );
        }
        return Ok(ApiResponse::ok(json!({"parse": Value::Object(out)})));
    }

    let render = pages::render(ctx.db(), &tenant, &site, &hidden, &src.text, &src.title).await?;
    let ex = &render.rendered.extracted;
    if want("text") {
        let html = render.rendered.html.clone();
        out.insert(
            "text".into(),
            if v2 { json!(html) } else { json!({"*": html}) },
        );
    }
    if want("langlinks") {
        out.insert("langlinks".into(), json!([]));
    }
    if want("categories") {
        let mut seen = BTreeSet::new();
        let cats: Vec<Value> = ex
            .categories
            .iter()
            .filter(|c| seen.insert(c.name.clone()))
            .map(|c| {
                let key = c.sort_key.clone().unwrap_or_default();
                if v2 {
                    json!({"sortkey": key, "category": c.name.replace(' ', "_")})
                } else {
                    json!({"sortkey": key, "*": c.name.replace(' ', "_")})
                }
            })
            .collect();
        out.insert("categories".into(), Value::Array(cats));
    }
    if want("links") {
        let mut seen = BTreeSet::new();
        let mut links = Vec::new();
        for l in &ex.links {
            let t = l.target.without_fragment();
            if t.interwiki.is_some() || !seen.insert((t.namespace, t.text.clone())) {
                continue;
            }
            let exists = render.links.info(&t).exists;
            let title = site.prefixed(t.namespace, &t.text);
            let mut m = Map::new();
            m.insert("ns".into(), json!(t.namespace));
            if v2 {
                m.insert("title".into(), json!(title));
                m.insert("exists".into(), json!(exists));
            } else {
                if exists {
                    m.insert("exists".into(), json!(""));
                }
                m.insert("*".into(), json!(title));
            }
            links.push(Value::Object(m));
        }
        out.insert("links".into(), Value::Array(links));
    }
    if want("templates") {
        let mut templates = Vec::new();
        let mut seen = BTreeSet::new();
        let mut push = |ns: i32, title: &str, exists: bool| {
            if !seen.insert((ns, title.to_string())) {
                return;
            }
            let name = site.prefixed(ns, title);
            templates.push(if v2 {
                json!({"ns": ns, "title": name, "exists": exists})
            } else if exists {
                json!({"ns": ns, "exists": "", "*": name})
            } else {
                json!({"ns": ns, "*": name})
            });
        };
        for t in &render.transcluded {
            push(t.ns, &t.title, t.exists);
        }
        for call in &ex.templates {
            if let Some(t) = pages::resolve_title(&site, &format!("Template:{}", call.name))
                && !call.name.starts_with('#')
                && !call.name.contains(':')
            {
                let exists = pages::by_title(ctx.db(), &tenant, &hidden, t.namespace, &t.text)
                    .await?
                    .is_some();
                push(t.namespace, &t.text, exists);
            }
        }
        out.insert("templates".into(), Value::Array(templates));
    }
    if want("images") {
        out.insert("images".into(), json!([]));
    }
    if want("externallinks") {
        let mut seen = BTreeSet::new();
        let urls: Vec<Value> = ex
            .external_links
            .iter()
            .filter(|l| seen.insert(l.url.clone()))
            .map(|l| json!(l.url))
            .collect();
        out.insert("externallinks".into(), Value::Array(urls));
    }
    if want("sections") {
        let from = prefixed.replace(' ', "_");
        let sections: Vec<Value> = ex
            .sections
            .iter()
            .map(|s| {
                let mut line = String::new();
                scatter_wikitext::tree::to_html(&s.line, &mut line);
                json!({
                    "toclevel": s.number.split('.').count(),
                    "level": s.level.to_string(),
                    "line": line,
                    "number": s.number,
                    "index": s.index.to_string(),
                    "fromtitle": from,
                    "byteoffset": s.span.start,
                    "anchor": s.anchor,
                    "linkAnchor": s.anchor,
                })
            })
            .collect();
        out.insert("sections".into(), Value::Array(sections));
        out.insert(
            "showtoc".into(),
            if v2 {
                json!(render.rendered.html.contains("id=\"toc\""))
            } else if render.rendered.html.contains("id=\"toc\"") {
                json!("")
            } else {
                Value::Null
            },
        );
        if out.get("showtoc") == Some(&Value::Null) {
            out.remove("showtoc");
        }
    }
    if want("parsewarnings") {
        out.insert("parsewarnings".into(), json!([]));
    }
    if want("displaytitle") {
        out.insert(
            "displaytitle".into(),
            json!(display_title(&site, &src.title)),
        );
    }
    if want("iwlinks") {
        out.insert("iwlinks".into(), json!([]));
    }
    if want("properties") {
        out.insert("properties".into(), if v2 { json!({}) } else { json!([]) });
    }
    if want("wikitext") {
        out.insert(
            "wikitext".into(),
            if v2 {
                json!(src.text)
            } else {
                json!({"*": src.text})
            },
        );
    }
    let _ = flag;
    if let Some(p) = &src.page {
        ctx.cache_tags.push(format!("p:{}", p.page_id));
    }
    Ok(ApiResponse::ok(json!({"parse": Value::Object(out)})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_titles() {
        let site = Site {
            config: std::sync::Arc::new(scatter_wikitext::Config::new(
                triplespace_projections::pages::config_source(Some("Librarybase")),
            )),
            name: "Librarybase".into(),
            lang: "en".into(),
            registry: scatter_providers::Registry::default_registry(),
        };
        assert_eq!(
            display_title(&site, &Title::new(4, "Scope")),
            "<span class=\"mw-page-title-namespace\">Librarybase</span><span class=\"mw-page-title-separator\">:</span><span class=\"mw-page-title-main\">Scope</span>"
        );
        assert_eq!(
            display_title(&site, &Title::new(0, "A&B")),
            "<span class=\"mw-page-title-main\">A&amp;B</span>"
        );
    }
}
