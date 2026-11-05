//! Change feeds (0020 §1–2; 16 §4.1–4.2): activity rows filtered by a target set.
//!
//! A [`Feed`] is a target set, filters, a direction and a cursor. Recent changes is the
//! everything set; contributions is the actor set; an entity's or page's history is the
//! one-target set. **Every list of changes the API or the site shows is one of these**:
//! `list=recentchanges`, `list=usercontribs`, `list=logevents`, `GET /activity` and the
//! pages that read them all call [`query`], so a history, a watchlist and the downstream
//! feed cannot drift apart.
//!
//! The rows come from `view.activity` alone, ordered by `(time, partition, offset)`, the
//! display order of 16 §4.1, with a keyset [`Cursor`] for the next page. The actor's name
//! and status come from `view.actor`, the target's title from `view.page` or its entity
//! type from `view.entity`, and the target's label from `view.term` in the languages
//! asked for; each is one `LATERAL` lookup per row, bounded by the page size.
//!
//! The *watched* set is [`TargetSet::Watched`], expanded by [`crate::watched`].
//!
//! What is not here yet, and where the design puts it: the *related* set (16 §4.2),
//! cluster expansion of the one-target set (`cluster_member` does not
//! exist), `view.entity_history` for a mirrored entity's upstream events, the stream and
//! Atom deliveries (16 §4.4). Each is a further target set or delivery over this query.

use std::fmt::Write as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use scatter_log_postgres::PgClient;
use scatter_log_postgres::ids::{from_db, partition_to_db};
use serde_json::Value;
use tokio_postgres::types::ToSql;

pub use crate::common::target_kind;

/// The most rows one page may hold.
pub const MAX_LIMIT: usize = 500;

/// Which records a feed covers (16 §4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSet {
    /// Every local record: recent changes.
    Everything,
    /// One target, by [`target_kind`] and ID: a history.
    Target {
        /// The target kind.
        kind: i16,
        /// The entity ID, page ID, actor key, job ID or `{partition}:{offset}`.
        id: String,
    },
    /// Records attested by one actor: contributions.
    Actor(String),
    /// A page with the threads attached to it, home or listing, when it is a talk page or
    /// a board: its composite history (16 §6.1; 14 §3.7). A thread attached now brings
    /// its whole history, and one that moved away or was detached takes its rows with it,
    /// all but the event of its leaving: a `thread/detach` naming the page, or a
    /// `move/move` whose `from_talk` is it (14 §1.3). A deleted thread's rows are left out.
    /// For any other page, its own rows.
    Talk(u64),
    /// A watch set, expanded by [`crate::watched::expand`] (16 §4.2–4.3): the rows of these
    /// entities and of these pages.
    Watched {
        /// Entity IDs.
        entities: Vec<String>,
        /// Page IDs, as text.
        pages: Vec<String>,
    },
}

/// Which way the rows run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    /// Newest first (MediaWiki's `older`).
    #[default]
    Older,
    /// Oldest first (MediaWiki's `newer`).
    Newer,
}

/// The row kinds of `view.activity`.
pub const KINDS: [&str; 5] = ["edit", "sync", "log", "job", "erased"];

/// A three-way switch on a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Flag {
    /// Rows with the flag set.
    Only,
    /// Rows without it.
    Not,
    /// Either.
    #[default]
    Any,
}

impl Flag {
    /// From MediaWiki's `show=x|!x` lists: `Only` when `x` is named, `Not` when `!x` is,
    /// `Any` otherwise. Both at once is MediaWiki's `show` error, which the caller
    /// reports; here `Only` wins.
    #[must_use]
    pub fn from_show(show: &[String], name: &str) -> Self {
        let yes = show.iter().any(|s| s == name);
        let no = show.iter().any(|s| s.strip_prefix('!') == Some(name));
        match (yes, no) {
            (true, _) => Self::Only,
            (false, true) => Self::Not,
            (false, false) => Self::Any,
        }
    }
}

/// The filters of 16 §4.1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filters {
    /// Row kinds ([`KINDS`]); every kind when empty. Recent changes leaves `sync` out by
    /// default (16 §4.2), a history keeps it.
    pub kinds: Vec<String>,
    /// Only rows from these graphs (`local`, `mirror/wikidata`, …): a history's source
    /// switch (19 §3.5). Every graph when empty.
    pub sources: Vec<String>,
    /// Only the row with this revision ID.
    pub revid: Option<u64>,
    /// Only rows by this actor key.
    pub actor: Option<String>,
    /// No rows by this actor key.
    pub exclude_actor: Option<String>,
    /// Only rows carrying this tag.
    pub tag: Option<String>,
    /// Only rows of this job.
    pub job: Option<u64>,
    /// Only targets of these kinds ([`target_kind`]).
    pub target_kinds: Vec<i16>,
    /// Only entity targets of these types (`item`, `property`, `domain`, …), the API's
    /// namespace filter; `None` for the target's type means the row is not an entity's.
    pub entity_types: Vec<String>,
    /// With `kinds`: also the `edit` rows that are log events too, carrying a log type
    /// and a log ID, as a thread `create` or `pin` does (16 §6.1). The log sets it.
    pub log_events: bool,
    /// Only log events of this type (`log` and `job` rows).
    pub log_type: Option<String>,
    /// … and this action.
    pub log_action: Option<String>,
    /// The `bot` tag.
    pub bot: Flag,
    /// The `minor` tag.
    pub minor: Flag,
    /// The `new` flag: a `create` or `adopt`.
    pub new: Flag,
    /// Patrolled.
    pub patrolled: Flag,
    /// Rows at or after this time, µs since the epoch.
    pub since: Option<u64>,
    /// Rows at or before this time, µs since the epoch.
    pub until: Option<u64>,
}

