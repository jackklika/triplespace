//! A fake API serving recorded responses (`tests/fixtures/`, taken from a server loaded
//! with `crates/triplespace-server/tests/fixtures/`, plus a foreign item written by hand),
//! shared by the entity tests and the parity harness.
//!
//! The statement shapes (0003 §3–5) are served from the Wikidata snapshots in
//! `docs/samples/wikidata`, as a Wikidata mirror would serve them (`WDQ65`, `WDP1082`), so
//! that the mirror's role map applies.

#![allow(dead_code)]

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode, header};
use serde_json::{Value, json};
use triplespace_client::transport::BoxFuture;
use triplespace_client::{ApiRequest, ApiResponse, ClientError, Transport};

pub fn fixture(name: &str) -> Option<Value> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path)
        .ok()
        .map(|t| serde_json::from_str(&t).expect("fixture parses"))
}

/// An ID as the API normalizes a title.
pub fn id_of(text: &str) -> String {
    let t = text.trim();
    for (prefix, keyed) in [("Item:", ""), ("Property:", ""), ("Domain:", "domain:")] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return format!("{keyed}{rest}");
        }
    }
    t.to_string()
}

pub fn decode(s: &str) -> String {
    percent_encoding::percent_decode_str(s)
        .decode_utf8_lossy()
        .into_owned()
}

pub fn param(q: &str, name: &str) -> Option<String> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(&format!("{name}=")))
        .map(decode)
}

#[derive(Clone)]
pub struct FixtureApi {
    pub entities: BTreeMap<String, Value>,
}

/// Whether `s` is a Wikidata item, property or lexeme ID (`Q65`, `P1082`), or a
/// statement ID with one as its subject.
fn wikidata_id(s: &str) -> bool {
    let subject = s.split_once('$').map_or(s, |(subject, _)| subject);
    let mut chars = subject.chars();
    matches!(chars.next(), Some('Q' | 'P' | 'L'))
        && !chars.as_str().is_empty()
        && chars.as_str().bytes().all(|b| b.is_ascii_digit())
}

