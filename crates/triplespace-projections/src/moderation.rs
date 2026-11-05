//! What an ACL does to the views (0023 §4–§5; 09 §6.2–§6.3), after [`crate::actors::AclProjection`]
//! has written `view.acl`:
//!
//! - **A page or thread** with a moderation `read` restriction is deleted:
//!   `view.page.deleted` is set, so it leaves the title index, listings and search, and its
//!   title can be taken. With `talk: true` its talk page goes with it. Retiring the ACL
//!   clears both, the talk page only where no ACL of its own still deletes it.
//! - **A local entity** likewise sets `view.entity.deleted`.
//! - **A record** with a `record` ACL gets the visibility bits of 0023 §5 on its
//!   `view.activity` row: content (or a thread's or page's `text`), comment, attestation,
//!   and suppressed when the writer marked the record a suppression. The bits are the
//!   ACL's together with those the record carries itself (an adopted revision's withheld
//!   parts, 0035 A14), so retiring the ACL leaves the record's own.
//! - **A reserved title** (create-protection, 09 §4.8): an ACL carrying `reserved` holds
//!   the title with a `view.page` row that has no revision (`reserved`, `latest_offset`
//!   NULL), so the title index refuses any other page there and a `create` takes the
//!   reserved ID. Retiring the ACL before the page exists frees the title.
//!
//! **Suppression** is a restriction naming the tenant's `moderation.suppress_group`, a
//! setting a projection does not resolve; the ACL writer resolves it and marks the record
//! (`suppress: true` in the attestation map, [`marked_suppression`]).
//!
//! The log event each ACL record projects as is [`crate::activity`]'s. The cache purges and
//! search re-sends of 09 §6.3 wait for those subsystems.

use scatter_actors::acl::{Acl, ReadKind, Target};
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_log_postgres::PgClient;
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};

use scatter_log::cbor::Value;

use crate::actors::payload;
use crate::backend::PgCx;
use crate::common::{attestation, content, tenant_of, to_jsonb};

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// Whether the ACL writer marked the record a suppression: `suppress: true` in its
/// attestation map. For a retirement, whether what it retired was one.
#[must_use]
pub fn marked_suppression(record: &Record) -> bool {
    attestation(record)
        .as_ref()
        .and_then(|a| a.get("suppress"))
        .is_some_and(|v| matches!(v, Value::Bool(true)))
}

/// The visibility bits, as MediaWiki numbers them.
mod bits {
    pub const CONTENT: i16 = 1;
    pub const COMMENT: i16 = 2;
    pub const USER: i16 = 4;
    pub const SUPPRESSED: i16 = 8;
}

/// The bits a `record` ACL sets; `suppressed` is [`marked_suppression`] of its record.
#[must_use]
pub fn visibility_bits(acl: &Acl, suppressed: bool) -> i16 {
    let Some(read) = acl.restrictions.get(scatter_actors::acl::READ) else {
        return 0;
    };
    let mut v = 0;
    if acl.parts.is_empty() {
        v = bits::CONTENT | bits::COMMENT | bits::USER;
    }
    for p in &acl.parts {
        v |= match p.as_str() {
            "content" | "text" => bits::CONTENT,
            "comment" => bits::COMMENT,
            "attestation" => bits::USER,
            _ => 0,
        };
    }
    if suppressed || read.group.is_none() {
        v |= bits::SUPPRESSED;
    }
    v
}

fn deletes(acl: &Acl) -> bool {
    acl.restrictions.contains_key(scatter_actors::acl::READ)
        && acl.read_kind == Some(ReadKind::Moderation)
}

