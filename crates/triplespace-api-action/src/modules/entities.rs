//! `wbgetentities` (wikibase-compat §4.0–4.1; 0009 §4; 0017 §1; 0044 §1): entities by
//! ID, with `props` and `languages`; `missing` for an absent local or foreign ID, an
//! empty entity for a valid keyed ID no graph holds, `invalid-entity-id` for anything
//! that is not an ID. `titles` with `sites` is not served (no sitelinks in the milestone).

use scatter_normalize::keyed::KeyedRegistry;
use scatter_wikibase_model::id::{EntityId, IdForm};
use serde_json::{Value, json};

use crate::api::Ctx;
use crate::edit;
use crate::entity_json;
use crate::response::{ApiError, ApiResponse};
use triplespace_projections::read;

/// Parses and normalizes an ID as a client wrote it: `q6` → `Q6`, `QQQ6` → `Q6`
/// (0044 §1), `Item:Q6` → `Q6`, `domain:Example.ORG` → `domain:example.org`.
pub fn parse_id(text: &str) -> Result<EntityId, ApiError> {
    let t = text.trim();
    // A prefixed title stands for its ID.
    let t = if let Some(rest) = t
        .strip_prefix("Item:")
        .or_else(|| t.strip_prefix("Property:"))
    {
        rest.to_string()
    } else if let Some(rest) = t.strip_prefix("Domain:") {
        format!("domain:{rest}")
    } else if let Some(rest) = t.strip_prefix("Keyword:") {
        format!("keyword:{rest}")
    } else if let Some(rest) = t.strip_prefix("Notation:") {
        format!("notation:{rest}")
    } else {
        t.to_string()
    };
    let invalid = |why: String| {
        ApiError::new(
            "invalid-entity-id",
            format!("Invalid entity ID \"{text}\": {why}"),
        )
    };
    if let Some((kt, key)) = t.split_once(':')
        && let Some(def) = KeyedRegistry::default_registry().by_name(kt)
    {
        let key = def.normalize(key).map_err(|e| invalid(e.to_string()))?;
        return EntityId::parse(&format!("{kt}:{key}")).map_err(|e| invalid(e.to_string()));
    }
    // `QQQ6` is the tenant-relative form of `Q6` (0044 §1); `q6` is `Q6`.
    let t = t.strip_prefix("QQ").map_or(t.as_str(), |r| r);
    let mut upper = String::with_capacity(t.len());
    for (i, c) in t.chars().enumerate() {
        upper.push(if i == 0 { c.to_ascii_uppercase() } else { c });
    }
    EntityId::parse(&upper).map_err(|e| invalid(e.to_string()))
}

/// Whether a client may read an entity of this form: a local one, a foreign one of a
/// registered provider, a keyed one.
pub(crate) fn check_form(
    id: &EntityId,
    registry: &scatter_providers::Registry,
) -> Result<(), ApiError> {
    // A colon prefix is a keyed type or one of the tenant's sources (0078 §3); nothing
    // else is an ID.
    if id.form() == IdForm::Keyed
        && let Some((prefix, _)) = id.keyed_parts()
        && KeyedRegistry::default_registry().by_name(prefix).is_none()
        && registry.parse_foreign_id(id.as_str()).is_err()
    {
        return Err(ApiError::new(
            "invalid-entity-id",
            format!("\"{}\" names no keyed type or entity source.", id.as_str()),
        ));
    }
    if id.form() == IdForm::Foreign && registry.parse_foreign_id(id.as_str()).is_err() {
        return Err(ApiError::new(
            "invalid-entity-id",
            format!("\"{}\" names no registered provider.", id.as_str()),
        ));
    }
    Ok(())
}

/// The graph whose record serves an entity: the first contribution's, `local` where the
/// tenant holds it, else the highest-ranked mirror's.
pub fn serving_graph(current: &read::Current) -> Option<&str> {
    current.contributions.first().map(|c| c.graph.as_str())
}

