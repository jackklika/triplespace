//! `GET /entity/{id}/changes/{partition}/{offset}` and `GET /entity/{id}/compare`
//! (0012 §5, §7; 18 §5.1; 19 §3.7–3.8): the structured diff of one change set, and of
//! two states of one source.
//!
//! **A change** (`/changes/{partition}/{offset}`) is one record of the entity's history.
//! For a local record the response carries the change set's operation as stored, the
//! diff of [`scatter_wikibase_changeset::diff`] between the local graph's state before
//! and after it (terms and sitelinks as keyed lists, each touched property as a group
//! with its unchanged statements for context), the corrections and retention that
//! changed, the revision's own facts (revision ID, time, actor, comment, tags, size and
//! delta) and the previous local revision. For a mirror record — a `put` — the diff
//! comes from the `changes` summary the sync stored (per property, language and site,
//! how many added, removed and changed) and the upstream versions on either side, with
//! the provider's own diff left to the client to link; under the `latest` policy the
//! previous state is gone (18 §5.1).
//!
//! **A comparison** (`/compare?source=local&from={revid}&to={revid}`) is the same diff
//! between any two local revisions. Mirror states cannot be compared yet: under `latest`
//! only the current `put` exists, and `full` mirrors' states by offset wait for the
//! history policy's write path.
//!
//! Both read the local graph's past states through
//! [`triplespace_projections::revisions`]: the fold of the subject's local records,
//! which `view.entity_revision` replaces when 0083's tier-1 state lands.
//!
//! What is not here yet: the effect block of 18 §5.1 (what the resolved view, the
//! truthy triples and upstream saw), the checkpoint that includes the record, and
//! `action=compare`'s JSON view. The response says so with `effect: null`.

use std::collections::BTreeMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use scatter_log::store::Slot;
use scatter_log_postgres::log;
use scatter_wikibase_changeset::Operation;
use scatter_wikibase_changeset::apply::{Correction, LocalState};
use scatter_wikibase_changeset::diff::Diff;
use scatter_wikibase_model::id::StatementId;
use scatter_wikibase_model::json::Form;
use serde_json::{Map, Value, json};
use triplespace_api_action::App;
use triplespace_api_action::forwarded::Origin;
use triplespace_api_action::http_cache::{self, Class};
use triplespace_projections::common::operation;
use triplespace_projections::feed::iso8601;
use triplespace_projections::revisions::{self, LocalRevision};

use crate::entity_route::{self, error, internal};

/// A revision's own facts, as the response carries them.
fn revision_json(r: &LocalRevision, partition: u64) -> Value {
    json!({
        "partition": partition,
        "offset": r.offset(),
        "revid": r.revid(),
        "time": iso8601(r.header.appended_at),
        "actor": r.actor,
        "comment": r.comment,
        "tags": r.tags,
        "op": r.operation.name(),
    })
}

/// The corrections and retention that differ between two local states.
fn state_json(before: &LocalState, after: &LocalState) -> Value {
    let mut corrections = Vec::new();
    let keys: std::collections::BTreeSet<&StatementId> = before
        .corrections
        .keys()
        .chain(after.corrections.keys())
        .collect();
    let correction = |c: Option<&Correction>| match c {
        None => Value::Null,
        Some(Correction::Suppress) => json!("suppress"),
        Some(Correction::Rank(r)) => json!(r),
    };
    for k in keys {
        let (b, a) = (before.corrections.get(k), after.corrections.get(k));
        if b != a {
            corrections
                .push(json!({"statement": k, "before": correction(b), "after": correction(a)}));
        }
    }
    let mut m = Map::new();
    m.insert("corrections".into(), Value::Array(corrections));
    if before.retention != after.retention {
        m.insert(
            "retention".into(),
            json!({"before": before.retention, "after": after.retention}),
        );
    }
    if before.redirect_to != after.redirect_to {
        m.insert(
            "redirect".into(),
            json!({"before": before.redirect_to, "after": after.redirect_to}),
        );
    }
    Value::Object(m)
}

/// The activity row of one record, for the parts the diff does not hold.
struct RowFacts {
    source: String,
    kind: String,
    target_id: Option<String>,
    revid: Option<i64>,
    size: Option<i32>,
    delta: Option<i32>,
    changes: Option<Value>,
}

