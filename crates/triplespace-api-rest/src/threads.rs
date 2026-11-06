//! The thread routes (0019 §9; 0049 §11; 0069 §9; 18 §3.2): reading a thread and a talk
//! page's listing, and every write a person makes to a thread, through the one writer
//! ([`triplespace_api_ingest::threads`]).
//!
//! - `POST /thread`: `create`. Body: `subject`, `target` (`{kind: "page"|"actor", id}`),
//!   `text`, and optionally `also`, `status`, `pinned`. An actor target may name the
//!   account instead, `{kind: "actor", name}`, which is how a client that knows a user
//!   talk page's title starts a thread there; a name no local account holds is
//!   `ts-no-target`.
//! - `GET /thread/{id}`: subject, title, status, home and listings, and the posts as
//!   `as:Note` objects in tree order, with the collapses that stand; `base`, the offset a
//!   later write names.
//! - `POST /thread/{id}/post`: `inReplyTo`, `text`, and optionally `status`, `collapse`
//!   (`{first, last}`) or `uncollapse`.
//! - `PUT /thread/{id}/post/{revid}`: `edit`: `text`, `base`. The post's author needs
//!   `edit`; anyone else `ts-editpost` as well (09 §8.1), and the post then carries
//!   `editedBy`, the other actors who edited it, since a post is speech (19 §5).
//! - `POST /thread/{id}/rename`, `/move`, `/attach`, `/detach`, `/pin`, `/unpin`: each with
//!   `base` and an optional `summary`; `subject`, or `target` (and `keep` on a move). An
//!   actor target may name its account here too, `{kind: "actor", name}`.
//! - `GET /page/{id}/threads`: the threads attached to a talk page or board, pinned first.
//! - `GET /page/{id}/history`: a page's history as activity rows; for a talk page or
//!   board, composite: its own rows and those of every thread attached to it, home or
//!   listing (16 §6.1). The parameters are `GET /activity`'s, less `target`.
//!
//! Creating, posting and editing need `edit`; the rest need `move` (09 §8.1). A write with
//! a session cookie carries its CSRF token as `token`.
//!
//! **Reads follow the ACLs** (0023 §4–§5; 0056 §5). A thread is enclosed by its home talk
//! page, which a subject's deletion can take with it. A thread the viewer may not read is
//! answered as missing when the restriction is confidential, and 410 `ts-thread-deleted`
//! when it was deleted; its deletion group still reads it, marked `deleted`. A post whose
//! text or author a `record` ACL hides carries `texthidden` or `userhidden`, and the part
//! itself only for the group the ACL names; erased text is `texthidden` and `erased`.
//! Writes need what reads need, for the thread and every page they name, and no record
//! is accepted on a deleted thread.
//!
//! **Writes follow the ACLs too** (09 §2; 0019 §12, A27): `edit` on the thread's chain for
//! `create`, a post or an edit, `move` for the rest; and `edit` on the page each page
//! target lists the thread on (its talk page, a board itself) for `create` (the home and
//! every `also`), `move`, `attach`, `detach`, `pin` and `unpin`. A refusal is 403
//! `protectedpage` (`protectednamespace` for a namespace). Foreign threads and
//! `upstream-reply` are not served yet.

#![allow(
    clippy::result_large_err,
    reason = "the parsers return a ready response, as the async routes do"
)]

use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_actors::acl::{Denied, Target as AclTarget, check_on};
use scatter_actors::evaluate::Effective;
use scatter_actors::{ReadDecision, Timestamp, read_decision};
use scatter_ingest::{Attestation, Request};
use scatter_threads::{
    Category, Invalid, Limits, Range, Statuses, Target, ThreadOp, WIKITEXT, Writer,
};
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::auth::{Caller, bearer};
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_api_action::tenant::Tenant;
use triplespace_api_action::thread_view::target_json;
use triplespace_api_action::watching::{Auto, after_write_with};
use triplespace_api_ingest::threads::{PostSigner, ThreadError, ThreadWrite, write};
use triplespace_projections::acl_read::{
    current, listing_chain, page_chain, page_scope, removed, thread_chain,
};

use crate::entity_route::{error, internal};
use crate::settings::{check_token, prelude};

type Body = Map<String, Value>;

fn bad(message: impl Into<String>) -> Response {
    error(StatusCode::BAD_REQUEST, "badbody", message)
}

fn not_found(id: u64) -> Response {
    error(
        StatusCode::NOT_FOUND,
        "ts-thread-not-found",
        format!("no thread has page ID {id}"),
    )
}

