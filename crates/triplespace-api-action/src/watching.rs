//! The watch set as the API serves it (0020 §3, §5; 16 §4.3): what a title or page is
//! watched as, the caller's watch set as a feed's target set, the title a watch shows as,
//! and the `watchlist` parameter of the writes, with the auto-watch preferences.
//!
//! The rows are `triplespace-accounts`' ([`triplespace_accounts::watch`]); what a watch is
//! of and what it covers is read from `view` ([`triplespace_projections::watched`]).

use serde_json::Value;
use triplespace_accounts::watch::{self, Watch};
use triplespace_projections::feed::TargetSet;
use triplespace_projections::watched::{self, Expanded, Target};

use crate::api::Ctx;
use crate::pages::{self, Site};
use crate::response::ApiError;

/// The most watches a watch set is read with for a feed: MediaWiki's `$wgWatchlistExpiryMaxDuration`
/// has no count, but `Special:EditWatchlist` warns past a few thousand.
pub const MAX_WATCHES: i64 = 10_000;

fn internal(e: &dyn std::fmt::Display) -> ApiError {
    ApiError::internal(e.to_string())
}

/// What a title is watched as, or `None` for a title that cannot be watched (invalid, an
/// interwiki link, a special page).
///
/// # Errors
///
/// A database failure.
pub async fn target_of_title(
    ctx: &Ctx,
    site: &Site,
    name: &str,
) -> Result<Option<Target>, ApiError> {
    if let Some(id) = pages::entity_of_title(site, name) {
        return Ok(Some((watch::kind::ENTITY, id.to_string())));
    }
    let Some(t) = pages::resolve_title(site, name) else {
        return Ok(None);
    };
    if t.namespace < 0 {
        return Ok(None);
    }
    // A page the caller may not see is, to them, a title with no page: watched as the
    // title, so that watching it answers exactly as watching a page to come would
    // (09 §5.3), and its changes reach them only once they may read it.
    let hidden = pages::hidden(ctx).await?;
    let subject_ns =
        triplespace_projections::watched::subject_ns(t.namespace).unwrap_or(t.namespace);
    for ns in [t.namespace, subject_ns] {
        if let Some(r) = ctx
            .db()
            .query_opt(
                "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted",
                &[&ctx.tenant.slug, &ns, &t.text],
            )
            .await?
            && hidden.page(ns, u64::try_from(r.get::<_, i64>(0)).unwrap_or(0))
        {
            return Ok(Some((watch::kind::TITLE, format!("{subject_ns}:{}", t.text))));
        }
    }
    Ok(Some(
        watched::of_title(ctx.db(), &ctx.tenant.slug, t.namespace, &t.text)
            .await
            .map_err(|e| internal(&e))?,
    ))
}

/// What a page ID is watched as; `None` for a page that does not exist, or that the caller
/// may not see.
///
/// # Errors
///
/// A database failure.
pub async fn target_of_page(ctx: &Ctx, page_id: u64) -> Result<Option<Target>, ApiError> {
    let hidden = pages::hidden(ctx).await?;
    let id = i64::try_from(page_id).unwrap_or(i64::MAX);
    if let Some(r) = ctx
        .db()
        .query_opt(
            "SELECT ns FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&ctx.tenant.slug, &id],
        )
        .await?
        && hidden.page(r.get(0), page_id)
    {
        return Ok(None);
    }
    watched::of_page(ctx.db(), &ctx.tenant.slug, page_id)
        .await
        .map_err(|e| internal(&e))
}

/// The caller's watch set, and what it covers.
///
/// # Errors
///
/// A database failure.
pub async fn watch_set(ctx: &Ctx, actor: &str) -> Result<(Vec<Watch>, Expanded), ApiError> {
    let watches = watch::list(ctx.db(), actor, MAX_WATCHES, None).await?;
    let targets: Vec<Target> = watches.iter().map(|w| (w.kind, w.id.clone())).collect();
    let x = watched::expand(ctx.db(), &ctx.tenant.slug, &targets)
        .await
        .map_err(|e| internal(&e))?;
    Ok((watches, x))
}