/// Where a page starts: the sort key of the last row returned, exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// µs since the epoch.
    pub time: u64,
    /// The partition.
    pub partition: u64,
    /// The offset.
    pub offset: u64,
}

impl Cursor {
    /// The token a client sends back: `{time}|{partition}|{offset}`, the shape of
    /// MediaWiki's `rccontinue` (compat §2.6), which clients return unchanged.
    #[must_use]
    pub fn token(&self) -> String {
        format!("{}|{}|{}", self.time, self.partition, self.offset)
    }

    /// Parses a token; `None` for one this did not issue.
    #[must_use]
    pub fn parse(token: &str) -> Option<Self> {
        let mut parts = token.split('|');
        let time = parts.next()?.parse().ok()?;
        let partition = parts.next()?.parse().ok()?;
        let offset = parts.next()?.parse().ok()?;
        parts.next().is_none().then_some(Self {
            time,
            partition,
            offset,
        })
    }
}

/// One feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    /// The target set.
    pub set: TargetSet,
    /// The filters.
    pub filters: Filters,
    /// The direction.
    pub direction: Direction,
    /// Where to continue from.
    pub cursor: Option<Cursor>,
    /// Rows per page, at most [`MAX_LIMIT`].
    pub limit: usize,
    /// The languages a target's label is wanted in, best first.
    pub languages: Vec<String>,
}

impl Feed {
    /// The everything set, newest first, with recent changes' defaults.
    #[must_use]
    pub fn everything(limit: usize) -> Self {
        Self {
            set: TargetSet::Everything,
            filters: Filters::default(),
            direction: Direction::Older,
            cursor: None,
            limit,
            languages: vec!["en".into()],
        }
    }
}

/// The actor of a row, as `view.actor` has it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Actor {
    /// The key; absent when the attestation was erased.
    pub key: Option<String>,
    /// The current name; absent for a surrogate without one, or a hidden actor.
    pub name: Option<String>,
    /// `registered`, `temporary`, `bot`, `anonymous`, `imported`, `provider`, `federated`.
    pub kind: Option<String>,
    /// For a surrogate, the raw value it stands for (07 §1.4): the IP of an `anonymous`
    /// one, the `imported>Name` of an `imported` one, a remote IRI for a `federated` one;
    /// absent when erased or hidden.
    pub raw: Option<String>,
    /// The actor IRI.
    pub iri: Option<String>,
    /// Whether the actor is hidden (0023 §5): the name is withheld.
    pub hidden: bool,
}

/// The target of a row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    /// The kind ([`target_kind`]).
    pub kind: Option<i16>,
    /// The ID.
    pub id: Option<String>,
    /// The cluster member the record is keyed to, or the revision ID of a record target.
    pub member: Option<String>,
    /// An entity target's type.
    pub entity_type: Option<String>,
    /// An entity target's page ID.
    pub page_id: Option<u64>,
    /// A page target's namespace and title.
    pub page: Option<(i32, String)>,
    /// An entity target's label in the best of the languages asked for.
    pub label: Option<(String, String)>,
}

/// One activity row (16 §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct Row {
    /// When the event happened, µs since the epoch.
    pub time: u64,
    /// The record.
    pub partition: u64,
    /// The record.
    pub offset: u64,
    /// `edit`, `sync`, `log`, `job` or `erased`.
    pub kind: String,
    /// The graph: `local`, `pages`, `mirror/wikidata`, `log`, …
    pub source: String,
    /// The revision ID.
    pub revid: Option<u64>,
    /// The log ID.
    pub logid: Option<u64>,
    /// The target.
    pub target: Target,
    /// The actor.
    pub actor: Actor,
    /// The operator, for a subsidiary's row.
    pub operator: Option<String>,
    /// Who asked for the job.
    pub requested_by: Option<String>,
    /// The job.
    pub job: Option<u64>,
    /// The summary's operation.
    pub summary_op: Option<String>,
    /// The summary's arguments.
    pub summary_args: Option<String>,
    /// The user's comment.
    pub comment: Option<String>,
    /// Change tags.
    pub tags: Vec<String>,
    /// The minor flag.
    pub minor: bool,
    /// The bot flag.
    pub bot: bool,
    /// The new flag.
    pub new: bool,
    /// The size after.
    pub size: Option<i32>,
    /// The size change.
    pub delta: Option<i32>,
    /// The visibility bitmask (0023 §5).
    pub visibility: i16,
    /// Patrolled.
    pub patrolled: bool,
    /// A log event's type.
    pub log_type: Option<String>,
    /// … and action.
    pub log_action: Option<String>,
    /// A log event's parameters.
    pub params: Option<Value>,
    /// A sync's change summary.
    pub changes: Option<Value>,
}