fn gone(id: u64) -> Response {
    error(
        StatusCode::GONE,
        "ts-thread-deleted",
        format!("thread {id} has been deleted"),
    )
}

/// The page IDs a record names as targets.
fn page_targets(op: &ThreadOp) -> Vec<u64> {
    let mut out = Vec::new();
    let mut push = |t: &Target| {
        if let Target::Page(id) = t {
            out.push(*id);
        }
    };
    match op {
        ThreadOp::Create { target, also, .. } => {
            push(target);
            also.iter().for_each(&mut push);
        }
        ThreadOp::Move { target, .. }
        | ThreadOp::Attach { target }
        | ThreadOp::Detach { target }
        | ThreadOp::Pin { target }
        | ThreadOp::Unpin { target } => push(target),
        ThreadOp::Post { .. } | ThreadOp::Edit { .. } | ThreadOp::Rename { .. } => {}
    }
    out
}

/// Whether the viewer may read the thread: absent and missing threads answer alike, and a
/// deleted one answers 410 to those outside its deletion group (0056 §5; 0023 §4). The
/// chain, for a reader who may.
async fn thread_gate(
    app: &App,
    tenant: &str,
    who: &Effective,
    id: u64,
    at: Timestamp,
) -> Result<Vec<scatter_actors::Acl>, Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let chain = thread_chain(&*client, tenant, id)
        .await
        .map_err(|e| internal(&e))?
        .ok_or_else(|| not_found(id))?;
    match read_decision(who, &chain, at) {
        ReadDecision::Readable => Ok(chain),
        ReadDecision::Removed { .. } => Err(gone(id)),
        ReadDecision::Absent => Err(not_found(id)),
    }
}

/// Whether the viewer may read a page a record names; a page they may not read is one
/// that does not exist.
async fn page_gate(
    app: &App,
    tenant: &str,
    who: &Effective,
    page: u64,
    at: Timestamp,
) -> Result<(), Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let chain = page_chain(&*client, tenant, page)
        .await
        .map_err(|e| internal(&e))?;
    match read_decision(who, &chain, at) {
        ReadDecision::Readable if !removed(&chain, at) => Ok(()),
        _ => Err(error(
            StatusCode::NOT_FOUND,
            "ts-no-target",
            format!("page {page} does not exist"),
        )),
    }
}

/// A write an ACL refuses: 403, with MediaWiki's code.
fn protected(e: &Denied) -> Response {
    let code = match e {
        Denied::Permission(_) => "permissiondenied",
        Denied::Restricted { target, .. } | Denied::Nobody { target, .. }
            if target.starts_with("acl:namespace:") =>
        {
            "protectednamespace"
        }
        _ => "protectedpage",
    };
    error(StatusCode::FORBIDDEN, code, e.to_string())
}

/// The ACL checks a record needs beyond reading (see the module documentation): its own
/// permission on the thread's chain, which for a `create` is the `Thread` namespace, the
/// `pages` graph and the tenant, and `edit` on the page each page target lists it on.
async fn write_rights(
    app: &App,
    tenant: &str,
    who: &Effective,
    thread: Option<u64>,
    op: &ThreadOp,
    at: Timestamp,
) -> Result<(), Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let own = match op {
        ThreadOp::Create { .. } | ThreadOp::Post { .. } | ThreadOp::Edit { .. } => "edit",
        _ => "move",
    };
    let chain = match thread {
        Some(id) => thread_chain(&*client, tenant, id)
            .await
            .map_err(|e| internal(&e))?
            .unwrap_or_default(),
        None => current(
            &*client,
            tenant,
            &[
                AclTarget::Namespace(i64::from(scatter_threads::NS_THREAD)),
                AclTarget::Graph("pages".into()),
                AclTarget::Tenant(tenant.to_string()),
            ],
        )
        .await
        .map_err(|e| internal(&e))?,
    };
    let scope = if who.scoped.is_empty() {
        Vec::new()
    } else if let Some(id) = thread {
        page_scope(&*client, tenant, id)
            .await
            .map_err(|e| internal(&e))?
    } else {
        let mut s = vec![AclTarget::Namespace(i64::from(scatter_threads::NS_THREAD))];
        for page in page_targets(op) {
            s.extend(
                page_scope(&*client, tenant, page)
                    .await
                    .map_err(|e| internal(&e))?,
            );
        }
        s
    };
    check_on(own, who, &scope, &chain, at).map_err(|e| protected(&e))?;
    for page in page_targets(op) {
        let chain = listing_chain(&*client, tenant, page)
            .await
            .map_err(|e| internal(&e))?;
        let scope = if who.scoped.is_empty() {
            Vec::new()
        } else {
            page_scope(&*client, tenant, page)
                .await
                .map_err(|e| internal(&e))?
        };
        check_on("edit", who, &scope, &chain, at).map_err(|e| protected(&e))?;
    }
    Ok(())
}

