//! The typed calls the site makes, each returning its value with the caching facts a page
//! composes its own from (0057 §6).

use axum::http::{StatusCode, header};
use serde_json::{Map, Value};
use sha2::Digest as _;
use std::fmt::Write as _;

use crate::transport::{ApiResponse, ClientError};
use crate::{Client, Incoming};

/// The caching facts of one API response.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CacheInfo {
    /// Whether the API sent it `public`.
    pub public: bool,
    /// Its `ETag`.
    pub etag: Option<String>,
    /// Its `Cache-Tag`s.
    pub tags: Vec<String>,
}

impl CacheInfo {
    /// Read from a response's headers.
    #[must_use]
    pub fn of(response: &ApiResponse) -> Self {
        let h = &response.headers;
        let public = h
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|d| d.trim() == "public"));
        let etag = h
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let tags = h
            .get("cache-tag")
            .and_then(|v| v.to_str().ok())
            .map(|v| {
                v.split(',')
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Self { public, etag, tags }
    }
}

impl CacheInfo {
    /// The facts of several responses taken together: public only if each was, the
    /// `ETag`s joined (none if any lacks one), every tag kept.
    #[must_use]
    pub fn combine(all: &[CacheInfo]) -> Self {
        let public = !all.is_empty() && all.iter().all(|c| c.public);
        let etag = all
            .iter()
            .map(|c| c.etag.clone())
            .collect::<Option<Vec<_>>>()
            .map(|v| v.join(" "));
        let mut tags: Vec<String> = all.iter().flat_map(|c| c.tags.iter().cloned()).collect();
        tags.sort();
        tags.dedup();
        Self { public, etag, tags }
    }
}

/// A value and the caching facts of the response it came from.
#[derive(Debug, Clone)]
pub struct Fetched<T> {
    /// The value.
    pub value: T,
    /// The caching facts.
    pub cache: CacheInfo,
}

/// What the site needs of `meta=siteinfo`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SiteInfo {
    /// `general.sitename`.
    pub sitename: String,
    /// `general.server`: the tenant base the request is seen under.
    pub server: String,
    /// `general.lang`.
    pub lang: String,
    /// `triplespace.api_version`; 0 against a stock Wikibase, which has no such field.
    pub api_version: u32,
    /// `triplespace.theme`: the palettes and type sets offered and the theme settings
    /// (0086 §5); `None` from an API that reports none, such as a stock Wikibase.
    pub theme: Option<crate::theme::ThemeInfo>,
    /// Whether the API reports itself in development mode (`triplespace.insecure`).
    pub insecure: bool,
    /// `general.wikibase-conceptbaseuri`: the base of local entities' concept IRIs.
    pub concept_base: String,
    /// `general.wikibase-propertytypes`: the data types the repository registers, each
    /// with its value type, by ID.
    pub property_types: Vec<(String, String)>,
    /// `providers`: the registered providers.
    pub providers: Vec<ProviderInfo>,
    /// `triplespace.capabilities`: what the API serves (0012 §1.4).
    pub capabilities: Vec<String>,
    /// `general.mainpage`: the main page's title.
    pub mainpage: String,
    /// `triplespace.tenant`: the tenant's slug, which the `/acl/tenant/{slug}` route takes.
    pub tenant: String,
    /// Whether the request came to the farm base, which serves the instance's pages
    /// (0085 §4).
    pub at_farm: bool,
    /// Whether the wiki reviews new accounts (`account.approval`, 0087 §6).
    pub account_approval: bool,
    /// The external issuers the wiki offers for login (`triplespace.login`, 0088 §9).
    pub login_issuers: Vec<crate::issuers::LoginIssuer>,
    /// The markup of new posts, `markdown` or `wikitext` (`triplespace.thread.markup`;
    /// 14 §1.6).
    pub thread_markup: String,
    /// The tenant's issuer code, which its accounts' actor keys carry
    /// (`triplespace.issuer`; 0079 §7).
    pub issuer: String,
    /// The farm code, which the instance's own actor key carries, `instance:{code}`
    /// (`triplespace.farm_code`).
    pub farm_code: String,
    /// The wiki's interface overrides and names, by message key and then language, `""`
    /// for the base page (`triplespace.interface.messages`; 0095 §5).
    pub interface: std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// The wiki's skins (`triplespace.skins`; 0097 §8): its default and the ones it
    /// offers; `None` from an API that reports none, which leaves both to the site.
    pub skins: SkinsInfo,
}