async fn row_facts(
    client: &tokio_postgres::Client,
    tenant: &str,
    partition: u64,
    offset: u64,
) -> Result<Option<RowFacts>, tokio_postgres::Error> {
    let row = client
        .query_opt(
            "SELECT source, kind, target_id, revid, size, delta, changes
             FROM view.activity WHERE tenant IN ($1, '') AND partition = $2 AND \"offset\" = $3
             ORDER BY (tenant = $1) DESC LIMIT 1",
            &[
                &tenant,
                &scatter_log_postgres::ids::partition_to_db(partition),
                &i64::try_from(offset).unwrap_or(i64::MAX),
            ],
        )
        .await?;
    Ok(row.map(|r| RowFacts {
        source: r.get(0),
        kind: r.get(1),
        target_id: r.get(2),
        revid: r.get(3),
        size: r.get(4),
        delta: r.get(5),
        changes: r.get(6),
    }))
}

fn respond(
    headers: &HeaderMap,
    method: &Method,
    credentialed: bool,
    id: &str,
    body: &Value,
) -> Response {
    let class = Class::Public.permitted(method, credentialed, false, false);
    http_cache::respond(
        headers,
        StatusCode::OK,
        "application/json; charset=utf-8",
        body.to_string(),
        class,
        &[format!("entity:{id}")],
    )
}

/// `GET /entity/{id}/changes/{partition}/{offset}`.
#[allow(clippy::too_many_lines)]
pub async fn change(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path((id_text, partition, offset)): Path<(String, u64, u64)>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let r = match entity_route::resolve(&app, &client, &origin, &headers, &id_text).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let not_found = || {
        error(
            StatusCode::NOT_FOUND,
            "change-not-found",
            format!("no record {partition}:{offset} of {}", r.id.as_str()),
        )
    };
    let facts = match row_facts(&client, &r.tenant.slug, partition, offset).await {
        Ok(Some(f)) => f,
        Ok(None) => return not_found(),
        Err(e) => return internal(&e),
    };
    if facts.target_id.as_deref() != Some(r.id.as_str()) {
        return not_found();
    }
    let mut body = Map::new();
    body.insert("entity".into(), json!(r.id.as_str()));
    body.insert("source".into(), json!(facts.source));
    body.insert("kind".into(), json!(facts.kind));
    body.insert(
        "record".into(),
        json!({"partition": partition, "offset": offset}),
    );
    body.insert("size".into(), json!(facts.size));
    body.insert("delta".into(), json!(facts.delta));
    body.insert("effect".into(), Value::Null);
    body.insert(
        "integrity".into(),
        json!({"partition": partition, "offset": offset, "checkpoint": null}),
    );
    if facts.source == "local" {
        let change = match revisions::local_change_at(&client, &r.tenant.slug, &r.id, offset).await
        {
            Ok(Some(c)) => c,
            Ok(None) => return not_found(),
            Err(e) => return internal(&e),
        };
        let diff = Diff::between(change.before.entity.as_ref(), change.after.entity.as_ref());
        body.insert(
            "revision".into(),
            revision_json(&change.revision, partition),
        );
        body.insert(
            "previous".into(),
            change.previous.as_ref().map_or(
                Value::Null,
                |p| json!({"revid": p.revid(), "offset": p.offset()}),
            ),
        );
        body.insert(
            "operation".into(),
            serde_json::to_value(&change.revision.operation).unwrap_or(Value::Null),
        );
        body.insert(
            "diff".into(),
            serde_json::to_value(&diff).unwrap_or(Value::Null),
        );
        body.insert("state".into(), state_json(&change.before, &change.after));
        body.insert("changes".into(), Value::Null);
    } else {
        // A mirror record: the `put`'s upstream versions and the stored summary.
        let slot = match log::read(&client, partition, offset).await {
            Ok(s) => s,
            Err(e) => return internal(&e),
        };
        let record = match slot {
            Slot::Record(rec) => rec,
            Slot::Compacted { .. } => return not_found(),
        };
        let op = match operation(&record) {
            Ok(op) => op.map(std::sync::Arc::unwrap_or_clone),
            Err(e) => return internal(&e),
        };
        let (upstream, prev) = match &op {
            Some(Operation::Put {
                upstream,
                prev_upstream,
                ..
            }) => (
                serde_json::to_value(upstream).unwrap_or(Value::Null),
                prev_upstream.as_ref().map_or(Value::Null, |p| {
                    serde_json::to_value(p).unwrap_or(Value::Null)
                }),
            ),
            _ => (Value::Null, Value::Null),
        };
        body.insert(
            "revision".into(),
            json!({
                "partition": partition, "offset": offset,
                "revid": facts.revid,
                "time": iso8601(record.header().appended_at),
                "op": op.as_ref().map(Operation::name),
                "upstream": upstream, "prev_upstream": prev,
            }),
        );
        body.insert("previous".into(), Value::Null);
        body.insert("diff".into(), Value::Null);
        body.insert("changes".into(), facts.changes.unwrap_or(Value::Null));
        if let Some(Operation::Put { entity, .. }) = &op {
            body.insert("after".into(), entity.to_value(Form::Storage));
        }
    }
    respond(
        &headers,
        &method,
        r.credentialed,
        r.id.as_str(),
        &Value::Object(body),
    )
}