/// The served JSON of one entity, or `missing`. A key-mapped provider's own item ID
/// (`XDQ9`) is an alias of the keyed entity it was written under (05 §4.1): the answer is
/// that entity, under its own ID, with Wikibase's `redirects: {from, to}` beside it.
pub async fn entity_json(ctx: &Ctx, id: &EntityId, as_given: &str) -> Result<Value, ApiError> {
    let registry = ctx.registry().await?;
    check_form(id, registry)?;
    // A source ID in the tenant's table of names, canonicalized (0078 §2).
    let id = &entity_json::canonical_in(id.clone(), registry);
    let missing = || json!({"id": as_given, "missing": ""});
    // An entity under a restriction the caller does not satisfy is absent, and a deleted
    // one is missing to those outside its group (09 §5.3, §6.2); a private tenant's
    // entities are absent for everyone outside it (09 §4.7).
    match edit::current(ctx, id).await? {
        Some(current) => {
            if !crate::pages::entity_readable(ctx, id.as_str(), serving_graph(&current)).await? {
                return Ok(missing());
            }
            Ok(entity_json::present(&current, registry))
        }
        None if id.form() == IdForm::Keyed && !entity_json::is_source_id(id, registry) => {
            if !crate::pages::entity_readable(ctx, id.as_str(), None).await? {
                return Ok(missing());
            }
            Ok(entity_json::empty_keyed(id))
        }
        None => {
            let alias = read::mapped_alias(&ctx.client, registry, &ctx.tenant.slug, id)
                .await
                .map_err(ApiError::internal)?;
            if let Some(target) = alias
                && let Some(current) = edit::current(ctx, &target).await?
            {
                if !crate::pages::entity_readable(ctx, target.as_str(), serving_graph(&current))
                    .await?
                {
                    return Ok(missing());
                }
                let mut v = entity_json::present(&current, registry);
                v["redirects"] = json!({"from": as_given, "to": target.as_str()});
                return Ok(v);
            }
            Ok(missing())
        }
    }
}

/// Runs `wbgetentities`.
pub async fn wbgetentities(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    let ids = ctx.params.list("ids");
    if ids.is_empty() {
        if ctx.params.get("titles").is_some() {
            return Err(ApiError::new(
                "param-missing",
                "Lookup by sites and titles is not served; use ids.",
            ));
        }
        return Err(ApiError::new(
            "param-missing",
            "Either provide the item \"ids\" or pairs of \"sites\" and \"titles\" for corresponding pages.",
        ));
    }
    if ids.len() > 50 {
        return Err(ApiError::new(
            "toomanyvalues",
            "Too many values supplied for parameter \"ids\". The limit is 50.",
        )
        .with("parameter", json!("ids"))
        .with("limit", json!(50)));
    }
    let props = ctx.params.list("props");
    let languages = ctx.params.list("languages");
    ctx.caller.require("read")?;
    let mut entries = Vec::new();
    for given in ids {
        let id = parse_id(&given)?;
        let mut v = entity_json(ctx, &id, &given).await?;
        ctx.cache_tags.push(format!("entity:{}", id.as_str()));
        if v.get("missing").is_none() {
            entity_json::filter(&mut v, &props, &languages);
        }
        // Keyed by the canonical ID, as Wikibase keys by the normalized one; a redirect
        // by its target's, as Wikibase keys a redirected entity.
        let key = if v.get("missing").is_some() {
            given
        } else {
            v.get("id")
                .and_then(Value::as_str)
                .map_or_else(|| id.as_str().to_string(), str::to_string)
        };
        entries.push((key, v));
    }
    // Triplespace's: the visibility of each restricted local entity served, for the
    // site's Restricted pill (19 §1.3); absent where every entity is public.
    let mut visibility = serde_json::Map::new();
    for (key, v) in &entries {
        if v.get("missing").is_some() || !v.get("id").is_some_and(|i| i == key) {
            continue;
        }
        let Ok(id) = parse_id(key) else { continue };
        if id.form() == IdForm::Foreign {
            continue;
        }
        let chain = triplespace_projections::acl_read::entity_chain(
            ctx.db(),
            &ctx.tenant.slug,
            id.as_str(),
            None,
        )
        .await
        .map_err(ApiError::internal)?;
        if let Some(vis) = crate::pages::visibility_of(&chain) {
            visibility.insert(key.clone(), vis);
        }
    }
    let mut out = json!({
        "entities": entity_json::entities_object(entries),
        "success": 1,
    });
    if !visibility.is_empty() {
        out["ts-visibility"] = Value::Object(visibility);
    }
    Ok(ApiResponse::ok(out))
}