impl Row {
    /// The sort key, for the cursor of the next page.
    #[must_use]
    pub fn cursor(&self) -> Cursor {
        Cursor {
            time: self.time,
            partition: self.partition,
            offset: self.offset,
        }
    }
}

/// One page of a feed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Page {
    /// The rows, in the feed's direction.
    pub rows: Vec<Row>,
    /// Where the next page starts, when there is one.
    pub next: Option<Cursor>,
}

/// What a viewer may see: the groups they hold, for rows under a `read` restriction, and
/// the targets confidential restrictions hide from them (0056 §5; 09 §5.3), whose rows,
/// log events included, are left out as if the target did not exist.
#[derive(Debug, Clone, Default)]
pub struct Viewer {
    /// The groups whose `read` restrictions the viewer passes: its own and those they
    /// include (09 §3.2).
    pub groups: Vec<String>,
    /// Whether one of its groups includes `"*"`, every group but [`Self::excluded`].
    pub every: bool,
    /// What `"*"` leaves out.
    pub excluded: Vec<String>,
    /// What is absent for the viewer ([`crate::acl_read::hidden`]).
    pub hidden: crate::acl_read::Hidden,
}

impl Viewer {
    /// The viewer for an evaluated principal.
    #[must_use]
    pub fn of(effective: &scatter_actors::Effective, hidden: crate::acl_read::Hidden) -> Self {
        Self {
            groups: effective
                .groups
                .iter()
                .chain(&effective.reads_as)
                .cloned()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect(),
            every: effective.reads_every,
            excluded: effective.reads_excluded.iter().cloned().collect(),
            hidden,
        }
    }

    /// Whether the viewer passes a `read` restriction naming `group`.
    #[must_use]
    pub fn reads_in(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
            || (self.every && !self.excluded.iter().any(|g| g == group))
    }
}

type Param = Box<dyn ToSql + Sync + Send>;

