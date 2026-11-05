//! `prop=revisions` (compat §3.2; 0012 §4; 18 §2.3): an entity page's local revisions,
//! in MediaWiki's shape, with the entity's state at any of them as `content`.
//!
//! The page set is `titles=` (entity titles in any form the Action API accepts),
//! `pageids=` or `revids=`. The rows are the local graph's change-set records for the
//! entity — the one-target feed of [`triplespace_projections::feed`] narrowed to
//! `source=local`, `kind=edit` — so a mirror's syncs do not appear here (they are
//! `rckind=sync` rows of the feed and the History tab's). `rvprop` takes MediaWiki's
//! `ids`, `flags`, `timestamp`, `user`, `userid`, `size`, `comment`, `parsedcomment`,
//! `tags`, `content`, `contentmodel` and the Triplespace additions `source`, `job`,
//! `operator` and `record` (the revision's `{partition, offset}`, which the site's diff
//! page resolves a revision ID through); `rvslots=main` (or `*`) puts the content under `slots.main` as
//! MediaWiki does since 1.32, otherwise under `*`/`content` by `formatversion`.
//! `rvlimit`, `rvdir`, `rvstart`, `rvend`, `rvstartid`, `rvendid` and `rvcontinue`
//! page as `list=recentchanges` does. `rvprop=content` at a revision is the local
//! graph's state after it, read through [`triplespace_projections::revisions`]: the
//! fold for now, one `view.entity_revision` row once 0083's tier-1 state lands. An
//! erased part sets `texthidden`, `commenthidden` or `userhidden`, and `erased`.
//!
//! Several pages in one request are served in turn; `rvlimit` then applies per page,
//! as MediaWiki applies it only to a single page, and continuation names the page.

use scatter_wikibase_model::entity::PageInfo;
use serde_json::{Map, Value, json};
use triplespace_projections::feed::{self, Cursor, Direction, Feed, Flag, TargetSet, target_kind};
use triplespace_projections::revisions;

use crate::activity::{comment_text, mw_row, viewer, visibility};
use crate::api::Ctx;
use crate::entity_json::{add_derived_terms, hasher_for, iso8601};
use crate::modules::pageset::{self, PageAsk};
use crate::response::{ApiError, ApiResponse};

/// Whether a content model is a thread's or a talk page's, which `action=parse` does not
/// render.
pub(crate) fn is_discussion(model: &str) -> bool {
    matches!(model, "triplespace-thread" | "triplespace-talk")
}

/// The refusal for rendering a discussion page.
pub(crate) fn discussion_content() -> ApiError {
    ApiError::new(
        "ts-not-supported",
        "Threads and talk pages are not rendered by the Action API; read GET /thread/{id} or GET /page/{id}/threads.",
    )
}

/// Whether a page is a talk page, whose revisions are its threads' (composite,
/// content-models.toml `source = "composite"`; 16 §6.1).
pub(crate) fn is_talk(page: &PageAsk) -> bool {
    page.content_model() == "triplespace-talk"
}

const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 500;

/// The content model of an entity page, by its namespace (namespaces.toml).
pub(crate) fn content_model(ns: i64) -> &'static str {
    match ns {
        122 => "wikibase-property",
        146 => "wikibase-lexeme",
        210 => "triplespace-domain",
        212 => "triplespace-keyword",
        216 => "triplespace-notation",
        _ => "wikibase-item",
    }
}

