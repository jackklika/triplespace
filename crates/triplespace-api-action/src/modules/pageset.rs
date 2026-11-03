//! The page set of `action=query` (compat §3; 18 §2.3): `titles=`, `pageids=` or
//! `revids=`, resolved to entity pages and document pages alike, as one `pageid` space
//! covers both (03 §4.5).
//!
//! A title that parses as an entity ID is an entity page, as before document pages; any
//! other is resolved into the page namespaces with the tenant's names. A title written
//! in another form than its canonical one is reported under `normalized`; with
//! `redirects`, a document redirect is followed one hop and reported under `redirects`
//! (10 §2.4). Missing and invalid titles are reported as MediaWiki reports them.

use scatter_wikibase_model::id::EntityId;
use serde_json::{Map, Value, json};
use triplespace_projections::read;

use crate::api::Ctx;
use crate::entity_json::title_of;
use crate::pages::{self, Page, Site};
use crate::response::ApiError;

/// One page of the request.
#[derive(Debug, Clone)]
pub struct PageAsk {
    /// The entity, for an entity page.
    pub id: Option<EntityId>,
    /// The page's row, for a document page.
    pub doc: Option<Page>,
    /// The page ID.
    pub page_id: u64,
    /// The namespace.
    pub ns: i64,
    /// The title, prefixed.
    pub title: String,
    /// The revision `revids=` named for it.
    pub only_revid: Option<u64>,
}

impl PageAsk {
    fn document(site: &Site, page: Page, only_revid: Option<u64>) -> Self {
        Self {
            id: None,
            page_id: page.page_id,
            ns: i64::from(page.ns),
            title: site.prefixed(page.ns, &page.title),
            doc: Some(page),
            only_revid,
        }
    }

    /// The page's content model.
    #[must_use]
    pub fn content_model(&self) -> String {
        match &self.doc {
            Some(d) => d.model.clone(),
            None => crate::modules::revisions::content_model(self.ns).to_string(),
        }
    }
}

/// What the page set resolved.
#[derive(Debug, Default)]
pub struct PageSet {
    /// The pages found.
    pub pages: Vec<PageAsk>,
    /// Entries for the pages not found, as `pages` lists them.
    pub missing: Vec<Value>,
    /// `{from, to}` for titles given in another form.
    pub normalized: Vec<Value>,
    /// `{from, to}` for redirects followed.
    pub redirects: Vec<Value>,
}

impl PageSet {
    /// Puts `normalized`, `redirects` and the missing pages into the query object.
    pub fn report(&mut self, query: &mut Map<String, Value>) {
        if !self.normalized.is_empty() {
            query.insert(
                "normalized".into(),
                Value::Array(std::mem::take(&mut self.normalized)),
            );
        }
        if !self.redirects.is_empty() {
            query.insert(
                "redirects".into(),
                Value::Array(std::mem::take(&mut self.redirects)),
            );
        }
        if !self.missing.is_empty() {
            query.insert(
                "pages".into(),
                Value::Array(std::mem::take(&mut self.missing)),
            );
        }
    }
}

fn flag(ctx: &Ctx) -> Value {
    if ctx.formatversion == 2 {
        json!(true)
    } else {
        json!("")
    }
}