fn micros(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

fn time_of(micros: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(micros)
}

/// The query for a feed: the SQL and its parameters.
struct Built {
    sql: String,
    params: Vec<Param>,
}

/// Builds the query. Every condition is a parameter, never interpolated text, so the
/// statement cache (`PgClient::statement`) holds one entry per filter shape. A page's
/// row is found by its key, `page_id = target_id::bigint` behind a digits guard (a
/// `page_id::text` comparison would scan every page per row). A tenant's feed covers its own rows and the instance's (`tenant = ''`): the shared mirror
/// partitions' syncs, which a history wants, and the instance log's job events, which
/// is how a mirror sync appears in recent changes (16 §4.2).
#[allow(clippy::too_many_lines)]
fn build(tenant: &str, feed: &Feed, viewer: &Viewer) -> Built {
    let mut params: Vec<Param> = vec![Box::new(tenant.to_string())];
    let mut sql = String::from(
        "SELECT a.time, a.partition, a.\"offset\", a.kind, a.source, a.revid, a.logid,
                a.target_kind, a.target_id, a.target_member, a.actor_key, a.operator,
                a.requested_by, a.job_id, a.summary_op, a.summary_args, a.comment, a.tags,
                a.minor, a.bot, a.new, a.size, a.delta, a.visibility, a.patrolled,
                a.log_type, a.log_action, a.params, a.changes,
                ac.name, ac.kind, ac.iri, ac.status,
                e.type, e.page_id, p.ns, p.title, t.lang, t.text, ac.raw AS actor_raw
         FROM view.activity a
         LEFT JOIN LATERAL (
           SELECT x.name, x.kind, x.iri, x.status, x.raw FROM view.actor x
           WHERE x.actor_key = a.actor_key AND x.tenant IN (a.tenant, '')
           ORDER BY (x.tenant = a.tenant) DESC LIMIT 1) ac ON true
         LEFT JOIN LATERAL (
           SELECT x.type, x.page_id FROM view.entity x
           WHERE a.target_kind = 1 AND x.id = a.target_id AND x.tenant IN (a.tenant, '')
           ORDER BY (x.tenant = a.tenant) DESC LIMIT 1) e ON true
         LEFT JOIN LATERAL (
           SELECT x.ns, x.title FROM view.page x
           WHERE a.target_kind = 2 AND x.tenant = a.tenant
             AND x.page_id = CASE WHEN a.target_id ~ '^[0-9]+$' THEN a.target_id::bigint END
           LIMIT 1) p ON true
         LEFT JOIN LATERAL (
           SELECT x.lang, x.text FROM view.term x
           WHERE a.target_kind = 1 AND x.entity_id = a.target_id AND x.tenant IN (a.tenant, '')
             AND x.kind = 1 AND x.lang = ANY($2)
           ORDER BY array_position($2, x.lang), (x.tenant = a.tenant) DESC LIMIT 1) t ON true
         WHERE a.tenant IN ($1, '')",
    );
    params.push(Box::new(feed.languages.clone()));
    let mut push = |sql: &mut String, clause: &str, p: Param| {
        params.push(p);
        let _ = write!(
            sql,
            " AND {}",
            clause.replace("$n", &format!("${}", params.len()))
        );
    };
    match &feed.set {
        TargetSet::Everything => {}
        TargetSet::Target { kind, id } => {
            push(&mut sql, "a.target_kind = $n", Box::new(*kind));
            push(&mut sql, "a.target_id = $n", Box::new(id.clone()));
        }
        TargetSet::Actor(key) => push(&mut sql, "a.actor_key = $n", Box::new(key.clone())),
        TargetSet::Talk(id) => {
            push(&mut sql, "a.target_kind = $n", Box::new(target_kind::PAGE));
            push(
                &mut sql,
                "(a.target_id = $n OR a.target_id IN (
                   SELECT ta.thread_id::text FROM view.thread_attachment ta
                   JOIN view.page tp ON tp.tenant = ta.tenant AND tp.page_id = ta.thread_id
                   WHERE ta.tenant = a.tenant AND ta.talk_page_id = $n::bigint AND NOT tp.deleted)
                   OR (a.log_type = 'thread' AND a.log_action = 'detach' AND a.params->>'talk' = $n)
                   OR (a.log_type = 'move' AND a.params->>'from_talk' = $n))",
                Box::new(id.to_string()),
            );
        }
        TargetSet::Watched { entities, pages } => {
            // One parameter for both lists, so the clause stays one `push`: entities as
            // `e:{id}` and pages as `p:{id}`.
            let keys: Vec<String> = entities
                .iter()
                .map(|e| format!("e:{e}"))
                .chain(pages.iter().map(|p| format!("p:{p}")))
                .collect();
            push(
                &mut sql,
                "((a.target_kind = 1 AND ('e:' || a.target_id) = ANY($n))
                  OR (a.target_kind = 2 AND ('p:' || a.target_id) = ANY($n)))",
                Box::new(keys),
            );
        }
    }
    // A deleted page's or local entity's changes leave the lists, recent changes,
    // contributions and watchlists, for everyone, as MediaWiki moves a deleted page's
    // revisions out of `recentchanges`; its log events, the deletion's included, stay. A
    // single target's history keeps them: `prop=deletedrevisions` reads it there, for the
    // deletion group (0023 §8). Read-time, so an undeletion brings them back (16 §4.1;
    // 0020 §1, A19).
    if !matches!(feed.set, TargetSet::Target { .. } | TargetSet::Talk(_)) {
        sql.push_str(DELETED_TARGETS_LEFT_OUT);
    }
    let f = &feed.filters;
    if !f.kinds.is_empty() && f.log_events {
        push(
            &mut sql,
            "(a.kind = ANY($n) OR (a.log_type IS NOT NULL AND a.logid IS NOT NULL))",
            Box::new(f.kinds.clone()),
        );
    } else if !f.kinds.is_empty() {
        push(&mut sql, "a.kind = ANY($n)", Box::new(f.kinds.clone()));
    }
    if !f.sources.is_empty() {
        push(&mut sql, "a.source = ANY($n)", Box::new(f.sources.clone()));
    }
    if let Some(r) = f.revid {
        push(
            &mut sql,
            "a.revid = $n",
            Box::new(i64::try_from(r).unwrap_or(i64::MAX)),
        );
    }
    if let Some(a) = &f.actor {
        push(&mut sql, "a.actor_key = $n", Box::new(a.clone()));
    }
    if let Some(a) = &f.exclude_actor {
        push(
            &mut sql,
            "(a.actor_key IS NULL OR a.actor_key <> $n)",
            Box::new(a.clone()),
        );
    }
    if let Some(t) = &f.tag {
        push(&mut sql, "$n = ANY(a.tags)", Box::new(t.clone()));
    }
    if let Some(j) = f.job {
        push(
            &mut sql,
            "a.job_id = $n",
            Box::new(i64::try_from(j).unwrap_or(i64::MAX)),
        );
    }
    if !f.target_kinds.is_empty() {
        push(
            &mut sql,
            "a.target_kind = ANY($n)",
            Box::new(f.target_kinds.clone()),
        );
    }
    if !f.entity_types.is_empty() {
        push(
            &mut sql,
            "e.type = ANY($n)",
            Box::new(f.entity_types.clone()),
        );
    }
    if let Some(t) = &f.log_type {
        push(&mut sql, "a.log_type = $n", Box::new(t.clone()));
    }
    if let Some(t) = &f.log_action {
        push(&mut sql, "a.log_action = $n", Box::new(t.clone()));
    }
    for (flag, expr) in [
        (f.bot, "(a.bot OR 'bot' = ANY(a.tags))"),
        (f.minor, "(a.minor OR 'minor' = ANY(a.tags))"),
        (f.new, "a.new"),
        (f.patrolled, "a.patrolled"),
    ] {
        match flag {
            Flag::Only => {
                let _ = write!(sql, " AND coalesce({expr}, false)");
            }
            Flag::Not => {
                let _ = write!(sql, " AND NOT coalesce({expr}, false)");
            }
            Flag::Any => {}
        }
    }
    if let Some(s) = f.since {
        push(&mut sql, "a.time >= $n", Box::new(time_of(s)));
    }
    if let Some(u) = f.until {
        push(&mut sql, "a.time <= $n", Box::new(time_of(u)));
    }
    // A suppression's event is for the group it names, as MediaWiki's suppression log is
    // for oversighters (09 §6.5); for a retirement, the group of the ACL it retired.
    let named = "coalesce(a.params->'restrictions'->'read'->>'group', a.params->'previous'->'read'->>'group')";
    if viewer.every {
        // Every group but the excluded ones not also named.
        let excluded: Vec<String> = viewer
            .excluded
            .iter()
            .filter(|g| !viewer.groups.contains(g))
            .cloned()
            .collect();
        push(
            &mut sql,
            &format!("(a.log_type IS DISTINCT FROM 'suppress' OR {named} <> ALL($n))"),
            Box::new(excluded),
        );
    } else {
        push(
            &mut sql,
            &format!("(a.log_type IS DISTINCT FROM 'suppress' OR {named} = ANY($n))"),
            Box::new(viewer.groups.clone()),
        );
    }
    // Rows on a target a confidential restriction hides are absent (0056 §5; 09 §5.3).
    let h = &viewer.hidden;
    let page = target_kind::PAGE;
    let record = target_kind::RECORD;
    if h.tenant {
        sql.push_str(" AND false");
    } else if h.all_pages {
        // A revision-deletion event names its record; `params.of` is the record's page.
        push(
            &mut sql,
            &format!(
                "a.target_kind IS DISTINCT FROM $n
                 AND NOT (a.target_kind = {record} AND a.params->'of'->>'kind' = '{page}')"
            ),
            Box::new(page),
        );
    } else {
        if !h.pages.is_empty() {
            let ids: Vec<String> = h.pages.iter().map(u64::to_string).collect();
            push(
                &mut sql,
                &format!(
                    "NOT ((a.target_kind = {page} AND a.target_id = ANY($n))
                      OR (a.target_kind = {record} AND a.params->'of'->>'kind' = '{page}'
                          AND a.params->'of'->>'id' = ANY($n)))"
                ),
                Box::new(ids),
            );
        }
        if !h.acl_keys.is_empty() {
            push(
                &mut sql,
                "NOT (a.target_kind IS NULL AND a.target_id = ANY($n))",
                Box::new(h.acl_keys.iter().cloned().collect::<Vec<String>>()),
            );
        }
        if !h.namespaces.is_empty() {
            push(
                &mut sql,
                &format!(
                    "NOT ((a.target_kind = {page} AND p.ns = ANY($n))
                      OR (a.target_kind = {record} AND a.params->'of'->>'kind' = '{page}'
                          AND EXISTS (SELECT 1 FROM view.page op WHERE op.tenant = a.tenant
                            AND op.page_id = CASE WHEN a.params->'of'->>'id' ~ '^[0-9]+$'
                                                  THEN (a.params->'of'->>'id')::bigint END
                            AND op.ns = ANY($n))))"
                ),
                Box::new(h.namespace_list()),
            );
        }
        // Entities: those hidden themselves or through a set, and those of a hidden
        // entity namespace (by type); a revision-deletion names the entity in `of`.
        let entity = target_kind::ENTITY;
        if !h.entities.is_empty() {
            push(
                &mut sql,
                &format!(
                    "NOT ((a.target_kind = {entity} AND a.target_id = ANY($n))
                      OR (a.target_kind = {record} AND a.params->'of'->>'kind' = '{entity}'
                          AND a.params->'of'->>'id' = ANY($n)))"
                ),
                Box::new(h.entity_ids()),
            );
        }
        let types = h.entity_types();
        if !types.is_empty() {
            push(
                &mut sql,
                &format!(
                    "NOT ((a.target_kind = {entity} AND e.type = ANY($n))
                      OR (a.target_kind = {record} AND a.params->'of'->>'kind' = '{entity}'
                          AND EXISTS (SELECT 1 FROM view.entity oe WHERE oe.tenant IN (a.tenant, '')
                            AND oe.id = a.params->'of'->>'id' AND oe.type = ANY($n))))"
                ),
                Box::new(types),
            );
        }
        if !h.graphs.is_empty() {
            // Rows of a restricted graph's partition (a mirror's records) are absent.
            push(
                &mut sql,
                "NOT EXISTS (SELECT 1 FROM log.partition lp WHERE lp.partition = a.partition
                             AND lp.name = ANY($n))",
                Box::new(h.graphs.iter().cloned().collect::<Vec<String>>()),
            );
        }
    }
    if let Some(c) = feed.cursor {
        let cmp = match feed.direction {
            Direction::Older => "<",
            Direction::Newer => ">",
        };
        params.push(Box::new(time_of(c.time)));
        params.push(Box::new(partition_to_db(c.partition)));
        params.push(Box::new(i64::try_from(c.offset).unwrap_or(i64::MAX)));
        let n = params.len();
        let _ = write!(
            sql,
            " AND (a.time, a.partition, a.\"offset\") {cmp} (${}, ${}, ${n})",
            n - 2,
            n - 1
        );
    }
    let order = match feed.direction {
        Direction::Older => "DESC",
        Direction::Newer => "ASC",
    };
    let _ = write!(
        sql,
        " ORDER BY a.time {order}, a.partition {order}, a.\"offset\" {order} LIMIT ${}",
        params.len() + 1
    );
    params.push(Box::new(
        i64::try_from(feed.limit.clamp(1, MAX_LIMIT) + 1).unwrap_or(i64::MAX),
    ));
    Built { sql, params }
}