/// The feed of one page's local revisions, from the request's `rv*` parameters.
fn feed_for(ctx: &Ctx, page: &PageAsk) -> Result<Feed, ApiError> {
    let mut f = Feed::everything(DEFAULT_LIMIT);
    f.set = match &page.id {
        Some(id) => TargetSet::Target {
            kind: target_kind::ENTITY,
            id: id.as_str().to_string(),
        },
        // A talk page's revisions are those of the threads attached to it (16 §6.1).
        None if is_talk(page) => TargetSet::Talk(page.page_id),
        None => TargetSet::Target {
            kind: target_kind::PAGE,
            id: page.page_id.to_string(),
        },
    };
    f.filters.kinds = vec!["edit".into(), "erased".into()];
    f.filters.sources = vec![if page.doc.is_some() { "pages" } else { "local" }.into()];
    f.direction = match ctx.params.get("rvdir").unwrap_or("older") {
        "older" => Direction::Older,
        "newer" => Direction::Newer,
        other => return Err(ApiError::bad_value("rvdir", other)),
    };
    let time = |name: &str| -> Result<Option<u64>, ApiError> {
        match ctx.params.get(name) {
            None => Ok(None),
            Some(v) => feed::parse_timestamp(v)
                .map(Some)
                .ok_or_else(|| ApiError::bad_value(name, v)),
        }
    };
    let (start, end) = (time("rvstart")?, time("rvend")?);
    match f.direction {
        Direction::Older => {
            f.filters.until = start;
            f.filters.since = end;
        }
        Direction::Newer => {
            f.filters.since = start;
            f.filters.until = end;
        }
    }
    f.limit = match ctx.params.get("rvlimit") {
        None => DEFAULT_LIMIT,
        Some("max") => MAX_LIMIT,
        Some(v) => v
            .parse::<usize>()
            .map_err(|_| ApiError::bad_value("rvlimit", v))?
            .clamp(1, MAX_LIMIT),
    };
    if let Some(c) = ctx.params.get("rvcontinue") {
        f.cursor = Some(Cursor::parse(c).ok_or_else(|| ApiError::bad_value("rvcontinue", c))?);
    }
    if let Some(u) = ctx.params.get("rvuser") {
        f.filters.actor = Some(u.to_string());
    }
    if ctx.params.list("rvshow").iter().any(|s| s == "minor") {
        f.filters.minor = Flag::Only;
    }
    Ok(f)
}

/// The revision before `row` among the page's revisions, whoever made it and whenever:
/// MediaWiki's `parentid`, 0 for the first.
async fn parent_of(ctx: &Ctx, page: &PageAsk, row: &feed::Row) -> Result<u64, ApiError> {
    let mut f = Feed::everything(1);
    f.set = feed_for(ctx, page)?.set;
    f.filters.kinds = vec!["edit".into(), "erased".into()];
    f.filters.sources = vec![if page.doc.is_some() { "pages" } else { "local" }.into()];
    f.direction = Direction::Older;
    f.cursor = Some(row.cursor());
    let p = feed::query(ctx.db(), &ctx.tenant.slug, &f, &viewer(ctx).await?)
        .await
        .map_err(ApiError::internal)?;
    Ok(p.rows.first().and_then(|r| r.revid).unwrap_or(0))
}

/// The parent of each row, in order: the next older row of the list where it has one,
/// one query at the list's older end otherwise.
async fn parents(
    ctx: &Ctx,
    page: &PageAsk,
    rows: &[feed::Row],
    direction: Direction,
) -> Result<Vec<u64>, ApiError> {
    let mut out = vec![0; rows.len()];
    let n = rows.len();
    for i in 0..n {
        let older = match direction {
            Direction::Older => rows.get(i + 1),
            Direction::Newer => i.checked_sub(1).and_then(|j| rows.get(j)),
        };
        out[i] = match older {
            Some(r) => r.revid.unwrap_or(0),
            None => parent_of(ctx, page, &rows[i]).await?,
        };
    }
    Ok(out)
}

