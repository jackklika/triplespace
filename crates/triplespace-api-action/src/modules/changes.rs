//! `list=recentchanges`, `list=usercontribs` and `list=logevents` (compat §3.3;
//! 0012 §4; 0020 §5): three target sets over one feed query
//! ([`triplespace_projections::feed`]), in MediaWiki's shapes.
//!
//! - **`recentchanges`**: the everything set. One row per local revision and local log
//!   event; mirror syncs never appear, each mirror job run appearing as its `job` events
//!   (16 §4.2). `rcstart`, `rcend`, `rcdir`, `rcnamespace`, `rcuser`, `rcexcludeuser`,
//!   `rctag`, `rcprop`, `rcshow` (`minor`, `bot`, `new`, `patrolled`, `autopatrolled`,
//!   `unpatrolled` and their negations), `rctype` (`edit`, `new`, `log`), `rclimit`,
//!   `rccontinue`; and Triplespace's `rcjob`, `rckind` (adds `sync`) and the `rcprop`
//!   values `source`, `job`, `operator`.
//! - **`usercontribs`**: the actor set. `ucuser` (names or actor keys, several), or
//!   `ucuserids`; `ucstart`, `ucend`, `ucdir`, `ucnamespace`, `uctag`, `ucprop`,
//!   `ucshow`, `uclimit`, `uccontinue`, `ucjob`.
//! - **`logevents`**: the log rows. `letype`, `leaction` (`type/action`), `lestart`,
//!   `leend`, `ledir`, `leuser`, `letitle` (a page's, a deleted page's or an entity's
//!   events; an unknown title has none), `leprop`, `lelimit`,
//!   `lecontinue`. `leprovider` (a provider's events from `view.entity_history`) waits
//!   for that table.
//!
//! - **`watchlist`**: the watched set ([`crate::watching`]): `wltype`, `wluser`,
//!   `wlexcludeuser`, `wlshow`, `wlprop` with `notificationtimestamp`, `wlallrev`,
//!   `wlstart`, `wlend`, `wldir`, `wllimit`, `wlcontinue`. Mirror syncs are in it.
//!
//! Continuation tokens are the feed's cursors (compat §2.6): a client returns the
//! `continue` object unchanged.

use serde_json::{Map, Value, json};
use triplespace_projections::feed::{
    self, Cursor, Direction, Feed, Filters, Flag, TargetSet, parse_timestamp,
};

use crate::activity::{actor_key_of, continuation, mw_row, viewer};
use crate::api::Ctx;
use crate::response::{ApiError, ApiResponse};

/// The default and the most rows per request.
const DEFAULT_LIMIT: usize = 10;
const MAX_LIMIT: usize = 500;

/// Which module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Module {
    RecentChanges,
    UserContribs,
    LogEvents,
    Watchlist,
}

impl Module {
    fn name(self) -> &'static str {
        match self {
            Self::RecentChanges => "recentchanges",
            Self::UserContribs => "usercontribs",
            Self::LogEvents => "logevents",
            Self::Watchlist => "watchlist",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            Self::RecentChanges => "rc",
            Self::UserContribs => "uc",
            Self::LogEvents => "le",
            Self::Watchlist => "wl",
        }
    }

    fn default_props(self) -> &'static [&'static str] {
        match self {
            Self::RecentChanges => &["title", "timestamp", "ids"],
            Self::UserContribs => &["ids", "title", "timestamp", "comment", "size", "flags"],
            Self::Watchlist => &["ids", "title", "flags"],
            Self::LogEvents => &[
                "ids",
                "title",
                "type",
                "user",
                "timestamp",
                "comment",
                "details",
            ],
        }
    }
}

