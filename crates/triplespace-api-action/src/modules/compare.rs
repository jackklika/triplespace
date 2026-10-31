//! `action=compare` (compat `#mod-compare`; 18 §2.3; 19 §3.8): a line diff between two
//! revisions, as MediaWiki's table formatter writes it.
//!
//! Each side is named by `{from,to}rev`, `{from,to}id` (the page's latest revision) or
//! `{from,to}title`; `torelative=prev|next|cur` names the other side from `from`, and as
//! in MediaWiki `prev` diffs the previous revision against `from`. A first revision's
//! `prev` is the empty text, with MediaWiki's warning, so a creation reads as all
//! additions; the latest revision's `next` is itself. `prop` takes `diff`, `diffsize`,
//! `ids`, `title`, `user`, `comment`, `parsedcomment`, `size`, `timestamp` and `rel`;
//! `difftype` `table` (the default) or `inline`, MediaWiki's one-column layout. The diff is
//! [`scatter_pages::diff`]'s.
//!
//! **A document page** is compared by its source text. **An entity** is compared by the
//! local graph's assertions after each revision, the state `prop=revisions` gives as
//! `content`, written as Wikibase JSON with sorted keys, one value to a line, and without
//! the fields that change with every revision (`lastrevid`, `modified`, `pageid`, `ns`,
//! `title`), so the diff shows what the revision asserted (18 §2.3). Its revisions are the
//! local graph's, as `prop=revisions` lists them; a mirror's syncs are not, and their
//! structured diff is the REST route's (`GET /entity/{id}/changes/…`).
//!
//! **A talk page** is compared by its wikitext as of each side ([`crate::talk_text`]): the
//! threads attached to it now, each as it stood. Its revisions are its threads'; a revision
//! named alone is its thread's, and named with `{from,to}title` (or `id`) of the talk page,
//! the talk page's (16 §6.1; 0019 A29).
//!
//! The revisions are rows of the activity feed, so a revision the viewer may not see is
//! not found, and one whose content is hidden or erased is `missingcontent`. Supplied text
//! (`fromtext`, `fromslots`) is not supported yet.

use scatter_wikibase_model::id::EntityId;
use serde_json::{Map, Value, json};
use triplespace_projections::feed::{self, Direction, Feed, TargetSet, target_kind};

use crate::activity::{comment_text, user_id, viewer, visibility};
use crate::api::Ctx;
use crate::entity_json::iso8601;
use crate::modules::pageset::{self, PageAsk};
use crate::modules::revisions::content_at;
use crate::pages::{self, Site};
use crate::response::{ApiError, ApiResponse};

/// A revision on one side of the comparison: its page, document or entity, and its row.
struct Rev {
    ask: PageAsk,
    row: feed::Row,
}

/// The feed of a page's revisions: a document page's in `pages`, an entity's in `local`.
fn revisions_feed(ask: &PageAsk) -> Feed {
    let mut f = Feed::everything(1);
    f.set = match &ask.id {
        Some(id) => TargetSet::Target {
            kind: target_kind::ENTITY,
            id: id.as_str().to_string(),
        },
        // A talk page's revisions are its threads' (16 §6.1).
        None if crate::modules::revisions::is_talk(ask) => TargetSet::Talk(ask.page_id),
        None => TargetSet::Target {
            kind: target_kind::PAGE,
            id: ask.page_id.to_string(),
        },
    };
    f.filters.kinds = vec!["edit".into()];
    f.filters.sources = vec![if ask.id.is_some() { "local" } else { "pages" }.into()];
    f
}

/// One of a page's revisions: the one with `revid`, the newest where neither is given,
/// or the one before or after `from`.
async fn feed_row(
    ctx: &Ctx,
    ask: &PageAsk,
    revid: Option<u64>,
    after: Option<(&feed::Row, Direction)>,
) -> Result<Option<feed::Row>, ApiError> {
    let mut f = revisions_feed(ask);
    f.filters.revid = revid;
    if let Some((row, direction)) = after {
        f.cursor = Some(row.cursor());
        f.direction = direction;
    }
    let p = feed::query(ctx.db(), &ctx.tenant.slug, &f, &viewer(ctx).await?)
        .await
        .map_err(ApiError::internal)?;
    Ok(p.rows.into_iter().next())
}