/// The content of a revision: a document page's text after it, a thread as it stood
/// after it (the `GET /thread/{id}` body as this viewer may see it, 0041 §5), a talk page
/// as wikitext with its threads as they stood after it ([`crate::talk_text`]), or the
/// entity's local state after it in Wikibase's JSON.
pub(crate) async fn content_at(
    ctx: &Ctx,
    page: &PageAsk,
    offset: u64,
    revid: Option<u64>,
) -> Result<Option<String>, ApiError> {
    if is_talk(page) {
        let Some(r) = revid else {
            return Ok(None);
        };
        return crate::talk_text::text_at(
            ctx.db(),
            &ctx.tenant.slug,
            &ctx.caller.effective,
            page.page_id,
            Some(r),
        )
        .await
        .map(Some);
    }
    if page.content_model() == "triplespace-thread" {
        let Some(r) = revid else {
            return Ok(None);
        };
        let body = crate::thread_view::body(
            ctx.db(),
            &ctx.tenant.slug,
            &ctx.caller.effective,
            page.page_id,
            Some(r),
            scatter_actors::Timestamp(crate::edit::now_micros()),
        )
        .await
        .map_err(ApiError::internal)?;
        return Ok(body.map(|b| serde_json::to_string_pretty(&b).unwrap_or_default()));
    }
    let Some(id) = &page.id else {
        return match revid {
            Some(r) => crate::pages::text_at(ctx.db(), &ctx.tenant.slug, page.page_id, r).await,
            None => Ok(None),
        };
    };
    let Some((rev, state)) = revisions::local_state_at(ctx.db(), &ctx.tenant.slug, id, offset)
        .await
        .map_err(ApiError::internal)?
    else {
        return Ok(None);
    };
    let Some(mut entity) = state.entity else {
        return Ok(Some(String::new()));
    };
    add_derived_terms(&mut entity);
    let info = PageInfo {
        pageid: Some(page.page_id),
        ns: Some(page.ns),
        title: Some(page.title.clone()),
        lastrevid: rev.revid(),
        modified: Some(iso8601(rev.header.appended_at)),
    };
    Ok(Some(entity.to_wikibase_json(
        &hasher_for(id, ctx.registry().await?),
        Some(&info),
    )))
}

/// `prop=revisions`.
#[allow(clippy::too_many_lines)]
pub async fn run(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let site = crate::pages::Site::load(ctx).await?;
    let mut set = pageset::resolve(ctx, &site).await?;
    set.report(query);
    let pages = std::mem::take(&mut set.pages);
    revisions_of(ctx, query, body, response, &pages, "revisions").await
}

/// `prop=deletedrevisions` (0023 §8; 18 §2.4): the revisions of the deleted pages that
/// had the titles named, for members of their deletion group, with `drv*` parameters as
/// `prop=revisions` takes `rv*`. For anyone else the page is a title with no deleted
/// revisions. By title only: a deleted page has no live page ID to ask by.
pub async fn deleted(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("deletedhistory")?;
    let site = crate::pages::Site::load(ctx).await?;
    let mut pages = Vec::new();
    for t in ctx.params.list("titles") {
        if crate::pages::names_entity(&site, &t) {
            continue;
        }
        let Some(title) = crate::pages::resolve_title(&site, &t) else {
            continue;
        };
        for page in crate::pages::deleted_by_title(ctx, title.namespace, &title.text).await? {
            pages.push(pageset::document_ask(&site, page));
        }
    }
    let as_rv = ctx.params.with_prefix_as("drv", "rv");
    let ctx = Ctx {
        params: as_rv,
        client: ctx.app.pool().get().await.map_err(ApiError::internal)?,
        app: ctx.app.clone(),
        tenant: ctx.tenant.clone(),
        origin: ctx.origin.clone(),
        caller: ctx.caller.clone(),
        formatversion: ctx.formatversion,
        set_cookie: None,
        warnings: Vec::new(),
        cache_class: crate::http_cache::Class::Private,
        cache_tags: Vec::new(),
        accept_language: ctx.accept_language.clone(),
    };
    revisions_of(&ctx, query, body, response, &pages, "deletedrevisions").await
}