fn row_of(r: &tokio_postgres::Row) -> Result<Row, String> {
    let u = |v: Option<i64>| -> Result<Option<u64>, String> {
        v.map(|n| from_db(n).map_err(|e| e.to_string())).transpose()
    };
    let status: Option<String> = r.get(32);
    let target_kind: Option<i16> = r.get(7);
    let page_id: Option<i64> = r.get(34);
    let ns: Option<i32> = r.get(35);
    let title: Option<String> = r.get(36);
    let lang: Option<String> = r.get(37);
    let text: Option<String> = r.get(38);
    Ok(Row {
        time: micros(r.get(0)),
        partition: scatter_log_postgres::ids::partition_from_db(r.get(1)),
        offset: from_db(r.get(2)).map_err(|e| e.to_string())?,
        kind: r.get(3),
        source: r.get(4),
        revid: u(r.get(5))?,
        logid: u(r.get(6))?,
        target: Target {
            kind: target_kind,
            id: r.get(8),
            member: r.get(9),
            entity_type: r.get(33),
            page_id: u(page_id)?,
            page: match (ns, title) {
                (Some(ns), Some(title)) => Some((ns, title)),
                _ => None,
            },
            label: match (text, lang) {
                (Some(text), Some(lang)) => Some((text, lang)),
                _ => None,
            },
        },
        actor: Actor {
            key: r.get(10),
            name: if status.as_deref() == Some("hidden") {
                None
            } else {
                r.get(29)
            },
            kind: r.get(30),
            raw: if status.as_deref() == Some("hidden") {
                None
            } else {
                r.get("actor_raw")
            },
            iri: r.get(31),
            hidden: status.as_deref() == Some("hidden"),
        },
        operator: r.get(11),
        requested_by: r.get(12),
        job: u(r.get(13))?,
        summary_op: r.get(14),
        summary_args: r.get(15),
        comment: r.get(16),
        tags: r.get(17),
        minor: r.get::<_, Option<bool>>(18).unwrap_or(false)
            || r.get::<_, Vec<String>>(17).iter().any(|t| t == "minor"),
        bot: r.get::<_, Option<bool>>(19).unwrap_or(false)
            || r.get::<_, Vec<String>>(17).iter().any(|t| t == "bot"),
        new: r.get::<_, Option<bool>>(20).unwrap_or(false),
        size: r.get(21),
        delta: r.get(22),
        visibility: r.get(23),
        patrolled: r.get(24),
        log_type: r.get(25),
        log_action: r.get(26),
        params: r.get(27),
        changes: r.get(28),
    })
}