/// Resolves the request's page set.
#[allow(clippy::too_many_lines)]
pub async fn resolve(ctx: &Ctx, site: &Site) -> Result<PageSet, ApiError> {
    let titles = ctx.params.list("titles");
    let pageids = ctx.params.list("pageids");
    let revids = ctx.params.list("revids");
    if titles.is_empty() && pageids.is_empty() && revids.is_empty() {
        return Err(ApiError::new(
            "missingparam",
            "One of the parameters \"titles\", \"pageids\" and \"revids\" is required.",
        ));
    }
    let tenant = ctx.tenant.slug.as_str();
    let hidden = pages::hidden(ctx).await?;
    let follow = ctx.params.flag("redirects");
    let mut set = PageSet::default();
    let mut entities: Vec<(EntityId, Option<u64>, Option<String>)> = Vec::new();
    let mut docs: Vec<(Page, Option<u64>)> = Vec::new();

    for t in &titles {
        if let Some(id) = pages::entity_of_title(site, t) {
            entities.push((id, None, Some(t.clone())));
            continue;
        }
        let Some(title) = pages::resolve_title(site, t) else {
            set.missing.push(json!({"title": t, "invalid": flag(ctx),
                "invalidreason": "The requested page title is empty or contains only the name of a namespace."}));
            continue;
        };
        let prefixed = site.prefixed(title.namespace, &title.text);
        if prefixed != *t {
            set.normalized.push(json!({"from": t, "to": prefixed}));
        }
        match pages::by_title(ctx.db(), tenant, &hidden, title.namespace, &title.text).await? {
            Some(p) => docs.push((p, None)),
            None => set
                .missing
                .push(json!({"ns": title.namespace, "title": prefixed, "missing": flag(ctx)})),
        }
    }
    for p in &pageids {
        let n: i64 = p.parse().map_err(|_| ApiError::bad_value("pageids", p))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT id FROM view.entity WHERE tenant IN ($1, '') AND page_id = $2 AND NOT deleted
                 ORDER BY (tenant = $1) DESC LIMIT 1",
                &[&ctx.tenant.slug, &n],
            )
            .await?;
        if let Some(id) = row.and_then(|r| EntityId::parse(&r.get::<_, String>(0)).ok()) {
            // An entity the caller may not read is absent; its page ID names nothing
            // (09 §5.3).
            if crate::pages::entity_readable(ctx, id.as_str(), None).await? {
                entities.push((id, None, None));
                continue;
            }
            set.missing.push(json!({"pageid": n, "missing": flag(ctx)}));
            continue;
        }
        match pages::by_id(ctx.db(), tenant, &hidden, u64::try_from(n).unwrap_or(0)).await? {
            Some(page) => docs.push((page, None)),
            None => set.missing.push(json!({"pageid": n, "missing": flag(ctx)})),
        }
    }
    for r in &revids {
        let n: u64 = r.parse().map_err(|_| ApiError::bad_value("revids", r))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT target_kind, target_id FROM view.activity
                 WHERE tenant = $1 AND revid = $2 AND source IN ('local', 'pages') AND target_kind IN (1, 2)",
                &[&ctx.tenant.slug, &i64::try_from(n).unwrap_or(i64::MAX)],
            )
            .await?;
        match row.map(|r| (r.get::<_, Option<i16>>(0), r.get::<_, Option<String>>(1))) {
            Some((Some(1), Some(id))) => match EntityId::parse(&id) {
                Ok(id) => entities.push((id, Some(n), None)),
                Err(_) => set.missing.push(json!({"revid": n, "missing": flag(ctx)})),
            },
            Some((Some(2), Some(id))) => {
                match pages::by_id(ctx.db(), tenant, &hidden, id.parse().unwrap_or(0)).await? {
                    Some(page) => docs.push((page, Some(n))),
                    None => set.missing.push(json!({"revid": n, "missing": flag(ctx)})),
                }
            }
            _ => set.missing.push(json!({"revid": n, "missing": flag(ctx)})),
        }
    }

    for (page, only_revid) in docs {
        let page = if follow && page.is_redirect && only_revid.is_none() {
            let target = ctx
                .db()
                .query_opt(
                    "SELECT target_page_id, target_ns, target_title FROM view.redirect
                     WHERE tenant = $1 AND page_id = $2",
                    &[&tenant, &i64::try_from(page.page_id).unwrap_or(i64::MAX)],
                )
                .await?;
            match target {
                Some(r) => {
                    let from = site.prefixed(page.ns, &page.title);
                    let to_ns: Option<i32> = r.get(1);
                    let to_title: String = r.get(2);
                    set.redirects.push(
                        json!({"from": from, "to": site.prefixed(to_ns.unwrap_or(0), &to_title)}),
                    );
                    let Some(t) = r
                        .get::<_, Option<i64>>(0)
                        .and_then(|t| u64::try_from(t).ok())
                    else {
                        set.missing.push(json!({"ns": to_ns, "title": site.prefixed(to_ns.unwrap_or(0), &to_title), "missing": flag(ctx)}));
                        continue;
                    };
                    match pages::by_id(ctx.db(), tenant, &hidden, t).await? {
                        Some(p) => p,
                        None => continue,
                    }
                }
                None => page,
            }
        } else {
            page
        };
        set.pages.push(PageAsk::document(site, page, only_revid));
    }

    for (id, only_revid, title) in entities {
        if let Some(mut ask) = entity_ask(ctx, &id).await? {
            ask.only_revid = only_revid;
            set.pages.push(ask);
        } else {
            let t = title.unwrap_or_else(|| id.as_str().to_string());
            set.missing.push(json!({"title": t, "missing": flag(ctx)}));
        }
    }
    Ok(set)
}

/// An entity's page, or `None` where the entity does not exist or is deleted.
pub async fn entity_ask(ctx: &Ctx, id: &EntityId) -> Result<Option<PageAsk>, ApiError> {
    let current = read::current(ctx.db(), ctx.app.order(), &ctx.tenant.slug, id)
        .await
        .map_err(ApiError::internal)?;
    // Absent under a restriction the caller does not satisfy, and for a deleted one
    // outside its group (09 §5.3, §6.2).
    let graph = current
        .as_ref()
        .and_then(|c| c.contributions.first().map(|g| g.graph.clone()));
    if !crate::pages::entity_readable(ctx, id.as_str(), graph.as_deref()).await? {
        return Ok(None);
    }
    Ok(match current {
        Some(c) => {
            let (ns, title) = title_of(id, &c.resolved.entity.entity_type);
            Some(PageAsk {
                id: Some(id.clone()),
                doc: None,
                page_id: c.page_id,
                ns,
                title,
                only_revid: None,
            })
        }
        _ => None,
    })
}

/// A document page's page.
#[must_use]
pub fn document_ask(site: &Site, page: Page) -> PageAsk {
    PageAsk::document(site, page, None)
}