/// What `triplespace.skins` reports (0097 §8).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct SkinsInfo {
    /// `skins.default`: the wiki's skin for readers who have not chosen.
    #[serde(default)]
    pub default: Option<String>,
    /// `skins.available`: the skins readers may choose from; `None` for every skin the
    /// instance has.
    #[serde(default)]
    pub available: Option<Vec<String>>,
}

/// A provider as `siprop=providers` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ProviderInfo {
    /// The two-letter code.
    pub code: String,
    /// The slug, as in `mirror/{slug}`.
    pub slug: String,
    /// The display name.
    pub name: String,
    /// The entity types it mints.
    #[serde(default)]
    pub types: Vec<ProviderType>,
    /// The chip's colours.
    #[serde(default)]
    pub chip: Option<ChipColours>,
}

/// One entity type a provider mints.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ProviderType {
    /// The one-letter type code.
    pub code: String,
    /// The Wikibase entity type.
    pub entity_type: String,
    /// The concept IRI template, with `{upstream_id}`.
    pub iri: String,
    /// What the provider's own IDs start with.
    #[serde(default)]
    pub upstream_prefix: String,
    /// The namespace its entities' pages are in, by canonical name (`Item`).
    #[serde(default)]
    pub namespace: Option<String>,
}

/// A provider chip's colours.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
pub struct ChipColours {
    /// The text colour.
    pub color: String,
    /// The background colour.
    pub background: String,
}

/// What the site needs of `meta=userinfo`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserInfo {
    /// The name; for an anonymous viewer, what the API reports in its place.
    pub name: String,
    /// The user ID; 0 when anonymous.
    pub id: u64,
    /// Whether the viewer is anonymous.
    pub anon: bool,
    /// The viewer's preferences, resolved (`uiprop=options`, 0085 §2.2): what the frame
    /// draws the Appearance menu and the `<html>` attributes from.
    pub options: Map<String, Value>,
    /// The open account requests, for a holder of `confirmaccount` (0087 §6).
    pub account_requests: Option<u64>,
    /// The viewer's tenant-wide rights (`uiprop=rights`), which decide what the site
    /// offers: Protect, Block, `Special:UserRights`.
    pub rights: Vec<String>,
    /// The viewer's groups (`uiprop=groups`).
    pub groups: Vec<String>,
}

impl UserInfo {
    /// Whether the viewer holds a right tenant-wide.
    #[must_use]
    pub fn holds(&self, right: &str) -> bool {
        self.rights.iter().any(|r| r == right)
    }
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// What happens when the API refuses the request outright.
pub(crate) fn check(response: &ApiResponse) -> Result<Value, ClientError> {
    match response.status {
        StatusCode::OK => {
            let v: Value = serde_json::from_slice(&response.body)
                .map_err(|e| ClientError::Unexpected(format!("not JSON: {e}")))?;
            if let Some(e) = v.get("error") {
                return Err(ClientError::Api {
                    code: e["code"].as_str().unwrap_or("unknown").to_string(),
                    info: e["info"].as_str().unwrap_or_default().to_string(),
                });
            }
            Ok(v)
        }
        StatusCode::MISDIRECTED_REQUEST => Err(ClientError::Unexpected(
            "the API does not serve this host (421)".into(),
        )),
        s => Err(ClientError::Unexpected(format!("HTTP {s}"))),
    }
}

/// A namespace, as `siprop=namespaces|namespacealiases` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NamespaceRow {
    /// The number.
    pub id: i64,
    /// The local name; empty for the main namespace.
    pub name: String,
    /// The canonical name, where it has one.
    pub canonical: Option<String>,
    /// Other names that reach it (`namespacealiases`).
    pub aliases: Vec<String>,
    /// Whether its pages may have subpages.
    pub subpages: bool,
    /// Whether its pages count as content.
    pub content: bool,
    /// `first-letter` or `case-sensitive`.
    pub case: String,
    /// Its default content model, where not wikitext.
    pub model: Option<String>,
}

/// A message, as `meta=allmessages&amprop=default` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageRow {
    /// The key.
    pub name: String,
    /// The text the wiki shows; `None` when no message has the key.
    pub content: Option<String>,
    /// The text the build ships, where it ships one.
    pub default: Option<String>,
    /// Whether the wiki overrides it (`customised`).
    pub customised: bool,
}

/// What `Special:AllMessages` asks `meta=allmessages` for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageQuery {
    /// `amprefix`.
    pub prefix: String,
    /// `amfilter`: a substring of the key.
    pub filter: String,
    /// `amcustomised`: `all`, `modified` or `unmodified`.
    pub customised: String,
    /// `amlang`; the wiki's language when empty.
    pub lang: String,
    /// `amfrom`: the first key.
    pub from: String,
}