/// A watch set as a feed's target set.
#[must_use]
pub fn target_set(x: &Expanded) -> TargetSet {
    TargetSet::Watched {
        entities: x.entities.clone(),
        pages: x.pages.clone(),
    }
}

/// Whether an activity row is a change the holder has not seen: newer than the watch's
/// `seen` time, or than when it was added, and not their own.
#[must_use]
pub fn unseen(
    watches: &[Watch],
    x: &Expanded,
    kind: i16,
    id: &str,
    time_us: i64,
    actor: Option<&str>,
    holder: &str,
) -> bool {
    if actor == Some(holder) {
        return false;
    }
    x.watch_of(kind, id)
        .and_then(|i| watches.get(i))
        .is_some_and(|w| time_us > w.seen.unwrap_or(w.added))
}

/// The title a watch shows as, `(ns, prefixed title)`; `None` for a target that has no
/// title the holder may see now (a page since deleted, a hidden name).
///
/// # Errors
///
/// A database failure.
pub async fn title_of(
    ctx: &Ctx,
    site: &Site,
    w: &Watch,
) -> Result<Option<(i32, String)>, ApiError> {
    let db = ctx.db();
    let tenant = &ctx.tenant.slug;
    Ok(match w.kind {
        watch::kind::ENTITY => triplespace_projections::acl_read::entity_namespace(&w.id)
            .map(|ns| (ns, site.prefixed(ns, &w.id))),
        watch::kind::PAGE => {
            let Ok(id) = w.id.parse::<i64>() else {
                return Ok(None);
            };
            let hidden = pages::hidden(ctx).await?;
            db.query_opt(
                "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2 AND NOT deleted",
                &[tenant, &id],
            )
            .await?
            .filter(|r| !hidden.page(r.get(0), u64::try_from(id).unwrap_or(0)))
            .map(|r| {
                let ns: i32 = r.get(0);
                (ns, site.prefixed(ns, &r.get::<_, String>(1)))
            })
        }
        watch::kind::ACTOR => {
            let hidden = triplespace_projections::acl_read::hidden_actors(
                db,
                tenant,
                std::slice::from_ref(&w.id),
                |g| ctx.caller.effective.reads_in(g),
                scatter_actors::Timestamp(crate::edit::now_micros()),
            )
            .await
            .map_err(|e| internal(&e))?;
            if hidden.contains(&w.id) {
                None
            } else {
                db.query_opt(
                    "SELECT name FROM view.actor WHERE tenant = $1 AND actor_key = $2 AND name IS NOT NULL",
                    &[tenant, &w.id],
                )
                .await?
                .map(|r| (2, site.prefixed(2, &r.get::<_, String>(0))))
            }
        }
        watch::kind::TITLE => w.id.split_once(':').and_then(|(ns, t)| {
            let ns: i32 = ns.parse().ok()?;
            Some((ns, site.prefixed(ns, t)))
        }),
        _ => None,
    })
}

/// What a write does to the watch set (MediaWiki's `watchlist` parameter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auto {
    /// An edit to an existing page or entity: `autowatch.edit`.
    Edit,
    /// A page or entity created: `autowatch.create`.
    Create,
    /// A thread started: `autowatch.thread`.
    Thread,
    /// A post in a thread: `autowatch.post`.
    Post,
}

impl Auto {
    fn key(self) -> &'static str {
        match self {
            Self::Edit => "autowatch.edit",
            Self::Create => "autowatch.create",
            Self::Thread => "autowatch.thread",
            Self::Post => "autowatch.post",
        }
    }
}