/// The entity types a namespace list selects (namespaces.toml).
fn entity_types_of(namespaces: &[String]) -> Result<Vec<String>, ApiError> {
    let mut types = Vec::new();
    for ns in namespaces {
        if ns == "*" {
            return Ok(Vec::new());
        }
        let t = match ns.as_str() {
            "120" => "item",
            "122" => "property",
            "146" => "lexeme",
            "210" => "domain",
            "212" => "keyword",
            "216" => "notation",
            other => {
                other
                    .parse::<i64>()
                    .map_err(|_| ApiError::bad_value("namespace", other))?;
                // A pages namespace: no entity type; such rows are page targets, which
                // the feed's page join serves once the pages stack writes them.
                continue;
            }
        };
        types.push(t.to_string());
    }
    Ok(types)
}

/// A limit parameter: `max` is the most.
fn limit_of(ctx: &Ctx, name: &str) -> Result<usize, ApiError> {
    match ctx.params.get(name) {
        None => Ok(DEFAULT_LIMIT),
        Some("max") => Ok(MAX_LIMIT),
        Some(v) => {
            let n: usize = v.parse().map_err(|_| {
                ApiError::new(
                    "badinteger",
                    format!("Invalid value \"{v}\" for integer parameter \"{name}\"."),
                )
            })?;
            Ok(n.clamp(1, MAX_LIMIT))
        }
    }
}

/// A timestamp parameter.
fn time_of(ctx: &Ctx, name: &str) -> Result<Option<u64>, ApiError> {
    match ctx.params.get(name) {
        None => Ok(None),
        Some(v) => parse_timestamp(v)
            .map(Some)
            .ok_or_else(|| ApiError::bad_value(name, v)),
    }
}

/// The parts of a request every module shares: the direction, the window, the limit,
/// the cursor and the flags.
async fn common(ctx: &Ctx, module: Module, feed: &mut Feed) -> Result<Vec<String>, ApiError> {
    let p = module.prefix();
    let get = |n: &str| ctx.params.get(&format!("{p}{n}"));
    feed.direction = match get("dir").unwrap_or("older") {
        "older" => Direction::Older,
        "newer" => Direction::Newer,
        other => return Err(ApiError::bad_value(&format!("{p}dir"), other)),
    };
    let start = time_of(ctx, &format!("{p}start"))?;
    let end = time_of(ctx, &format!("{p}end"))?;
    // `start` is where enumeration starts, so which bound it is depends on the direction.
    match feed.direction {
        Direction::Older => {
            feed.filters.until = start;
            feed.filters.since = end;
        }
        Direction::Newer => {
            feed.filters.since = start;
            feed.filters.until = end;
        }
    }
    if let (Some(s), Some(u)) = (feed.filters.since, feed.filters.until)
        && s > u
    {
        return Err(ApiError::new(
            "badvalue",
            format!(
                "{p}start has to be {} {p}end.",
                match feed.direction {
                    Direction::Older => "later than",
                    Direction::Newer => "before",
                }
            ),
        ));
    }
    feed.limit = limit_of(ctx, &format!("{p}limit"))?;
    if let Some(c) = get("continue") {
        feed.cursor =
            Some(Cursor::parse(c).ok_or_else(|| ApiError::bad_value(&format!("{p}continue"), c))?);
    }
    let show = ctx.params.list(&format!("{p}show"));
    for name in [
        "minor",
        "bot",
        "new",
        "patrolled",
        "unpatrolled",
        "anon",
        "redirect",
    ] {
        let yes = show.iter().any(|s| s == name);
        let no = show.iter().any(|s| s.strip_prefix('!') == Some(name));
        if yes && no {
            return Err(ApiError::new(
                "show",
                "Incorrect parameter - mutually exclusive values may not be supplied.",
            ));
        }
    }
    feed.filters.minor = Flag::from_show(&show, "minor");
    feed.filters.bot = Flag::from_show(&show, "bot");
    feed.filters.new = Flag::from_show(&show, "new");
    feed.filters.patrolled = Flag::from_show(&show, "patrolled");
    if show.iter().any(|s| s == "unpatrolled") {
        feed.filters.patrolled = Flag::Not;
    }
    if let Some(t) = get("tag") {
        feed.filters.tag = Some(t.to_string());
    }
    if let Some(j) = get("job") {
        feed.filters.job = Some(
            j.parse()
                .map_err(|_| ApiError::bad_value(&format!("{p}job"), j))?,
        );
    }
    feed.filters.entity_types = entity_types_of(&ctx.params.list(&format!("{p}namespace")))?;
    if let Some(u) = get("excludeuser") {
        feed.filters.exclude_actor =
            Some(actor_key_of(ctx, u).await?.unwrap_or_else(|| u.to_string()));
    }
    let mut props = ctx.params.list(&format!("{p}prop"));
    if props.is_empty() {
        props = module
            .default_props()
            .iter()
            .map(|s| (*s).to_string())
            .collect();
    }
    Ok(props)
}