/// The API code and status of a refused record.
fn refusal(e: &Invalid) -> (StatusCode, &'static str) {
    let code = match e {
        Invalid::Create | Invalid::Text => "badbody",
        Invalid::Subject(_) => "ts-bad-subject",
        Invalid::Status(_) => "ts-unknown-status",
        Invalid::MediaType(_) => "ts-bad-media-type",
        Invalid::ImportOnly(_) | Invalid::Heading(_) => "ts-import-only",
        Invalid::NotAPost(_) => "ts-not-a-post",
        Invalid::CollapseOrder
        | Invalid::CollapseCrosses(_)
        | Invalid::CollapseReply
        | Invalid::CollapseAndUncollapse
        | Invalid::NotCollapsing(_) => "ts-bad-collapse",
        Invalid::NoTarget(_) => "ts-no-target",
        Invalid::NoHomes(_)
        | Invalid::ActorListing
        | Invalid::Attached(_)
        | Invalid::NotAListing(_)
        | Invalid::NotAttached(_)
        | Invalid::SameHome => "ts-bad-attachment",
        Invalid::TooManyAttachments | Invalid::TooManyPinned(_) => "ts-limit",
        Invalid::Pinned(_) | Invalid::NotPinned(_) => "ts-bad-pin",
    };
    let status = match e {
        Invalid::NoTarget(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, code)
}

fn refused(e: &ThreadError) -> Response {
    match e {
        ThreadError::Invalid(i) => {
            let (status, code) = refusal(i);
            error(status, code, i.to_string())
        }
        ThreadError::NoThread(id) => not_found(*id),
        ThreadError::Deleted(id) => gone(*id),
        ThreadError::NeedsBase(_) => {
            error(StatusCode::BAD_REQUEST, "ts-base-required", e.to_string())
        }
        ThreadError::Conflict { .. } => error(StatusCode::CONFLICT, "editconflict", e.to_string()),
        ThreadError::EmptyPost => error(StatusCode::BAD_REQUEST, "ts-empty-post", e.to_string()),
        ThreadError::Store(_) => internal(e),
    }
}

fn target_of(v: Option<&Value>) -> Result<Target, Response> {
    let v = v.ok_or_else(|| bad("`target` is required"))?;
    match (v.get("kind").and_then(Value::as_str), v.get("id")) {
        (Some("page"), Some(id)) => id
            .as_u64()
            .or_else(|| id.as_str().and_then(|s| s.parse().ok()))
            .map(Target::Page)
            .ok_or_else(|| bad("a page target's `id` is a page ID")),
        (Some("actor"), Some(Value::String(key))) => Ok(Target::Actor(key.clone())),
        _ => Err(bad("`target` is `{kind: \"page\"|\"actor\", id}`")),
    }
}

fn string(body: &Body, key: &str) -> Result<Option<String>, Response> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(bad(format!("`{key}` is a string"))),
    }
}

fn required(body: &Body, key: &str) -> Result<String, Response> {
    string(body, key)?.ok_or_else(|| bad(format!("`{key}` is required")))
}

fn number(body: &Body, key: &str) -> Result<Option<u64>, Response> {
    match body.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_u64()
            .map(Some)
            .ok_or_else(|| bad(format!("`{key}` is a non-negative integer"))),
    }
}

/// A write's tenant, author and body: the caller must hold `right` and have an account,
/// and a session's write must carry its token.
async fn begin_write(
    app: &App,
    origin: &Origin,
    headers: &HeaderMap,
    body: &Bytes,
    right: &str,
) -> Result<(Tenant, String, Body, Effective, String), Response> {
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let (tenant, caller) = prelude(app, &client, origin, headers).await?;
    // A scoped group's right is checked on the thread's chain (09 §3.4).
    if caller.require_somewhere(right).is_err() {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            format!("this needs the \"{right}\" right"),
        ));
    }
    let Some(actor) = caller.identity.actor_key.clone() else {
        return Err(error(
            StatusCode::FORBIDDEN,
            "permissiondenied",
            "writing to a thread needs an account",
        ));
    };
    let mut object: Body = match serde_json::from_slice(body) {
        Ok(Value::Object(o)) => o,
        _ => return Err(bad("the body is a JSON object")),
    };
    check_token(app, &caller, headers, &object)?;
    object.remove("token");
    // The name a wikitext post's signatures link to (14 §1.6).
    let name = caller
        .identity
        .name
        .clone()
        .unwrap_or_else(|| actor.clone());
    Ok((tenant, actor, object, caller.effective, name))
}

