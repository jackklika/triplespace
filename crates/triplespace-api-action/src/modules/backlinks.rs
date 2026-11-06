//! `list=backlinks` (compat `#mod-query+backlinks`; 18 §2.3; 0008 §10): the pages whose
//! latest text links to a title, from the links projection (`view.page_link`), as
//! MediaWiki answers it from `pagelinks`.
//!
//! - `bltitle` or `blpageid` names the target. A title in an entity namespace that names an
//!   entity (`Item:Q42`) is matched by the entity's ID, since a link to an entity is kept
//!   by ID (10 §3.3); every other title as `{ns}:{title}`, existing or not, so a missing
//!   page's backlinks are its "wanted" links.
//! - `blnamespace` (a `|` list) keeps the linking pages in those namespaces;
//!   `blfilterredir` (`all`, `redirects`, `nonredirects`) keeps redirects or the rest;
//!   `bllimit` (default 10, at most 500) and `blcontinue`, the next linking page ID, page
//!   through them in page-ID order, as MediaWiki does.
//! - Pages a confidential restriction hides from the caller are not listed (09 §5.3).
//!
//! Not yet: `blredirect`, which would list the links to each redirect to the title too;
//! it is answered with a warning.

use serde_json::{Map, Value, json};

use crate::api::Ctx;
use crate::pages::Site;
use crate::response::{ApiError, ApiResponse};

const DEFAULT_LIMIT: i64 = 10;
const MAX_LIMIT: i64 = 500;

/// `view.page_link.target_kind` of a link to an entity, and of one to a title.
const ENTITY: i16 = 1;
const TITLE: i16 = 6;

/// `list=backlinks`.
///
/// # Errors
///
/// A bad parameter, a title that cannot be a page's, or the database away.
pub async fn backlinks(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = Site::load(ctx).await?;
    let hidden = crate::pages::hidden(ctx).await?;
    let (kind, key) = target(ctx, &site, &hidden).await?;
    let limit = match ctx.params.get("bllimit") {
        None => DEFAULT_LIMIT,
        Some("max") => MAX_LIMIT,
        Some(v) => v
            .parse::<i64>()
            .map_err(|_| ApiError::bad_value("bllimit", v))?
            .clamp(1, MAX_LIMIT),
    };
    let after: i64 = match ctx.params.get("blcontinue") {
        None => 0,
        Some(v) => v
            .rsplit('|')
            .next()
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| ApiError::bad_value("blcontinue", v))?,
    };
    let namespaces: Option<Vec<i32>> = match ctx.params.get("blnamespace") {
        None | Some("") => None,
        Some(v) => Some(
            v.split('|')
                .map(|n| n.parse().map_err(|_| ApiError::bad_value("blnamespace", n)))
                .collect::<Result<_, _>>()?,
        ),
    };
    let redirects: Option<bool> = match ctx.params.get("blfilterredir").unwrap_or("all") {
        "all" => None,
        "redirects" => Some(true),
        "nonredirects" => Some(false),
        other => return Err(ApiError::bad_value("blfilterredir", other)),
    };
    if ctx.params.get("blredirect").is_some() {
        response.warn(
            "backlinks",
            "blredirect is not supported yet; the links to the title itself are listed.",
        );
    }
    let rows = if hidden.every_page() {
        Vec::new()
    } else {
        let hidden_ns: Vec<i32> = hidden.namespaces.iter().copied().collect();
        ctx.db()
            .query(
                "SELECT DISTINCT p.page_id, p.ns, p.title, p.is_redirect
                   FROM view.page_link l
                   JOIN view.page p ON p.tenant = l.tenant AND p.page_id = l.from_page
                  WHERE l.tenant = $1 AND l.target_kind = $2 AND l.target_id = $3
                    AND NOT p.deleted AND p.latest_offset IS NOT NULL
                    AND p.page_id > $4
                    AND ($5::int[] IS NULL OR p.ns = ANY($5))
                    AND ($6::bool IS NULL OR p.is_redirect = $6)
                    AND NOT (p.ns = ANY($7) OR p.page_id = ANY($8))
                  ORDER BY p.page_id LIMIT $9",
                &[
                    &ctx.tenant.slug,
                    &kind,
                    &key,
                    &after,
                    &namespaces,
                    &redirects,
                    &hidden_ns,
                    &hidden.page_ids(),
                    &(limit + 1),
                ],
            )
            .await?
    };
    let v2 = ctx.formatversion == 2;
    let mut out = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        let id: i64 = r.get(0);
        if i64::try_from(i).unwrap_or(i64::MAX) == limit {
            body.insert(
                "continue".into(),
                json!({"blcontinue": format!("{key}|{id}"), "continue": "-||"}),
            );
            break;
        }
        let ns: i32 = r.get(1);
        let title: String = r.get(2);
        let mut row = json!({"pageid": id, "ns": ns, "title": site.prefixed(ns, &title)});
        if r.get::<_, bool>(3) {
            row["redirect"] = if v2 { json!(true) } else { json!("") };
        }
        out.push(row);
    }
    query.insert("backlinks".into(), Value::Array(out));
    Ok(())
}

/// The `view.page_link` key of the target: an entity's ID, or `{ns}:{title}`.
async fn target(
    ctx: &Ctx,
    site: &Site,
    hidden: &triplespace_projections::acl_read::Hidden,
) -> Result<(i16, String), ApiError> {
    let title = match (ctx.params.get("bltitle"), ctx.params.get("blpageid")) {
        (Some(t), None) => t.to_string(),
        (None, Some(id)) => {
            let n: u64 = id
                .parse()
                .map_err(|_| ApiError::bad_value("blpageid", id))?;
            let page = crate::pages::by_id(ctx.db(), &ctx.tenant.slug, hidden, n)
                .await?
                .ok_or_else(|| {
                    ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
                })?;
            site.prefixed(page.ns, &page.title)
        }
        (Some(_), Some(_)) => {
            return Err(ApiError::new(
                "invalidparammix",
                "The parameters \"bltitle\" and \"blpageid\" can not be used together.",
            ));
        }
        (None, None) => {
            return Err(ApiError::new(
                "missingparam",
                "One of the parameters \"bltitle\" and \"blpageid\" is required.",
            ));
        }
    };
    if let Some(id) = crate::pages::entity_of_title(site, &title) {
        return Ok((ENTITY, id.to_string()));
    }
    let t = crate::pages::resolve_title(site, &title)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{title}\".")))?;
    Ok((TITLE, format!("{}:{}", t.namespace, t.text)))
}