async fn revisions_of(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
    pages: &[PageAsk],
    field: &str,
) -> Result<(), ApiError> {
    let mut props = ctx.params.list("rvprop");
    if props.is_empty() {
        props = ["ids", "timestamp", "flags", "comment", "user"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    let want = |p: &str| props.iter().any(|x| x == p);
    let slots = ctx.params.list("rvslots");
    let in_slots = slots.iter().any(|s| s == "main" || s == "*");
    if want("content") && !in_slots {
        response.warn(
            "revisions",
            "Because \"rvslots\" was not specified, a legacy format has been used for the output. This format is deprecated, and in the future the new format will always be used.",
        );
    }
    let v2 = ctx.formatversion == 2;
    let mut out: Vec<Value> = query
        .remove("pages")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    let viewer = viewer(ctx).await?;
    for page in pages {
        let mut feed = feed_for(ctx, page)?;
        if let Some(r) = page.only_revid {
            // `revids=`: the one revision, whatever the window says.
            feed.filters.revid = Some(r);
            feed.filters.since = None;
            feed.filters.until = None;
            feed.cursor = None;
            feed.limit = 1;
        }
        let p = feed::query(ctx.db(), &ctx.tenant.slug, &feed, &viewer)
            .await
            .map_err(ApiError::internal)?;
        let mut revs = Vec::with_capacity(p.rows.len());
        let parent_ids = if want("ids") {
            parents(ctx, page, &p.rows, feed.direction).await?
        } else {
            vec![0; p.rows.len()]
        };
        for (row, parent) in p.rows.iter().zip(parent_ids) {
            let mut r = revision_row(ctx, page, row, &props, in_slots, v2);
            if want("ids") {
                r["parentid"] = json!(parent);
            }
            revs.push(with_content(ctx, page, r, &props, in_slots, v2).await?);
        }
        let mut pg = page_json(page);
        pg[field] = Value::Array(revs);
        out.push(pg);
        if let Some(next) = p.next {
            body.insert(
                "continue".into(),
                json!({"rvcontinue": next.token(), "continue": "||"}),
            );
            break;
        }
    }
    query.insert("pages".into(), pages_value(out, v2));
    Ok(())
}

/// `pages` as `formatversion` shapes it: an array, or an object keyed by page ID.
fn pages_value(pages: Vec<Value>, v2: bool) -> Value {
    if v2 {
        return Value::Array(pages);
    }
    let mut m = Map::new();
    let mut negative = -1i64;
    for p in pages {
        let key = if let Some(id) = p.get("pageid").and_then(Value::as_u64) {
            id.to_string()
        } else {
            let k = negative.to_string();
            negative -= 1;
            k
        };
        m.insert(key, p);
    }
    Value::Object(m)
}

/// The `contentformat` of a model, from the content-model registry.
pub(crate) fn format_of(model: &str) -> String {
    scatter_pages::ContentModels::embedded()
        .get(model)
        .and_then(|m| m.format.clone())
        .unwrap_or_else(|| "application/json".into())
}

fn page_json(page: &PageAsk) -> Value {
    json!({"pageid": page.page_id, "ns": page.ns, "title": page.title})
}

/// A revision row, from the activity row: MediaWiki's fields.
fn revision_row(
    ctx: &Ctx,
    page: &PageAsk,
    row: &feed::Row,
    props: &[String],
    in_slots: bool,
    v2: bool,
) -> Value {
    let mut m = Map::new();
    let flag = |m: &mut Map<String, Value>, name: &str, on: bool| {
        if v2 {
            m.insert(name.into(), json!(on));
        } else if on {
            m.insert(name.into(), json!(""));
        }
    };
    let want = |p: &str| props.iter().any(|x| x == p);
    if want("ids") {
        m.insert("revid".into(), json!(row.revid.unwrap_or(0)));
        m.insert("parentid".into(), json!(0));
    }
    if want("flags") {
        flag(&mut m, "minor", row.minor);
    }
    if want("user") || want("userid") {
        if row.visibility & visibility::USER != 0 || row.actor.hidden {
            flag(&mut m, "userhidden", true);
        } else {
            if want("user") {
                m.insert(
                    "user".into(),
                    json!(crate::activity::user_name(row).unwrap_or_else(|| "(erased)".into())),
                );
            }
            if want("userid") {
                m.insert(
                    "userid".into(),
                    json!(crate::activity::user_id(row.actor.key.as_deref())),
                );
            }
        }
    }
    if want("timestamp") {
        m.insert("timestamp".into(), json!(iso8601(row.time)));
    }
    if want("size") {
        m.insert("size".into(), json!(row.size.unwrap_or(0)));
    }
    if want("comment") || want("parsedcomment") {
        if row.visibility & visibility::COMMENT != 0 {
            flag(&mut m, "commenthidden", true);
        } else {
            let text = comment_text(row);
            if want("comment") {
                m.insert("comment".into(), json!(text));
            }
            if want("parsedcomment") {
                m.insert("parsedcomment".into(), json!(crate::html_escape(&text)));
            }
        }
    }
    if want("tags") {
        m.insert("tags".into(), json!(row.tags));
    }
    if want("contentmodel") && !in_slots {
        let model = page.content_model();
        m.insert("contentformat".into(), json!(format_of(&model)));
        m.insert("contentmodel".into(), json!(model));
    }
    if row.kind == "erased" {
        flag(&mut m, "texthidden", true);
        flag(&mut m, "erased", true);
    }
    // A `record` ACL that hides the text (0023 §5): the flag in place of the content, for
    // everyone; members of the group read it through `prop=deletedrevisions`.
    if row.visibility & visibility::CONTENT != 0 {
        flag(&mut m, "texthidden", true);
    }
    if row.visibility & visibility::SUPPRESSED != 0 {
        flag(&mut m, "suppressed", true);
    }
    if want("record") {
        m.insert(
            "record".into(),
            json!({"partition": row.partition, "offset": row.offset}),
        );
    }
    // Triplespace's additions, as on `list=recentchanges`.
    let extra = mw_row(ctx, row, props, "revisions");
    for key in ["source", "job", "operator"] {
        if let Some(v) = extra.get(key) {
            m.insert(key.into(), v.clone());
        }
    }
    m.insert("_offset".into(), json!(row.offset));
    m.insert("_revid".into(), json!(row.revid));
    Value::Object(m)
}

/// Adds the content to a revision row when asked for, and drops the private `_offset`.
async fn with_content(
    ctx: &Ctx,
    page: &PageAsk,
    mut row: Value,
    props: &[String],
    in_slots: bool,
    v2: bool,
) -> Result<Value, ApiError> {
    let offset = row["_offset"].as_u64().unwrap_or(0);
    let revid = row["_revid"].as_u64();
    let m = row.as_object_mut().expect("an object");
    m.remove("_offset");
    m.remove("_revid");
    let want = |p: &str| props.iter().any(|x| x == p);
    if !want("content") || m.contains_key("texthidden") {
        return Ok(row);
    }
    let Some(text) = content_at(ctx, page, offset, revid).await? else {
        if page.doc.is_some() {
            let v2 = ctx.formatversion == 2;
            m.insert(
                "texthidden".into(),
                if v2 { json!(true) } else { json!("") },
            );
        }
        return Ok(row);
    };
    let model = page.content_model();
    let format = format_of(&model);
    if in_slots {
        let mut slot = Map::new();
        slot.insert("contentmodel".into(), json!(model));
        slot.insert("contentformat".into(), json!(format));
        if v2 {
            slot.insert("content".into(), json!(text));
        } else {
            slot.insert("*".into(), json!(text));
        }
        m.insert("slots".into(), json!({"main": Value::Object(slot)}));
    } else {
        m.insert("contentmodel".into(), json!(model));
        m.insert("contentformat".into(), json!(format));
        if v2 {
            m.insert("content".into(), json!(text));
        } else {
            m.insert("*".into(), json!(text));
        }
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_models_follow_the_namespace() {
        assert_eq!(content_model(120), "wikibase-item");
        assert_eq!(content_model(122), "wikibase-property");
        assert_eq!(content_model(210), "triplespace-domain");
    }

    #[test]
    fn pages_are_keyed_by_id_in_formatversion_1() {
        let v = pages_value(
            vec![
                json!({"pageid": 7, "title": "Item:Q7"}),
                json!({"title": "Item:Q404", "missing": true}),
            ],
            false,
        );
        assert_eq!(v["7"]["title"], "Item:Q7");
        assert_eq!(v["-1"]["missing"], true);
        let v = pages_value(vec![json!({"pageid": 7})], true);
        assert!(v.is_array());
    }
}