/// `thread.max_attachments`, `thread.max_pinned` and `thread.markup`, as the tenant
/// resolves them (14 §1.6, §3.5, §4.2).
async fn limits(app: &App, tenant: &str) -> Limits {
    let mut l = Limits::default();
    let Ok(client) = app.pool().get().await else {
        return l;
    };
    if let Ok((s, _)) = triplespace_accounts::settings::load(&**client, Some(tenant)).await {
        let n = |k: &str| {
            s.value(k)
                .and_then(|v| v.as_u64())
                .and_then(|n| usize::try_from(n).ok())
        };
        if let Some(n) = n("thread.max_attachments") {
            l.max_attachments = n;
        }
        if let Some(n) = n("thread.max_pinned") {
            l.max_pinned = n;
        }
        if s.value("thread.markup")
            .is_some_and(|v| v.as_str() == Some("wikitext"))
        {
            l.markup = WIKITEXT;
        }
    }
    l
}

/// The names a wikitext post's pre-save transform signs with: the writer's, and the
/// tenant's local User and User talk namespaces (11 §1.5; 14 §1.6).
async fn signer(app: &App, tenant: &str, name: &str) -> Option<PostSigner> {
    let client = app.pool().get().await.ok()?;
    let site = triplespace_api_action::pages::Site::load_for(&client, tenant)
        .await
        .ok()?;
    let ns = |n: i32| site.prefixed(n, "x").trim_end_matches(":x").to_string();
    Some(PostSigner {
        name: name.to_string(),
        user_ns: ns(2),
        user_talk_ns: ns(3),
        talk: "talk".into(),
    })
}

/// Writes one record and answers with the thread, the record and the next base.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn save(
    app: &App,
    tenant: &Tenant,
    actor: &str,
    name: &str,
    who: &Effective,
    thread: Option<u64>,
    op: ThreadOp,
    text: Option<String>,
    body: &Body,
) -> Response {
    let base = match number(body, "base") {
        Ok(b) => b,
        Err(r) => return r,
    };
    let summary = match string(body, "summary") {
        Ok(s) => s,
        Err(r) => return r,
    };
    let created = thread.is_none();
    let watchlist = match string(body, "watchlist") {
        Ok(w) => w,
        Err(r) => return r,
    };
    let watchlist_expiry = match string(body, "watchlistexpiry") {
        Ok(w) => w,
        Err(r) => return r,
    };
    if let Some(w) = &watchlist
        && !matches!(w.as_str(), "watch" | "unwatch" | "preferences" | "nochange")
    {
        return bad(format!(
            "`watchlist` is watch, unwatch, preferences or nochange, not \"{w}\""
        ));
    }
    let auto = match &op {
        ThreadOp::Create { .. } => Some(Auto::Thread),
        ThreadOp::Post { .. } => Some(Auto::Post),
        _ => None,
    };
    // The writer must be able to read the thread and every page the record names
    // (0056 §5; 09 §5.5).
    let at = Timestamp(triplespace_api_action::edit::now_micros());
    if let Some(id) = thread
        && let Err(r) = thread_gate(app, &tenant.slug, who, id, at).await
    {
        return r;
    }
    for page in page_targets(&op) {
        if let Err(r) = page_gate(app, &tenant.slug, who, page, at).await {
            return r;
        }
    }
    if let Err(r) = write_rights(app, &tenant.slug, who, thread, &op, at).await {
        return r;
    }
    let mut request = Request::new(
        &tenant.slug,
        Attestation::by(actor),
        triplespace_api_action::edit::now_micros(),
    );
    request.base_offset = base;
    request.comment = summary.filter(|_| op.needs_base());
    let w = ThreadWrite {
        thread,
        op,
        text,
        writer: Writer::Editor,
        limits: limits(app, &tenant.slug).await,
        date: None,
        signer: signer(app, &tenant.slug, name).await,
    };
    let store = app.store();
    let mut cx = match scatter_projection::Backend::begin(store).await {
        Ok(cx) => cx,
        Err(e) => return internal(&e),
    };
    let out = match write(store, app.pipeline(), &mut cx, &request, w).await {
        Ok(out) => out,
        Err(e) => {
            let _ = scatter_projection::Backend::rollback(store, cx).await;
            return refused(&e);
        }
    };
    if let Err(e) = scatter_projection::Backend::commit(store, cx).await {
        return internal(&e);
    }
    // The record is committed; the writer's watch on the thread follows it (16 §4.3).
    if let Some(auto) = auto
        && let Ok(client) = app.pool().get().await
        && let Err(e) = after_write_with(
            &client,
            &tenant.slug,
            actor,
            &(
                triplespace_accounts::watch::kind::PAGE,
                out.thread.to_string(),
            ),
            auto,
            watchlist.as_deref(),
            watchlist_expiry.as_deref(),
        )
        .await
    {
        return error(StatusCode::BAD_REQUEST, &e.code, e.info);
    }
    let title = title_of(app, &tenant.slug, out.thread).await;
    let body = json!({
        "thread": out.thread,
        "title": title,
        "revid": out.revid,
        "base": out.written.offset(),
    });
    http_cache::respond(
        &HeaderMap::new(),
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        "application/json; charset=utf-8",
        body.to_string(),
        Class::Private,
        &[],
    )
}