/// Runs one module, adding its rows to `query` and its continuation to `body`.
#[allow(clippy::too_many_lines)]
async fn run(
    ctx: &Ctx,
    module: Module,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    ctx.caller.require("read")?;
    let p = module.prefix();
    let mut feed = Feed::everything(DEFAULT_LIMIT);
    feed.languages = vec!["en".into()];
    match module {
        Module::RecentChanges => {
            let types = ctx.params.list("rctype");
            let kinds = ctx.params.list("rckind");
            if !kinds.is_empty() {
                feed.filters.kinds = kinds;
            } else if types.is_empty() {
                feed.filters.kinds = vec!["edit".into(), "log".into(), "job".into()];
            } else {
                let mut kinds = Vec::new();
                for t in &types {
                    match t.as_str() {
                        "edit" => kinds.push("edit".to_string()),
                        "new" => {
                            kinds.push("edit".to_string());
                            feed.filters.new = Flag::Only;
                        }
                        "log" => {
                            kinds.push("log".to_string());
                            kinds.push("job".to_string());
                        }
                        "categorize" | "external" => {}
                        other => return Err(ApiError::bad_value("rctype", other)),
                    }
                }
                kinds.sort();
                kinds.dedup();
                feed.filters.kinds = kinds;
            }
            if let Some(u) = ctx.params.get("rcuser") {
                feed.filters.actor =
                    Some(actor_key_of(ctx, u).await?.unwrap_or_else(|| u.to_string()));
            }
        }
        Module::UserContribs => {
            let users = ctx.params.list("ucuser");
            let ids = ctx.params.list("ucuserids");
            if users.is_empty() && ids.is_empty() {
                return Err(ApiError::new(
                    "missingparam",
                    "One of the parameters \"ucuser\", \"ucuserids\" and \"ucuserprefix\" is required.",
                ));
            }
            let mut keys = Vec::new();
            for u in &users {
                if let Some(k) = actor_key_of(ctx, u).await? {
                    keys.push(k);
                }
            }
            for id in &ids {
                let n: u64 = id
                    .parse()
                    .map_err(|_| ApiError::bad_value("ucuserids", id))?;
                let row = ctx
                    .db()
                    .query_opt(
                        "SELECT actor_key FROM view.actor
                         WHERE tenant IN ($1, '') AND actor_key LIKE '%:' || $2
                         ORDER BY (tenant = $1) DESC LIMIT 1",
                        &[&ctx.tenant.slug, &n.to_string()],
                    )
                    .await?;
                if let Some(r) = row {
                    keys.push(r.get(0));
                }
            }
            if keys.is_empty() {
                // An unknown user has no contributions: an empty list, as MediaWiki answers.
                query.insert("usercontribs".into(), json!([]));
                return Ok(());
            }
            // One actor per call for now; several are served in turn, as MediaWiki orders
            // them by user then time.
            feed.set = TargetSet::Actor(keys[0].clone());
            if keys.len() > 1 {
                response.warn(
                    "usercontribs",
                    "Only the first user is served per request; ask for the others separately.",
                );
            }
            feed.filters.kinds = vec!["edit".into()];
        }
        Module::Watchlist => {}
        Module::LogEvents => {
            feed.filters.kinds = vec!["log".into(), "job".into()];
            // A thread record that is also a log event is an `edit` row with a log ID.
            feed.filters.log_events = true;
            if let Some(t) = ctx.params.get("letype") {
                feed.filters.log_type = Some(t.to_string());
            }
            if let Some(a) = ctx.params.get("leaction") {
                let (t, a) = a
                    .split_once('/')
                    .ok_or_else(|| ApiError::bad_value("leaction", a))?;
                feed.filters.log_type = Some(t.to_string());
                feed.filters.log_action = Some(a.to_string());
            }
            if let Some(u) = ctx.params.get("leuser") {
                feed.filters.actor =
                    Some(actor_key_of(ctx, u).await?.unwrap_or_else(|| u.to_string()));
            }
            if let Some(t) = ctx.params.get("letitle") {
                let Some(set) = log_target(ctx, t).await? else {
                    query.insert("logevents".into(), json!([]));
                    return Ok(());
                };
                feed.set = set;
            }
        }
    }
    let mut holding = None;
    if module == Module::Watchlist {
        let holder = watch_holder(ctx)?;
        let (watches, x) = crate::watching::watch_set(ctx, &holder).await?;
        feed.set = crate::watching::target_set(&x);
        // Mirror syncs are on for a watchlist (16 §4.2).
        feed.filters.kinds = vec!["edit".into(), "log".into(), "job".into(), "sync".into()];
        let types = ctx.params.list("wltype");
        if !types.is_empty() {
            let mut kinds = Vec::new();
            for t in &types {
                match t.as_str() {
                    "edit" | "new" => kinds.push("edit".to_string()),
                    "log" => {
                        kinds.push("log".to_string());
                        kinds.push("job".to_string());
                    }
                    "categorize" | "external" => {}
                    other => return Err(ApiError::bad_value("wltype", other)),
                }
            }
            if types.iter().all(|t| t == "new") {
                feed.filters.new = Flag::Only;
            }
            kinds.sort();
            kinds.dedup();
            feed.filters.kinds = kinds;
        }
        if let Some(u) = ctx.params.get("wluser") {
            feed.filters.actor = Some(actor_key_of(ctx, u).await?.unwrap_or_else(|| u.to_string()));
        }
        holding = Some((holder, watches, x));
    }
    let props = common(ctx, module, &mut feed).await?;
    let page = feed::query(ctx.db(), &ctx.tenant.slug, &feed, &viewer(ctx).await?)
        .await
        .map_err(ApiError::internal)?;
    let mut rows: Vec<Value> = Vec::with_capacity(page.rows.len());
    let all_revisions = ctx.params.flag("wlallrev");
    let mut targets_seen: Vec<(Option<i16>, Option<String>)> = Vec::new();
    for r in &page.rows {
        let Some((holder, watches, x)) = &holding else {
            rows.push(mw_row(ctx, r, &props, module.name()));
            continue;
        };
        // Without `wlallrev`, a page shows its newest change only, as MediaWiki's does
        // (within each page of results).
        let key = (r.target.kind, r.target.id.clone());
        if !all_revisions && targets_seen.contains(&key) {
            continue;
        }
        targets_seen.push(key);
        let mut row = mw_row(ctx, r, &props, "recentchanges");
        if let Value::Object(m) = &mut row {
            m.remove("rcid");
            if props.iter().any(|p| p == "notificationtimestamp") {
                let unseen = match (r.target.kind, r.target.id.as_deref()) {
                    (Some(k), Some(id)) => crate::watching::unseen(
                        watches,
                        x,
                        k,
                        id,
                        i64::try_from(r.time).unwrap_or(i64::MAX),
                        r.actor.key.as_deref(),
                        holder,
                    ),
                    _ => false,
                };
                m.insert(
                    "notificationtimestamp".into(),
                    json!(if unseen {
                        crate::entity_json::iso8601(r.time)
                    } else {
                        String::new()
                    }),
                );
            }
        }
        rows.push(row);
    }
    query.insert(module.name().into(), Value::Array(rows));
    continuation(body, p, page.next);
    Ok(())
}