/// The clause that leaves a deleted target's changes out of a list (see [`query`]): a row
/// that is a change (`edit`, `sync`, `erased`) whose page or local entity is deleted now.
/// The page ID is cast only for a page target, inside `CASE`, since SQL does not promise
/// to evaluate the target kind's test first.
const DELETED_TARGETS_LEFT_OUT: &str = "
     AND NOT (a.kind IN ('edit', 'sync', 'erased') AND (
          EXISTS (SELECT 1 FROM view.page dp
                  WHERE a.target_kind = 2 AND dp.tenant = a.tenant AND dp.deleted
                    AND dp.page_id = CASE WHEN a.target_kind = 2 THEN a.target_id::bigint END)
       OR EXISTS (SELECT 1 FROM view.entity de
                  WHERE a.target_kind = 1 AND de.tenant = a.tenant AND de.id = a.target_id
                    AND de.deleted)))";

/// Reads one page of a feed on a tenant, for a viewer.
pub async fn query<C: PgClient>(
    client: &C,
    tenant: &str,
    feed: &Feed,
    viewer: &Viewer,
) -> Result<Page, String> {
    let built = build(tenant, feed, viewer);
    let refs: Vec<&(dyn ToSql + Sync)> = built
        .params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect();
    let rows = client
        .query(&built.sql, &refs)
        .await
        .map_err(|e| e.to_string())?;
    let limit = feed.limit.clamp(1, MAX_LIMIT);
    let mut out = Vec::with_capacity(rows.len().min(limit));
    for r in rows.iter().take(limit) {
        out.push(row_of(r)?);
    }
    // Hidden usernames (09 §6.3): attributed to nobody for those outside the group, as
    // performers and as the accounts a rights or block event is about.
    let mut keys: Vec<String> = out
        .iter()
        .filter(|r| !r.actor.hidden)
        .filter_map(|r| r.actor.key.clone())
        .collect();
    let about: Vec<String> = out
        .iter()
        .filter(|r| r.target.kind == Some(target_kind::ACTOR))
        .filter_map(|r| r.target.id.clone())
        .collect();
    keys.extend(about.iter().cloned());
    keys.sort();
    keys.dedup();
    let hidden = crate::acl_read::hidden_actors(
        client,
        tenant,
        &keys,
        |g| viewer.reads_in(g),
        scatter_actors::Timestamp(micros(SystemTime::now())),
    )
    .await?;
    // The names of the accounts the events are about, for `User:{name}` titles.
    let names: std::collections::HashMap<String, String> = if about.is_empty() {
        std::collections::HashMap::new()
    } else {
        client
            .query(
                "SELECT actor_key, name FROM view.actor
                 WHERE tenant IN ($1, '') AND actor_key = ANY($2) AND name IS NOT NULL",
                &[&tenant, &about],
            )
            .await
            .map_err(|e| e.to_string())?
            .iter()
            .map(|r| (r.get::<_, String>(0), r.get::<_, String>(1)))
            .collect()
    };
    for r in &mut out {
        if r.actor.key.as_ref().is_some_and(|k| hidden.contains(k)) {
            r.actor.hidden = true;
            r.actor.name = None;
            r.actor.raw = None;
        }
        if r.target.kind == Some(target_kind::ACTOR)
            && let Some(k) = &r.target.id
        {
            // A hidden account's events are titled by its key, which names nobody.
            if !hidden.contains(k)
                && let Some(n) = names.get(k)
            {
                r.target.label = Some((n.clone(), String::new()));
            }
        }
    }
    let next = (rows.len() > limit)
        .then(|| out.last().map(Row::cursor))
        .flatten();
    Ok(Page { rows: out, next })
}

