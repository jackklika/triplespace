//! The moderation modules (0023 §8; 18 §2.3; 09 §6): `action=delete`, `action=undelete`,
//! `action=protect` and `action=revisiondelete`, each an ACL record written through the one
//! writer ([`triplespace_api_ingest::acl`]).
//!
//! A target has one ACL, so each module merges into it: deleting a protected page adds a
//! moderation `read` restriction beside its protection, undeleting takes that restriction
//! away and retires the ACL only when nothing is left, and protecting a deleted page keeps
//! it deleted. Hiding merges the parts already hidden with those named.
//!
//! - **`action=delete`**: `title` or `pageid`, `reason`, `deletetalk`. Document pages,
//!   threads and local entities by title (`Q5`, `Property:P3`); a mirrored entity is its
//!   provider's to moderate (`ts-not-supported`).
//! - **`action=undelete`**: `title`, `reason`, and `pageid` (ours) to choose among several
//!   deleted pages that had the title, which is otherwise `ts-ambiguous-undelete`, listing
//!   their IDs. `timestamps` or `revisions` name the revisions to restore (partial
//!   undeletion, 09 §4.3): the others stay hidden under `record` ACLs to the deletion
//!   group, written in the same transaction before the page ACL is retired, and the
//!   `delete/restore` event lists them as `kept`. A title a new page or a reservation has
//!   taken meanwhile is `ts-title-taken`.
//! - **`action=protect`**: `title` or `pageid`, `protections` (`edit=autoconfirmed|move=sysop`,
//!   `read={group}` for a confidential restriction, `{action}=all` to lift one), `expiry`
//!   (one, or one per protection), `reason`, and `tstalk` (`1` or `0`), which takes the
//!   subject's talk page and its threads under a confidential restriction (09 §4.4). `cascade` is `ts-no-cascade`. A title with no
//!   page takes `create={group}` and `read={group}` only, and reserves the title under a
//!   new page ID (09 §4.8): a `create` there takes the ID, and is refused with
//!   `ts-restricted-title` to anyone the ACL does not admit. `read` on a deleted page is
//!   `ts-deleted`, and `create` on an existing one MediaWiki's `create-titleexists`.
//! - **`action=revisiondelete`**: `type=revision` (`archive` too) with revision `ids`, or
//!   `type=logging` with log IDs (`deletelogentry`; the event is `delete/event`, and a hidden
//!   content part is MediaWiki's `actionhidden`); `hide` and `show` (`content|comment|user`),
//!   `suppress` (`yes|no|nochange`), `reason`. For the revisions of document pages and
//!   thread posts, whose text is the `text` part, and of entities, whose change set is the
//!   `content` part. `oldimage` and `filearchive` wait on files.
//!
//! The deletion and suppression groups are the tenant's `moderation.delete_group` and
//! `moderation.suppress_group`.
//!
//! `enforce` is how a write is checked against the ACLs along its target's chain
//! ([`scatter_actors::acl::check`]), with the refusals MediaWiki gives.

use scatter_actors::Timestamp;
use scatter_actors::acl::{Acl, Denied, READ, ReadKind, ReservedTitle, Restriction, Target};
use scatter_actors::evaluate::Effective;
use scatter_projection::Backend as _;
use serde_json::{Value, json};
use triplespace_api_ingest::acl::{AclWrite, AclWriteError, write};

use crate::api::Ctx;
use crate::edit::now_micros;
use crate::pages::{self, Page, Site};
use crate::response::{ApiError, ApiResponse};

/// The tenant's deletion and suppression groups.
pub(crate) async fn groups(ctx: &Ctx) -> Result<(String, String), ApiError> {
    let (s, _) = triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let get = |k: &str, d: &str| {
        s.value(k)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| d.to_string())
    };
    Ok((
        get("moderation.delete_group", "sysop"),
        get("moderation.suppress_group", "suppress"),
    ))
}

fn refused(e: AclWriteError) -> ApiError {
    match e {
        AclWriteError::Denied(right) => ApiError::permission_denied(right),
        AclWriteError::LockedOut(g) => ApiError::new(
            "ts-locked-out",
            format!(
                "A confidential restriction may be set, changed or lifted only by a member of {g}."
            ),
        ),
        AclWriteError::Invalid(m) => ApiError::new("badvalue", m),
        AclWriteError::Policy(m) => ApiError::new("ts-policy", m),
        AclWriteError::NothingToRetire(t) => {
            ApiError::new("cantundelete", format!("{t} is not restricted."))
        }
        AclWriteError::Store(m) => ApiError::internal(m),
    }
}

/// The namespaces and sets enclosing a target, for a caller whose rights some scoped group
/// grants (09 §3.4); nothing for anyone else, whose rights hold everywhere or nowhere.
/// A record's are its page's; a free title's, its namespace's and its ancestors' sets.
pub(crate) async fn enclosing(
    ctx: &Ctx,
    target: &Target,
    reserve: Option<&ReservedTitle>,
) -> Result<Vec<Target>, ApiError> {
    use triplespace_projections::acl_read::{page_scope, title_scope};
    if ctx.caller.effective.scoped.is_empty() {
        return Ok(Vec::new());
    }
    let tenant = &ctx.tenant.slug;
    let out = match (target, reserve) {
        (_, Some(r)) => {
            title_scope(
                ctx.db(),
                tenant,
                i32::try_from(r.namespace).unwrap_or_default(),
                &r.title,
                None,
            )
            .await
        }
        (Target::Page(id), None) => page_scope(ctx.db(), tenant, *id).await,
        (Target::Namespace(_) | Target::Set(_), None) => Ok(vec![target.clone()]),
        (Target::Record { partition, offset }, None) => {
            let row = ctx
                .db()
                .query_opt(
                    "SELECT target_id FROM view.activity
                     WHERE partition = $1 AND \"offset\" = $2 AND target_kind = $3",
                    &[
                        &scatter_log_postgres::ids::partition_to_db(*partition),
                        &i64::try_from(*offset).unwrap_or(i64::MAX),
                        &triplespace_projections::common::target_kind::PAGE,
                    ],
                )
                .await?;
            match row.and_then(|r| r.get::<_, Option<String>>(0)?.parse::<u64>().ok()) {
                Some(page) => page_scope(ctx.db(), tenant, page).await,
                None => Ok(Vec::new()),
            }
        }
        _ => Ok(Vec::new()),
    };
    out.map_err(ApiError::internal)
}