async fn title_of(app: &App, tenant: &str, thread: u64) -> Option<String> {
    let client = app.pool().get().await.ok()?;
    let id = i64::try_from(thread).ok()?;
    client
        .query_opt(
            "SELECT title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &id],
        )
        .await
        .ok()
        .flatten()
        .map(|r| r.get(0))
}

/// Replaces an actor target that names its account, `{kind: "actor", name}`, with the
/// account's key: a registered account or a bot on the tenant.
async fn actor_by_name(app: &App, tenant: &str, b: &mut Body) -> Result<(), Response> {
    let Some(target) = b.get_mut("target").and_then(Value::as_object_mut) else {
        return Ok(());
    };
    if target.get("kind").and_then(Value::as_str) != Some("actor") || target.contains_key("id") {
        return Ok(());
    }
    let Some(name) = target
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return Ok(());
    };
    let client = app.pool().get().await.map_err(|e| internal(&e))?;
    let key: Option<String> = client
        .query_opt(
            "SELECT actor_key FROM view.actor WHERE tenant = $1 AND name = $2
               AND kind IN ('registered', 'bot')",
            &[&tenant, &name],
        )
        .await
        .map_err(|e| internal(&e))?
        .map(|r| r.get(0));
    match key {
        Some(k) => {
            target.remove("name");
            target.insert("id".into(), Value::String(k));
            Ok(())
        }
        None => Err(error(
            StatusCode::NOT_FOUND,
            "ts-no-target",
            format!("no account here is named \"{name}\""),
        )),
    }
}

/// `POST /thread`.
pub async fn create(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (tenant, actor, b, who, name) =
        match begin_write(&app, &origin, &headers, &body, "edit").await {
            Ok(x) => x,
            Err(r) => return r,
        };
    let mut b = b;
    if let Err(r) = actor_by_name(&app, &tenant.slug, &mut b).await {
        return r;
    }
    let parsed = (|| {
        let name = required(&b, "subject")?;
        let text = required(&b, "text")?;
        let target = target_of(b.get("target"))?;
        let also = match b.get("also") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|t| target_of(Some(t)))
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(bad("`also` is a list of targets")),
        };
        let pinned = b.get("pinned").and_then(Value::as_bool).unwrap_or(false);
        let op = ThreadOp::Create {
            name,
            target,
            also,
            status: string(&b, "status")?,
            pinned,
            media_type: string(&b, "mediaType")?,
            imported_from: None,
        };
        Ok((op, text))
    })();
    match parsed {
        Ok((op, text)) => save(&app, &tenant, &actor, &name, &who, None, op, Some(text), &b).await,
        Err(r) => r,
    }
}