/// The target set `letitle` names: an entity, the page with the title, or the newest
/// deleted page that had it (the group's to see; outsiders find nothing, 09 §5.3).
async fn log_target(ctx: &Ctx, title: &str) -> Result<Option<TargetSet>, ApiError> {
    use triplespace_projections::common::target_kind;
    let site = crate::pages::Site::load(ctx).await?;
    if let Some(id) = crate::pages::entity_of_title(&site, title) {
        return Ok(Some(TargetSet::Target {
            kind: target_kind::ENTITY,
            id: id.as_str().to_string(),
        }));
    }
    let Some(t) = crate::pages::resolve_title(&site, title) else {
        return Ok(None);
    };
    let hidden = crate::pages::hidden(ctx).await?;
    let page_id =
        match crate::pages::by_title(ctx.db(), &ctx.tenant.slug, &hidden, t.namespace, &t.text)
            .await?
        {
            Some(p) => Some(p.page_id),
            // The newest deleted page that had the title: its public deletion is everyone's
            // to see; the feed's own visibility hides a confidential one's rows (09 §5.3).
            None => ctx
                .db()
                .query_opt(
                    "SELECT page_id FROM view.page
                 WHERE tenant = $1 AND ns = $2 AND title = $3 AND deleted
                 ORDER BY page_id DESC LIMIT 1",
                    &[&ctx.tenant.slug, &t.namespace, &t.text],
                )
                .await?
                .and_then(|r| u64::try_from(r.get::<_, i64>(0)).ok()),
        };
    Ok(page_id.map(|id| TargetSet::Target {
        kind: target_kind::PAGE,
        id: id.to_string(),
    }))
}