/// A Wikidata entity as the mirror serves it: every Wikidata ID it names, in IDs,
/// properties, keys, orders and badges, in its `WD` form (0017 §2).
pub fn mirrored(v: &Value, key: &str) -> Value {
    let wd = |s: &str| {
        if wikidata_id(s) {
            format!("WD{s}")
        } else {
            s.to_string()
        }
    };
    match v {
        Value::String(s) if matches!(key, "id" | "property" | "list") => Value::String(wd(s)),
        Value::Array(a) => {
            let inner = if matches!(key, "qualifiers-order" | "snaks-order" | "badges") {
                "list"
            } else {
                key
            };
            Value::Array(a.iter().map(|x| mirrored(x, inner)).collect())
        }
        Value::Object(o) => {
            let keyed = matches!(key, "claims" | "qualifiers" | "snaks");
            Value::Object(
                o.iter()
                    .filter(|(k, _)| k.as_str() != "numeric-id")
                    .map(|(k, x)| {
                        let k2 = if keyed { wd(k) } else { k.clone() };
                        (k2, mirrored(x, k))
                    })
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

impl FixtureApi {
    pub fn new() -> Self {
        let e = fixture("entities.json").unwrap();
        Self {
            entities: e["entities"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// The fixtures, with the Wikidata snapshots mirrored over them (the real `WDQ65`
    /// replaces the one written by hand), and an item made up for the shapes the
    /// snapshots lack.
    pub fn wikidata() -> Self {
        let mut api = Self::new();
        for q in ["Q65", "Q339", "Q731", "Q1520"] {
            let path = format!(
                "{}/../../docs/samples/wikidata/wikidata-{q}.json",
                env!("CARGO_MANIFEST_DIR")
            );
            let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            api.entities
                .insert(format!("WD{q}"), mirrored(&v["entities"][q], ""));
        }
        api.entities.insert("WDQ900001".into(), made_up());
        api
    }

    #[allow(clippy::too_many_lines)]
    fn answer(&self, path_and_query: &str, signed_in: bool) -> (StatusCode, Value, Vec<String>) {
        let (path, query) = path_and_query
            .split_once('?')
            .unwrap_or((path_and_query, ""));
        if let Some(rest) = path.strip_prefix("/w/rest.php/triplespace/v0/entity/")
            && let Some(id) = rest.strip_suffix("/history")
        {
            // The entity's rows of the recorded feed (tests/fixtures/activity.json).
            let id = id_of(&decode(id));
            if !self.entities.contains_key(&id) {
                return (
                    StatusCode::NOT_FOUND,
                    json!({"code": "entity-not-found", "message": "no entity"}),
                    vec![],
                );
            }
            let source = param(query, "source");
            let rows: Vec<Value> = fixture("activity.json").unwrap()["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["target"]["id"] == id)
                .filter(|r| source.as_deref().is_none_or(|s| r["source"] == s))
                .cloned()
                .collect();
            return (
                StatusCode::OK,
                json!({"rows": rows, "set": {"kind": "entity", "id": id}, "kinds": [], "sources": []}),
                vec![format!("entity:{id}")],
            );
        }
        if let Some(rest) = path.strip_prefix("/w/rest.php/triplespace/v0/entity/")
            && let Some((id, record)) = rest.split_once("/changes/")
        {
            // One change's diff, recorded (tests/fixtures/change-*.json).
            let id = id_of(&decode(id));
            let name = format!(
                "change-{}-{}.json",
                id.replace(':', "-"),
                record.replace('/', "-")
            );
            return match fixture(&name) {
                Some(v) if self.entities.contains_key(&id) => {
                    (StatusCode::OK, v, vec![format!("entity:{id}")])
                }
                _ => (
                    StatusCode::NOT_FOUND,
                    json!({"code": "change-not-found", "message": "no such record"}),
                    vec![],
                ),
            };
        }
        if let Some(rest) = path.strip_prefix("/w/rest.php/triplespace/v0/entity/")
            && let Some(id) = rest.strip_suffix("/compare")
        {
            // A comparison: the recorded change's diff under two revisions.
            let id = id_of(&decode(id));
            let (from, to) = (param(query, "from"), param(query, "to"));
            if from.as_deref() != Some("900") || to.as_deref() != Some("907") {
                return (
                    StatusCode::NOT_FOUND,
                    json!({"code": "revision-not-found", "message": "no such revision"}),
                    vec![],
                );
            }
            let change = fixture("change-Q6-4098-9.json").unwrap();
            return (
                StatusCode::OK,
                json!({"entity": id, "source": "local",
                       "from": {"partition": 4098, "offset": 1, "revid": 900, "time": "2026-10-01T09:30:00Z", "actor": "librarybase:7", "op": "adopt"},
                       "to": change["revision"], "diff": change["diff"], "state": {"corrections": []}}),
                vec![format!("entity:{id}")],
            );
        }
        if query.contains("prop=revisions") {
            // `revids=907` is Q6's recorded change; its content is Q6 as it is now.
            let Some(revid) = param(query, "revids") else {
                return (StatusCode::OK, json!({"query": {"pages": []}}), vec![]);
            };
            if revid != "907" {
                return (
                    StatusCode::OK,
                    json!({"query": {"pages": [{"revid": revid, "missing": true}]}}),
                    vec![],
                );
            }
            let content = self.entities["Q6"].to_string();
            return (
                StatusCode::OK,
                json!({"query": {"pages": [{"pageid": 1, "ns": 120, "title": "Item:Q6", "revisions": [{
                    "revid": 907, "timestamp": "2026-10-02T14:05:11Z", "user": "Alice bot", "comment": "rank and a value",
                    "record": {"partition": 4098, "offset": 9},
                    "slots": {"main": {"contentmodel": "wikibase-item", "contentformat": "application/json", "content": content}}
                }]}]}}),
                vec!["entity:Q6".into()],
            );
        }
        if let Some(rest) = path.strip_prefix("/w/rest.php/triplespace/v0/entity/") {
            let id = id_of(&decode(rest.trim_end_matches("/provenance")));
            return match fixture(&format!("provenance-{}.json", id.replace(':', "-"))) {
                Some(v) => (StatusCode::OK, v, vec![format!("entity:{id}")]),
                None => (
                    StatusCode::NOT_FOUND,
                    json!({"code": "entity-not-found", "message": "no entity"}),
                    vec![],
                ),
            };
        }
        if query.contains("meta=siteinfo") {
            return (StatusCode::OK, fixture("siteinfo.json").unwrap(), vec![]);
        }
        if query.contains("meta=userinfo") && signed_in {
            return (
                StatusCode::OK,
                json!({"query": {"userinfo": {"id": 7, "name": "Alice"}}}),
                vec![],
            );
        }
        if query.contains("meta=userinfo") {
            return (
                StatusCode::OK,
                json!({"query": {"userinfo": {"id": 0, "name": "127.0.0.1", "anon": true}}}),
                vec![],
            );
        }
        if query.contains("meta=tokens") {
            return (
                StatusCode::OK,
                json!({"query": {"tokens": {"logintoken": "t+\\", "csrftoken": "c+\\"}}}),
                vec![],
            );
        }
        if query.contains("action=wbsearchentities") {
            return (StatusCode::OK, self.search(query), vec![]);
        }
        if query.contains("action=wbgetentities") {
            let mut out = serde_json::Map::new();
            let mut tags = Vec::new();
            for given in param(query, "ids").unwrap_or_default().split('|') {
                let id = id_of(given);
                if id.chars().next().is_some_and(char::is_lowercase) && !id.contains(':') {
                    return (
                        StatusCode::OK,
                        json!({"error": {"code": "invalid-entity-id", "info": "Invalid entity ID"}}),
                        vec![],
                    );
                }
                tags.push(format!("entity:{id}"));
                match self.entities.get(&id) {
                    Some(v) => {
                        out.insert(id, v.clone());
                    }
                    None => {
                        out.insert(given.to_string(), json!({"id": given, "missing": ""}));
                    }
                }
            }
            return (StatusCode::OK, json!({"entities": out, "success": 1}), tags);
        }
        (StatusCode::NOT_FOUND, json!({}), vec![])
    }
}

impl FixtureApi {
    /// `wbsearchentities` over the fixtures, as the API's Postgres fallback answers it: a
    /// prefix of a label (first) or an alias, in the language, `mul` or English.
    fn search(&self, query: &str) -> Value {
        let text = param(query, "search").unwrap_or_default().to_lowercase();
        let lang = param(query, "language").unwrap_or_else(|| "en".into());
        let ty = param(query, "type").unwrap_or_else(|| "item".into());
        let limit: usize = param(query, "limit")
            .and_then(|l| l.parse().ok())
            .unwrap_or(7);
        let offset: usize = param(query, "continue")
            .and_then(|l| l.parse().ok())
            .unwrap_or(0);
        let langs = [lang.as_str(), "mul", "en"];
        let mut hits = Vec::new();
        for (id, e) in &self.entities {
            if e["type"].as_str() != Some(ty.as_str()) {
                continue;
            }
            let label = langs
                .iter()
                .find_map(|l| e["labels"][*l]["value"].as_str().map(|v| (*l, v)));
            let matched = langs
                .iter()
                .find_map(|l| {
                    e["labels"][*l]["value"]
                        .as_str()
                        .filter(|v| v.to_lowercase().starts_with(&text))
                        .map(|v| ("label", *l, v))
                })
                .or_else(|| {
                    langs.iter().find_map(|l| {
                        e["aliases"][*l].as_array().and_then(|a| {
                            a.iter()
                                .filter_map(|x| x["value"].as_str())
                                .find(|v| v.to_lowercase().starts_with(&text))
                                .map(|v| ("alias", *l, v))
                        })
                    })
                });
            let Some((kind, ml, mt)) = matched else {
                continue;
            };
            let mut hit = json!({
                "id": id, "title": e["title"].as_str().unwrap_or(id),
                "display": {}, "match": {"type": kind, "language": ml, "text": mt}
            });
            if let Some((l, v)) = label {
                hit["display"]["label"] = json!({"value": v, "language": l});
            }
            if let Some(d) = langs
                .iter()
                .find_map(|l| e["descriptions"][*l]["value"].as_str().map(|v| (*l, v)))
            {
                hit["display"]["description"] = json!({"value": d.1, "language": d.0});
            }
            hits.push(hit);
        }
        let mut body = json!({"searchinfo": {"search": text}, "search": hits.iter().skip(offset).take(limit).collect::<Vec<_>>(), "success": 1});
        if hits.len() > offset + limit {
            body["search-continue"] = json!(offset + limit);
        }
        body
    }
}

impl Transport for FixtureApi {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let signed_in = request.headers.contains_key(header::COOKIE);
            let (status, body, tags) = self.answer(&request.path_and_query, signed_in);
            let body = serde_json::to_vec(&body).unwrap();
            let mut headers = HeaderMap::new();
            if status == StatusCode::OK {
                let etag = format!("\"{:x}\"", body.len() * 31 + request.path_and_query.len());
                headers.insert(header::ETAG, etag.parse().unwrap());
                let cc = if signed_in {
                    "private, no-cache"
                } else {
                    "public, max-age=0, s-maxage=60"
                };
                headers.insert(header::CACHE_CONTROL, cc.parse().unwrap());
                if !tags.is_empty() {
                    headers.insert("cache-tag", tags.join(", ").parse().unwrap());
                }
            }
            Ok(ApiResponse {
                status,
                headers,
                body,
            })
        })
    }

    fn embedded(&self) -> bool {
        false
    }
}

/// An item with an article's authors by series ordinal (a numbered list) and a property
/// whose only value is deprecated (no current value).
pub fn made_up() -> Value {
    let author = |n: u32| {
        json!({
            "id": format!("WDQ900001$00000000-0000-0000-0000-{n:012}"),
            "mainsnak": {"snaktype": "value", "property": "WDP2093", "datatype": "string",
                "datavalue": {"type": "string", "value": format!("Author {n}")}},
            "qualifiers": {"WDP1545": [{"snaktype": "value", "property": "WDP1545", "datatype": "string",
                "datavalue": {"type": "string", "value": n.to_string()}}]},
            "qualifiers-order": ["WDP1545"],
            "rank": "normal", "type": "statement"
        })
    };
    let authors: Vec<Value> = [12, 3, 1, 10, 2].into_iter().map(author).collect();
    json!({
        "type": "item", "id": "WDQ900001", "title": "Item:WDQ900001", "ns": 120,
        "labels": {"en": {"language": "en", "value": "A made-up article"}},
        "claims": {
            "WDP2093": authors,
            "WDP31": [{
                "id": "WDQ900001$00000000-0000-0000-0000-00000000aaaa",
                "mainsnak": {"snaktype": "value", "property": "WDP31", "datatype": "wikibase-item",
                    "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "WDQ5"}}},
                "qualifiers": {"WDP2241": [{"snaktype": "value", "property": "WDP2241", "datatype": "wikibase-item",
                    "datavalue": {"type": "wikibase-entityid", "value": {"entity-type": "item", "id": "WDQ6"}}}]},
                "qualifiers-order": ["WDP2241"],
                "rank": "deprecated", "type": "statement"
            }]
        }
    })
}