/// Parses a timestamp as MediaWiki's API takes them (compat §2.4): ISO 8601 with a `Z`
/// or an offset, or the 14-digit `YYYYMMDDHHMMSS`. Returns µs since the epoch.
#[must_use]
pub fn parse_timestamp(s: &str) -> Option<u64> {
    let s = s.trim();
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    if s.len() == 14 && digits.len() == 14 {
        return civil(&digits, 0);
    }
    // 2026-09-25T00:43:32Z, 2026-09-25T00:43:32.123Z, 2026-09-25T00:43:32+02:00,
    // 2026-09-25 00:43:32
    let (main, zone) = if let Some(i) = s.find(['Z', 'z']) {
        (&s[..i], 0i64)
    } else if let Some(i) = s.get(10..).unwrap_or("").rfind(['+', '-']) {
        let (m, z) = s.split_at(10 + i);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let zd: Vec<i64> = z[1..].split(':').map(|p| p.parse().unwrap_or(0)).collect();
        let hh = zd.first().copied().unwrap_or(0);
        let mm = zd.get(1).copied().unwrap_or(0);
        (m, sign * (hh * 3600 + mm * 60))
    } else {
        (s, 0)
    };
    let (main, frac) = main.split_once('.').unwrap_or((main, ""));
    let d: String = main.chars().filter(char::is_ascii_digit).collect();
    if d.len() < 8 {
        return None;
    }
    let mut padded = d;
    while padded.len() < 14 {
        padded.push('0');
    }
    let frac_micros: u64 = {
        let mut f: String = frac.chars().filter(char::is_ascii_digit).collect();
        f.truncate(6);
        while f.len() < 6 {
            f.push('0');
        }
        f.parse().unwrap_or(0)
    };
    civil(&padded, -zone).map(|m| m + frac_micros)
}