/// A `siteinfo` section decoded whole. An absent section (`null`) is the default, since a
/// stock Wikibase or an older API sends none; a section that is present but does not
/// decode is an error naming the section and the field, never a silent empty value that
/// would hide every provider, issuer or override behind one mistyped field.
fn section<T: serde::de::DeserializeOwned + Default>(
    v: &Value,
    name: &str,
) -> Result<T, ClientError> {
    if v.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(v.clone())
        .map_err(|e| ClientError::Unexpected(format!("siteinfo `{name}` does not decode: {e}")))
}

impl Client {
    /// `meta=allmessages` with `amprop=default`: every message the build ships, with the
    /// wiki's override and the shipped text, filtered as asked (0095 §8).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn all_messages(
        &self,
        incoming: &Incoming,
        ask: &MessageQuery,
    ) -> Result<Fetched<Vec<MessageRow>>, ClientError> {
        let enc =
            |t: &str| -> String { url::form_urlencoded::byte_serialize(t.as_bytes()).collect() };
        let mut path = String::from(
            "/w/api.php?action=query&meta=allmessages&ammessages=*&amprop=default&format=json&formatversion=2",
        );
        for (k, v) in [
            ("amprefix", &ask.prefix),
            ("amfilter", &ask.filter),
            ("amcustomised", &ask.customised),
            ("amlang", &ask.lang),
            ("amfrom", &ask.from),
        ] {
            if !v.is_empty() {
                let _ = write!(path, "&{k}={}", enc(v));
            }
        }
        let r = self.get(incoming, &path).await?;
        let v = check(&r)?;
        let text = |x: &Value| x.as_str().map(str::to_string);
        let value = v["query"]["allmessages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|m| MessageRow {
                        name: text(&m["name"]).unwrap_or_default(),
                        content: text(&m["content"]),
                        default: text(&m["default"]),
                        customised: m
                            .get("customised")
                            .is_some_and(|c| c != &Value::Bool(false)),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `siprop=namespaces|namespacealiases`: the tenant's namespaces, by number.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn namespaces(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Vec<NamespaceRow>>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=siteinfo&siprop=namespaces%7Cnamespacealiases&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let text = |x: &Value| x.as_str().map(str::to_string);
        let mut rows: Vec<NamespaceRow> = v["query"]["namespaces"]
            .as_object()
            .map(|o| {
                o.values()
                    .map(|n| NamespaceRow {
                        id: n["id"].as_i64().unwrap_or_default(),
                        name: text(&n["name"]).unwrap_or_default(),
                        canonical: text(&n["canonical"]),
                        aliases: Vec::new(),
                        subpages: n["subpages"].as_bool().unwrap_or(false),
                        content: n["content"].as_bool().unwrap_or(false),
                        case: text(&n["case"]).unwrap_or_else(|| "first-letter".into()),
                        model: text(&n["defaultcontentmodel"]),
                    })
                    .collect()
            })
            .unwrap_or_default();
        rows.sort_by_key(|n| n.id);
        for a in v["query"]["namespacealiases"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let (Some(id), Some(alias)) = (a["id"].as_i64(), text(&a["alias"]))
                && let Some(row) = rows.iter_mut().find(|n| n.id == id)
            {
                row.aliases.push(alias);
            }
        }
        Ok(Fetched {
            value: rows,
            cache: CacheInfo::of(&r),
        })
    }

    /// `meta=siteinfo&siprop=general|triplespace|providers`, alone, so that the API may
    /// mark it stable and the response is the same for every viewer (0057 §5).
    pub async fn siteinfo(&self, incoming: &Incoming) -> Result<Fetched<SiteInfo>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=siteinfo&siprop=general%7Ctriplespace%7Cproviders&format=json&formatversion=2",
            )
            .await?;
        let mut v = check(&r)?;
        // `general.time` is the server's clock, which makes every response's `ETag` new
        // each second: the page's validator is taken from the rest (0057 §6).
        let mut cache = CacheInfo::of(&r);
        if let Some(g) = v["query"]["general"].as_object_mut() {
            g.remove("time");
        }
        if cache.etag.is_some() {
            let digest = sha2::Sha256::digest(v.to_string().as_bytes());
            let hex = digest[..16].iter().fold(String::new(), |mut h, b| {
                let _ = write!(h, "{b:02x}");
                h
            });
            cache.etag = Some(format!("\"{hex}\""));
        }
        let general = &v["query"]["general"];
        let ts = &v["query"]["triplespace"];
        let value = SiteInfo {
            sitename: general["sitename"].as_str().unwrap_or_default().to_string(),
            server: general["server"].as_str().unwrap_or_default().to_string(),
            lang: general["lang"].as_str().unwrap_or("en").to_string(),
            api_version: ts["api_version"]
                .as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            theme: section(&ts["theme"], "triplespace.theme")?,
            insecure: ts["insecure"].as_bool().unwrap_or(false),
            property_types: general["wikibase-propertytypes"]
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(id, v)| {
                            let vt = v["valuetype"].as_str().unwrap_or_default();
                            (id.clone(), vt.to_string())
                        })
                        .collect()
                })
                .unwrap_or_default(),
            concept_base: general["wikibase-conceptbaseuri"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            providers: section(&v["query"]["providers"], "providers")?,
            capabilities: section(&ts["capabilities"], "triplespace.capabilities")?,
            mainpage: general["mainpage"]
                .as_str()
                .unwrap_or("Main Page")
                .to_string(),
            tenant: ts["tenant"].as_str().unwrap_or_default().to_string(),
            at_farm: ts["at_farm"].as_bool().unwrap_or(false),
            account_approval: ts["account"]["approval"].as_bool().unwrap_or(false),
            login_issuers: section(&ts["login"], "triplespace.login")?,
            thread_markup: ts["thread"]["markup"]
                .as_str()
                .unwrap_or("markdown")
                .to_string(),
            issuer: ts["issuer"].as_str().unwrap_or_default().to_string(),
            farm_code: ts["farm_code"].as_str().unwrap_or_default().to_string(),
            interface: section(
                &ts["interface"]["messages"],
                "triplespace.interface.messages",
            )?,
            skins: if ts["skins"].is_object() {
                section(&ts["skins"], "triplespace.skins")?
            } else {
                SkinsInfo::default()
            },
        };
        Ok(Fetched { value, cache })
    }

    /// `meta=userinfo&uiprop=options|rights|groups`: who the viewer is, their preferences,
    /// and what they may do.
    pub async fn userinfo(&self, incoming: &Incoming) -> Result<Fetched<UserInfo>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=userinfo&uiprop=options|accountrequests|rights|groups&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let u = &v["query"]["userinfo"];
        let anon = match &u["anon"] {
            Value::Bool(b) => *b,
            Value::String(_) => true,
            _ => u["id"].as_u64().unwrap_or(0) == 0,
        };
        let value = UserInfo {
            name: u["name"].as_str().unwrap_or_default().to_string(),
            id: u["id"].as_u64().unwrap_or(0),
            anon,
            options: u["options"].as_object().cloned().unwrap_or_default(),
            account_requests: u["accountrequests"].as_u64(),
            rights: strings(&u["rights"]),
            groups: strings(&u["groups"]),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    #[test]
    fn a_section_is_empty_when_absent_and_an_error_when_malformed() {
        let none: Vec<ProviderInfo> = section(&Value::Null, "providers").unwrap();
        assert!(none.is_empty());
        let bad = serde_json::json!([{"code": "WD", "slug": "wikidata", "name": "Wikidata", "types": [{"code": "Q", "entity_type": "item", "iri": "x", "namespace": 120}]}]);
        let e = section::<Vec<ProviderInfo>>(&bad, "providers").unwrap_err();
        let text = e.to_string();
        assert!(
            text.contains("`providers`") && text.contains("invalid type"),
            "{text}"
        );
        let good = serde_json::json!([{"code": "WD", "slug": "wikidata", "name": "Wikidata", "types": [{"code": "Q", "entity_type": "item", "iri": "x", "namespace": "Item"}]}]);
        let ok: Vec<ProviderInfo> = section(&good, "providers").unwrap();
        assert_eq!(ok[0].types[0].namespace.as_deref(), Some("Item"));
    }

    #[test]
    fn cache_info_reads_the_headers() {
        let mut h = HeaderMap::new();
        h.insert(
            header::CACHE_CONTROL,
            "public, max-age=0, s-maxage=60".parse().unwrap(),
        );
        h.insert(header::ETAG, "\"abc\"".parse().unwrap());
        h.insert("cache-tag", "entity:Q6, entity:Q7".parse().unwrap());
        let c = CacheInfo::of(&ApiResponse {
            status: StatusCode::OK,
            headers: h,
            body: Vec::new(),
        });
        assert!(c.public);
        assert_eq!(c.etag.as_deref(), Some("\"abc\""));
        assert_eq!(c.tags, vec!["entity:Q6", "entity:Q7"]);
        let p = CacheInfo::of(&ApiResponse {
            status: StatusCode::OK,
            headers: HeaderMap::new(),
            body: Vec::new(),
        });
        assert!(!p.public);
    }
}
