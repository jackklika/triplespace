//! A thread's body made ready for display (14 §1.6; 0019 §5, A28), when a reader asks
//! for it (`GET /thread/{id}?html=1`): `html` beside each post's `content`, `author`, the
//! name its `attributedTo` actor shows, and `prefixed`, each attachment's talk page title
//! with its namespace's local name.
//!
//! A markdown post is rendered by `scatter-pages`' markdown, its `[[wiki links]]` resolved
//! by the tenant's title rules and its HTML sanitized to the allow-list of 0008 §8; a
//! wikitext post by the tenant's wikitext pipeline, with expansion, in the context of the
//! thread's own title, so templates render through the stack as an imported talk page's
//! do. Rendered HTML is never stored.
//!
//! Not yet: an entity mention, `[[Item:Q42]]` in markdown, renders its title rather than
//! the entity's label, and links are not marked as missing.

use std::collections::HashMap;

use scatter_actors::Timestamp;
use scatter_actors::evaluate::Effective;
use scatter_threads::{NS_THREAD, WIKITEXT};
use scatter_wikitext::html::url_encode_title;
use scatter_wikitext::{Title, parse_target};
use serde_json::{Value, json};
use tokio_postgres::Client;

use crate::pages::{self, Site};
use crate::response::ApiError;

/// Where a markdown post's wiki link goes: the article path of the title it names, with
/// its fragment; the target as written when it is not a title.
fn href(site: &Site, page: &Title, target: &str) -> String {
    let Some(t) = parse_target(target, &site.config, page) else {
        return format!("/wiki/{}", url_encode_title(target));
    };
    let mut h = format!(
        "/wiki/{}",
        url_encode_title(&t.title.without_fragment().prefixed(&site.config))
    );
    if let Some(f) = t.title.fragment.as_deref().filter(|f| !f.is_empty()) {
        h.push('#');
        h.push_str(&f.replace(' ', "_"));
    }
    h
}

/// Renders one post's text by its media type.
///
/// # Errors
///
/// A failure of the wikitext pipeline's reads.
pub async fn render_post(
    db: &Client,
    tenant: &str,
    site: &Site,
    hidden: &pages::Hidden,
    page: &Title,
    media_type: &str,
    text: &str,
) -> Result<String, ApiError> {
    if media_type == WIKITEXT {
        let r = pages::render(db, tenant, site, hidden, text, page, false).await?;
        return Ok(r.rendered.html);
    }
    Ok(
        scatter_pages::markdown::render(
            text,
            &|t| href(site, page, t),
            &scatter_wikitext::sanitize,
        )
        .html,
    )
}

/// The names actors show to a viewer: the user name, a surrogate's raw value, or the key
/// (0007 §4); `None` for an actor whose name an `actor` ACL hides from them (09 §6.3). An
/// actor no row names is not in the map.
pub(crate) async fn names(
    db: &Client,
    tenant: &str,
    who: &Effective,
    keys: &[String],
) -> Result<HashMap<String, Option<String>>, ApiError> {
    let rows = db
        .query(
            "SELECT DISTINCT ON (actor_key) actor_key, coalesce(name, raw, actor_key)
             FROM view.actor WHERE actor_key = ANY($1) AND tenant IN ($2, '')
             ORDER BY actor_key, (tenant = $2) DESC",
            &[&keys, &tenant],
        )
        .await?;
    let hidden = triplespace_projections::acl_read::hidden_actors(
        db,
        tenant,
        keys,
        |g| who.reads_in(g),
        Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    Ok(rows
        .iter()
        .map(|r| {
            let key: String = r.get(0);
            let name = (!hidden.contains(&key)).then(|| r.get::<_, String>(1));
            (key, name)
        })
        .collect())
}

/// Adds `html` and `author` to every post of a thread body, as far as the viewer may see
/// them, and `prefixed` to every attachment's talk page.
///
/// # Errors
///
/// A database failure.
pub async fn add_html(
    db: &Client,
    tenant: &str,
    who: &Effective,
    body: &mut Value,
) -> Result<(), ApiError> {
    let site = Site::load_for(db, tenant).await?;
    let hidden = triplespace_projections::acl_read::hidden(
        db,
        tenant,
        who,
        Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    let title = body["title"].as_str().unwrap_or_default().to_string();
    let page = Title::new(NS_THREAD, &title);
    for key in ["home", "listings"] {
        let attached = match &mut body[key] {
            Value::Array(a) => a.iter_mut().collect::<Vec<_>>(),
            v @ Value::Object(_) => vec![v],
            _ => Vec::new(),
        };
        for a in attached {
            let tp = &mut a["talk_page"];
            if let (Some(ns), Some(t)) = (tp["ns"].as_i64(), tp["title"].as_str()) {
                let ns = i32::try_from(ns).unwrap_or(0);
                tp["prefixed"] = json!(site.prefixed(ns, t));
            }
        }
    }
    let keys: Vec<String> = body["posts"]
        .as_array()
        .map(|ps| {
            ps.iter()
                .filter_map(|p| p["attributedTo"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let names = names(db, tenant, who, &keys).await?;
    let Some(posts) = body["posts"].as_array_mut() else {
        return Ok(());
    };
    for note in posts {
        if let Some(key) = note["attributedTo"].as_str() {
            // A hidden name is shown as no name; an actor no row names, by key.
            match names.get(key) {
                Some(Some(name)) => note["author"] = json!(name),
                Some(None) => {}
                None => note["author"] = json!(key),
            }
        }
        let Some(text) = note["content"].as_str().map(str::to_string) else {
            continue;
        };
        let media = note["mediaType"]
            .as_str()
            .unwrap_or(scatter_threads::MARKDOWN)
            .to_string();
        let html = render_post(db, tenant, &site, &hidden, &page, &media, &text).await?;
        note["html"] = json!(html);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wiki_links_go_to_the_article_path() {
        let site = crate::pages::tests_site();
        let page = Title::new(NS_THREAD, "2026-10-25/Scope");
        assert_eq!(
            href(&site, &page, "Project:Scope"),
            "/wiki/Librarybase:Scope"
        );
        assert_eq!(
            href(&site, &page, "item:Q42#Sitelinks here"),
            "/wiki/Item:Q42#Sitelinks_here"
        );
        let html = scatter_pages::markdown::render(
            "See [[Project:Scope|the scope]] <script>alert(1)</script>",
            &|t| href(&site, &page, t),
            &scatter_wikitext::sanitize,
        )
        .html;
        assert!(html.contains("href=\"/wiki/Librarybase:Scope\""), "{html}");
        assert!(!html.contains("<script"), "{html}");
    }
}