/// The views an ACL changes.
#[derive(Debug, Default, Clone, Copy)]
pub struct ModerationProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for ModerationProjection {
    fn name(&self) -> &'static str {
        "moderation"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == payload::ACL
    }

    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let tenant = tenant_of(cx, h.partition).await?;
            let key = h
                .key
                .as_deref()
                .ok_or("an acl record is keyed by its target")?;
            let target = Target::parse(key).map_err(|e| e.to_string())?;
            let acl: Option<Acl> = match content(record)? {
                Some(v) => Some(serde_json::from_value(to_jsonb(&v)).map_err(|e| e.to_string())?),
                None => None,
            };
            let rows = match &target {
                Target::Page(id) => page(cx, &tenant, *id, acl.as_ref()).await?,
                Target::Entity(id) => {
                    let deleted = acl.as_ref().is_some_and(deletes);
                    cx.execute(
                        "UPDATE view.entity SET deleted = $3 WHERE tenant = $1 AND id = $2",
                        &[&tenant, id, &deleted],
                    )
                    .await
                    .map_err(|e| sql(&e))?
                }
                Target::Record { partition, offset } => {
                    let own = crate::activity::own_visibility(cx, *partition, *offset).await?;
                    let suppressed = marked_suppression(record);
                    let v = own | acl.as_ref().map_or(0, |a| visibility_bits(a, suppressed));
                    let p = scatter_log_postgres::ids::partition_to_db(*partition);
                    let o = i64::try_from(*offset).map_err(|e| e.to_string())?;
                    cx.execute(
                        "UPDATE view.activity SET visibility = $4
                         WHERE tenant = $1 AND partition = $2 AND \"offset\" = $3",
                        &[&tenant, &p, &o, &v],
                    )
                    .await
                    .map_err(|e| sql(&e))?
                }
                _ => 0,
            };
            // What the ACL makes stale (03 §9.6; 0056 §7): every ACL record is a read
            // restriction written, changed or retired, or a protection, which purges
            // nothing. The hidden set is decided per request, so the purge is for the
            // layers that hold public forms: a proxy's tags, and the tenant epoch.
            let reads = acl.as_ref().is_some_and(|a| {
                a.restrictions.contains_key(scatter_actors::acl::READ) || a.confidential.is_some()
            });
            let read_before = read_restricted_before(cx, h.partition, key, h.offset).await?;
            if reads || read_before {
                purge(cx, &tenant, &target, record).await?;
            }
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(0)))
        })
    }

    fn reset<'a>(&'a self, _: &'a mut PgCx, _: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

/// Whether the previous ACL record under the key restricted `read`: a retirement or a
/// change of one is a visibility change too.
async fn read_restricted_before(
    cx: &PgCx,
    partition: u64,
    key: &str,
    offset: u64,
) -> Result<bool, String> {
    let previous = scatter_log_postgres::log::read_by_key(cx, partition, key)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .rev()
        .find(|(o, _)| *o < offset)
        .map(|(_, r)| r);
    let Some(r) = previous else {
        return Ok(false);
    };
    Ok(match content(&r)? {
        Some(v) => {
            let j = to_jsonb(&v);
            j.get("restrictions").and_then(|r| r.get("read")).is_some()
                || j.get("confidential").is_some()
        }
        None => false,
    })
}

/// Queues the purge a read ACL on `target` calls for (03 §9.6): the target's own tag, and
/// for a namespace or the tenant a bump of the tenant's visibility epoch beside a tag for
/// the whole space; for a set, its members' tags.
async fn purge(cx: &PgCx, tenant: &str, target: &Target, record: &Record) -> Result<(), String> {
    let h = record.header();
    let mut tags: Vec<String> = Vec::new();
    match target {
        Target::Page(p) => tags.push(format!("page:{p}")),
        Target::Entity(e) => tags.push(format!("entity:{e}")),
        Target::Actor(a) => tags.push(format!("actor:{a}")),
        Target::Record { partition, offset } => tags.push(format!("record:{partition}:{offset}")),
        Target::Namespace(n) => {
            tags.push(format!("ns:{n}"));
            bump_epoch(cx, tenant).await?;
        }
        Target::Tenant(t) => {
            tags.push(format!("tenant:{t}"));
            bump_epoch(cx, tenant).await?;
        }
        Target::Graph(g) => {
            tags.push(format!("graph:{g}"));
            bump_epoch(cx, tenant).await?;
        }
        Target::Set(id) => {
            tags.push(format!("set:{id}"));
            let members = cx
                .query(
                    "SELECT kind, id FROM view.set_member WHERE tenant = $1 AND set_id = $2",
                    &[&tenant, &i64::try_from(*id).map_err(|e| e.to_string())?],
                )
                .await
                .map_err(|e| sql(&e))?;
            for m in members {
                let (kind, id): (String, String) = (m.get(0), m.get(1));
                tags.push(format!("{kind}:{id}"));
            }
        }
        Target::Statement(_) | Target::Property(_) | Target::Blob(_) => {
            tags.push(target.key().trim_start_matches("acl:").to_string());
        }
    }
    for tag in tags {
        cx.execute(
            "INSERT INTO ops.purge (tenant, tag, reason, partition, \"offset\")
             VALUES ($1, $2, 'acl', $3, $4)",
            &[
                &tenant,
                &tag,
                &scatter_log_postgres::ids::partition_to_db(h.partition),
                &i64::try_from(h.offset).map_err(|e| e.to_string())?,
            ],
        )
        .await
        .map_err(|e| sql(&e))?;
    }
    Ok(())
}