/// `POST /thread/{id}/post`.
pub async fn post(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    body: Bytes,
) -> Response {
    let (tenant, actor, b, who, name) =
        match begin_write(&app, &origin, &headers, &body, "edit").await {
            Ok(x) => x,
            Err(r) => return r,
        };
    let parsed = (|| {
        let text = required(&b, "text")?;
        let collapse = match b.get("collapse") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                serde_json::from_value::<Range>(v.clone())
                    .map_err(|_| bad("`collapse` is `{first, last}`, two revision IDs"))?,
            ),
        };
        let op = ThreadOp::Post {
            in_reply_to: number(&b, "inReplyTo")?,
            status: string(&b, "status")?,
            media_type: string(&b, "mediaType")?,
            heading: None,
            collapse,
            uncollapse: number(&b, "uncollapse")?,
            imported_from: None,
        };
        Ok((op, text))
    })();
    match parsed {
        Ok((op, text)) => {
            save(
                &app,
                &tenant,
                &actor,
                &name,
                &who,
                Some(id),
                op,
                Some(text),
                &b,
            )
            .await
        }
        Err(r) => r,
    }
}

/// `PUT /thread/{id}/post/{revid}`: an edit, by the post's author.
pub async fn edit(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    headers: HeaderMap,
    Path((id, revid)): Path<(u64, u64)>,
    body: Bytes,
) -> Response {
    let (tenant, actor, b, who, name) =
        match begin_write(&app, &origin, &headers, &body, "edit").await {
            Ok(x) => x,
            Err(r) => return r,
        };
    let text = match required(&b, "text") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let author = async {
        let client = app.pool().get().await.ok()?;
        let (r, t) = (i64::try_from(revid).ok()?, i64::try_from(id).ok()?);
        client
            .query_opt(
                "SELECT actor FROM view.post WHERE tenant = $1 AND revid = $2 AND thread_id = $3",
                &[&tenant.slug, &r, &t],
            )
            .await
            .ok()
            .flatten()
            .map(|row| row.get::<_, Option<String>>(0))
    }
    .await;
    // `ts-editpost` from a scoped group holds on the threads its scope encloses (09 §3.4).
    let scope = if who.needs_scope("ts-editpost") {
        match app.pool().get().await {
            Ok(client) => page_scope(&*client, &tenant.slug, id)
                .await
                .unwrap_or_default(),
            Err(e) => return internal(&e),
        }
    } else {
        Vec::new()
    };
    match author {
        None => {
            return error(
                StatusCode::NOT_FOUND,
                "ts-not-a-post",
                format!("revision {revid} is not a post of thread {id}"),
            );
        }
        // Another actor's post, or one whose author was erased, needs `ts-editpost`.
        Some(a) if a.as_deref() != Some(actor.as_str()) && !who.holds_on("ts-editpost", &scope) => {
            return error(
                StatusCode::FORBIDDEN,
                "permissiondenied",
                "editing another actor's post needs the \"ts-editpost\" right",
            );
        }
        Some(_) => {}
    }
    let media_type = match string(&b, "mediaType") {
        Ok(m) => m,
        Err(r) => return r,
    };
    let op = ThreadOp::Edit {
        object: revid,
        media_type,
    };
    save(
        &app,
        &tenant,
        &actor,
        &name,
        &who,
        Some(id),
        op,
        Some(text),
        &b,
    )
    .await
}

/// The operations that change a thread's subject or attachments.
#[derive(Clone, Copy)]
pub(crate) enum Change {
    Rename,
    Move,
    Attach,
    Detach,
    Pin,
    Unpin,
}

async fn change(
    app: App,
    origin: Origin,
    headers: HeaderMap,
    id: u64,
    body: Bytes,
    what: Change,
) -> Response {
    let (tenant, actor, b, who, name) =
        match begin_write(&app, &origin, &headers, &body, "move").await {
            Ok(x) => x,
            Err(r) => return r,
        };
    // A user talk page is named by its account, as on `create` (0019 A30), which is how
    // the site's forms name it (19 §6.1).
    let mut b = b;
    if let Err(r) = actor_by_name(&app, &tenant.slug, &mut b).await {
        return r;
    }
    let op = (|| {
        Ok(match what {
            Change::Rename => ThreadOp::Rename {
                name: required(&b, "subject")?,
            },
            Change::Move => ThreadOp::Move {
                target: target_of(b.get("target"))?,
                keep: b.get("keep").and_then(Value::as_bool).unwrap_or(false),
            },
            Change::Attach => ThreadOp::Attach {
                target: target_of(b.get("target"))?,
            },
            Change::Detach => ThreadOp::Detach {
                target: target_of(b.get("target"))?,
            },
            Change::Pin => ThreadOp::Pin {
                target: target_of(b.get("target"))?,
            },
            Change::Unpin => ThreadOp::Unpin {
                target: target_of(b.get("target"))?,
            },
        })
    })();
    match op {
        Ok(op) => save(&app, &tenant, &actor, &name, &who, Some(id), op, None, &b).await,
        Err(r) => r,
    }
}