/// The revision before or after one, as a side.
async fn neighbour(ctx: &Ctx, rev: &Rev, direction: Direction) -> Result<Option<Rev>, ApiError> {
    Ok(feed_row(ctx, &rev.ask, None, Some((&rev.row, direction)))
        .await?
        .map(|row| Rev {
            ask: rev.ask.clone(),
            row,
        }))
}

fn no_such_revid(r: u64) -> ApiError {
    ApiError::new("nosuchrevid", format!("There is no revision with ID {r}."))
}

fn missing_title() -> ApiError {
    ApiError::new("missingtitle", "The page you specified doesn't exist.")
}

/// An entity's page, or `missingtitle`.
async fn entity(ctx: &Ctx, id: &EntityId) -> Result<PageAsk, ApiError> {
    pageset::entity_ask(ctx, id)
        .await?
        .ok_or_else(missing_title)
}

/// A side as its parameters name it, or `None` when it names none.
#[allow(clippy::too_many_lines)]
async fn side(ctx: &Ctx, site: &Site, prefix: &str) -> Result<Option<Rev>, ApiError> {
    let tenant = ctx.tenant.slug.as_str();
    let hidden = pages::hidden(ctx).await?;
    let p = |name: &str| ctx.params.get(&format!("{prefix}{name}"));
    if p("text").is_some() || p("slots").is_some() {
        return Err(ApiError::new(
            "ts-compare-text",
            "Comparing supplied text is not supported yet; name two revisions.",
        ));
    }
    // A talk page shares its revisions with its threads: `{prefix}title` (or `id`) naming a
    // talk page together with `{prefix}rev` compares the talk page as of that revision, where
    // the revision alone names its thread (16 §6.1; 0019 A29).
    if let (Some(r), Some(talk)) = (p("rev"), talk_side(ctx, site, prefix).await?) {
        let name = format!("{prefix}rev");
        let r: u64 = r.parse().map_err(|_| ApiError::bad_value(&name, r))?;
        let row = feed_row(ctx, &talk, Some(r), None)
            .await?
            .ok_or_else(|| no_such_revid(r))?;
        return Ok(Some(Rev { ask: talk, row }));
    }
    let (ask, revid) = if let Some(r) = p("rev") {
        let name = format!("{prefix}rev");
        let r: u64 = r.parse().map_err(|_| ApiError::bad_value(&name, r))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT target_kind, target_id FROM view.activity
                 WHERE tenant = $1 AND revid = $2 AND source IN ('local', 'pages')
                   AND target_kind IN (1, 2)",
                &[&ctx.tenant.slug, &i64::try_from(r).unwrap_or(i64::MAX)],
            )
            .await?;
        let ask = match row.map(|row| {
            (
                row.get::<_, Option<i16>>(0),
                row.get::<_, Option<String>>(1),
            )
        }) {
            Some((Some(1), Some(id))) => {
                let id = EntityId::parse(&id).map_err(|_| no_such_revid(r))?;
                entity(ctx, &id).await?
            }
            Some((Some(2), Some(id))) => {
                let page = pages::by_id(ctx.db(), tenant, &hidden, id.parse().unwrap_or(0))
                    .await?
                    .ok_or_else(|| no_such_revid(r))?;
                pageset::document_ask(site, page)
            }
            _ => return Err(no_such_revid(r)),
        };
        (ask, Some(r))
    } else if let Some(id) = p("id") {
        let name = format!("{prefix}id");
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value(&name, id))?;
        let row = ctx
            .db()
            .query_opt(
                "SELECT id FROM view.entity WHERE tenant IN ($1, '') AND page_id = $2 AND NOT deleted
                 ORDER BY (tenant = $1) DESC LIMIT 1",
                &[&ctx.tenant.slug, &i64::try_from(n).unwrap_or(i64::MAX)],
            )
            .await?;
        let ask = match row.and_then(|r| EntityId::parse(&r.get::<_, String>(0)).ok()) {
            Some(id) => entity(ctx, &id).await?,
            None => pageset::document_ask(
                site,
                pages::by_id(ctx.db(), tenant, &hidden, n)
                    .await?
                    .ok_or_else(|| {
                        ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
                    })?,
            ),
        };
        (ask, None)
    } else if let Some(t) = p("title") {
        let ask = if let Some(id) = pages::entity_of_title(site, t) {
            entity(ctx, &id).await?
        } else {
            let title = pages::resolve_title(site, t)
                .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{t}\".")))?;
            pageset::document_ask(
                site,
                pages::by_title(ctx.db(), tenant, &hidden, title.namespace, &title.text)
                    .await?
                    .ok_or_else(missing_title)?,
            )
        };
        (ask, None)
    } else {
        return Ok(None);
    };
    let row = feed_row(ctx, &ask, revid, None)
        .await?
        .ok_or_else(|| match revid {
            Some(r) => no_such_revid(r),
            // An entity with no local revision: its states are a mirror's.
            None => ApiError::new(
                "ts-no-local-revision",
                format!("{} has no local revision to compare.", ask.title),
            ),
        })?;
    Ok(Some(Rev { ask, row }))
}