/// `GET /entity/{id}/compare?source=local&from={revid}&to={revid}`.
pub async fn compare(
    State(app): State<App>,
    Extension(origin): Extension<Origin>,
    method: Method,
    headers: HeaderMap,
    Path(id_text): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    let client = match app.pool().get().await {
        Ok(c) => c,
        Err(e) => return internal(&e),
    };
    let r = match entity_route::resolve(&app, &client, &origin, &headers, &id_text).await {
        Ok(r) => r,
        Err(r) => return r,
    };
    let bad = |name: &str, v: &str| {
        error(
            StatusCode::BAD_REQUEST,
            "bad-parameter",
            format!("\"{v}\" is not a value for {name}"),
        )
    };
    let source = q.get("source").map_or("local", String::as_str);
    if source != "local" {
        return error(
            StatusCode::BAD_REQUEST,
            "ts-compare-source",
            format!(
                "\"{source}\" cannot be compared yet: only the local graph keeps every state (19 §3.7)"
            ),
        );
    }
    let revid = |name: &str| -> Result<u64, Box<Response>> {
        let v = q.get(name).ok_or_else(|| {
            Box::new(error(
                StatusCode::BAD_REQUEST,
                "missing-parameter",
                format!("{name} is required"),
            ))
        })?;
        v.parse().map_err(|_| Box::new(bad(name, v)))
    };
    let (from, to) = match (revid("from"), revid("to")) {
        (Ok(f), Ok(t)) => (f, t),
        (Err(r), _) | (_, Err(r)) => return *r,
    };
    let not_found = |which: &str, revid: u64| {
        error(
            StatusCode::NOT_FOUND,
            "revision-not-found",
            format!(
                "{which}={revid} is not a local revision of {}",
                r.id.as_str()
            ),
        )
    };
    let all = match revisions::local_revisions(&client, &r.tenant.slug, &r.id).await {
        Ok(v) => v,
        Err(e) => return internal(&e),
    };
    let find = |revid: u64| {
        all.iter()
            .find(|x| x.revid() == Some(revid))
            .map(LocalRevision::offset)
    };
    let (Some(from_offset), Some(to_offset)) = (find(from), find(to)) else {
        return if find(from).is_none() {
            not_found("from", from)
        } else {
            not_found("to", to)
        };
    };
    let (Some((from_rev, before)), Some((to_rev, after))) = (
        revisions::fold_to(&all, from_offset),
        revisions::fold_to(&all, to_offset),
    ) else {
        return internal(&"a revision found by ID has no fold");
    };
    let diff = Diff::between(before.entity.as_ref(), after.entity.as_ref());
    let partition = from_rev.header.partition;
    let body = json!({
        "entity": r.id.as_str(),
        "source": "local",
        "from": revision_json(&from_rev, partition),
        "to": revision_json(&to_rev, partition),
        "diff": diff,
        "state": state_json(&before, &after),
    });
    respond(&headers, &method, r.credentialed, r.id.as_str(), &body)
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_contract_names_both_routes() {
        let doc: serde_json::Value = serde_json::from_str(include_str!(
            "../../../docs/api/triplespace-v0.openapi.json"
        ))
        .unwrap();
        let change = &doc["paths"]["/entity/{id}/changes/{partition}/{offset}"]["get"];
        assert_eq!(change["operationId"], "getEntityChange");
        let compare = &doc["paths"]["/entity/{id}/compare"]["get"];
        assert_eq!(compare["operationId"], "compareEntityRevisions");
        let names: Vec<&str> = compare["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| p["name"].as_str())
            .collect();
        for p in ["id", "source", "from", "to"] {
            assert!(names.contains(&p), "{p} is not in the contract");
        }
        let d = &doc["components"]["schemas"]["EntityDiff"]["properties"];
        for key in ["labels", "descriptions", "aliases", "sitelinks", "claims"] {
            assert!(d.get(key).is_some(), "{key} is not in EntityDiff");
        }
    }
}
