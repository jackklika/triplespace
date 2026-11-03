//! `view.activity` (step 5; 0010 §13; 0012 §3; 0013 §5.5): one row per change-set record
//! and log event in the tenant, mirror and page partitions. It is the time-ordered index
//! history, contributions and recent changes read.
//!
//! The row's `kind` is `edit` for a local or page change set, `sync` for a mirror one,
//! `job` for a job event, `log` for any other event, and `erased` when the content part
//! is gone. The performer is the attestation's `actor` and nowhere else, so an erased
//! attestation leaves `actor_key` null. Patrol, visibility and read groups (0023 §5–6;
//! 0056 §14) are left at their defaults until those projections exist, except the bits
//! for the parts an adopted revision's source withheld (0035 A14). An adopted revision
//! that kept the source's revision ID shows at its source time (0035 A12).
//!
//! A document page's record (`scatter:v0/page`) is an `edit` row on the page, with the
//! text's length as its size and the difference from the page's previous row as its delta
//! (19 §3.3: a page's history is its activity rows).
//!
//! A thread record (`scatter:v0/thread`) is an `edit` row on the thread's page ID, and the
//! operations that are also log events carry their log type and action and the record's
//! log ID on the same row (16 §6.1), which `list=logevents` reads beside the `log` rows.

use scatter_log::cbor::Value;
use scatter_log::header::{CONFIG_PARTITION, Header};
use scatter_log::record::Record;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_model::id::Subject;

use crate::backend::PgCx;
use crate::common::{
    PAYLOAD_CHANGESET, PAYLOAD_LOGEVENT, PAYLOAD_RECLAIM, attested, comment, content, operation,
    partition_info, target_kind, time_of, to_jsonb,
};
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// `view.activity`.
#[derive(Debug, Default, Clone, Copy)]
pub struct ActivityProjection;

/// The target columns of a row.
#[derive(Debug, Default)]
struct Target {
    kind: Option<i16>,
    id: Option<String>,
    member: Option<String>,
}

/// A row's content-derived columns.
#[derive(Debug, Default)]
struct Columns {
    kind: &'static str,
    target: Target,
    summary_op: Option<String>,
    summary_args: Option<String>,
    new: Option<bool>,
    size: Option<i32>,
    delta: Option<i32>,
    changes: Option<serde_json::Value>,
    log_type: Option<String>,
    log_action: Option<String>,
    params: Option<serde_json::Value>,
    /// The time the row shows, when it is not the record's: an adopted revision's
    /// source time (0035 A12).
    time: Option<u64>,
    minor: Option<bool>,
    bot: Option<bool>,
    /// Visibility bits a record carries itself: the parts an adopted revision's source
    /// withheld (0035 A14), until the `record` ACL projection sets them.
    visibility: i16,
}

/// The visibility bits of 0023 §5, as the Action API reads them.
mod visibility {
    pub const CONTENT: i16 = 1;
    pub const COMMENT: i16 = 2;
    pub const USER: i16 = 4;
    pub const SUPPRESSED: i16 = 8;
}

/// The bits for the parts an adopted revision's source withheld, each restricted to
/// `suppress` (0035 A14).
fn withheld_bits(withheld: &[String]) -> i16 {
    let mut bits = 0;
    for w in withheld {
        bits |= match w.as_str() {
            "content" | "text" => visibility::CONTENT,
            "comment" => visibility::COMMENT,
            "attestation" => visibility::USER,
            _ => 0,
        };
    }
    if bits == 0 {
        0
    } else {
        bits | visibility::SUPPRESSED
    }
}

/// The visibility bits a record carries itself, without any `record` ACL: the parts an
/// adopted revision's source withheld (0035 A14). What retiring a `record` ACL leaves,
/// and what a `record` ACL's bits are added to.
pub(crate) async fn own_visibility(cx: &PgCx, partition: u64, offset: u64) -> Result<i16, String> {
    let slot = match scatter_log_postgres::log::read(cx, partition, offset).await {
        Ok(s) => s,
        Err(scatter_log::store::StoreError::NoOffset { .. }) => return Ok(0),
        Err(e) => return Err(e.to_string()),
    };
    let Some(record) = slot.record() else {
        return Ok(0);
    };
    let h = record.header();
    if h.payload_type == PAYLOAD_CHANGESET {
        let mirror = partition_info(cx, h.partition).await?.is_mirror();
        Ok(changeset_columns(record, mirror)?.visibility)
    } else if h.payload_type == scatter_pages::PAYLOAD_PAGE {
        Ok(page_columns(cx, record).await?.visibility)
    } else {
        Ok(0)
    }
}