/// A boolean preference of an account's, resolved.
async fn pref(
    db: &tokio_postgres::Client,
    tenant: &str,
    actor: &str,
    key: &str,
) -> Result<bool, ApiError> {
    let r = crate::prefs::resolve(db, tenant, Some(actor), None, None)
        .await
        .map_err(|e| internal(&e))?;
    Ok(r.values
        .get(key)
        .and_then(|p| p.value.as_ref())
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

/// Applies a write's `watchlist` parameter (`watch`, `unwatch`, `preferences`, the default,
/// or `nochange`) and `watchlistexpiry` to the target it wrote, as MediaWiki does: under
/// `preferences`, the auto-watch preference for the kind of write decides, and an
/// auto-watch carries `notify` when `autowatch.notify` is on (16 §4.3; 0021 §2); a target
/// already watched keeps its watch. Returns whether the account watches the target after.
///
/// # Errors
///
/// A bad `watchlist` or `watchlistexpiry`; a database failure.
pub async fn after_write_with(
    db: &tokio_postgres::Client,
    tenant: &str,
    actor: &str,
    target: &Target,
    auto: Auto,
    watchlist: Option<&str>,
    expiry: Option<&str>,
) -> Result<bool, ApiError> {
    let expires = match expiry {
        Some(e) => {
            crate::modules::moderation::expiry(e)?.map(|t| i64::try_from(t.0).unwrap_or(i64::MAX))
        }
        None => None,
    };
    let current = watch::get(db, actor, target.0, &target.1).await?;
    match watchlist.unwrap_or("preferences") {
        "watch" => {
            watch::watch(db, actor, target.0, &target.1, expires, None).await?;
            Ok(true)
        }
        "unwatch" => {
            watch::unwatch(db, actor, target.0, &target.1).await?;
            Ok(false)
        }
        "nochange" => Ok(current.is_some()),
        "preferences" => {
            if current.is_some() {
                return Ok(true);
            }
            if pref(db, tenant, actor, auto.key()).await? {
                let notify = pref(db, tenant, actor, "autowatch.notify").await?;
                watch::watch(db, actor, target.0, &target.1, expires, Some(notify)).await?;
                return Ok(true);
            }
            Ok(false)
        }
        other => Err(ApiError::bad_value("watchlist", other)),
    }
}

/// [`after_write_with`] with the request's `watchlist` and `watchlistexpiry`.
///
/// # Errors
///
/// As [`after_write_with`].
pub async fn after_write(
    ctx: &Ctx,
    actor: &str,
    target: &Target,
    auto: Auto,
) -> Result<bool, ApiError> {
    after_write_with(
        ctx.db(),
        &ctx.tenant.slug,
        actor,
        target,
        auto,
        ctx.params.get("watchlist"),
        ctx.params.get("watchlistexpiry"),
    )
    .await
}

/// When the first change to a watched target that the holder has not seen happened, in
/// microseconds: MediaWiki's `notificationtimestamp`. `None` when every change is seen.
/// Read through the feed, so a change the holder may not see is not one.
///
/// # Errors
///
/// A database failure.
pub async fn first_unseen(ctx: &Ctx, holder: &str, w: &Watch) -> Result<Option<i64>, ApiError> {
    let x = watched::expand(ctx.db(), &ctx.tenant.slug, &[(w.kind, w.id.clone())])
        .await
        .map_err(|e| internal(&e))?;
    if x.entities.is_empty() && x.pages.is_empty() {
        return Ok(None);
    }
    let since = u64::try_from(w.seen.unwrap_or(w.added)).unwrap_or(0) + 1;
    let mut feed = triplespace_projections::feed::Feed::everything(1);
    feed.set = target_set(&x);
    feed.direction = triplespace_projections::feed::Direction::Newer;
    feed.filters.since = Some(since);
    feed.filters.exclude_actor = Some(holder.to_string());
    feed.filters.kinds = triplespace_projections::feed::KINDS
        .iter()
        .map(|k| (*k).to_string())
        .collect();
    let page = triplespace_projections::feed::query(
        ctx.db(),
        &ctx.tenant.slug,
        &feed,
        &crate::activity::viewer(ctx).await?,
    )
    .await
    .map_err(ApiError::internal)?;
    Ok(page
        .rows
        .first()
        .map(|r| i64::try_from(r.time).unwrap_or(i64::MAX)))
}

/// The holder's newest-seen time for a target becomes now: they have looked at it.
///
/// # Errors
///
/// A database failure.
pub async fn seen_now(ctx: &Ctx, holder: &str, target: &Target) -> Result<(), ApiError> {
    let now = i64::try_from(crate::edit::now_micros()).unwrap_or(i64::MAX);
    watch::mark_seen(ctx.db(), holder, Some((target.0, &target.1)), now).await?;
    Ok(())
}