/// `ts-restricted-title`: a title held by a restriction the editor does not satisfy,
/// whether a reservation or a confidential page, which the refusal does not tell apart
/// (09 §4.8, §5.3).
pub(crate) fn restricted_title(title: &str) -> ApiError {
    ApiError::new(
        "ts-restricted-title",
        format!("The title \"{title}\" is restricted; you cannot create a page there."),
    )
}

/// MediaWiki's refusal for an action an ACL restricts: `protectednamespace` for a
/// namespace, `protectedpage` otherwise, `permissiondenied` for a right not held.
pub(crate) fn denied(e: &Denied) -> ApiError {
    match e {
        Denied::Permission(p) => ApiError::permission_denied(p),
        Denied::Restricted { target, .. } | Denied::Nobody { target, .. }
            if target.starts_with("acl:namespace:") =>
        {
            ApiError::new(
                "protectednamespace",
                format!("You're not allowed to edit pages in this namespace ({e})."),
            )
        }
        _ => ApiError::new(
            "protectedpage",
            format!("This page has been protected to prevent editing or other actions ({e})."),
        ),
    }
}

/// Checks `permission` against a chain at `at`: a chain whose `read` the actor does not
/// satisfy refuses as a restricted title (an absent target is not named), then the
/// permission's own restrictions refuse with [`denied`].
pub(crate) fn enforce(
    permission: &str,
    who: &Effective,
    scope: &[Target],
    chain: &[Acl],
    at: Timestamp,
    title: &str,
) -> Result<(), ApiError> {
    use scatter_actors::acl::{ReadDecision, check_on, read_decision};
    match read_decision(who, chain, at) {
        ReadDecision::Readable => {}
        ReadDecision::Absent => return Err(restricted_title(title)),
        ReadDecision::Removed { .. } => return Err(ApiError::permission_denied("read")),
    }
    check_on(permission, who, scope, chain, at).map_err(|e| denied(&e))
}

/// The target's current ACL.
pub(crate) async fn current(ctx: &Ctx, target: &Target) -> Result<Option<Acl>, ApiError> {
    Ok(triplespace_projections::acl_read::current(
        ctx.db(),
        &ctx.tenant.slug,
        std::slice::from_ref(target),
    )
    .await
    .map_err(ApiError::internal)?
    .pop())
}

/// Writes one ACL in its own unit of work and returns the record's log ID and its
/// target, which for a reservation names the page ID reserved.
pub(crate) async fn save(
    ctx: &Ctx,
    target: Target,
    acl: Option<Acl>,
    reason: &str,
    suppress_group: &str,
    reserve: Option<ReservedTitle>,
) -> Result<(Option<u64>, Target), ApiError> {
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("delete"))?;
    let enclosing = enclosing(ctx, &target, reserve.as_ref()).await?;
    let store = ctx.app.store();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    let out = write(
        store,
        ctx.app.pipeline(),
        &mut cx,
        &ctx.tenant.slug,
        &actor,
        &ctx.caller.effective,
        AclWrite {
            target,
            acl,
            reason: reason.to_string(),
            suppress_group: suppress_group.to_string(),
            reserve,
            enclosing,
        },
        now_micros(),
    )
    .await;
    match out {
        Ok(record) => {
            store.commit(cx).await.map_err(ApiError::internal)?;
            let h = record.header();
            let target = Target::parse(h.key.as_deref().unwrap_or_default())
                .map_err(|e| ApiError::internal(e.to_string()))?;
            Ok((h.logid, target))
        }
        Err(e) => {
            let _ = store.rollback(cx).await;
            Err(refused(e))
        }
    }
}

/// What a moderation module may do with a page: whether the caller may know it exists,
/// and whether the module's own permission passes the ACLs on its chain.
enum Admit {
    /// The caller may act.
    Yes,
    /// A confidential restriction the caller does not satisfy makes the page absent for
    /// it: the module answers as it would for a page that does not exist (09 §5.3).
    Absent,
}

/// Admits `permission` on a page: absent when a confidential restriction on its chain
/// refuses the caller; otherwise the permission is checked against the chain as an edit
/// is (09 §4.4), and refused with [`denied`]. A deleted page is not absent: undeleting
/// and protecting it are what its deletion group does (0056 A18).
async fn admit(ctx: &Ctx, page_id: u64, permission: &str) -> Result<Admit, ApiError> {
    use scatter_actors::acl::{ReadDecision, check_on, read_decision};
    let chain = triplespace_projections::acl_read::page_chain(ctx.db(), &ctx.tenant.slug, page_id)
        .await
        .map_err(ApiError::internal)?;
    let at = Timestamp(now_micros());
    let who = &ctx.caller.effective;
    if read_decision(who, &chain, at) == ReadDecision::Absent {
        return Ok(Admit::Absent);
    }
    let scope = enclosing(ctx, &Target::Page(page_id), None).await?;
    check_on(permission, who, &scope, &chain, at).map_err(|e| denied(&e))?;
    Ok(Admit::Yes)
}