async fn bump_epoch(cx: &PgCx, tenant: &str) -> Result<(), String> {
    cx.execute(
        "INSERT INTO ops.visibility_epoch (tenant, epoch) VALUES ($1, 1)
         ON CONFLICT (tenant) DO UPDATE SET epoch = ops.visibility_epoch.epoch + 1",
        &[&tenant],
    )
    .await
    .map_err(|e| sql(&e))?;
    Ok(())
}

async fn page(cx: &PgCx, tenant: &str, id: u64, acl: Option<&Acl>) -> Result<u64, String> {
    let page = i64::try_from(id).map_err(|e| e.to_string())?;
    let mut rows = reserve(cx, tenant, page, acl).await?;
    let deleted = acl.is_some_and(deletes);
    rows += cx
        .execute(
            "UPDATE view.page SET deleted = $3 WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &page, &deleted],
        )
        .await
        .map_err(|e| sql(&e))?;
    // The talk page goes with it, or comes back unless its own ACL deletes it.
    let talk = cx
        .query_opt(
            "SELECT page_id FROM view.talk_page WHERE tenant = $1 AND target_kind = 1 AND target_id = $2",
            &[&tenant, &id.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?
        .map(|r| r.get::<_, i64>(0))
        .filter(|t| *t != page);
    if let Some(t) = talk {
        let talk_deleted = if deleted {
            acl.is_some_and(|a| a.talk)
        } else {
            let own = crate::acl_read::current(
                cx,
                tenant,
                &[Target::Page(u64::try_from(t).map_err(|e| e.to_string())?)],
            )
            .await?;
            own.iter().any(deletes)
        };
        if deleted && !talk_deleted {
            return Ok(rows);
        }
        rows += cx
            .execute(
                "UPDATE view.page SET deleted = $3 WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &t, &talk_deleted],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(rows)
}

/// Holds or frees a reserved title (09 §4.8). A row that already exists, the page itself
/// once created, is left alone; one that no revision has filled is freed when the ACL no
/// longer reserves it.
async fn reserve(cx: &PgCx, tenant: &str, page: i64, acl: Option<&Acl>) -> Result<u64, String> {
    match acl.and_then(|a| a.reserved.as_ref()) {
        Some(r) => {
            let ns = i32::try_from(r.namespace).map_err(|e| e.to_string())?;
            cx.execute(
                concat!(
                    "INSERT INTO view.page (tenant, page_id, ns, title, content_model, reserved, random)
                 VALUES ($1, $2, $3, $4, '', true, ",
                    crate::page_random_sql!(),
                    ")
                 ON CONFLICT DO NOTHING"
                ),
                &[&tenant, &page, &ns, &r.title],
            )
            .await
            .map_err(|e| sql(&e))
        }
        None => cx
            .execute(
                "DELETE FROM view.page
                 WHERE tenant = $1 AND page_id = $2 AND reserved AND latest_offset IS NULL",
                &[&tenant, &page],
            )
            .await
            .map_err(|e| sql(&e)),
    }
}