fn i32_of(n: u64) -> i32 {
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// A short account of what an `add` or `remove` touched: `claims:P31,P50 labels:en,fr`.
fn summarize(op: &Operation) -> Option<String> {
    let mut parts = Vec::new();
    let list = |name: &str, items: Vec<String>| {
        (!items.is_empty()).then(|| format!("{name}:{}", items.join(",")))
    };
    match op {
        Operation::Add {
            labels,
            descriptions,
            aliases,
            claims,
            sitelinks,
            references,
            qualifiers,
            ..
        } => {
            parts.extend(list(
                "claims",
                claims.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list("labels", labels.keys().cloned().collect()));
            parts.extend(list("descriptions", descriptions.keys().cloned().collect()));
            parts.extend(list("aliases", aliases.keys().cloned().collect()));
            parts.extend(list("sitelinks", sitelinks.keys().cloned().collect()));
            parts.extend(list(
                "references",
                references.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list(
                "qualifiers",
                qualifiers.keys().map(ToString::to_string).collect(),
            ));
        }
        Operation::Remove {
            statements,
            labels,
            descriptions,
            aliases,
            sitelinks,
            references,
            qualifiers,
            link,
            ..
        } => {
            parts.extend(list(
                "statements",
                statements.iter().map(ToString::to_string).collect(),
            ));
            parts.extend(list("labels", labels.clone()));
            parts.extend(list("descriptions", descriptions.clone()));
            parts.extend(list("aliases", aliases.keys().cloned().collect()));
            parts.extend(list("sitelinks", sitelinks.clone()));
            parts.extend(list(
                "references",
                references.keys().map(ToString::to_string).collect(),
            ));
            parts.extend(list(
                "qualifiers",
                qualifiers.keys().map(ToString::to_string).collect(),
            ));
            if let Some(l) = link {
                parts.push(format!("link:{}", l.op()));
            }
        }
        Operation::Override {
            statement,
            rank,
            suppress,
            ..
        } => {
            parts.push(statement.to_string());
            if let Some(r) = rank {
                parts.push(format!("rank:{}", serde_json::to_value(r).ok()?.as_str()?));
            }
            if suppress.is_some() {
                parts.push("suppress".to_string());
            }
        }
        Operation::Retain { policy, .. } => {
            parts.push(serde_json::to_value(policy).ok()?.as_str()?.to_string());
        }
        Operation::Redirect { to, .. } => parts.push(format!("to:{to}")),
        Operation::Convert { local, .. } => parts.push(format!("local:{local}")),
        Operation::SameAs { ids, .. }
        | Operation::DifferentFrom { ids, .. }
        | Operation::EquivalentProperty { ids, .. } => {
            parts.push(
                ids.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        _ => {}
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn changeset_columns(record: &Record, mirror: bool) -> Result<Columns, String> {
    let Some(op) = operation(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let target = match op.subject() {
        Some(Subject::Entity(id)) => Target {
            kind: Some(target_kind::ENTITY),
            id: Some(id.to_string()),
            member: None,
        },
        Some(Subject::Page(p)) => Target {
            kind: Some(target_kind::PAGE),
            id: Some(p.to_string()),
            member: None,
        },
        None => Target::default(),
    };
    let content_len = record
        .body()
        .content()
        .bytes()
        .map(|b| i32_of(b.len() as u64));
    let (size, delta, changes) = match &op {
        Operation::Put {
            size,
            prev_size,
            changes,
            ..
        } => {
            let size = size.map(i32_of).or(content_len);
            let delta = match (size, prev_size) {
                (Some(s), Some(p)) => Some(s.saturating_sub(i32_of(*p))),
                _ => None,
            };
            let changes = changes
                .as_ref()
                .map(|c| serde_json::to_value(c).map_err(|e| e.to_string()))
                .transpose()?;
            (size, delta, changes)
        }
        _ => (content_len, None, None),
    };
    // An adopted revision that kept the source's revision ID is history: it shows at its
    // source time, `new` only on the first revision adopted, with its minor flag and the
    // bot flag, and with the source's edit summary as the source showed it, with no
    // automatic summary of the `adopt` before it (0035 A12). A `--current-only` record is
    // the job's own revision, summarized as `adopt`.
    let history = matches!(&op, Operation::Adopt { source_revid, .. }
        if record.header().revid == Some(*source_revid));
    let (new, time, minor, bot, visibility) = match &op {
        Operation::Adopt {
            source_parentid,
            source_time,
            minor,
            withheld,
            ..
        } => (
            Some(!history || source_parentid.is_none()),
            history
                .then(|| crate::feed::parse_timestamp(source_time))
                .flatten(),
            Some(*minor),
            Some(true),
            withheld_bits(withheld),
        ),
        Operation::Create { .. } => (Some(true), None, None, None, 0),
        _ => (Some(false), None, None, None, 0),
    };
    Ok(Columns {
        kind: if mirror { "sync" } else { "edit" },
        target,
        summary_op: (!history).then(|| op.name().to_string()),
        summary_args: if history { None } else { summarize(&op) },
        new,
        size,
        delta,
        changes,
        time,
        minor,
        bot,
        visibility,
        ..Columns::default()
    })
}

/// A page record as an activity row: an `edit` on the page, and for a `move` also the
/// `move/move` log event (10 §2.5; 16 §2.3), whose params carry the new title as
/// MediaWiki's do (`target_ns`, `target_title`, `suppressredirect`, with the catalogue's
/// `noredirect` beside it), the old title under
/// `ts-from-ns` and `ts-from-title`, and the ID of the redirect left under `ts-redirect`.
/// The old title is the one the page's previous `create` or `move` record set: read from
/// the page's own history, since the move record names only where the page went.
async fn page_columns(cx: &PgCx, record: &Record) -> Result<Columns, String> {
    let page = crate::pages::PageRecord::read(record)?;
    let Some(op) = page.op else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let h = record.header();
    // A page's first record is its `create`, adopted or not.
    let new = matches!(op, scatter_pages::PageOp::Create { .. });
    let (log, params) = match &op {
        scatter_pages::PageOp::Move {
            ns,
            title,
            redirect,
        } => {
            let from = title_before(cx, record).await?;
            let prefixed =
                |ns: i32, title: &str| match scatter_titles::Namespaces::embedded().get(ns) {
                    Some(n) if ns != 0 => {
                        format!("{}:{title}", n.name.as_ref().unwrap_or(&n.canonical))
                    }
                    _ => title.to_string(),
                };
            let mut params = serde_json::json!({
                "target_ns": ns,
                "target_title": prefixed(*ns, title),
            });
            if redirect.is_none() {
                // The catalogue's name (16 §2.3) and MediaWiki's, for clients that read
                // the latter.
                params["noredirect"] = serde_json::Value::Bool(true);
                params["suppressredirect"] = serde_json::Value::Bool(true);
            } else {
                params["ts-redirect"] = serde_json::json!(redirect);
            }
            if let Some((fns, ftitle)) = from {
                params["ts-from-ns"] = serde_json::json!(fns);
                params["ts-from-title"] = serde_json::json!(prefixed(fns, &ftitle));
            }
            (Some(("move", "move")), Some(params))
        }
        _ => (None, None),
    };
    Ok(Columns {
        kind: "edit",
        target: Target {
            kind: Some(target_kind::PAGE),
            id: h.page_id.map(|p| p.to_string()),
            member: None,
        },
        // A page revision's comment is the editor's own, as MediaWiki stores it (an adopted
        // one round-trips); only a move carries an automatic summary.
        summary_op: matches!(op, scatter_pages::PageOp::Move { .. }).then(|| op.name().to_string()),
        new: Some(new),
        size: page.text.as_ref().map(|t| i32_of(t.len() as u64)),
        time: Some(crate::pages::shown_time(h, &op)),
        minor: Some(op.minor()),
        bot: op.source().map(|_| true),
        visibility: op.source().map_or(0, |s| withheld_bits(&s.withheld)),
        log_type: log.map(|(t, _)| t.to_string()),
        log_action: log.map(|(_, a)| a.to_string()),
        params,
        ..Columns::default()
    })
}

/// The title a page had before one of its records: what its latest `create` or `move`
/// before that record set, or nothing for a page whose earlier records are erased.
async fn title_before(cx: &PgCx, record: &Record) -> Result<Option<(i32, String)>, String> {
    let h = record.header();
    let Some(key) = h.key.as_deref() else {
        return Ok(None);
    };
    let earlier = scatter_log_postgres::log::read_by_key(cx, h.partition, key)
        .await
        .map_err(|e| e.to_string())?;
    for (offset, r) in earlier.into_iter().rev() {
        if offset >= h.offset {
            continue;
        }
        if let Some(
            scatter_pages::PageOp::Create { ns, title, .. }
            | scatter_pages::PageOp::Move { ns, title, .. },
        ) = crate::pages::PageRecord::read(&r)?.op
        {
            return Ok(Some((ns, title)));
        }
    }
    Ok(None)
}

/// A thread record as an activity row (16 §6.1): an `edit` on the thread's page ID with
/// the operation as the parsed summary, and for the operations that are also log events
/// their type and action (16 §2.3): `create/create`, `move/move` for a rename or a move,
/// `thread/attach`, `thread/detach`, `thread/pin`, `thread/unpin`. The params name what
/// the event touched, with the talk page IDs the record minted or found. A record whose
/// content part is erased is an `erased` row.
fn thread_columns(record: &Record) -> Result<Columns, String> {
    use scatter_threads::{Target as ThreadTarget, ThreadOp};
    let Some(rec) = crate::threads::read_record(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let h = record.header();
    let minted = crate::threads::minted(record);
    let target = |t: &ThreadTarget| match t {
        ThreadTarget::Page(id) => serde_json::json!({"kind": "page", "id": id}),
        ThreadTarget::Actor(key) => serde_json::json!({"kind": "actor", "id": key}),
    };
    let arg = |t: &ThreadTarget| match t {
        ThreadTarget::Page(id) => format!("page:{id}"),
        ThreadTarget::Actor(key) => format!("actor:{key}"),
    };
    let (args, log, params) = match &rec.op {
        ThreadOp::Create {
            name,
            target: t,
            also,
            pinned,
            ..
        } => (
            Some(name.clone()),
            Some(("create", "create")),
            Some(serde_json::json!({
                "subject": name,
                "target": target(t),
                "talk": minted.talk,
                "also": also.iter().map(target).collect::<Vec<_>>(),
                "also_talk": minted.also,
                "pinned": pinned,
            })),
        ),
        ThreadOp::Rename { name } => (
            Some(name.clone()),
            Some(("move", "move")),
            Some(serde_json::json!({"subject": name})),
        ),
        ThreadOp::Move { target: t, keep } => (
            Some(arg(t)),
            Some(("move", "move")),
            Some(serde_json::json!({"target": target(t), "talk": minted.talk, "keep": keep})),
        ),
        ThreadOp::Attach { target: t }
        | ThreadOp::Detach { target: t }
        | ThreadOp::Pin { target: t }
        | ThreadOp::Unpin { target: t } => (
            Some(arg(t)),
            Some(("thread", rec.op.name())),
            Some(serde_json::json!({"target": target(t), "talk": minted.talk})),
        ),
        ThreadOp::Post { .. } | ThreadOp::Edit { .. } => (None, None, None),
    };
    Ok(Columns {
        kind: "edit",
        target: Target {
            kind: Some(target_kind::PAGE),
            id: h.page_id.map(|p| p.to_string()),
            member: None,
        },
        summary_op: Some(rec.op.name().to_string()),
        summary_args: args,
        new: Some(matches!(rec.op, ThreadOp::Create { .. })),
        size: crate::threads::text(record).map(|t| i32_of(t.len() as u64)),
        log_type: log.map(|(t, _)| t.to_string()),
        log_action: log.map(|(_, a)| a.to_string()),
        params,
        ..Columns::default()
    })
}

/// A thread's move names the talk page it left as well as the one it went to, so the
/// event appears in both pages' histories (14 §1.3; 16 §6.1): `from_talk`, the home the
/// thread's previous `create` or `move` row set. Those rows are earlier in the same
/// partition, so a replay finds them as the first projection did.
async fn moved_from(
    cx: &PgCx,
    tenant: &str,
    partition: u64,
    offset: u64,
    c: &mut Columns,
) -> Result<(), String> {
    let is_move = c.log_type.as_deref() == Some("move")
        && c.params.as_ref().is_some_and(|p| p.get("target").is_some());
    if !is_move {
        return Ok(());
    }
    let row = cx
        .query_opt(
            "SELECT params->'talk' FROM view.activity
             WHERE tenant = $1 AND partition = $2 AND target_kind = $3 AND target_id = $4
               AND \"offset\" < $5
               AND ((log_type = 'create' AND log_action = 'create')
                    OR (log_type = 'move' AND params ? 'target'))
             ORDER BY \"offset\" DESC LIMIT 1",
            &[
                &tenant,
                &scatter_log_postgres::ids::partition_to_db(partition),
                &target_kind::PAGE,
                &c.target.id,
                &i64::try_from(offset).map_err(|e| e.to_string())?,
            ],
        )
        .await
        .map_err(|e| sql(&e))?;
    if let (Some(r), Some(params)) = (row, c.params.as_mut()) {
        let from: Option<serde_json::Value> = r.get(0);
        if let Some(f) = from.filter(|f| !f.is_null()) {
            params["from_talk"] = f;
        }
    }
    Ok(())
}

fn logevent_columns(record: &Record) -> Result<Columns, String> {
    let Some(event) = content(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let log_type = event
        .get("type")
        .and_then(Value::as_text)
        .map(str::to_owned);
    let log_action = event
        .get("action")
        .and_then(Value::as_text)
        .map(str::to_owned);
    let target = match event.get("target") {
        Some(t) => {
            let kind = t.get("kind").and_then(Value::as_text);
            match kind {
                Some("entity") => Target {
                    kind: Some(target_kind::ENTITY),
                    id: t.get("id").and_then(Value::as_text).map(str::to_owned),
                    member: None,
                },
                Some("page") => Target {
                    kind: Some(target_kind::PAGE),
                    id: t.get("id").and_then(Value::as_u64).map(|n| n.to_string()),
                    member: None,
                },
                Some("actor") => Target {
                    kind: Some(target_kind::ACTOR),
                    id: t.get("key").and_then(Value::as_text).map(str::to_owned),
                    member: None,
                },
                Some("job") => Target {
                    kind: Some(target_kind::JOB),
                    id: t.get("id").and_then(Value::as_u64).map(|n| n.to_string()),
                    member: None,
                },
                Some("record") => Target {
                    kind: Some(target_kind::RECORD),
                    id: match (
                        t.get("partition").and_then(Value::as_u64),
                        t.get("offset").and_then(Value::as_u64),
                    ) {
                        (Some(p), Some(o)) => Some(format!("{p}:{o}")),
                        _ => None,
                    },
                    member: t
                        .get("revid")
                        .and_then(Value::as_u64)
                        .map(|r| r.to_string()),
                },
                _ => Target::default(),
            }
        }
        None => Target::default(),
    };
    Ok(Columns {
        kind: if log_type.as_deref() == Some("job") {
            "job"
        } else {
            "log"
        },
        target,
        log_type,
        log_action,
        params: event.get("params").map(to_jsonb),
        ..Columns::default()
    })
}

/// A `scatter:v0/reclaim` record as `reclaim/reclaim` on the reclaimed account, with its
/// `method` (0088 §8).
fn reclaim_columns(record: &Record) -> Result<Columns, String> {
    let Some(content) = content(record)? else {
        return Ok(Columns {
            kind: "erased",
            ..Columns::default()
        });
    };
    let method = content
        .get("method")
        .and_then(Value::as_text)
        .unwrap_or("manual")
        .to_string();
    Ok(Columns {
        kind: "log",
        target: Target {
            kind: Some(target_kind::ACTOR),
            id: record.header().key.clone(),
            member: None,
        },
        log_type: Some("reclaim".into()),
        log_action: Some("reclaim".into()),
        params: Some(serde_json::json!({"method": method})),
        ..Columns::default()
    })
}

/// The log type and action of an ACL record: see [`acl_columns`]. `marked` is the
/// writer's suppression mark; `event` says a `record` target is a log event rather than a
/// revision.
fn acl_event(
    target: &scatter_actors::acl::Target,
    acl: Option<&scatter_actors::acl::Acl>,
    previous: Option<&scatter_actors::acl::Acl>,
    marked: bool,
    event: bool,
) -> (&'static str, &'static str) {
    use scatter_actors::acl::{Acl, ReadKind, Target as AclTarget};
    let moderation = |a: &Acl| {
        a.restrictions.contains_key(scatter_actors::acl::READ)
            && a.read_kind == Some(ReadKind::Moderation)
    };
    let suppressed = |a: &Acl| {
        marked
            || a.restrictions
                .get(scatter_actors::acl::READ)
                .is_some_and(|r| r.group.is_none())
    };
    // What the event is about: the ACL written, or for a retirement the one retired.
    let about = acl.or(previous);
    match (target, acl, about) {
        (AclTarget::Actor(_), _, _) => (
            "suppress",
            if acl.is_some() {
                "hide-user"
            } else {
                "unhide-user"
            },
        ),
        (AclTarget::Record { .. }, _, Some(a)) => (
            if suppressed(a) { "suppress" } else { "delete" },
            if event { "event" } else { "revision" },
        ),
        (_, Some(a), _) if moderation(a) => {
            (if suppressed(a) { "suppress" } else { "delete" }, "delete")
        }
        (_, None, Some(a)) if moderation(a) => ("delete", "restore"),
        (_, Some(_), _) if previous.is_some() => ("protect", "modify"),
        (_, Some(_), _) => ("protect", "protect"),
        (_, None, _) => ("protect", "unprotect"),
    }
}

/// An ACL record as the log event of 09 §4.3: `protect/protect`, `protect/modify` or
/// `protect/unprotect`; `delete/delete` or `delete/restore`; `delete/revision` for a
/// `record` target; `suppress/…` when the writer marked the record a suppression; and
/// `suppress/hide-user` for an actor. The previous record under the key tells a change
/// from a first restriction, and what a retirement undid.
async fn acl_columns(cx: &PgCx, record: &Record) -> Result<Columns, String> {
    use scatter_actors::acl::{Acl, Target as AclTarget};
    let h = record.header();
    let key = h
        .key
        .as_deref()
        .ok_or("an acl record is keyed by its target")?;
    let target = AclTarget::parse(key).map_err(|e| e.to_string())?;
    let decode = |r: &Record| -> Result<Option<Acl>, String> {
        match content(r)? {
            Some(v) => serde_json::from_value(to_jsonb(&v))
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    };
    let acl = decode(record)?;
    let previous = scatter_log_postgres::log::read_by_key(cx, h.partition, key)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .rev()
        .find(|(o, _)| *o < h.offset)
        .map(|(_, r)| decode(&r))
        .transpose()?
        .flatten();
    let about = acl.as_ref().or(previous.as_ref());
    let (log_type, log_action) = acl_event(
        &target,
        acl.as_ref(),
        previous.as_ref(),
        crate::moderation::marked_suppression(record),
        match &target {
            AclTarget::Record { partition, offset } => is_event(cx, *partition, *offset).await?,
            _ => false,
        },
    );
    let target_columns = match &target {
        AclTarget::Page(id) => Target {
            kind: Some(target_kind::PAGE),
            id: Some(id.to_string()),
            member: None,
        },
        AclTarget::Entity(id) => Target {
            kind: Some(target_kind::ENTITY),
            id: Some(id.clone()),
            member: None,
        },
        AclTarget::Actor(key) => Target {
            kind: Some(target_kind::ACTOR),
            id: Some(key.to_string()),
            member: None,
        },
        AclTarget::Record { partition, offset } => Target {
            kind: Some(target_kind::RECORD),
            id: Some(format!("{partition}:{offset}")),
            member: None,
        },
        other => Target {
            kind: None,
            id: Some(other.key()),
            member: None,
        },
    };
    let mut params = serde_json::json!({
        "restrictions": acl.as_ref().map(|a| &a.restrictions),
        "previous": previous.as_ref().map(|a| &a.restrictions),
        "parts": about.map(|a| &a.parts),
        "talk": about.is_some_and(|a| a.talk),
    });
    // A revision-deletion event names its record; what the record was about goes in `of`,
    // so that the event is restricted with that page or entity (09 §6.5, §5.3).
    if let AclTarget::Record { partition, offset } = &target
        && let Some(r) = cx
            .query_opt(
                "SELECT target_kind, target_id FROM view.activity
                 WHERE partition = $1 AND \"offset\" = $2",
                &[
                    &scatter_log_postgres::ids::partition_to_db(*partition),
                    &i64::try_from(*offset).unwrap_or(i64::MAX),
                ],
            )
            .await
            .map_err(|e| sql(&e))?
        && let (Some(kind), Some(id)) = (r.get::<_, Option<i16>>(0), r.get::<_, Option<String>>(1))
    {
        params["of"] = serde_json::json!({"kind": kind, "id": id});
    }
    // A restore names the revisions that stay hidden under `record` ACLs, written before
    // it in a partial undeletion (09 §4.3).
    if let AclTarget::Page(page) = &target
        && (log_type, log_action) == ("delete", "restore")
    {
        params["kept"] = serde_json::json!(kept_hidden(cx, h.partition, *page).await?);
    }
    Ok(Columns {
        kind: "log",
        target: target_columns,
        log_type: Some(log_type.into()),
        log_action: Some(log_action.into()),
        params: Some(params),
        ..Columns::default()
    })
}

/// Whether a record is a log event and not a revision (a block, a membership, an ACL), so
/// that a `record` ACL on it is `delete/event` rather than `delete/revision` (09 §4.3).
async fn is_event(cx: &PgCx, partition: u64, offset: u64) -> Result<bool, String> {
    match scatter_log_postgres::log::read(cx, partition, offset).await {
        Ok(slot) => Ok(slot
            .record()
            .is_some_and(|r| r.header().revid.is_none() && r.header().logid.is_some())),
        Err(scatter_log::store::StoreError::NoOffset { .. }) => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}

/// The revisions of a page whose rows carry hidden parts: what a restore leaves under
/// `record` ACLs.
async fn kept_hidden(cx: &PgCx, partition: u64, page: u64) -> Result<Vec<i64>, String> {
    let info = partition_info(cx, partition).await?;
    Ok(cx
        .query(
            "SELECT revid FROM view.activity
             WHERE tenant = $1 AND target_kind = $2 AND target_id = $3
               AND revid IS NOT NULL AND visibility & 7 <> 0
             ORDER BY revid",
            &[&info.tenant, &target_kind::PAGE, &page.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?
        .iter()
        .map(|r| r.get::<_, i64>(0))
        .collect())
}

/// A membership record as `rights/rights` (09 §3.3): public, on the account, with the
/// group, whether it was added or removed, and the expiry in `params`.
fn membership_columns(record: &Record) -> Result<Columns, String> {
    let h = record.header();
    let key = h
        .key
        .as_deref()
        .ok_or("a membership record is keyed by its actor")?;
    let params = match content(record)? {
        Some(v) => {
            let m: scatter_actors::membership::Membership =
                scatter_log::cbor::from_value(v).map_err(|e| e.to_string())?;
            serde_json::json!({
                "group": m.group,
                "action": match m.action {
                    scatter_actors::membership::MembershipAction::Add => "add",
                    scatter_actors::membership::MembershipAction::Remove => "remove",
                },
                "expires": m.expires.map(|t| t.0),
            })
        }
        None => serde_json::Value::Null,
    };
    Ok(Columns {
        kind: "log",
        target: Target {
            kind: Some(target_kind::ACTOR),
            id: Some(key.to_string()),
            member: None,
        },
        log_type: Some("rights".into()),
        log_action: Some("rights".into()),
        params: Some(params),
        ..Columns::default()
    })
}

/// A block record as `block/block`, `block/reblock` (a block where the previous record
/// under the key is one too) or `block/unblock` (09 §3.3), on the account, with what it
/// removes and its expiry in `params`. Public, as MediaWiki's block log is.
async fn block_columns(cx: &PgCx, record: &Record) -> Result<Columns, String> {
    use scatter_actors::membership::{Block, BlockScope};
    let h = record.header();
    let key = h
        .key
        .as_deref()
        .ok_or("a block record is keyed by its actor")?;
    let decode = |r: &Record| -> Result<Option<Block>, String> {
        match content(r)? {
            Some(v) => scatter_log::cbor::from_value(v)
                .map(Some)
                .map_err(|e| e.to_string()),
            None => Ok(None),
        }
    };
    let block = decode(record)?;
    // The account's other actor records (its actor record, memberships) share the key.
    let previous = scatter_log_postgres::log::read_by_key(cx, h.partition, key)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .rev()
        .find(|(o, r)| *o < h.offset && r.header().payload_type == crate::actors::payload::BLOCK)
        .map(|(_, r)| decode(&r))
        .transpose()?
        .flatten();
    let (action, params) = match &block {
        Some(Block::Block { removes, expires }) => (
            if matches!(previous, Some(Block::Block { .. })) {
                "reblock"
            } else {
                "block"
            },
            serde_json::json!({
                "removes": match removes {
                    BlockScope::AllButRead => serde_json::json!("all-but-read"),
                    BlockScope::Permissions(p) => serde_json::json!(p),
                },
                "expires": expires.map(|t| t.0),
            }),
        ),
        _ => ("unblock", serde_json::Value::Null),
    };
    Ok(Columns {
        kind: "log",
        target: Target {
            kind: Some(target_kind::ACTOR),
            id: Some(key.to_string()),
            member: None,
        },
        log_type: Some("block".into()),
        log_action: Some(action.into()),
        params: Some(params),
        ..Columns::default()
    })
}

/// The size of a page's row before a revision, 0 for its first.
async fn previous_size(
    cx: &PgCx,
    tenant: &str,
    page: Option<&String>,
    revid: u64,
) -> Result<i32, String> {
    let row = cx
        .query_opt(
            "SELECT size FROM view.activity WHERE tenant = $1 AND target_kind = $2 AND target_id = $3
               AND revid < $4 AND size IS NOT NULL
             ORDER BY revid DESC LIMIT 1",
            &[
                &tenant,
                &target_kind::PAGE,
                &page,
                &i64::try_from(revid).unwrap_or(i64::MAX),
            ],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.and_then(|r| r.get::<_, Option<i32>>(0)).unwrap_or(0))
}

impl<B: Backend<Cx = PgCx>> Projection<B> for ActivityProjection {
    fn name(&self) -> &'static str {
        "activity"
    }

    fn step(&self) -> Step {
        Step::Activity
    }

    fn accepts(&self, header: &Header) -> bool {
        header.partition != CONFIG_PARTITION
            && (header.payload_type == PAYLOAD_CHANGESET
                || header.payload_type == PAYLOAD_LOGEVENT
                || header.payload_type == PAYLOAD_RECLAIM
                || header.payload_type == scatter_pages::PAYLOAD_PAGE
                || header.payload_type == scatter_threads::PAYLOAD_THREAD
                || header.payload_type == crate::actors::payload::ACL
                || header.payload_type == crate::actors::payload::MEMBERSHIP
                || header.payload_type == crate::actors::payload::BLOCK)
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            let mut columns = if h.payload_type == PAYLOAD_CHANGESET {
                changeset_columns(record, info.is_mirror())?
            } else if h.payload_type == scatter_pages::PAYLOAD_PAGE {
                page_columns(cx, record).await?
            } else if h.payload_type == PAYLOAD_RECLAIM {
                reclaim_columns(record)?
            } else if h.payload_type == scatter_threads::PAYLOAD_THREAD {
                let mut c = thread_columns(record)?;
                moved_from(cx, &info.tenant, h.partition, h.offset, &mut c).await?;
                c
            } else if h.payload_type == crate::actors::payload::ACL {
                acl_columns(cx, record).await?
            } else if h.payload_type == crate::actors::payload::MEMBERSHIP {
                membership_columns(record)?
            } else if h.payload_type == crate::actors::payload::BLOCK {
                block_columns(cx, record).await?
            } else {
                logevent_columns(record)?
            };
            if h.payload_type == scatter_pages::PAYLOAD_PAGE
                && let (Some(size), Some(revid)) = (columns.size, h.revid)
            {
                columns.delta = Some(
                    size - previous_size(cx, &info.tenant, columns.target.id.as_ref(), revid)
                        .await?,
                );
            }
            let attested = attested(record);
            let as_i64 = |n: Option<u64>| {
                n.map(i64::try_from)
                    .transpose()
                    .map_err(|e: std::num::TryFromIntError| e.to_string())
            };
            let n = cx.execute(
                    "INSERT INTO view.activity
                       (tenant, time, partition, \"offset\", kind, source, revid, logid, target_kind, target_id, target_member,
                        actor_key, job_id, summary_op, summary_args, comment, tags, new, size, delta,
                        log_type, log_action, params, changes, minor, bot, visibility)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26, $27)
                     ON CONFLICT (tenant, partition, \"offset\") DO UPDATE SET
                       kind = EXCLUDED.kind, target_kind = EXCLUDED.target_kind, target_id = EXCLUDED.target_id,
                       target_member = EXCLUDED.target_member, actor_key = EXCLUDED.actor_key, job_id = EXCLUDED.job_id,
                       summary_op = EXCLUDED.summary_op, summary_args = EXCLUDED.summary_args, comment = EXCLUDED.comment,
                       tags = EXCLUDED.tags, new = EXCLUDED.new, size = EXCLUDED.size, delta = EXCLUDED.delta,
                       log_type = EXCLUDED.log_type, log_action = EXCLUDED.log_action, params = EXCLUDED.params,
                       changes = EXCLUDED.changes, minor = EXCLUDED.minor, bot = EXCLUDED.bot,
                       visibility = view.activity.visibility | EXCLUDED.visibility",
                    &[
                        &info.tenant,
                        &time_of(columns.time.unwrap_or(h.appended_at)),
                        &scatter_log_postgres::ids::partition_to_db(h.partition),
                        &as_i64(Some(h.offset))?,
                        &columns.kind,
                        &info.name,
                        &as_i64(h.revid)?,
                        &as_i64(h.logid)?,
                        &columns.target.kind,
                        &columns.target.id,
                        &columns.target.member,
                        &attested.actor,
                        &as_i64(attested.job)?,
                        &columns.summary_op,
                        &columns.summary_args,
                        &comment(record),
                        &attested.tags,
                        &columns.new,
                        &columns.size,
                        &columns.delta,
                        &columns.log_type,
                        &columns.log_action,
                        &columns.params,
                        &columns.changes,
                        &columns.minor,
                        &columns.bot,
                        &columns.visibility,
                    ],
                )
                .await
                .map_err(|e| sql(&e))?;
            Ok(Applied::rows(usize::try_from(n).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            cx.execute(
                "DELETE FROM view.activity WHERE tenant = $1 AND partition = $2",
                &[
                    &info.tenant,
                    &scatter_log_postgres::ids::partition_to_db(partition),
                ],
            )
            .await
            .map(|_| ())
            .map_err(|e| sql(&e))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn summaries() {
        let op: Operation = serde_json::from_value(json!({"op": "add", "id": "Q1",
            "labels": {"en": "x", "fr": "y"},
            "claims": {"P31": [{"mainsnak": {"snaktype": "somevalue", "property": "P31"}, "type": "statement", "rank": "normal"}]}}))
        .unwrap();
        assert_eq!(summarize(&op).as_deref(), Some("claims:P31 labels:en,fr"));
        let op: Operation = serde_json::from_value(json!({"op": "override", "id": "OAW1",
            "statement": "OAW1$2F1C0000-0000-0000-0000-000000000000", "rank": "deprecated"}))
        .unwrap();
        assert_eq!(
            summarize(&op).as_deref(),
            Some("OAW1$2F1C0000-0000-0000-0000-000000000000 rank:deprecated")
        );
        let op: Operation =
            serde_json::from_value(json!({"op": "retain", "id": "WDQ1", "policy": "orphan"}))
                .unwrap();
        assert_eq!(summarize(&op).as_deref(), Some("orphan"));
        let op: Operation = serde_json::from_value(
            json!({"op": "tombstone", "id": "WDQ1", "upstream": {"revid": 1}}),
        )
        .unwrap();
        assert_eq!(summarize(&op), None);
    }
}