/// [`admit`] for an entity: its chain (09 §4.4), with the graph serving it when it is
/// held. `Absent` for an entity under a confidential restriction the caller does not
/// satisfy, or a deleted one outside its group, so that a request names only what the
/// caller could read (09 §5.3).
async fn admit_entity(ctx: &Ctx, id: &str, permission: &str) -> Result<Admit, ApiError> {
    use scatter_actors::acl::{ReadDecision, check_on};
    let current = crate::edit::current(ctx, &crate::modules::entities::parse_id(id)?).await?;
    // An entity no graph holds does not exist, as a title with no page.
    let Some(current) = current else {
        return Ok(Admit::Absent);
    };
    let graph = current.contributions.first().map(|g| g.graph.clone());
    let chain = triplespace_projections::acl_read::entity_chain(
        ctx.db(),
        &ctx.tenant.slug,
        id,
        graph.as_deref(),
    )
    .await
    .map_err(ApiError::internal)?;
    let at = Timestamp(now_micros());
    let who = &ctx.caller.effective;
    if scatter_actors::read_decision(who, &chain, at) != ReadDecision::Readable {
        return Ok(Admit::Absent);
    }
    let scope = if who.scoped.is_empty() {
        Vec::new()
    } else {
        triplespace_projections::acl_read::entity_scope(ctx.db(), &ctx.tenant.slug, id)
            .await
            .map_err(ApiError::internal)?
    };
    check_on(permission, who, &scope, &chain, at).map_err(|e| denied(&e))?;
    Ok(Admit::Yes)
}

/// The entity a `title` names, when it names one the tenant holds and the caller may
/// read; `None` for a title that is not an entity's. A local entity only: a mirrored one
/// is the provider's to moderate (09 §5.6), `ts-not-supported`.
fn entity_of(site: &Site, title: &str) -> Result<Option<String>, ApiError> {
    let Some(id) = pages::entity_of_title(site, title) else {
        return Ok(None);
    };
    if id.form() == scatter_wikibase_model::id::IdForm::Foreign {
        return Err(ApiError::new(
            "ts-not-supported",
            "A mirrored entity is moderated by its provider, not here.",
        ));
    }
    Ok(Some(id.as_str().to_string()))
}

/// The page a request names by `title` or `pageid`, not deleted, admitted for
/// `permission` ([`admit`]); a page absent for the caller is missing.
async fn admitted_page(ctx: &Ctx, site: &Site, permission: &str) -> Result<Page, ApiError> {
    let page = page_of(ctx, site).await?;
    match admit(ctx, page.page_id, permission).await? {
        Admit::Yes => Ok(page),
        Admit::Absent => Err(missing_page(ctx)),
    }
}

/// The refusal for a page that does not exist, as the request named it.
fn missing_page(ctx: &Ctx) -> ApiError {
    match ctx.params.get("pageid") {
        Some(n) => ApiError::new("nosuchpageid", format!("There is no page with ID {n}.")),
        None => ApiError::new("missingtitle", "The page you specified doesn't exist."),
    }
}