/// µs since the epoch of `YYYYMMDDHHMMSS`, shifted by `shift` seconds.
fn civil(d: &str, shift: i64) -> Option<u64> {
    let n = |a: usize, b: usize| d.get(a..b)?.parse::<i64>().ok();
    let (y, m, day) = (n(0, 4)?, n(4, 6)?, n(6, 8)?);
    let (hh, mm, ss) = (n(8, 10)?, n(10, 12)?, n(12, 14)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hh * 3600 + mm * 60 + ss + shift;
    u64::try_from(secs).ok().map(|s| s * 1_000_000)
}

/// MediaWiki's 14-digit timestamp from µs since the epoch.
#[must_use]
pub fn mw_timestamp(micros: u64) -> String {
    let iso = iso8601(micros);
    iso.chars().filter(char::is_ascii_digit).collect()
}

/// `YYYY-MM-DDTHH:MM:SSZ` from µs since the epoch.
#[must_use]
pub fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursors_round_trip() {
        let c = Cursor {
            time: 1_700_000_000_000_000,
            partition: 0x1002,
            offset: 42,
        };
        assert_eq!(Cursor::parse(&c.token()), Some(c));
        assert_eq!(Cursor::parse("x"), None);
        assert_eq!(Cursor::parse("1|2"), None);
        assert_eq!(Cursor::parse("1|2|3|4"), None);
    }

    #[test]
    fn timestamps_parse_both_shapes() {
        let t = parse_timestamp("2026-09-25T00:43:32Z").unwrap();
        assert_eq!(iso8601(t), "2026-09-25T00:43:32Z");
        assert_eq!(mw_timestamp(t), "20260925004332");
        assert_eq!(parse_timestamp("20260925004332"), Some(t));
        assert_eq!(parse_timestamp("2026-09-25 00:43:32"), Some(t));
        assert_eq!(
            parse_timestamp("2026-09-25T02:43:32+02:00"),
            Some(t),
            "an offset is applied"
        );
        assert_eq!(parse_timestamp("2026-09-25T00:43:32.5Z"), Some(t + 500_000));
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_timestamp("nope"), None);
    }

    #[test]
    fn flags_from_show() {
        let show = vec!["bot".to_string(), "!minor".to_string()];
        assert_eq!(Flag::from_show(&show, "bot"), Flag::Only);
        assert_eq!(Flag::from_show(&show, "minor"), Flag::Not);
        assert_eq!(Flag::from_show(&show, "new"), Flag::Any);
    }

    #[test]
    fn the_query_parameterizes_every_filter() {
        let mut feed = Feed::everything(50);
        feed.filters.kinds = vec!["edit".into()];
        feed.filters.actor = Some("librarybase:7".into());
        feed.filters.tag = Some("bot".into());
        feed.filters.bot = Flag::Not;
        feed.filters.since = Some(1);
        feed.cursor = Some(Cursor {
            time: 5,
            partition: 2,
            offset: 3,
        });
        let b = build("librarybase", &feed, &Viewer::default());
        assert!(!b.sql.contains("librarybase:7"), "no interpolated values");
        assert!(b.sql.contains("a.kind = ANY($3)"));
        assert!(
            b.sql
                .contains("NOT coalesce((a.bot OR 'bot' = ANY(a.tags)), false)")
        );
        assert!(b.sql.contains("< ($8, $9, $10)"), "{}", b.sql);
        assert!(b.sql.ends_with("LIMIT $11"));
        assert_eq!(b.params.len(), 11);
        feed.direction = Direction::Newer;
        let b = build("librarybase", &feed, &Viewer::default());
        assert!(b.sql.contains("> ($8, $9, $10)"));
        assert!(b.sql.contains("ORDER BY a.time ASC"));
    }

    #[test]
    fn hidden_targets_are_left_out() {
        let feed = Feed::everything(50);
        let none = build("librarybase", &feed, &Viewer::default());
        let mut viewer = Viewer::default();
        viewer.hidden.pages.insert(7);
        viewer.hidden.namespaces.insert(4);
        let b = build("librarybase", &feed, &viewer);
        assert!(
            b.sql
                .contains("NOT ((a.target_kind = 2 AND a.target_id = ANY($4))"),
            "{}",
            b.sql
        );
        assert!(
            b.sql
                .contains("NOT ((a.target_kind = 2 AND p.ns = ANY($5))"),
            "{}",
            b.sql
        );
        assert_eq!(b.params.len(), none.params.len() + 2);
        viewer.hidden.tenant = true;
        let b = build("librarybase", &feed, &viewer);
        assert!(b.sql.contains(" AND false"));
        assert_eq!(b.params.len(), none.params.len());
    }
}