macro_rules! change_route {
    ($name:ident, $what:expr, $doc:literal) => {
        #[doc = $doc]
        pub async fn $name(
            State(app): State<App>,
            Extension(origin): Extension<Origin>,
            headers: HeaderMap,
            Path(id): Path<u64>,
            body: Bytes,
        ) -> Response {
            change(app, origin, headers, id, body, $what).await
        }
    };
}

change_route!(rename, Change::Rename, "`POST /thread/{id}/rename`.");
change_route!(move_to, Change::Move, "`POST /thread/{id}/move`.");
change_route!(attach, Change::Attach, "`POST /thread/{id}/attach`.");
change_route!(detach, Change::Detach, "`POST /thread/{id}/detach`.");
change_route!(pin, Change::Pin, "`POST /thread/{id}/pin`.");
change_route!(unpin, Change::Unpin, "`POST /thread/{id}/unpin`.");

fn respond(headers: &HeaderMap, method: &Method, caller: &Caller, body: &Value) -> Response {
    let credentialed = caller.has_session() || bearer(headers).is_some();
    let class = Class::Public.permitted(method, credentialed, false, false);
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[],
    )
}

/// Talk page IDs with their titles, `{ns}:{title}` as `view.page` holds them.
/// `GET /thread/{id}`; with `html=1`, each post also carries `html`, its text rendered by
/// its media type (14 §1.6).
pub async fn get(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<u64>,
    axum::extract::Query(q): axum::extract::Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let at = Timestamp(triplespace_api_action::edit::now_micros());
    let chain = match thread_gate(&app, &tenant.slug, &caller.effective, id, at).await {
        Ok(c) => c,
        Err(r) => return r,
    };
    let mut body = match triplespace_api_action::thread_view::body(
        &*client,
        &tenant.slug,
        &caller.effective,
        id,
        None,
        at,
    )
    .await
    {
        Ok(Some(b)) => b,
        Ok(None) => return not_found(id),
        Err(e) => return internal(&e),
    };
    body["deleted"] = json!(removed(&chain, at));
    if q.get("html").is_some_and(|v| v == "1" || v == "true")
        && let Err(e) = triplespace_api_action::thread_html::add_html(
            &client,
            &tenant.slug,
            &caller.effective,
            &mut body,
        )
        .await
    {
        return internal(&e);
    }
    respond(&headers, &method, &caller, &body)
}

/// Whether the viewer may read a talk page or board: a deleted one answers 410, one they
/// may not read answers as missing.
async fn talk_gate(
    client: &deadpool_postgres::Object,
    tenant: &str,
    who: &Effective,
    id: u64,
    at: Timestamp,
) -> Result<(), Response> {
    let page = page_chain(&**client, tenant, id)
        .await
        .map_err(|e| internal(&e))?;
    match read_decision(who, &page, at) {
        ReadDecision::Readable => Ok(()),
        ReadDecision::Removed { .. } => Err(error(
            StatusCode::GONE,
            "ts-page-deleted",
            format!("page {id} has been deleted"),
        )),
        ReadDecision::Absent => Err(error(
            StatusCode::NOT_FOUND,
            "ts-not-a-talk-page",
            format!("page {id} is not a talk page or board with threads"),
        )),
    }
}