/// The page a request names by `title` or `pageid`, not deleted.
async fn page_of(ctx: &Ctx, site: &Site) -> Result<Page, ApiError> {
    let tenant = ctx.tenant.slug.clone();
    if let Some(id) = ctx.params.get("pageid") {
        let n: u64 = id.parse().map_err(|_| ApiError::bad_value("pageid", id))?;
        return pages::by_id(ctx.db(), &tenant, &pages::Hidden::NONE, n)
            .await?
            .ok_or_else(|| {
                ApiError::new("nosuchpageid", format!("There is no page with ID {n}."))
            });
    }
    let name = ctx.params.required("title")?;
    if pages::names_entity(site, name) {
        return Err(ApiError::new(
            "ts-not-supported",
            "Deleting and protecting entity pages is not served here yet.",
        ));
    }
    let t = pages::resolve_title(site, name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
    pages::by_title(
        ctx.db(),
        &tenant,
        &pages::Hidden::NONE,
        t.namespace,
        &t.text,
    )
    .await?
    .ok_or_else(|| ApiError::new("missingtitle", "The page you specified doesn't exist."))
}

/// `action=delete`.
pub async fn delete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require_somewhere("delete")?;
    let site = Site::load(ctx).await?;
    let (target, title) = match ctx.params.get("title") {
        Some(t) if entity_of(&site, t)?.is_some() => {
            let id = entity_of(&site, t)?.unwrap_or_default();
            match admit_entity(ctx, &id, "delete").await? {
                Admit::Yes => {}
                Admit::Absent => return Err(missing_page(ctx)),
            }
            // A deleted entity is missing already (09 §6.2); deleting it twice is as
            // MediaWiki's deleting a missing page.
            if current(ctx, &Target::Entity(id.clone()))
                .await?
                .is_some_and(|a| a.read_kind == Some(ReadKind::Moderation))
            {
                return Err(missing_page(ctx));
            }
            (Target::Entity(id), t.to_string())
        }
        _ => {
            let page = admitted_page(ctx, &site, "delete").await?;
            (
                Target::Page(page.page_id),
                site.prefixed(page.ns, &page.title),
            )
        }
    };
    let (delete_group, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let mut acl = current(ctx, &target)
        .await?
        .unwrap_or_else(|| Acl::read(target.clone(), ReadKind::Moderation, &delete_group, None));
    // A confidential page keeps its group beside the deletion group, so that the deleted
    // page is seen by members of both and stays absent for everyone else (09 §5.2).
    if acl.read_kind == Some(ReadKind::Confidential) {
        acl.confidential = acl.restrictions.remove(READ);
        acl.confidential_talk = acl.talk;
    }
    acl.restrictions
        .insert(READ.into(), Restriction::to_group(&delete_group, None));
    acl.read_kind = Some(ReadKind::Moderation);
    acl.talk = ctx.params.flag("deletetalk");
    let (logid, _) = save(ctx, target, Some(acl), &reason, &suppress_group, None).await?;
    Ok(ApiResponse::ok(json!({"delete": {
        "title": title,
        "reason": reason,
        "logid": logid,
    }})))
}

/// `action=undelete`.
#[allow(clippy::too_many_lines)]
pub async fn undelete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require_somewhere("undelete")?;
    let site = Site::load(ctx).await?;
    let name = ctx.params.required("title")?.to_string();
    if let Some(id) = entity_of(&site, &name)? {
        return undelete_entity(ctx, &id, &name).await;
    }
    let t = pages::resolve_title(&site, &name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
    let tenant = ctx.tenant.slug.clone();
    let ids: Vec<u64> = ctx
        .db()
        .query(
            "SELECT page_id FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND deleted
             ORDER BY page_id DESC",
            &[&tenant, &t.namespace, &t.text],
        )
        .await?
        .iter()
        .filter_map(|r| u64::try_from(r.get::<_, i64>(0)).ok())
        .collect();
    // A deleted page that is confidential to a group the caller is outside is not one of
    // its deleted pages to undelete, list or count (09 §5.3).
    let mut ids_seen = Vec::with_capacity(ids.len());
    for id in ids {
        if matches!(admit(ctx, id, "undelete").await?, Admit::Yes) {
            ids_seen.push(id);
        }
    }
    let ids = ids_seen;
    let none = no_deleted_revisions;
    // Several deleted pages may have had the title; which one is the caller's to say.
    let page_id = match ctx.params.get("pageid") {
        Some(p) => {
            let n: u64 = p.parse().map_err(|_| ApiError::bad_value("pageid", p))?;
            if !ids.contains(&n) {
                return Err(ApiError::new(
                    "cantundelete",
                    format!("Page {n} is not a deleted page with this title."),
                ));
            }
            n
        }
        None => match ids.as_slice() {
            [] => return Err(none()),
            [one] => *one,
            many => {
                return Err(ApiError::new(
                    "ts-ambiguous-undelete",
                    format!(
                        "{} deleted pages had this title; say which with `pageid`.",
                        many.len()
                    ),
                )
                .with("pageids", json!(many)));
            }
        },
    };
    // A page or a reservation that has taken the title since.
    if ctx
        .db()
        .query_opt(
            "SELECT 1 FROM view.page WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted",
            &[&tenant, &t.namespace, &t.text],
        )
        .await?
        .is_some()
    {
        return Err(ApiError::new(
            "ts-title-taken",
            "A new page or a reservation has taken this title; undeleting into it is refused.",
        ));
    }
    let (delete_group, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let target = Target::Page(page_id);
    // Undeleting a confidential page restores its confidential restriction (09 §5.2),
    // with the talk page under it where the restriction took it (`talk`, kept in the
    // ACL's `confidential_talk` while deleted).
    let acl = current(ctx, &target).await?.and_then(|mut a| {
        a.restrictions.remove(READ);
        a.read_kind = None;
        a.talk = false;
        if let Some(c) = a.confidential.take() {
            a.restrictions.insert(READ.into(), c);
            a.read_kind = Some(ReadKind::Confidential);
            a.talk = std::mem::take(&mut a.confidential_talk);
        }
        (!a.restrictions.is_empty()).then_some(a)
    });
    // Partial undeletion (09 §4.3): the revisions named by `timestamps` or `revisions`
    // come back; every other revision of the page stays hidden under a `record` ACL to
    // the deletion group, written before the page ACL is retired so that the restore
    // event can list what stays. With neither, every revision comes back.
    let revisions = page_revisions(ctx, page_id).await?;
    let kept: Vec<&PageRevision> = match partial_undelete(ctx, &revisions)? {
        Some(restore) => revisions
            .iter()
            .filter(|r| !restore.contains(&r.revid))
            .collect(),
        None => Vec::new(),
    };
    let restored = revisions.len() - kept.len();
    if restored == 0 {
        return Err(ApiError::new(
            "cantundelete",
            "None of the named revisions is a deleted revision of this page.",
        ));
    }
    let records: Vec<(Target, &str)> = kept
        .iter()
        .map(|r| {
            (
                Target::Record {
                    partition: r.partition,
                    offset: r.offset,
                },
                if r.source == "pages" {
                    "text"
                } else {
                    "content"
                },
            )
        })
        .collect();
    restore(
        ctx,
        target,
        acl,
        &records,
        &reason,
        (&delete_group, &suppress_group),
    )
    .await?;
    ctx.cache_tags.push(format!("p:{page_id}"));
    Ok(ApiResponse::ok(json!({"undelete": {
        "title": site.prefixed(t.namespace, &t.text),
        "pageid": page_id,
        "revisions": restored,
        "fileversions": 0,
        "reason": reason,
        "ts-kept": kept.iter().map(|r| r.revid).collect::<Vec<_>>(),
    }})))
}

/// Writes an undeletion in one transaction: a `record` ACL to the deletion group on each
/// of `kept` (the revision's content part named beside it), then the page ACL `acl`
/// (its remaining restrictions, or `None` to retire it).
async fn restore(
    ctx: &Ctx,
    target: Target,
    acl: Option<Acl>,
    kept: &[(Target, &str)],
    reason: &str,
    (delete_group, suppress_group): (&str, &str),
) -> Result<(), ApiError> {
    let actor = ctx
        .caller
        .identity
        .actor_key
        .clone()
        .ok_or_else(|| ApiError::permission_denied("undelete"))?;
    let enclosing = enclosing(ctx, &target, None).await?;
    let store = ctx.app.store();
    let pipeline = ctx.app.pipeline();
    let mut cx = store.begin().await.map_err(ApiError::internal)?;
    let mut writes: Vec<AclWrite> = kept
        .iter()
        .map(|(record, content_part)| {
            let mut a = Acl::read(record.clone(), ReadKind::Moderation, delete_group, None);
            a.parts = vec![
                (*content_part).to_string(),
                "comment".into(),
                "attestation".into(),
            ];
            AclWrite {
                target: record.clone(),
                acl: Some(a),
                reason: reason.to_string(),
                suppress_group: suppress_group.to_string(),
                reserve: None,
                enclosing: enclosing.clone(),
            }
        })
        .collect();
    writes.push(AclWrite {
        target,
        acl,
        reason: reason.to_string(),
        suppress_group: suppress_group.to_string(),
        reserve: None,
        enclosing,
    });
    for w in writes {
        if let Err(e) = write(
            store,
            pipeline,
            &mut cx,
            &ctx.tenant.slug,
            &actor,
            &ctx.caller.effective,
            w,
            now_micros(),
        )
        .await
        {
            let _ = store.rollback(cx).await;
            return Err(refused(e));
        }
    }
    store.commit(cx).await.map_err(ApiError::internal)
}

/// A revision of a page, as a partial undeletion addresses it.
struct PageRevision {
    revid: u64,
    partition: u64,
    offset: u64,
    source: String,
    time: u64,
}

/// A page's revisions, oldest first: every record of the page with a revision ID.
async fn page_revisions(ctx: &Ctx, page_id: u64) -> Result<Vec<PageRevision>, ApiError> {
    let rows = ctx
        .db()
        .query(
            "SELECT revid, partition, \"offset\", source,
                    (EXTRACT(EPOCH FROM time) * 1000000)::bigint
             FROM view.activity
             WHERE tenant = $1 AND target_kind = $2 AND target_id = $3 AND revid IS NOT NULL
             ORDER BY \"offset\"",
            &[
                &ctx.tenant.slug,
                &triplespace_projections::common::target_kind::PAGE,
                &page_id.to_string(),
            ],
        )
        .await?;
    Ok(rows
        .iter()
        .map(|r| PageRevision {
            revid: u64::try_from(r.get::<_, i64>(0)).unwrap_or(0),
            partition: r.get::<_, i64>(1).cast_unsigned(),
            offset: u64::try_from(r.get::<_, i64>(2)).unwrap_or(0),
            source: r.get(3),
            time: u64::try_from(r.get::<_, i64>(4)).unwrap_or(0),
        })
        .collect())
}

/// The revisions `timestamps` or `revisions` name among a page's, or `None` when neither
/// is given and the whole page comes back. A timestamp names every revision in its
/// second, as `basetimestamp` does (`editpage`).
fn partial_undelete(ctx: &Ctx, revisions: &[PageRevision]) -> Result<Option<Vec<u64>>, ApiError> {
    let mut named: Vec<u64> = Vec::new();
    let mut any = false;
    if let Some(r) = ctx.params.get("revisions") {
        any = true;
        for s in r.split('|').filter(|s| !s.is_empty()) {
            named.push(s.parse().map_err(|_| ApiError::bad_value("revisions", s))?);
        }
    }
    if let Some(ts) = ctx.params.get("timestamps") {
        any = true;
        for s in ts.split('|').filter(|s| !s.is_empty()) {
            let at = triplespace_projections::feed::parse_timestamp(s)
                .ok_or_else(|| ApiError::bad_value("timestamps", s))?;
            let second = at / 1_000_000;
            named.extend(
                revisions
                    .iter()
                    .filter(|r| r.time / 1_000_000 == second)
                    .map(|r| r.revid),
            );
        }
    }
    Ok(any.then_some(named))
}

fn no_deleted_revisions() -> ApiError {
    ApiError::new(
        "cantundelete",
        "There are no deleted revisions of this page.",
    )
}

/// `action=undelete` on an entity: an entity has one page and one ID, so there is nothing
/// to choose among and no title to have been taken. The deletion group's members undelete;
/// anyone else finds nothing deleted (09 §5.3).
async fn undelete_entity(ctx: &Ctx, id: &str, title: &str) -> Result<ApiResponse, ApiError> {
    let target = Target::Entity(id.to_string());
    let existing = current(ctx, &target).await?;
    let deleted = existing
        .as_ref()
        .is_some_and(|a| a.read_kind == Some(ReadKind::Moderation));
    // The read test admits the deletion group to its own deletion (`Removed` is not
    // `Absent`); `admit_entity` would call a deleted entity absent, so the test is made
    // here from the chain.
    let chain =
        triplespace_projections::acl_read::entity_chain(ctx.db(), &ctx.tenant.slug, id, None)
            .await
            .map_err(ApiError::internal)?;
    let at = Timestamp(now_micros());
    let who = &ctx.caller.effective;
    let none = no_deleted_revisions;
    match scatter_actors::read_decision(who, &chain, at) {
        scatter_actors::ReadDecision::Absent => return Err(none()),
        scatter_actors::ReadDecision::Removed { .. } | scatter_actors::ReadDecision::Readable => {}
    }
    if !deleted {
        return Err(none());
    }
    let scope = if who.scoped.is_empty() {
        Vec::new()
    } else {
        triplespace_projections::acl_read::entity_scope(ctx.db(), &ctx.tenant.slug, id)
            .await
            .map_err(ApiError::internal)?
    };
    // The deletion restricts `read` to its group, which the undeleter must be in; the
    // chain test is against the rest of the chain.
    if !existing
        .as_ref()
        .and_then(|a| a.restrictions.get(READ))
        .and_then(|r| r.group.as_deref())
        .is_some_and(|g| who.in_group(g))
    {
        return Err(none());
    }
    scatter_actors::acl::check_on(
        "undelete",
        who,
        &scope,
        chain.iter().filter(|a| a.target != target),
        at,
    )
    .map_err(|e| denied(&e))?;
    let (_, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let acl = existing.and_then(|mut a| {
        a.restrictions.remove(READ);
        a.read_kind = None;
        if let Some(c) = a.confidential.take() {
            a.restrictions.insert(READ.into(), c);
            a.read_kind = Some(ReadKind::Confidential);
        }
        (!a.restrictions.is_empty()).then_some(a)
    });
    save(ctx, target, acl, &reason, &suppress_group, None).await?;
    Ok(ApiResponse::ok(json!({"undelete": {
        "title": title,
        "revisions": 0,
        "fileversions": 0,
        "reason": reason,
    }})))
}

/// An expiry: `infinite` and its synonyms are none; a time; or, as MediaWiki's forms
/// take it, a span from now (`1 week`, `3 days`, `2 hours`, `6 months`, `1 year`).
///
/// # Errors
///
/// `invalidexpiry` for anything else.
pub fn expiry(text: &str) -> Result<Option<Timestamp>, ApiError> {
    match text.trim() {
        "" | "infinite" | "indefinite" | "infinity" | "never" => Ok(None),
        t => triplespace_projections::feed::parse_timestamp(t)
            .or_else(|| relative_expiry(t))
            .map(|us| Some(Timestamp(us)))
            .ok_or_else(|| ApiError::new("invalidexpiry", format!("Invalid expiry time \"{t}\"."))),
    }
}

/// `N unit[s]` from now, in microseconds; months are 30 days and years 365.
fn relative_expiry(text: &str) -> Option<u64> {
    let (n, unit) = text.split_once(char::is_whitespace)?;
    let n: u64 = n.trim().parse().ok()?;
    let unit = unit.trim().to_ascii_lowercase();
    let unit = unit.strip_suffix('s').unwrap_or(&unit);
    let seconds: u64 = match unit {
        "second" | "sec" => 1,
        "minute" | "min" => 60,
        "hour" => 3_600,
        "day" => 86_400,
        "week" => 7 * 86_400,
        "month" => 30 * 86_400,
        "year" => 365 * 86_400,
        _ => return None,
    };
    now_micros().checked_add(n.checked_mul(seconds)?.checked_mul(1_000_000)?)
}

/// What `action=protect` acts on: a page, or a title with no page, reserved or not yet.
enum Protected {
    Page(Page),
    Entity(String),
    Title {
        ns: i32,
        title: String,
        reserved: Option<u64>,
    },
}

/// The page or free title a protection names.
async fn protected_of(ctx: &Ctx, site: &Site) -> Result<Protected, ApiError> {
    if ctx.params.get("pageid").is_some() {
        return admitted_page(ctx, site, "protect")
            .await
            .map(Protected::Page);
    }
    let name = ctx.params.required("title")?;
    if let Some(id) = entity_of(site, name)? {
        return match admit_entity(ctx, &id, "protect").await? {
            Admit::Yes => Ok(Protected::Entity(id)),
            Admit::Absent => Err(restricted_title(name)),
        };
    }
    let t = pages::resolve_title(site, name)
        .ok_or_else(|| ApiError::new("invalidtitle", format!("Bad title \"{name}\".")))?;
    let tenant = ctx.tenant.slug.clone();
    if let Some(p) = pages::by_title(
        ctx.db(),
        &tenant,
        &pages::Hidden::NONE,
        t.namespace,
        &t.text,
    )
    .await?
    {
        // A title held by a page the caller may not read is refused as a reserved title
        // is, which tells the caller only that something holds it (09 §5.3, §4.8).
        return match admit(ctx, p.page_id, "protect").await? {
            Admit::Yes => Ok(Protected::Page(p)),
            Admit::Absent => Err(restricted_title(&site.prefixed(p.ns, &p.title))),
        };
    }
    if !site.holds_pages(t.namespace) {
        return Err(ApiError::new(
            "ts-not-supported",
            "Only a title for a document page can be reserved.",
        ));
    }
    let reserved =
        triplespace_projections::acl_read::reservation(ctx.db(), &tenant, t.namespace, &t.text)
            .await
            .map_err(ApiError::internal)?;
    if let Some(id) = reserved
        && matches!(admit(ctx, id, "protect").await?, Admit::Absent)
    {
        return Err(restricted_title(&site.prefixed(t.namespace, &t.text)));
    }
    Ok(Protected::Title {
        ns: t.namespace,
        title: t.text,
        reserved,
    })
}

/// `action=protect`.
#[allow(clippy::too_many_lines)]
pub async fn protect(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    ctx.caller.require_somewhere("protect")?;
    if ctx.params.flag("cascade") {
        return Err(ApiError::new(
            "ts-no-cascade",
            "There is no cascading protection.",
        ));
    }
    let site = Site::load(ctx).await?;
    let protected = protected_of(ctx, &site).await?;
    let (_, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let protections: Vec<(String, String)> = ctx
        .params
        .required("protections")?
        .split('|')
        .filter(|p| !p.is_empty())
        .map(|p| match p.split_once('=') {
            Some((a, g)) => Ok((a.trim().to_string(), g.trim().to_string())),
            None => Err(ApiError::new(
                "invalidprotection",
                format!("Invalid protection \"{p}\"."),
            )),
        })
        .collect::<Result<_, _>>()?;
    let expiries: Vec<&str> = ctx
        .params
        .get("expiry")
        .unwrap_or("infinite")
        .split('|')
        .collect();
    if expiries.len() != 1 && expiries.len() != protections.len() {
        return Err(ApiError::new(
            "toofewexpiries",
            format!(
                "{} expiry timestamps were provided where {} were needed.",
                expiries.len(),
                protections.len()
            ),
        ));
    }
    let (target, title, reserve) = match &protected {
        Protected::Page(p) => (Target::Page(p.page_id), site.prefixed(p.ns, &p.title), None),
        Protected::Entity(id) => (
            Target::Entity(id.clone()),
            ctx.params.get("title").unwrap_or_default().to_string(),
            None,
        ),
        Protected::Title {
            ns,
            title,
            reserved: Some(id),
        } => (Target::Page(*id), site.prefixed(*ns, title), None),
        Protected::Title {
            ns,
            title,
            reserved: None,
        } => (
            // The writer replaces the placeholder with the ID it reserves.
            Target::Page(0),
            site.prefixed(*ns, title),
            Some(ReservedTitle {
                namespace: i64::from(*ns),
                title: title.clone(),
            }),
        ),
    };
    let existing = match &reserve {
        Some(_) => None,
        None => current(ctx, &target).await?,
    };
    let exists = matches!(protected, Protected::Page(_) | Protected::Entity(_));
    let mut acl = existing
        .clone()
        .unwrap_or_else(|| Acl::protect(target.clone(), &[], "", None));
    let mut shown = Vec::new();
    for (k, (action, group)) in protections.iter().enumerate() {
        let lift = group == "all" || group.is_empty();
        // A title with no page takes `create` and `read` only, as MediaWiki's
        // create-protection takes `create` only; a page cannot be create-protected.
        let permission = match (action.as_str(), exists) {
            ("edit" | "move" | "read", true) | ("read", false) => action.as_str(),
            ("create", false) => "createpage",
            ("create", true) if lift => "createpage",
            ("create", true) => {
                return Err(ApiError::new(
                    "create-titleexists",
                    "Existing titles can't be protected with \"create\".",
                ));
            }
            ("edit" | "move", false) => {
                return Err(ApiError::new(
                    "missingtitle-createonly",
                    "Missing titles can only be protected with \"create\" or \"read\".",
                ));
            }
            _ => {
                return Err(ApiError::new(
                    "protect-invalidaction",
                    format!("Invalid protection type \"{action}\"."),
                ));
            }
        };
        let ex_text = expiries
            .get(k)
            .or_else(|| expiries.first())
            .copied()
            .unwrap_or("infinite");
        let ex = expiry(ex_text)?;
        if permission == READ && acl.read_kind == Some(ReadKind::Moderation) {
            return Err(ApiError::new(
                "ts-deleted",
                "The page is deleted; undelete it before restricting who may read it.",
            ));
        }
        if lift {
            acl.restrictions.remove(permission);
            if permission == READ {
                acl.read_kind = None;
            }
        } else {
            acl.restrictions
                .insert(permission.to_string(), Restriction::to_group(group, ex));
            if permission == READ {
                acl.read_kind = Some(ReadKind::Confidential);
            }
        }
        shown.push(json!({action.as_str(): group, "expiry": if ex.is_none() { "infinite".to_string() } else { ex_text.to_string() }}));
    }
    // `tstalk`: a confidential restriction takes the subject's talk page and its threads
    // with it (09 §4.4; 0056 A24); it is the talk page's own only while `read` is set.
    if ctx.params.get("tstalk").is_some() {
        acl.talk = ctx.params.flag("tstalk") && acl.read_kind == Some(ReadKind::Confidential);
    } else if acl.read_kind != Some(ReadKind::Confidential) {
        acl.talk = false;
    }
    let acl = (!acl.restrictions.is_empty()).then_some(acl);
    if acl.is_none() && existing.is_none() {
        // Nothing was protected and nothing is: there is nothing to write.
        return Ok(ApiResponse::ok(json!({"protect": {
            "title": title,
            "reason": reason,
            "protections": shown,
        }})));
    }
    let (_, written) = save(ctx, target, acl, &reason, &suppress_group, reserve).await?;
    let mut out = json!({"protect": {
        "title": title,
        "reason": reason,
        "protections": shown,
    }});
    // A title with no page names the page ID its reservation holds, which a `create`
    // there will take.
    if let (Protected::Title { .. }, Target::Page(id)) = (&protected, written) {
        out["protect"]["pageid"] = json!(id);
    }
    Ok(ApiResponse::ok(out))
}

/// The record a revision ID names: its partition and offset, and its graph.
async fn record_of(ctx: &Ctx, revid: u64) -> Result<Option<(u64, u64, String)>, ApiError> {
    let r = i64::try_from(revid).unwrap_or(i64::MAX);
    let row = ctx
        .db()
        .query_opt(
            "SELECT partition, \"offset\", source, target_kind, target_id FROM view.activity
             WHERE tenant = $1 AND revid = $2 ORDER BY \"offset\" DESC LIMIT 1",
            &[&ctx.tenant.slug, &r],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    // A revision of a page or entity the caller may not read does not exist for it
    // (09 §5.3).
    let kind = row.get::<_, Option<i16>>(3);
    let target: Option<String> = row.get(4);
    if kind == Some(triplespace_projections::feed::target_kind::PAGE)
        && let Some(page) = target.as_deref().and_then(|id| id.parse::<u64>().ok())
        && matches!(admit(ctx, page, "deleterevision").await, Ok(Admit::Absent))
    {
        return Ok(None);
    }
    if kind == Some(triplespace_projections::feed::target_kind::ENTITY)
        && let Some(id) = target.as_deref()
        && matches!(
            admit_entity(ctx, id, "deleterevision").await,
            Ok(Admit::Absent)
        )
    {
        return Ok(None);
    }
    Ok(Some((
        row.get::<_, i64>(0).cast_unsigned(),
        u64::try_from(row.get::<_, i64>(1)).unwrap_or(0),
        row.get::<_, String>(2),
    )))
}

/// The record a log ID names, for `type=logging`: a log event in any partition, which a
/// revision-deletion hides as `delete/event` (09 §4.3). An event about a page or entity
/// the caller may not read does not exist for it (09 §5.3), and so does one restricted to
/// a group the caller is outside (a `suppress/*` event).
async fn record_of_event(ctx: &Ctx, logid: u64) -> Result<Option<(u64, u64, String)>, ApiError> {
    let l = i64::try_from(logid).unwrap_or(i64::MAX);
    let row = ctx
        .db()
        .query_opt(
            "SELECT partition, \"offset\", source, target_kind, target_id, log_type
             FROM view.activity
             WHERE tenant = $1 AND logid = $2 ORDER BY \"offset\" DESC LIMIT 1",
            &[&ctx.tenant.slug, &l],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let kind = row.get::<_, Option<i16>>(3);
    let target: Option<String> = row.get(4);
    if kind == Some(triplespace_projections::feed::target_kind::PAGE)
        && let Some(page) = target.as_deref().and_then(|id| id.parse::<u64>().ok())
        && matches!(admit(ctx, page, "deletelogentry").await, Ok(Admit::Absent))
    {
        return Ok(None);
    }
    if kind == Some(triplespace_projections::feed::target_kind::ENTITY)
        && let Some(id) = target.as_deref()
        && matches!(
            admit_entity(ctx, id, "deletelogentry").await,
            Ok(Admit::Absent)
        )
    {
        return Ok(None);
    }
    if row.get::<_, Option<String>>(5).as_deref() == Some("suppress")
        && !ctx.caller.effective.holds("suppressrevision")
    {
        return Ok(None);
    }
    Ok(Some((
        row.get::<_, i64>(0).cast_unsigned(),
        u64::try_from(row.get::<_, i64>(1)).unwrap_or(0),
        row.get::<_, String>(2),
    )))
}

/// `action=revisiondelete`.
#[allow(clippy::too_many_lines)]
pub async fn revisiondelete(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    ctx.caller.check_csrf(&ctx.app, &ctx.params)?;
    let kind = ctx.params.required("type")?.to_string();
    if !matches!(kind.as_str(), "revision" | "archive" | "logging") {
        // `oldimage` and `filearchive` wait on files (0039).
        return Err(ApiError::new(
            "ts-not-supported",
            format!("type={kind} is not served yet."),
        ));
    }
    let logging = kind == "logging";
    if logging {
        ctx.caller.require_somewhere("deletelogentry")?;
    }
    let names = |p: Option<&str>| -> Result<Vec<String>, ApiError> {
        p.unwrap_or_default()
            .split('|')
            .filter(|s| !s.is_empty())
            .map(|s| match s {
                "content" | "comment" | "user" => Ok(s.to_string()),
                other => Err(ApiError::bad_value("hide", other)),
            })
            .collect()
    };
    let hide = names(ctx.params.get("hide"))?;
    let show = names(ctx.params.get("show"))?;
    if hide.is_empty() && show.is_empty() {
        return Err(ApiError::new(
            "revdelete-nooversight",
            "You must hide or show at least one part.",
        ));
    }
    if hide.iter().any(|h| show.contains(h)) {
        return Err(ApiError::new(
            "badparams",
            "A part cannot be both hidden and shown.",
        ));
    }
    let suppress = ctx.params.get("suppress").unwrap_or("nochange").to_string();
    let (delete_group, suppress_group) = groups(ctx).await?;
    let reason = ctx.params.get("reason").unwrap_or_default().to_string();
    let ids: Vec<u64> = ctx
        .params
        .required("ids")?
        .split('|')
        .map(|s| s.parse().map_err(|_| ApiError::bad_value("ids", s)))
        .collect::<Result<_, _>>()?;
    let mut items = Vec::new();
    for revid in ids {
        let found = if logging {
            record_of_event(ctx, revid).await?
        } else {
            record_of(ctx, revid).await?
        };
        let Some((partition, offset, source)) = found else {
            let code = if logging {
                "nosuchlogid"
            } else {
                "nosuchrevid"
            };
            items.push(json!({"id": revid, "status": "Fail", "errors": [{"code": code}]}));
            continue;
        };
        // The part a page's or a post's text is, or an entity change set's content.
        let content_part = if source == "pages" { "text" } else { "content" };
        let part = |n: &str| match n {
            "content" => content_part.to_string(),
            "comment" => "comment".to_string(),
            _ => "attestation".to_string(),
        };
        let target = Target::Record { partition, offset };
        let existing = current(ctx, &target).await?;
        let mut parts: Vec<String> = match &existing {
            Some(a) if a.parts.is_empty() => {
                vec![content_part.into(), "comment".into(), "attestation".into()]
            }
            Some(a) => a.parts.clone(),
            None => Vec::new(),
        };
        for h in &hide {
            let p = part(h);
            if !parts.contains(&p) {
                parts.push(p);
            }
        }
        parts.retain(|p| !show.iter().any(|s| part(s) == *p));
        parts.sort();
        let group = match suppress.as_str() {
            "yes" => suppress_group.clone(),
            "no" => delete_group.clone(),
            _ => existing
                .as_ref()
                .and_then(|a| a.restrictions.get(READ))
                .and_then(|r| r.group.clone())
                .unwrap_or_else(|| delete_group.clone()),
        };
        let acl = (!parts.is_empty()).then(|| {
            let mut a = Acl::read(target.clone(), ReadKind::Moderation, &group, None);
            a.parts.clone_from(&parts);
            a
        });
        if acl.is_none() && existing.is_none() {
            items.push(json!({"id": revid, "status": "Success"}));
            continue;
        }
        save(ctx, target, acl, &reason, &suppress_group, None).await?;
        let mut item = json!({"id": revid, "status": "Success"});
        for (flag, p) in [
            ("texthidden", content_part),
            ("commenthidden", "comment"),
            ("userhidden", "attestation"),
        ] {
            if parts.iter().any(|x| x == p) {
                item[flag] = Value::Bool(true);
            }
        }
        if group == suppress_group && !parts.is_empty() {
            item["suppressed"] = Value::Bool(true);
        }
        items.push(item);
    }
    Ok(ApiResponse::ok(json!({"revisiondelete": {
        "status": "Success",
        "type": kind,
        "items": items,
    }})))
}