/// The talk page `{prefix}title` or `{prefix}id` names, if it names one.
async fn talk_side(ctx: &Ctx, site: &Site, prefix: &str) -> Result<Option<PageAsk>, ApiError> {
    let tenant = ctx.tenant.slug.as_str();
    let hidden = pages::hidden(ctx).await?;
    let page = if let Some(t) = ctx.params.get(&format!("{prefix}title")) {
        match pages::resolve_title(site, t) {
            Some(title) => {
                pages::by_title(ctx.db(), tenant, &hidden, title.namespace, &title.text).await?
            }
            None => None,
        }
    } else if let Some(id) = ctx.params.get(&format!("{prefix}id")) {
        match id.parse::<u64>() {
            Ok(n) => pages::by_id(ctx.db(), tenant, &hidden, n).await?,
            Err(_) => None,
        }
    } else {
        None
    };
    Ok(page
        .map(|p| pageset::document_ask(site, p))
        .filter(crate::modules::revisions::is_talk))
}

/// A JSON value with every object's keys sorted.
fn sorted(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<(String, Value)> = m.into_iter().collect();
            keys.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(keys.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// An entity's JSON as compared: keys sorted, one value to a line, and without the fields
/// that change with every revision.
fn comparable_json(json: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<Value>(json) else {
        return json.to_string();
    };
    if let Some(m) = v.as_object_mut() {
        for k in ["lastrevid", "modified", "pageid", "ns", "title"] {
            m.remove(k);
        }
    }
    serde_json::to_string_pretty(&sorted(v)).unwrap_or_else(|_| json.to_string())
}

/// The content after a revision, as compared, or `missingcontent`.
async fn text_of(ctx: &Ctx, rev: &Rev) -> Result<String, ApiError> {
    let revid = rev.row.revid.unwrap_or(0);
    if rev.row.visibility & visibility::CONTENT != 0 {
        return Err(missing_content(revid));
    }
    let content = content_at(ctx, &rev.ask, rev.row.offset, rev.row.revid)
        .await?
        .ok_or_else(|| missing_content(revid))?;
    Ok(if rev.ask.id.is_some() && !content.is_empty() {
        comparable_json(&content)
    } else {
        content
    })
}

fn missing_content(revid: u64) -> ApiError {
    ApiError::new(
        "missingcontent",
        format!("The content for revision {revid} is not available."),
    )
}

/// The fields of one side that `prop` asks for.
fn side_fields(out: &mut Map<String, Value>, prefix: &str, rev: &Rev, props: &[String], v2: bool) {
    let want = |p: &str| props.iter().any(|x| x == p);
    let key = |name: &str| format!("{prefix}{name}");
    let flag = |out: &mut Map<String, Value>, name: String| {
        out.insert(name, if v2 { json!(true) } else { json!("") });
    };
    let row = &rev.row;
    if want("ids") {
        out.insert(key("id"), json!(rev.ask.page_id));
        out.insert(key("revid"), json!(row.revid.unwrap_or(0)));
    }
    if want("title") {
        out.insert(key("ns"), json!(rev.ask.ns));
        out.insert(key("title"), json!(rev.ask.title));
    }
    if want("size") {
        out.insert(key("size"), json!(row.size.unwrap_or(0)));
    }
    if want("timestamp") {
        out.insert(key("timestamp"), json!(iso8601(row.time)));
    }
    if want("user") {
        if row.visibility & visibility::USER != 0 || row.actor.hidden {
            flag(out, key("userhidden"));
        } else {
            let name = row
                .actor
                .name
                .clone()
                .or_else(|| row.actor.key.clone())
                .unwrap_or_default();
            out.insert(key("user"), json!(name));
            out.insert(key("userid"), json!(user_id(row.actor.key.as_deref())));
        }
    }
    if want("comment") || want("parsedcomment") {
        if row.visibility & visibility::COMMENT != 0 {
            flag(out, key("commenthidden"));
        } else {
            let text = comment_text(row);
            if want("parsedcomment") {
                out.insert(key("parsedcomment"), json!(escape(&text)));
            }
            if want("comment") {
                out.insert(key("comment"), json!(text));
            }
        }
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `action=compare`.
#[allow(clippy::too_many_lines)]
pub async fn run(ctx: &Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.require("read")?;
    let inline = match ctx.params.get("difftype") {
        None | Some("table") => false,
        Some("inline") => true,
        Some(other) => return Err(ApiError::bad_value("difftype", other)),
    };
    let site = Site::load(ctx).await?;
    let v2 = ctx.formatversion == 2;
    let mut props = ctx.params.list("prop");
    if props.is_empty() {
        props = ["diff", "ids", "title"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    let mut response_warnings: Vec<String> = Vec::new();
    let named_from = side(ctx, &site, "from").await?.ok_or_else(|| {
        ApiError::new(
            "missingparam",
            "One of the parameters \"fromtitle\", \"fromid\", \"fromrev\", \"fromtext\" and \"fromslots\" is required.",
        )
    })?;
    // `from` and `to`; `None` for `from` is the empty text before a page's first revision.
    let (from, to): (Option<Rev>, Rev) = match ctx.params.get("torelative") {
        Some("prev") => {
            let prev = neighbour(ctx, &named_from, Direction::Older).await?;
            if prev.is_none() {
                response_warnings.push("There is no previous revision.".into());
            }
            (prev, named_from)
        }
        Some("next") => {
            let next = neighbour(ctx, &named_from, Direction::Newer).await?;
            if next.is_none() {
                response_warnings.push("There is no next revision.".into());
            }
            let to = next.unwrap_or_else(|| Rev {
                ask: named_from.ask.clone(),
                row: named_from.row.clone(),
            });
            (Some(named_from), to)
        }
        Some("cur") => {
            let row = feed_row(ctx, &named_from.ask, None, None)
                .await?
                .ok_or_else(missing_title)?;
            let to = Rev {
                ask: named_from.ask.clone(),
                row,
            };
            (Some(named_from), to)
        }
        Some(other) => return Err(ApiError::bad_value("torelative", other)),
        None => {
            let to = side(ctx, &site, "to").await?.ok_or_else(|| {
                ApiError::new(
                    "missingparam",
                    "One of the parameters \"totitle\", \"toid\", \"torev\", \"totext\", \"torelative\" and \"toslots\" is required.",
                )
            })?;
            (Some(named_from), to)
        }
    };
    let old = match &from {
        Some(r) => text_of(ctx, r).await?,
        None => String::new(),
    };
    let new = text_of(ctx, &to).await?;
    let hunks = scatter_pages::diff::diff(&old, &new);
    let line = |n: usize| format!("Line {n}:");
    let body = if inline {
        scatter_pages::diff::inline_rows(&hunks, line)
    } else {
        scatter_pages::diff::table_rows(&hunks, line)
    };

    let mut out = Map::new();
    if let Some(f) = &from {
        side_fields(&mut out, "from", f, &props, v2);
    }
    side_fields(&mut out, "to", &to, &props, v2);
    let want = |p: &str| props.iter().any(|x| x == p);
    if want("rel") {
        if let Some(f) = &from
            && let Some(prev) = neighbour(ctx, f, Direction::Older).await?
        {
            out.insert("prev".into(), json!(prev.row.revid.unwrap_or(0)));
        }
        if let Some(next) = neighbour(ctx, &to, Direction::Newer).await? {
            out.insert("next".into(), json!(next.row.revid.unwrap_or(0)));
        }
    }
    if want("diffsize") {
        out.insert("diffsize".into(), json!(body.len()));
    }
    if want("diff") {
        out.insert(if v2 { "body" } else { "*" }.into(), json!(body));
    }
    let mut r = ApiResponse::ok(json!({"compare": Value::Object(out)}));
    for w in response_warnings {
        r.warn("compare", &w);
    }
    Ok(r)
}