/// The caller whose watch set a watchlist module reads: themselves, logged in and holding
/// `viewmywatchlist`. Another account's, by `wlowner` and `wltoken`, waits for the token.
pub(crate) fn watch_holder(ctx: &Ctx) -> Result<String, ApiError> {
    let Some(holder) = ctx.caller.identity.actor_key.clone() else {
        return Err(ApiError::new(
            "notloggedin",
            "You must be logged in to have a watchlist.",
        ));
    };
    for p in ["wlowner", "wrowner"] {
        if ctx.params.get(p).is_some() {
            return Err(ApiError::new(
                "ts-not-supported",
                "Reading another account's watchlist by its token is not served yet.",
            ));
        }
    }
    ctx.caller.require("viewmywatchlist")?;
    Ok(holder)
}

/// `list=watchlist`: the caller's watch set as a feed (16 §4.2–4.3): MediaWiki's
/// parameters with the `wl` prefix, `wlprop=notificationtimestamp` for a change the holder
/// has not seen, and `wlallrev`.
pub async fn watchlist(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    run(ctx, Module::Watchlist, query, body, response).await
}

/// `list=recentchanges`.
pub async fn recentchanges(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    run(ctx, Module::RecentChanges, query, body, response).await
}

/// `list=usercontribs`.
pub async fn usercontribs(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    run(ctx, Module::UserContribs, query, body, response).await
}

/// `list=logevents`.
pub async fn logevents(
    ctx: &Ctx,
    query: &mut Map<String, Value>,
    body: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    run(ctx, Module::LogEvents, query, body, response).await
}

/// The filters a request's parameters would give: for tests, without a database.
#[must_use]
pub fn filters_for_test(show: &[String]) -> Filters {
    Filters {
        minor: Flag::from_show(show, "minor"),
        bot: Flag::from_show(show, "bot"),
        new: Flag::from_show(show, "new"),
        patrolled: Flag::from_show(show, "patrolled"),
        ..Filters::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_name_entity_types() {
        assert_eq!(
            entity_types_of(&["120".into(), "210".into()]).unwrap(),
            vec!["item", "domain"]
        );
        assert!(entity_types_of(&["*".into()]).unwrap().is_empty());
        assert!(entity_types_of(&["4".into()]).unwrap().is_empty());
        assert!(entity_types_of(&["x".into()]).is_err());
    }

    #[test]
    fn show_lists_become_flags() {
        let f = filters_for_test(&["!bot".into(), "new".into()]);
        assert_eq!(f.bot, Flag::Not);
        assert_eq!(f.new, Flag::Only);
        assert_eq!(f.minor, Flag::Any);
    }
}