/// `GET /page/{id}/threads`: the threads attached to a talk page or board, pinned first in
/// the order they were pinned, then by last activity.
pub async fn list(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
        Ok(x) => x,
        Err(r) => return r,
    };
    let Ok(talk) = i64::try_from(id) else {
        return not_found(id);
    };
    let at = Timestamp(triplespace_api_action::edit::now_micros());
    if let Err(r) = talk_gate(&client, &tenant.slug, &caller.effective, id, at).await {
        return r;
    }
    let target = client
        .query_opt(
            "SELECT target_kind, target_id FROM view.talk_page WHERE tenant = $1 AND page_id = $2",
            &[&tenant.slug, &talk],
        )
        .await;
    let target = match target {
        Ok(Some(r)) => match r.get::<_, i16>(0) {
            1 => r.get::<_, String>(1).parse().map(Target::Page).ok(),
            _ => Some(Target::Actor(r.get(1))),
        },
        Ok(None) => {
            return error(
                StatusCode::NOT_FOUND,
                "ts-not-a-talk-page",
                format!("page {id} is not a talk page or board with threads"),
            );
        }
        Err(e) => return internal(&e),
    };
    let rows = client
        .query(
            "SELECT t.page_id, p.title, t.subject, t.status, a.home,
               to_char(a.pinned AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
               to_char(t.last_post AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'),
               t.post_count
             FROM view.thread_attachment a
             JOIN view.thread t ON t.tenant = a.tenant AND t.page_id = a.thread_id
             JOIN view.page p ON p.tenant = t.tenant AND p.page_id = t.page_id AND NOT p.deleted
             WHERE a.tenant = $1 AND a.talk_page_id = $2
             ORDER BY a.pinned IS NULL, a.pinned, t.last_post DESC, t.page_id DESC",
            &[&tenant.slug, &talk],
        )
        .await;
    let rows = match rows {
        Ok(r) => r,
        Err(e) => return internal(&e),
    };
    let statuses = Statuses::embedded();
    // A thread the viewer may not read leaves the listing; a deleted one stays for its
    // deletion group, marked (0023 §4).
    let mut threads: Vec<Value> = Vec::with_capacity(rows.len());
    for r in &rows {
        let thread = u64::try_from(r.get::<_, i64>(0)).unwrap_or_default();
        let chain = match thread_chain(&*client, &tenant.slug, thread).await {
            Ok(Some(c)) => c,
            Ok(None) => continue,
            Err(e) => return internal(&e),
        };
        if read_decision(&caller.effective, &chain, at) != ReadDecision::Readable {
            continue;
        }
        let status: String = r.get(3);
        let category = statuses.get(&status).map_or("open", |s| match s.category {
            Category::Open => "open",
            Category::Closed => "closed",
        });
        threads.push(json!({
            "id": thread,
            "title": r.get::<_, String>(1),
            "subject": r.get::<_, String>(2),
            "status": status,
            "category": category,
            "home": r.get::<_, bool>(4),
            "pinned": r.get::<_, Option<String>>(5),
            "last_post": r.get::<_, Option<String>>(6),
            "posts": r.get::<_, i32>(7),
            "deleted": removed(&chain, at),
        }));
    }
    let body = json!({
        "talk_page_id": id,
        "target": target.as_ref().map(target_json),
        "threads": threads,
    });
    respond(&headers, &method, &caller, &body)
}

/// `GET /page/{id}/history`: the page's activity rows, with those of the threads attached
/// to it, after the same read gate as the listing.
pub async fn history(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id): Path<u64>,
    axum::extract::Query(q): axum::extract::Query<std::collections::BTreeMap<String, String>>,
) -> Response {
    {
        let client = match app.pool().get().await {
            Ok(c) => c,
            Err(e) => return internal(&e),
        };
        let (tenant, caller) = match prelude(&app, &client, &origin, &headers).await {
            Ok(x) => x,
            Err(r) => return r,
        };
        let Ok(page) = i64::try_from(id) else {
            return not_found(id);
        };
        let exists = client
            .query_opt(
                "SELECT 1 FROM view.page WHERE tenant = $1 AND page_id = $2 AND latest_offset IS NOT NULL",
                &[&tenant.slug, &page],
            )
            .await;
        match exists {
            Ok(Some(_)) => {}
            Ok(None) => {
                return error(
                    StatusCode::NOT_FOUND,
                    "ts-no-such-page",
                    format!("there is no page {id}"),
                );
            }
            Err(e) => return internal(&e),
        }
        let at = Timestamp(triplespace_api_action::edit::now_micros());
        let chain = match page_chain(&*client, &tenant.slug, id).await {
            Ok(c) => c,
            Err(e) => return internal(&e),
        };
        match read_decision(&caller.effective, &chain, at) {
            ReadDecision::Readable => {}
            ReadDecision::Removed { .. } => {
                return error(
                    StatusCode::GONE,
                    "ts-page-deleted",
                    format!("page {id} has been deleted"),
                );
            }
            ReadDecision::Absent => {
                return error(
                    StatusCode::NOT_FOUND,
                    "ts-no-such-page",
                    format!("there is no page {id}"),
                );
            }
        }
    }
    crate::activity::serve(
        app,
        origin,
        &method,
        &headers,
        &q,
        Some(triplespace_projections::feed::TargetSet::Talk(id)),
    )
    .await
}
