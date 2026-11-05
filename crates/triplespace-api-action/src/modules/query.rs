//! `action=query` (mediawiki-compat §5.4): `meta=siteinfo`, `meta=tokens`,
//! `meta=userinfo`, `meta=wikibase`; `list=recentchanges`, `list=usercontribs` and
//! `list=logevents` ([`super::changes`]). Other `meta`, `prop` and `list` values are
//! warned about and ignored, as MediaWiki does with an unknown parameter value in a
//! module it does serve.

use scatter_providers::Registry;
use scatter_wikibase_model::value::DataType;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use triplespace_accounts::session;
use triplespace_projections::pages::TenantNames;

use crate::api::Ctx;
use crate::app::Mode;
use crate::auth::set_cookie;
use crate::response::{ApiError, ApiResponse};

/// Runs `action=query`.
pub async fn run(ctx: &mut Ctx) -> Result<ApiResponse, ApiError> {
    let mut query = Map::new();
    let mut body = Map::new();
    let mut response = ApiResponse::default();
    for meta in ctx.params.list("meta") {
        match meta.as_str() {
            "siteinfo" => siteinfo(ctx, &mut query).await?,
            "tokens" => tokens(ctx, &mut query, &mut response).await?,
            "userinfo" => {
                userinfo(ctx, &mut query).await?;
                // The options depend on the reader and on `Accept-Language`.
                if ctx
                    .params
                    .list("uiprop")
                    .iter()
                    .any(|p| p == "options" || p == "*")
                {
                    ctx.cache_class = crate::http_cache::Class::Private;
                }
            }
            "wikibase" => wikibase(ctx, &mut query),
            "authmanagerinfo" => super::account::authmanagerinfo(ctx, &mut query).await?,
            "notifications" => super::notifications::meta(ctx, &mut query).await?,
            "allmessages" => allmessages(ctx, &mut query).await?,
            other => response.warn(
                "query",
                &format!("Unrecognized value for parameter \"meta\": {other}."),
            ),
        }
    }
    // `meta=siteinfo` alone changes only with configuration, which no purge touches, so
    // the web tier may keep it for a minute (0057 §5).
    if ctx.params.list("meta") == ["siteinfo"]
        && ctx.params.list("prop").is_empty()
        && ctx.params.list("list").is_empty()
    {
        ctx.cache_class = crate::http_cache::Class::Stable;
    }
    // `prop=revisions` lists its pages first, so `prop=info` adds its fields to them.
    let mut props = ctx.params.list("prop");
    props.sort_by_key(|p| p != "revisions");
    for p in props {
        match p.as_str() {
            "revisions" => {
                super::revisions::run(ctx, &mut query, &mut body, &mut response).await?;
            }
            "deletedrevisions" => {
                super::revisions::deleted(ctx, &mut query, &mut body, &mut response).await?;
            }
            "info" => super::info::info(ctx, &mut query).await?,
            "templates" => super::info::templates(ctx, &mut query).await?,
            other => response.warn(
                "query",
                &format!("Unrecognized value for parameter \"prop\": {other}."),
            ),
        }
    }
    for l in ctx.params.list("list") {
        match l.as_str() {
            "recentchanges" => {
                super::changes::recentchanges(ctx, &mut query, &mut body, &mut response).await?;
            }
            "usercontribs" => {
                super::changes::usercontribs(ctx, &mut query, &mut body, &mut response).await?;
            }
            "logevents" => {
                super::changes::logevents(ctx, &mut query, &mut body, &mut response).await?;
            }
            "watchlist" => {
                ctx.cache_class = crate::http_cache::Class::Private;
                super::changes::watchlist(ctx, &mut query, &mut body, &mut response).await?;
            }
            "watchlistraw" => {
                ctx.cache_class = crate::http_cache::Class::Private;
                super::watch::raw(ctx, &mut body).await?;
            }
            "allpages" => {
                super::info::allpages(ctx, &mut query, &mut body, &mut response).await?;
            }
            "protectedpages" => super::protected::pages(ctx, &mut query, &mut body).await?,
            "protectedtitles" => super::protected::titles(ctx, &mut query, &mut body).await?,
            "protectedsets" => super::protected::sets(ctx, &mut query).await?,
            "users" => super::userrights::users(ctx, &mut query).await?,
            "blocks" => super::block::list(ctx, &mut query).await?,
            "querypage" => super::querypage::querypage(ctx, &mut query, &mut body).await?,
            other => response.warn(
                "query",
                &format!("Unrecognized value for parameter \"list\": {other}."),
            ),
        }
    }
    if ctx.formatversion == 2 {
        body.insert("batchcomplete".into(), json!(true));
    } else {
        body.insert("batchcomplete".into(), json!(""));
    }
    if !query.is_empty() {
        body.insert("query".into(), Value::Object(query));
    }
    response.body = body;
    Ok(response)
}

/// `meta=tokens`: a token per requested type for the caller's session or key. A caller
/// without either gets a session, so a login token has something to bind to.
async fn tokens(
    ctx: &mut Ctx,
    query: &mut Map<String, Value>,
    response: &mut ApiResponse,
) -> Result<(), ApiError> {
    let mut types = ctx.params.list("type");
    if types.is_empty() {
        types.push("csrf".into());
    }
    if types.iter().any(|t| t == "*") {
        types = [
            "createaccount",
            "csrf",
            "login",
            "patrol",
            "rollback",
            "userrights",
            "watch",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    }
    if ctx.caller.identity.token_binding().is_none() {
        let s = session::create(ctx.db(), &ctx.tenant.slug, None, None).await?;
        response.set_cookie = Some(set_cookie(
            &ctx.app,
            &s.id,
            session::ANONYMOUS_LIFETIME.as_secs(),
        ));
        ctx.caller.identity.session = Some(s);
    }
    let mut out = Map::new();
    for t in types {
        match t.as_str() {
            t if TOKEN_TYPES.contains(&t) => {
                out.insert(format!("{t}token"), json!(ctx.caller.token(&ctx.app, t)));
            }
            other => response.warn(
                "tokens",
                &format!("Unrecognized value for parameter \"type\": {other}."),
            ),
        }
    }
    query.insert("tokens".into(), Value::Object(out));
    Ok(())
}

/// `meta=userinfo` (0016 §7; 0024 §8; 0027 §6): `id`, `name`, `anon`, and by `uiprop`
/// `groups`, `rights`, `blockinfo`, `ratelimits` and `options` (every registered
/// preference's effective value, defaults filled in, as MediaWiki reports them), plus
/// `operator` and `grants` for a subsidiary.
/// `ts-scopedrights`: each permission a scoped group grants, with where it holds.
fn scoped_rights(e: &scatter_actors::Effective) -> Option<Value> {
    let scoped: Map<String, Value> = e
        .scoped
        .iter()
        .map(|(p, within)| {
            (
                p.clone(),
                json!(
                    within
                        .iter()
                        .map(|t| t.key().trim_start_matches("acl:").to_string())
                        .collect::<Vec<_>>()
                ),
            )
        })
        .collect();
    (!scoped.is_empty()).then_some(Value::Object(scoped))
}

async fn userinfo(ctx: &Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    let id = &ctx.caller.identity;
    let mut u = Map::new();
    let numeric = id
        .actor_key
        .as_deref()
        .and_then(|k| k.rsplit_once(':'))
        .and_then(|(_, n)| n.parse::<u64>().ok());
    u.insert("id".into(), json!(numeric.unwrap_or(0)));
    if let Some(n) = &id.name {
        u.insert("name".into(), json!(n));
    } else {
        u.insert("name".into(), json!("127.0.0.1"));
        u.insert(
            "anon".into(),
            if ctx.formatversion == 2 {
                json!(true)
            } else {
                json!("")
            },
        );
    }
    let props = ctx.params.list("uiprop");
    let want = |p: &str| props.iter().any(|x| x == p || x == "*");
    if want("groups") {
        u.insert(
            "groups".into(),
            json!(
                ctx.caller
                    .effective
                    .groups
                    .iter()
                    .map(|g| if g == "universe" { "*" } else { g.as_str() })
                    .collect::<Vec<_>>()
            ),
        );
    }
    if want("rights") {
        u.insert("rights".into(), json!(ctx.caller.effective.permissions));
        // A scoped group's permissions hold only where its `within` says, so they are
        // not MediaWiki's `rights`, which clients read as holding everywhere (09 §3.4).
        if let Some(scoped) = scoped_rights(&ctx.caller.effective) {
            u.insert("ts-scopedrights".into(), scoped);
        }
    }
    if want("changeablegroups") {
        let registry = triplespace_accounts::groups::registry(ctx.db(), &ctx.tenant.slug).await?;
        u.insert(
            "changeablegroups".into(),
            super::userrights::changeable(&ctx.caller.effective, &registry),
        );
    }
    if want("blockinfo") {
        // No blocks in the milestone's write path; the field is present and empty-shaped.
    }
    if want("ratelimits") {
        u.insert("ratelimits".into(), json!({}));
    }
    if want("hasmsg") {
        // No notifications yet.
    }
    if let Some(op) = &id.operator {
        u.insert("operator".into(), json!(op));
    }
    if want("accountrequests") && ctx.caller.effective.holds("confirmaccount") {
        // Triplespace's: the open account requests a reviewer has waiting (0087 §6).
        u.insert(
            "accountrequests".into(),
            json!(triplespace_accounts::requests::open_count(ctx.db(), &ctx.tenant.slug).await?),
        );
    }
    if let Some(k) = &id.credential {
        u.insert("grants".into(), json!(k.grants));
    }
    if want("options") {
        let accept = ctx.accept_language.as_deref();
        let resolved = crate::prefs::resolve(
            ctx.db(),
            &ctx.tenant.slug,
            id.actor_key.as_deref(),
            ctx.params.get("uselang"),
            accept,
        )
        .await?;
        let options: Map<String, Value> = resolved
            .values
            .into_iter()
            .filter_map(|(k, r)| r.value.map(|v| (k, v)))
            .collect();
        u.insert("options".into(), Value::Object(options));
    }
    query.insert("userinfo".into(), Value::Object(u));
    Ok(())
}

#[derive(Deserialize)]
struct NamespaceRow {
    id: i64,
    canonical: String,
    kind: String,
    #[serde(default)]
    case: Option<String>,
    #[serde(default)]
    subpages: Option<bool>,
    #[serde(default)]
    default_model: Option<String>,
    #[serde(default)]
    enabled_by: Option<String>,
}

#[derive(Deserialize)]
struct NamespaceFile {
    namespace: Vec<NamespaceRow>,
}

/// `meta=allmessages` (0095 §8): the wiki's overrides over the messages the build ships.
async fn allmessages(ctx: &mut Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    let p = &ctx.params;
    let list = |name: &str| p.list(name);
    let ask = crate::interface::AllMessages {
        messages: list("ammessages"),
        lang: p.get("amlang").unwrap_or_default().to_string(),
        customised: p.get("amcustomised").unwrap_or("all").to_string(),
        prefix: p.get("amprefix").unwrap_or_default().to_string(),
        filter: p.get("amfilter").unwrap_or_default().to_string(),
        from: p.get("amfrom").unwrap_or_default().to_string(),
        to: p.get("amto").unwrap_or_default().to_string(),
        args: list("amargs"),
        nocontent: p.flag("amnocontent"),
        includelocal: p.flag("amincludelocal"),
        default: list("amprop").iter().any(|v| v == "default"),
    };
    let (settings, _) =
        triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let content_lang = settings
        .value("site.language")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".into());
    let pages = crate::interface::pages(ctx.db(), &ctx.tenant.slug).await?;
    let o = crate::interface::Overrides::from_pages(&pages);
    query.insert(
        "allmessages".into(),
        Value::Array(crate::interface::allmessages(
            &o,
            &ask,
            &content_lang,
            ctx.formatversion,
        )),
    );
    Ok(())
}

/// The namespaces `meta=siteinfo` lists (0008 §2–3; 0041 §8): the `pages` and `virtual`
/// namespaces of `namespaces.toml` that no tenant setting gates, under the tenant's names.
fn namespaces(names: &TenantNames) -> Value {
    let local: std::collections::HashMap<i32, String> = names
        .source()
        .namespaces
        .into_iter()
        .map(|n| (n.id, n.name))
        .collect();
    let file: NamespaceFile = toml::from_str(include_str!(
        "../../../../docs/registry/namespaces.toml"
    ))
    .unwrap_or(NamespaceFile {
        namespace: Vec::new(),
    });
    let mut out = Map::new();
    for ns in file.namespace {
        // Only Lua's namespaces are gated (`wikitext.lua`), and Lua is not built.
        let enabled = ns.enabled_by.is_none();
        if !enabled || !(ns.kind == "pages" || ns.kind == "virtual") {
            continue;
        }
        let name = local
            .get(&i32::try_from(ns.id).unwrap_or(i32::MIN))
            .cloned()
            .unwrap_or_else(|| ns.canonical.clone());
        let mut row = Map::new();
        row.insert("id".into(), json!(ns.id));
        row.insert(
            "case".into(),
            json!(ns.case.unwrap_or_else(|| "first-letter".into())),
        );
        row.insert("name".into(), json!(name));
        row.insert("subpages".into(), json!(ns.subpages.unwrap_or(false)));
        if !ns.canonical.is_empty() {
            row.insert("canonical".into(), json!(ns.canonical));
        }
        row.insert("content".into(), json!(ns.id == 0));
        row.insert("nonincludable".into(), json!(false));
        if let Some(m) = ns.default_model
            && m != "wikitext"
        {
            row.insert("defaultcontentmodel".into(), json!(m));
        }
        out.insert(ns.id.to_string(), Value::Object(row));
    }
    Value::Object(out)
}

fn property_types() -> Value {
    let mut m = Map::new();
    for dt in [
        DataType::CommonsMedia,
        DataType::GeoShape,
        DataType::TabularData,
        DataType::Url,
        DataType::ExternalId,
        DataType::WikibaseItem,
        DataType::WikibaseProperty,
        DataType::GlobeCoordinate,
        DataType::MonolingualText,
        DataType::Quantity,
        DataType::String,
        DataType::Time,
        DataType::WikibaseDomain,
    ] {
        if let Some(vt) = dt.value_type() {
            m.insert(dt.id().to_string(), json!({"valuetype": vt.name()}));
        }
    }
    Value::Object(m)
}

/// `siprop=providers` (0012 §4): codes, entity types, namespaces, IRI templates, whether
/// upstream history can be fetched, and the colours of each provider's chip (0010 §2).
fn providers() -> Value {
    Registry::default_registry()
        .providers()
        .iter()
        .map(|p| {
            json!({
                "code": p.code,
                "slug": p.slug,
                "name": p.name,
                "number": p.number,
                "revision_ids": p.revision_ids,
                "types": p.types.iter().map(|t| json!({
                    "code": t.code.to_string(),
                    "entity_type": t.entity_type,
                    "namespace": t.namespace,
                    "iri": t.iri,
                    "upstream_prefix": t.upstream_prefix,
                })).collect::<Vec<_>>(),
                "chip": p.chip.as_ref().map(|c| json!({"color": c.color, "background": c.background})),
            })
        })
        .collect()
}

/// `meta=wikibase` (Wikibase Client's module): where this site's repository is. The
/// site is its own repository, so the URL is the tenant's own; Pywikibot asks this to
/// decide whether a site has a data repository at all, and builds its `DataSite` from
/// the answer.
fn wikibase(ctx: &Ctx, query: &mut Map<String, Value>) {
    let base = ctx.tenant.public_base.trim_end_matches('/');
    query.insert(
        "wikibase".into(),
        json!({
            "repo": {"url": {"base": base, "scriptpath": "/w", "articlepath": "/wiki/$1"}},
            "siteid": ctx.tenant.slug.replace('-', "_"),
        }),
    );
}

/// The token types `meta=tokens` issues (MediaWiki's set).
pub const TOKEN_TYPES: &[&str] = &[
    "createaccount",
    "csrf",
    "login",
    "patrol",
    "rollback",
    "userrights",
    "watch",
];

/// Language codes the term API accepts, for `paraminfo` (Wikibase lists every
/// MediaWiki language; clients such as Pywikibot refuse a code not in the list). The
/// ISO 639-1 codes, `mul`, and the variant codes Wikibase terms commonly use.
pub const TERM_LANGUAGES: &[&str] = &[
    "mul",
    "aa",
    "ab",
    "ae",
    "af",
    "ak",
    "am",
    "an",
    "ar",
    "as",
    "av",
    "ay",
    "az",
    "ba",
    "be",
    "bg",
    "bi",
    "bm",
    "bn",
    "bo",
    "br",
    "bs",
    "ca",
    "ce",
    "ch",
    "co",
    "cr",
    "cs",
    "cu",
    "cv",
    "cy",
    "da",
    "de",
    "dv",
    "dz",
    "ee",
    "el",
    "en",
    "eo",
    "es",
    "et",
    "eu",
    "fa",
    "ff",
    "fi",
    "fj",
    "fo",
    "fr",
    "fy",
    "ga",
    "gd",
    "gl",
    "gn",
    "gu",
    "gv",
    "ha",
    "he",
    "hi",
    "ho",
    "hr",
    "ht",
    "hu",
    "hy",
    "hz",
    "ia",
    "id",
    "ie",
    "ig",
    "ii",
    "ik",
    "io",
    "is",
    "it",
    "iu",
    "ja",
    "jv",
    "ka",
    "kg",
    "ki",
    "kj",
    "kk",
    "kl",
    "km",
    "kn",
    "ko",
    "kr",
    "ks",
    "ku",
    "kv",
    "kw",
    "ky",
    "la",
    "lb",
    "lg",
    "li",
    "ln",
    "lo",
    "lt",
    "lu",
    "lv",
    "mg",
    "mh",
    "mi",
    "mk",
    "ml",
    "mn",
    "mr",
    "ms",
    "mt",
    "my",
    "na",
    "nb",
    "nd",
    "ne",
    "ng",
    "nl",
    "nn",
    "no",
    "nr",
    "nv",
    "ny",
    "oc",
    "oj",
    "om",
    "or",
    "os",
    "pa",
    "pi",
    "pl",
    "ps",
    "pt",
    "qu",
    "rm",
    "rn",
    "ro",
    "ru",
    "rw",
    "sa",
    "sc",
    "sd",
    "se",
    "sg",
    "si",
    "sk",
    "sl",
    "sm",
    "sn",
    "so",
    "sq",
    "sr",
    "ss",
    "st",
    "su",
    "sv",
    "sw",
    "ta",
    "te",
    "tg",
    "th",
    "ti",
    "tk",
    "tl",
    "tn",
    "to",
    "tr",
    "ts",
    "tt",
    "tw",
    "ty",
    "ug",
    "uk",
    "ur",
    "uz",
    "ve",
    "vi",
    "vo",
    "wa",
    "wo",
    "xh",
    "yi",
    "yo",
    "za",
    "zh",
    "zu",
    "ast",
    "azb",
    "bar",
    "be-tarask",
    "ceb",
    "ckb",
    "de-at",
    "de-ch",
    "de-formal",
    "en-ca",
    "en-gb",
    "es-419",
    "eml",
    "fil",
    "frr",
    "gsw",
    "hak",
    "hsb",
    "ilo",
    "jbo",
    "kab",
    "ksh",
    "lmo",
    "lzh",
    "min",
    "nan",
    "nap",
    "nds",
    "nds-nl",
    "pms",
    "pnb",
    "pt-br",
    "sco",
    "scn",
    "sh",
    "simple",
    "sr-ec",
    "sr-el",
    "szl",
    "tt-cyrl",
    "tt-latn",
    "vec",
    "war",
    "yue",
    "zh-cn",
    "zh-hans",
    "zh-hant",
    "zh-hk",
    "zh-mo",
    "zh-my",
    "zh-sg",
    "zh-tw",
    "zxx",
];

/// The main page's title: the project namespace's `Home`, which the site generates until
/// a page of that title exists.
pub const MAIN_PAGE: &str = "Project:Home";

/// `meta=siteinfo`: `general`, `namespaces`, `namespacealiases`, `extensions`,
/// `libraries`, `statistics`, `usergroups`, `providers`, `triplespace`; unknown `siprop`
/// values warn.
#[allow(clippy::too_many_lines)]
async fn siteinfo(ctx: &mut Ctx, query: &mut Map<String, Value>) -> Result<(), ApiError> {
    let mut props = ctx.params.list("siprop");
    if props.is_empty() {
        props.push("general".into());
    }
    let base = ctx.tenant.public_base.trim_end_matches('/').to_string();
    // The wiki's name and content language are settings (0085 §7), resolved through the
    // instance's defaults; the wiki ID stays the slug.
    let (settings, _) =
        triplespace_accounts::settings::load(ctx.db(), Some(&ctx.tenant.slug)).await?;
    let slug = ctx.tenant.slug.clone();
    let sitename = settings
        .value("site.name")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| slug.clone());
    let lang = settings
        .value("site.language")
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "en".into());
    let host = ctx.tenant.host.clone();
    for p in props {
        match p.as_str() {
            "general" => {
                query.insert("general".into(), json!({
                    "mainpage": MAIN_PAGE,
                    "base": format!("{base}/wiki/{}", MAIN_PAGE.replace(' ', "_")),
                    "sitename": sitename,
                    "generator": ctx.app.config().generator,
                    "phpversion": "0",
                    "dbtype": "postgres",
                    "lang": lang,
                    "fallback": [],
                    "case": "first-letter",
                    "legaltitlechars": " %!\"$&'()*,\\-.\\/0-9:;=?@A-Z\\\\^_`a-z~\\x80-\\xFF+",
                    "readonly": false,
                    "writeapi": true,
                    "maxarticlesize": 2_097_152,
                    "timezone": "UTC",
                    "timeoffset": 0,
                    "articlepath": "/wiki/$1",
                    "scriptpath": "/w",
                    "script": "/w/index.php",
                    "variantarticlepath": false,
                    "server": base,
                    "servername": host.split(':').next().unwrap_or(&host),
                    "wikiid": slug.replace('-', "_"),
                    "time": crate::entity_json::iso8601(crate::edit::now_micros()),
                    "uploadsenabled": false,
                    // Pywikibot's Siteinfo converts these three unconditionally, so they
                    // must be present, as MediaWiki emits them (index-keyed objects, even
                    // in formatversion 2; `magiclinks` a name → bool map). Values are
                    // MediaWiki's defaults; nothing here renders thumbnails.
                    "thumblimits": {"0": 120, "1": 150, "2": 180, "3": 200, "4": 250, "5": 300},
                    "imagelimits": {"0": {"width": 320, "height": 240}, "1": {"width": 640, "height": 480},
                                    "2": {"width": 800, "height": 600}, "3": {"width": 1024, "height": 768},
                                    "4": {"width": 1280, "height": 1024}},
                    "magiclinks": {"ISBN": false, "PMID": false, "RFC": false},
                    "linktrail": "/^([a-z]+)(.*)$/sD",
                    "misermode": false,
                    "centralidlookupprovider": "local",
                    "allcentralidlookupproviders": ["local"],
                    "wikibase-propertytypes": property_types(),
                    "wikibase-conceptbaseuri": format!("{}/entity/", ctx.tenant.base.trim_end_matches('/')),
                }));
                // PostgreSQL's version, as MediaWiki reports its database server's, unless
                // `version.services` withholds versions (0077 §10).
                if crate::version::disclosure(ctx.db()).await?.versions()
                    && let Some(v) = ctx.app.probes().postgres().and_then(|p| p.version)
                    && let Some(g) = query.get_mut("general").and_then(Value::as_object_mut)
                {
                    g.insert("dbversion".into(), json!(v));
                }
            }
            "namespaces" | "namespacealiases" => {
                let mut names = TenantNames::load(ctx.db(), &ctx.tenant.slug)
                    .await
                    .map_err(ApiError::internal)?;
                names.site_name = Some(sitename.clone());
                if p == "namespaces" {
                    query.insert("namespaces".into(), namespaces(&names));
                } else {
                    let v2 = ctx.formatversion == 2;
                    let list: Vec<Value> = names
                        .aliases()
                        .into_iter()
                        .map(|(id, alias)| {
                            if v2 {
                                json!({"id": id, "alias": alias})
                            } else {
                                json!({"id": id, "*": alias})
                            }
                        })
                        .collect();
                    query.insert("namespacealiases".into(), Value::Array(list));
                }
            }
            "extensions" => {
                // Only extensions whose API modules are served, since clients choose the
                // modules they call from this list (Pywikibot's `has_extension`); the
                // extensions that inspired features are on Special:Version, not here
                // (0077 §8, §10).
                let version = ctx.app.build().build()["version"].clone();
                query.insert("extensions".into(), json!([
                    {"type": "wikibase", "name": "WikibaseRepository", "descriptionmsg": "wikibase-desc", "version": version, "url": "https://github.com/scatterbase/triplespace", "license-name": "GPL-3.0-or-later"},
                    {"type": "other", "name": "Echo", "descriptionmsg": "echo-desc", "version": version, "url": "https://github.com/scatterbase/triplespace", "license-name": "GPL-3.0-or-later"},
                    {"type": "other", "name": "Triplespace", "version": version, "url": "https://github.com/scatterbase/triplespace", "license-name": "GPL-3.0-or-later"}
                ]));
            }
            "libraries" => {
                // The third-party crates and frontend packages, as MediaWiki lists its
                // Composer and npm libraries (0077 §10).
                query.insert("libraries".into(), json!(ctx.app.build().libraries()));
            }
            "statistics" => {
                query.insert("statistics".into(), json!({"pages": 0, "articles": 0, "edits": 0, "images": 0, "users": 0, "activeusers": 0, "admins": 1, "jobs": 0}));
            }
            "usergroups" => {
                let groups = triplespace_accounts::groups::registry(ctx.db(), &slug).await?;
                query.insert(
                    "usergroups".into(),
                    json!(
                        groups
                            .groups()
                            .iter()
                            .map(|g| {
                                let mut v = json!({
                                    "name": g.mediawiki_name,
                                    "rights": g.permissions,
                                });
                                // MediaWiki marks the groups nobody is added to.
                                if g.implicit.is_some() {
                                    v["implicit"] = json!("");
                                }
                                if let (Value::Object(m), Value::Object(d)) =
                                    (&mut v, super::userrights::delegates(g))
                                {
                                    m.extend(d);
                                }
                                v
                            })
                            .collect::<Vec<_>>()
                    ),
                );
            }
            "providers" => {
                query.insert("providers".into(), providers());
            }
            "issuers" => {
                query.insert(
                    "issuers".into(),
                    json!(
                        scatter_actors::issuer::IssuerRegistry::default_registry()
                            .login_providers()
                            .map(|i| json!({"code": i.code, "name": i.name}))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            "triplespace" => {
                let mut t = json!({
                    "tenant": slug,
                    "farm": ctx.app.config().farm.slug,
                    "search": "postgres",
                    "capabilities": ["wbgetentities", "wbsearchentities", "wbeditentity", "statements", "terms", "login", "clientlogin", "createaccount", "bearer", "version", "recentchanges", "activity", "history", "revisions", "diff", "options", "preferences", "settings", "pages", "parse", "edit", "move", "protect", "userrights", "block", "delete", "notifications", "watchlist", "reports"],
                    "providers": Registry::default_registry().providers().iter().map(|p| p.slug.clone()).collect::<Vec<_>>(),
                    "rate_limits": [],
                    "grants": scatter_actors::grant::GrantRegistry::default_registry().grants().iter().map(|g| g.name.clone()).collect::<Vec<_>>(),
                    "api_version": crate::API_VERSION,
                    "build": {
                        "version": ctx.app.build().build()["version"],
                        "commit": ctx.app.build().build()["commit"],
                        "modified": ctx.app.build().build()["modified"],
                    },
                });
                // The theme (0086 §5): the palettes and type sets offered and the
                // settings that choose from them, public as the logo is.
                let setting = |k: &str| {
                    settings
                        .value(k)
                        .and_then(|v| v.as_str().map(str::to_owned))
                };
                t["theme"] = json!(triplespace_accounts::theme::Theme::new(
                    triplespace_accounts::theme::Catalogue::shipped(),
                    setting("theme.palette").as_deref(),
                    setting("theme.typeset").as_deref(),
                    setting("theme.choice").as_deref(),
                ));
                // The markup a client's new posts must have (14 §1.6; 0019 A28).
                t["thread"] = json!({
                    "markup": setting("thread.markup").unwrap_or_else(|| "markdown".into()),
                });
                // The wiki's skins (0097 §8): its default, and the ones it offers where it
                // has narrowed them; the site holds the skins themselves.
                let mut skins = json!({ "default": setting("skins.default") });
                if let Some(available) = settings
                    .value("skins.available")
                    .and_then(|v| v.as_array().cloned())
                {
                    skins["available"] = json!(available);
                }
                t["skins"] = skins;
                if ctx.app.config().mode == Mode::Development {
                    t["insecure"] = json!(true);
                }
                // Registration (0087 §9): what is asked of a client before it asks.
                let (challenge, account) = super::account::Policy::load(ctx).await?.report();
                t["challenge"] = challenge;
                t["account"] = account;
                // External issuers the tenant offers (0088 §9).
                let offered = crate::external::offered(crate::external::Scope {
                    app: &ctx.app,
                    db: ctx.db(),
                    tenant: &ctx.tenant,
                })
                .await?;
                t["login"] = json!(
                    offered
                        .iter()
                        .map(|i| json!({
                            "code": i.code,
                            "name": i.name,
                            "label": crate::external::label(i),
                            "origin": crate::external::authorize_origin(i),
                        }))
                        .collect::<Vec<_>>()
                );
                // The farm base serves the instance's pages (0085 §4).
                let at_farm = crate::tenant::at_farm(&ctx.app, &ctx.tenant);
                t["at_farm"] = json!(at_farm);
                // The tenant's issuer code, which its actor keys carry, and the farm code,
                // which the operator's does (0079 §7); at the farm base, the primary tenant
                // (0046 §9). The tenancy policy (0028 §1).
                t["issuer"] = json!(ctx.tenant.issuer);
                t["farm_code"] = json!(ctx.app.config().farm.code);
                if at_farm {
                    t["primary"] =
                        json!(triplespace_projections::tenants::primary(ctx.db()).await?);
                }
                t["tenancy"] = crate::tenant::tenancy_report(ctx.db()).await?;
                // The wiki's interface overrides and names (0095 §5): the instance's, from
                // the primary tenant's Interface pages, under the tenant's own.
                let pages = crate::interface::pages(ctx.db(), &ctx.tenant.slug).await?;
                t["interface"] = crate::interface::Overrides::from_pages(&pages).report();
                query.insert("triplespace".into(), t);
            }
            other => ctx.warn(
                "siteinfo",
                format!("Unrecognized value for parameter \"siprop\": {other}."),
            ),
        }
    }
    // A private tenant answers an outsider with its name, language and theme, that it is
    // private, and what logging in needs, and nothing else (18 §2.2; 09 §4.7).
    let private = crate::pages::private_to(ctx).await?;
    if let Some(t) = query.get_mut("triplespace") {
        t["visibility"] = json!(if private || tenant_is_private(ctx).await? {
            "private"
        } else {
            "public"
        });
    }
    if private {
        query.retain(|k, _| k == "general" || k == "triplespace");
        if let Some(Value::Object(g)) = query.get_mut("general") {
            g.retain(|k, _| {
                matches!(
                    k.as_str(),
                    "sitename" | "lang" | "logo" | "favicon" | "server" | "mainpage"
                )
            });
            g.insert("private".into(), json!(true));
        }
        if let Some(Value::Object(t)) = query.get_mut("triplespace") {
            t.retain(|k, _| {
                matches!(
                    k.as_str(),
                    "tenant"
                        | "api_version"
                        | "theme"
                        | "skins"
                        | "visibility"
                        | "login"
                        | "auth"
                        | "auth_origin"
                        | "account"
                        | "challenge"
                        | "at_farm"
                )
            });
        }
    }
    Ok(())
}

/// Whether the tenant has a `read` restriction in force at all, whoever asks (09 §4.7).
async fn tenant_is_private(ctx: &Ctx) -> Result<bool, ApiError> {
    let at = scatter_actors::Timestamp(crate::edit::now_micros());
    Ok(triplespace_projections::acl_read::current(
        ctx.db(),
        &ctx.tenant.slug,
        &[scatter_actors::acl::Target::Tenant(ctx.tenant.slug.clone())],
    )
    .await
    .map_err(ApiError::internal)?
    .iter()
    .any(|a| a.restriction_at(scatter_actors::acl::READ, at).is_some()))
}

/// `action=paraminfo`: enough for a client to see which modules exist.
/// The action modules, as `action=paraminfo` describes them. Every `wb*` write module
/// is one entry here (`write::run` dispatches on the name), so a client that validates
/// against paraminfo, as Pywikibot does, sees each of them.
const ACTION_MODULES: &[(&str, bool)] = &[
    // (name, must be POSTed)
    ("query", false),
    ("paraminfo", false),
    ("help", false),
    ("login", true),
    ("clientlogin", true),
    ("createaccount", true),
    ("logout", true),
    ("options", true),
    ("parse", false),
    ("compare", false),
    ("edit", true),
    ("move", true),
    ("delete", true),
    ("undelete", true),
    ("protect", true),
    ("revisiondelete", true),
    ("userrights", true),
    ("block", true),
    ("unblock", true),
    ("echomarkread", true),
    ("echomarkseen", true),
    ("watch", true),
    ("setnotificationtimestamp", true),
    ("wbgetentities", false),
    ("wbsearchentities", false),
    ("wbeditentity", true),
    ("wbcreateclaim", true),
    ("wbsetclaim", true),
    ("wbremoveclaims", true),
    ("wbsetlabel", true),
    ("wbsetdescription", true),
    ("wbsetaliases", true),
    ("wbsetqualifier", true),
    ("wbremovequalifiers", true),
    ("wbsetreference", true),
    ("wbremovereferences", true),
];

/// `action=query`'s submodules by kind: `(kind, names)`.
const QUERY_MODULES: &[(&str, &[&str])] = &[
    (
        "meta",
        &[
            "siteinfo",
            "tokens",
            "userinfo",
            "wikibase",
            "authmanagerinfo",
            "notifications",
            "allmessages",
        ],
    ),
    (
        "prop",
        &["revisions", "deletedrevisions", "info", "templates"],
    ),
    (
        "list",
        &[
            "recentchanges",
            "usercontribs",
            "logevents",
            "allpages",
            "protectedpages",
            "protectedtitles",
            "protectedsets",
            "users",
            "blocks",
            "watchlist",
            "watchlistraw",
            "querypage",
        ],
    ),
];

fn paraminfo_module(name: &str, path: &str, prefix: &str, parameters: &[Value]) -> Value {
    let mut m = json!({
        "name": name,
        "classname": "Triplespace",
        "path": path,
        "prefix": prefix,
        "source": "Triplespace",
        "sourcename": "Triplespace",
        "licensetag": "GPL-3.0-or-later",
        "parameters": parameters,
    });
    if path.starts_with("query+")
        && let Some((kind, _)) = QUERY_MODULES
            .iter()
            .find(|(_, names)| names.contains(&name))
    {
        m["group"] = json!(kind);
    }
    m
}

fn parameter(index: usize, name: &str, kind: Value) -> Map<String, Value> {
    let mut p = Map::new();
    p.insert("index".into(), json!(index));
    p.insert("name".into(), json!(name));
    p.insert("type".into(), kind);
    p
}

/// A parameter that names submodules: its `type` lists them and `submodules` maps each
/// to its path, which is how Pywikibot learns the module tree.
fn submodule_parameter(index: usize, name: &str, names: &[&str], parent: Option<&str>) -> Value {
    let mut p = parameter(index, name, json!(names));
    let paths: Map<String, Value> = names
        .iter()
        .map(|n| {
            let path = parent.map_or_else(|| (*n).to_string(), |pa| format!("{pa}+{n}"));
            ((*n).to_string(), json!(path))
        })
        .collect();
    p.insert("submodules".into(), Value::Object(paths));
    p.insert("multi".into(), json!(true));
    p.insert("limit".into(), json!(50));
    p.insert("lowlimit".into(), json!(50));
    p.insert("highlimit".into(), json!(500));
    Value::Object(p)
}

fn multi(mut p: Map<String, Value>, limit: u32) -> Map<String, Value> {
    p.insert("multi".into(), json!(true));
    p.insert("limit".into(), json!(limit));
    p.insert("lowlimit".into(), json!(limit));
    p.insert("highlimit".into(), json!(limit * 10));
    p
}

/// A `limit`-type parameter's bounds (compat §2.3): `min`, `max` and `highmax`, which
/// Pywikibot reads to size its batches.
fn limit_parameter(mut p: Map<String, Value>, max: u32, highmax: u32) -> Map<String, Value> {
    p.insert("min".into(), json!(1));
    p.insert("max".into(), json!(max));
    p.insert("highmax".into(), json!(highmax));
    p
}

fn with_default(mut p: Map<String, Value>, default: Value) -> Map<String, Value> {
    p.insert("default".into(), default);
    p
}

/// The log types `list=logevents` enumerates (16 §2.3): MediaWiki's, and Triplespace's
/// own `erase`, `link`, `retention`, `convert`, `job` and `settings` (0085 §3).
const LOG_TYPES: &[&str] = &[
    "block",
    "protect",
    "rights",
    "delete",
    "upload",
    "move",
    "import",
    "patrol",
    "merge",
    "suppress",
    "tag",
    "managetags",
    "contentmodel",
    "renameuser",
    "newusers",
    "create",
    "erase",
    "link",
    "retention",
    "convert",
    "job",
    "settings",
    "reclaim",
];

/// The parameters of the three feed modules (`super::changes`), named without their
/// prefix, as paraminfo names them; the module carries the prefix.
#[allow(clippy::too_many_lines)]
fn feed_parameters(name: &str) -> Vec<Value> {
    // The `show` values MediaWiki's modules list, which Pywikibot validates its own
    // option sets against; the ones no projection serves yet (`anon`, `redirect`, `top`,
    // `autopatrolled`) are accepted and ignored.
    let rc_show = [
        "minor",
        "!minor",
        "bot",
        "!bot",
        "anon",
        "!anon",
        "redirect",
        "!redirect",
        "patrolled",
        "!patrolled",
        "unpatrolled",
        "autopatrolled",
        "!autopatrolled",
    ];
    let uc_show = [
        "minor",
        "!minor",
        "patrolled",
        "!patrolled",
        "autopatrolled",
        "!autopatrolled",
        "top",
        "!top",
        "new",
        "!new",
    ];
    let show: &[&str] = match name {
        "recentchanges" => &rc_show,
        "usercontribs" => &uc_show,
        _ => &[],
    };
    let (props, defaults): (Vec<&str>, &str) = match name {
        "recentchanges" => (
            vec![
                "comment",
                "flags",
                "ids",
                "loginfo",
                "parsedcomment",
                "patrolled",
                "sizes",
                "tags",
                "timestamp",
                "title",
                "user",
                "userid",
                "source",
                "job",
                "operator",
            ],
            "title|timestamp|ids",
        ),
        "usercontribs" => (
            vec![
                "ids",
                "title",
                "timestamp",
                "comment",
                "parsedcomment",
                "size",
                "sizediff",
                "flags",
                "patrolled",
                "tags",
                "source",
                "job",
                "operator",
            ],
            "ids|title|timestamp|comment|size|flags",
        ),
        _ => (
            vec![
                "ids",
                "title",
                "type",
                "user",
                "userid",
                "timestamp",
                "comment",
                "parsedcomment",
                "details",
                "tags",
            ],
            "ids|title|type|user|timestamp|comment|details",
        ),
    };
    let mut out = vec![
        Value::Object(parameter(0, "start", json!("timestamp"))),
        Value::Object(parameter(1, "end", json!("timestamp"))),
        Value::Object(with_default(
            parameter(2, "dir", json!(["newer", "older"])),
            json!("older"),
        )),
        Value::Object(multi(parameter(3, "namespace", json!("namespace")), 50)),
        Value::Object(with_default(
            multi(parameter(4, "prop", json!(props)), 50),
            json!(defaults),
        )),
        Value::Object(multi(parameter(5, "show", json!(show)), 50)),
        Value::Object(with_default(
            limit_parameter(parameter(6, "limit", json!("limit")), 500, 5000),
            json!(10),
        )),
        Value::Object(parameter(7, "continue", json!("string"))),
        Value::Object(parameter(8, "tag", json!("string"))),
        Value::Object(parameter(9, "job", json!("integer"))),
    ];
    match name {
        "recentchanges" => {
            out.push(Value::Object(parameter(10, "user", json!("user"))));
            out.push(Value::Object(parameter(11, "excludeuser", json!("user"))));
            out.push(Value::Object(multi(
                parameter(
                    12,
                    "type",
                    json!(["edit", "new", "log", "categorize", "external"]),
                ),
                50,
            )));
            out.push(Value::Object(multi(
                parameter(13, "kind", json!(["edit", "sync", "log", "job", "erased"])),
                50,
            )));
        }
        "usercontribs" => {
            out.push(Value::Object(multi(
                parameter(10, "user", json!("user")),
                50,
            )));
            out.push(Value::Object(multi(
                parameter(11, "userids", json!("integer")),
                50,
            )));
        }
        _ => {
            out.push(Value::Object(parameter(10, "type", json!(LOG_TYPES))));
            out.push(Value::Object(parameter(11, "action", json!("string"))));
            out.push(Value::Object(parameter(12, "user", json!("user"))));
        }
    }
    out
}

/// The parameters of the modules a client validates against before calling them:
/// Pywikibot reads `query+tokens.type` for the token types and
/// `wbsearchentities.language` for the languages it may search in, and sends only
/// parameters a module lists. Modules not described here answer with no parameters.
#[allow(clippy::too_many_lines)]
fn module_parameters(name: &str) -> Vec<Value> {
    let entity_types = json!(["item", "property", "domain"]);
    match name {
        "tokens" => vec![Value::Object(with_default(
            multi(parameter(0, "type", json!(TOKEN_TYPES)), 50),
            json!("csrf"),
        ))],
        "allmessages" => vec![
            Value::Object(with_default(
                multi(parameter(0, "messages", json!("string")), 50),
                json!("*"),
            )),
            Value::Object(with_default(
                parameter(1, "prop", json!(["default"])),
                json!(""),
            )),
            Value::Object(parameter(2, "enableparser", json!("boolean"))),
            Value::Object(parameter(3, "nocontent", json!("boolean"))),
            Value::Object(parameter(4, "includelocal", json!("boolean"))),
            Value::Object(multi(parameter(5, "args", json!("string")), 50)),
            Value::Object(parameter(6, "filter", json!("string"))),
            Value::Object(with_default(
                parameter(7, "customised", json!(["all", "modified", "unmodified"])),
                json!("all"),
            )),
            Value::Object(parameter(8, "lang", json!("string"))),
            Value::Object(parameter(9, "from", json!("string"))),
            Value::Object(parameter(10, "to", json!("string"))),
            Value::Object(parameter(11, "title", json!("string"))),
            Value::Object(parameter(12, "prefix", json!("string"))),
        ],
        "siteinfo" => vec![Value::Object(with_default(
            multi(
                parameter(
                    0,
                    "prop",
                    json!([
                        "general",
                        "namespaces",
                        "namespacealiases",
                        "extensions",
                        "libraries",
                        "statistics",
                        "usergroups",
                        "providers",
                        "issuers",
                        "triplespace"
                    ]),
                ),
                50,
            ),
            json!("general"),
        ))],
        "userinfo" => vec![Value::Object(multi(
            parameter(
                0,
                "prop",
                json!([
                    "blockinfo",
                    "groups",
                    "rights",
                    "ratelimits",
                    "operator",
                    "grants",
                    "options",
                    "accountrequests",
                    "changeablegroups"
                ]),
            ),
            50,
        ))],
        "authmanagerinfo" => vec![
            Value::Object(parameter(0, "requestsfor", json!(["login", "create"]))),
            Value::Object(parameter(1, "mergerequestfields", json!("boolean"))),
        ],
        "createaccount" => vec![
            Value::Object(parameter(0, "username", json!("string"))),
            Value::Object(parameter(1, "password", json!("password"))),
            Value::Object(parameter(2, "retype", json!("password"))),
            Value::Object(parameter(3, "createtoken", json!("string"))),
            Value::Object(parameter(4, "createreturnurl", json!("string"))),
            Value::Object(parameter(5, "createcontinue", json!("boolean"))),
            Value::Object(parameter(6, "tschallengeid", json!("string"))),
            Value::Object(parameter(7, "tschallengesolution", json!("string"))),
            Value::Object(parameter(8, "tsstatement", json!("text"))),
        ],
        "wikibase" => vec![Value::Object(with_default(
            multi(parameter(0, "prop", json!(["url", "siteid"])), 50),
            json!("url|siteid"),
        ))],
        "wbsearchentities" => vec![
            Value::Object(parameter(0, "search", json!("string"))),
            Value::Object(parameter(1, "language", json!(TERM_LANGUAGES))),
            Value::Object(parameter(2, "strictlanguage", json!("boolean"))),
            Value::Object(with_default(
                parameter(3, "type", entity_types),
                json!("item"),
            )),
            Value::Object(with_default(
                parameter(4, "limit", json!("limit")),
                json!(7),
            )),
            Value::Object(with_default(
                parameter(5, "continue", json!("integer")),
                json!(0),
            )),
        ],
        "recentchanges" | "usercontribs" | "logevents" => feed_parameters(name),
        "revisions" => vec![
            Value::Object(with_default(
                multi(
                    parameter(
                        0,
                        "prop",
                        json!([
                            "ids",
                            "flags",
                            "timestamp",
                            "user",
                            "userid",
                            "size",
                            "comment",
                            "parsedcomment",
                            "tags",
                            "content",
                            "contentmodel",
                            "source",
                            "job",
                            "operator",
                            "record"
                        ]),
                    ),
                    50,
                ),
                json!("ids|timestamp|flags|comment|user"),
            )),
            Value::Object(multi(parameter(1, "slots", json!(["main", "*"])), 50)),
            Value::Object(with_default(
                limit_parameter(parameter(2, "limit", json!("limit")), 500, 5000),
                json!(10),
            )),
            Value::Object(parameter(3, "startid", json!("integer"))),
            Value::Object(parameter(4, "endid", json!("integer"))),
            Value::Object(parameter(5, "start", json!("timestamp"))),
            Value::Object(parameter(6, "end", json!("timestamp"))),
            Value::Object(with_default(
                parameter(7, "dir", json!(["newer", "older"])),
                json!("older"),
            )),
            Value::Object(parameter(8, "user", json!("user"))),
            Value::Object(parameter(9, "excludeuser", json!("user"))),
            Value::Object(parameter(10, "continue", json!("string"))),
        ],
        "info" => vec![Value::Object(multi(
            parameter(0, "prop", json!(["protection", "url", "displaytitle"])),
            50,
        ))],
        "templates" => vec![
            Value::Object(multi(parameter(0, "namespace", json!("namespace")), 50)),
            Value::Object(with_default(
                limit_parameter(parameter(1, "limit", json!("limit")), 500, 5000),
                json!(10),
            )),
            Value::Object(parameter(2, "continue", json!("string"))),
            Value::Object(multi(parameter(3, "templates", json!("string")), 50)),
            Value::Object(with_default(
                parameter(4, "dir", json!(["ascending", "descending"])),
                json!("ascending"),
            )),
        ],
        "allpages" => vec![
            Value::Object(parameter(0, "from", json!("string"))),
            Value::Object(parameter(1, "continue", json!("string"))),
            Value::Object(parameter(2, "to", json!("string"))),
            Value::Object(parameter(3, "prefix", json!("string"))),
            Value::Object(with_default(
                parameter(4, "namespace", json!("namespace")),
                json!(0),
            )),
            Value::Object(with_default(
                parameter(
                    5,
                    "filterredir",
                    json!(["all", "redirects", "nonredirects"]),
                ),
                json!("all"),
            )),
            Value::Object(parameter(6, "minsize", json!("integer"))),
            Value::Object(parameter(7, "maxsize", json!("integer"))),
            Value::Object(with_default(
                limit_parameter(parameter(8, "limit", json!("limit")), 500, 5000),
                json!(10),
            )),
            Value::Object(with_default(
                parameter(9, "dir", json!(["ascending", "descending"])),
                json!("ascending"),
            )),
        ],
        "querypage" => vec![
            Value::Object(parameter(
                0,
                "page",
                json!(
                    triplespace_projections::reports::REPORTS
                        .iter()
                        .map(|r| r.mediawiki_name.unwrap_or(r.name))
                        .collect::<Vec<_>>()
                ),
            )),
            Value::Object(with_default(
                parameter(1, "offset", json!("integer")),
                json!(0),
            )),
            Value::Object(with_default(
                limit_parameter(parameter(2, "limit", json!("limit")), 500, 500),
                json!(10),
            )),
        ],
        "parse" => [
            ("title", json!("string")),
            ("text", json!("text")),
            ("summary", json!("string")),
            ("page", json!("string")),
            ("pageid", json!("integer")),
            ("redirects", json!("boolean")),
            ("oldid", json!("integer")),
            (
                "prop",
                json!([
                    "text",
                    "langlinks",
                    "categories",
                    "links",
                    "templates",
                    "images",
                    "externallinks",
                    "sections",
                    "revid",
                    "displaytitle",
                    "iwlinks",
                    "properties",
                    "parsewarnings",
                    "wikitext"
                ]),
            ),
            ("pst", json!("boolean")),
            ("onlypst", json!("boolean")),
            ("section", json!("string")),
            ("disablelimitreport", json!("boolean")),
            ("disableeditsection", json!("boolean")),
            ("preview", json!("boolean")),
            ("contentformat", json!(["text/x-wiki"])),
            ("contentmodel", json!(["wikitext"])),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, t))| {
            let p = parameter(i, n, t);
            Value::Object(if n == "prop" { multi(p, 50) } else { p })
        })
        .collect(),
        "compare" => [
            ("fromtitle", json!("string")),
            ("fromid", json!("integer")),
            ("fromrev", json!("integer")),
            ("totitle", json!("string")),
            ("toid", json!("integer")),
            ("torev", json!("integer")),
            ("torelative", json!(["cur", "next", "prev"])),
            (
                "prop",
                json!([
                    "comment",
                    "diff",
                    "diffsize",
                    "ids",
                    "parsedcomment",
                    "rel",
                    "size",
                    "timestamp",
                    "title",
                    "user"
                ]),
            ),
            ("difftype", json!(["inline", "table"])),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, t))| {
            let p = parameter(i, n, t);
            Value::Object(match n {
                "prop" => with_default(multi(p, 50), json!("diff|ids|title")),
                "difftype" => with_default(p, json!("table")),
                _ => p,
            })
        })
        .collect(),
        "edit" => [
            ("title", json!("string")),
            ("pageid", json!("integer")),
            ("section", json!("string")),
            ("text", json!("text")),
            ("summary", json!("string")),
            ("tags", json!("tags")),
            ("minor", json!("boolean")),
            ("notminor", json!("boolean")),
            ("bot", json!("boolean")),
            ("baserevid", json!("integer")),
            ("basetimestamp", json!("timestamp")),
            ("starttimestamp", json!("timestamp")),
            ("recreate", json!("boolean")),
            ("createonly", json!("boolean")),
            ("nocreate", json!("boolean")),
            (
                "watchlist",
                json!(["watch", "unwatch", "preferences", "nochange"]),
            ),
            ("md5", json!("string")),
            ("prependtext", json!("text")),
            ("appendtext", json!("text")),
            (
                "contentformat",
                json!([
                    "text/x-wiki",
                    "text/plain",
                    "application/json",
                    "text/markdown",
                    "application/yaml"
                ]),
            ),
            (
                "contentmodel",
                json!(["wikitext", "markdown", "json", "yaml", "text"]),
            ),
            ("token", json!("string")),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, t))| Value::Object(parameter(i, n, t)))
        .collect(),
        "watch" => [
            ("titles", json!("string")),
            ("pageids", json!("integer")),
            ("unwatch", json!("boolean")),
            ("expiry", json!("expiry")),
            ("tsnotify", json!("boolean")),
            ("token", json!("string")),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, t))| Value::Object(parameter(i, n, t)))
        .collect(),
        "move" => [
            ("from", json!("string")),
            ("fromid", json!("integer")),
            ("to", json!("string")),
            ("reason", json!("string")),
            ("movetalk", json!("boolean")),
            ("movesubpages", json!("boolean")),
            ("noredirect", json!("boolean")),
            (
                "watchlist",
                json!(["watch", "unwatch", "preferences", "nochange"]),
            ),
            ("ignorewarnings", json!("boolean")),
            ("tags", json!("tags")),
            ("token", json!("string")),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (n, t))| Value::Object(parameter(i, n, t)))
        .collect(),
        "wbgetentities" => vec![
            Value::Object(multi(parameter(0, "ids", json!("string")), 50)),
            Value::Object(with_default(
                multi(
                    parameter(
                        1,
                        "props",
                        json!([
                            "info",
                            "sitelinks",
                            "aliases",
                            "labels",
                            "descriptions",
                            "claims",
                            "datatype"
                        ]),
                    ),
                    50,
                ),
                json!("info|sitelinks|aliases|labels|descriptions|claims|datatype"),
            )),
            Value::Object(multi(parameter(2, "languages", json!(TERM_LANGUAGES)), 50)),
        ],
        _ => Vec::new(),
    }
}

/// `action=paraminfo` (mediawiki-compat.md §5): the module tree and each module's
/// parameters, in MediaWiki's shape. `main`, `query` and `paraminfo` are described in
/// full, since Pywikibot's `ParamInfo` reads the module tree from them; the action
/// modules carry their POST requirement; a `query+…` submodule or any other module
/// name is answered with an empty parameter list rather than refused.
pub fn paraminfo(ctx: &Ctx) -> Result<ApiResponse, ApiError> {
    let modules = paraminfo_modules(&ctx.params.list("modules"));
    Ok(ApiResponse::ok(json!({"paraminfo": {"modules": modules}})))
}

/// The `modules` array of `action=paraminfo` for the requested module paths.
#[allow(clippy::too_many_lines)]
pub fn paraminfo_modules(requested: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for m in requested {
        let module = match m.as_str() {
            "main" => {
                let actions: Vec<&str> = ACTION_MODULES.iter().map(|(n, _)| *n).collect();
                let mut action = submodule_parameter(0, "action", &actions, None);
                action["default"] = json!("help");
                if let Some(o) = action.as_object_mut() {
                    o.remove("multi");
                }
                let mut format = parameter(1, "format", json!(["json"]));
                format.insert("default".into(), json!("jsonfm"));
                format.insert("submodules".into(), json!({"json": "json"}));
                let mut maxlag = parameter(2, "maxlag", json!("integer"));
                maxlag.insert("default".into(), Value::Null);
                paraminfo_module(
                    "main",
                    "main",
                    "",
                    &[
                        action,
                        Value::Object(format),
                        Value::Object(maxlag),
                        Value::Object(parameter(3, "formatversion", json!(["1", "2", "latest"]))),
                    ],
                )
            }
            "paraminfo" => {
                let mut modules = parameter(0, "modules", json!("string"));
                modules.insert("multi".into(), json!(true));
                modules.insert("limit".into(), json!(50));
                modules.insert("lowlimit".into(), json!(50));
                modules.insert("highlimit".into(), json!(500));
                let mut helpformat =
                    parameter(1, "helpformat", json!(["html", "wikitext", "raw", "none"]));
                helpformat.insert("default".into(), json!("none"));
                paraminfo_module(
                    "paraminfo",
                    "paraminfo",
                    "",
                    &[Value::Object(modules), Value::Object(helpformat)],
                )
            }
            "query" => {
                let mut params: Vec<Value> = QUERY_MODULES
                    .iter()
                    .enumerate()
                    .map(|(i, (kind, names))| submodule_parameter(i, kind, names, Some("query")))
                    .collect();
                // Generators are submodules usable as a page set; none yet.
                let mut generator = parameter(params.len(), "generator", json!([]));
                generator.insert("submodules".into(), json!({}));
                params.push(Value::Object(generator));
                paraminfo_module("query", "query", "", &params)
            }
            other => {
                let (name, prefix) = match other.strip_prefix("query+") {
                    Some(sub) => (
                        sub,
                        match sub {
                            "siteinfo" => "si",
                            "userinfo" => "ui",
                            "wikibase" => "wb",
                            "authmanagerinfo" => "ami",
                            "notifications" => "not",
                            "allmessages" => "am",
                            "recentchanges" => "rc",
                            "usercontribs" => "uc",
                            "logevents" => "le",
                            "revisions" => "rv",
                            "deletedrevisions" => "drv",
                            "info" => "in",
                            "templates" => "tl",
                            "allpages" => "ap",
                            "protectedpages" => "pr",
                            "protectedtitles" => "pt",
                            "protectedsets" => "ps",
                            "users" => "us",
                            "blocks" => "bk",
                            "querypage" => "qp",
                            _ => "",
                        },
                    ),
                    None => (other, ""),
                };
                let mut module = paraminfo_module(name, other, prefix, &module_parameters(name));
                if let Some((_, post)) = ACTION_MODULES.iter().find(|(n, _)| *n == name)
                    && *post
                {
                    module["mustbeposted"] = json!(true);
                    module["writerights"] = json!(true);
                }
                if let Some(sub) = other.strip_prefix("query+")
                    && !QUERY_MODULES.iter().any(|(_, names)| names.contains(&sub))
                    && !ACTION_MODULES.iter().any(|(n, _)| *n == name)
                {
                    module = json!({"name": name, "path": other, "missing": true});
                }
                module
            }
        };
        out.push(module);
    }
    out
}

#[cfg(test)]
mod paraminfo_tests {
    use super::*;

    fn by_path(modules: &[Value], path: &str) -> Value {
        modules
            .iter()
            .find(|m| m["path"] == path)
            .cloned()
            .unwrap_or_else(|| panic!("module {path}"))
    }

    fn param<'a>(module: &'a Value, name: &str) -> &'a Value {
        module["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("parameter {name}"))
    }

    /// The invariants Pywikibot's `ParamInfo._init` and `_generate_submodules` assert.
    #[test]
    fn pywikibot_reads_the_module_tree() {
        let req: Vec<String> = [
            "main",
            "paraminfo",
            "query",
            "query+siteinfo",
            "wbeditentity",
            "query+nothing",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        let modules = paraminfo_modules(&req);

        // main: `action` lists the actions and maps each to itself; `format` exists.
        let main = by_path(&modules, "main");
        let action = param(&main, "action");
        let names: Vec<&str> = action["type"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let sub = action["submodules"].as_object().unwrap();
        assert_eq!(names.len(), sub.len());
        for n in &names {
            assert_eq!(sub[*n], json!(n));
        }
        assert!(names.contains(&"query") && names.contains(&"wbeditentity"));
        param(&main, "format");

        // query: prop/list/meta carry type, submodules (`query+name`) and limit; a
        // generator parameter exists and its type is within the submodules.
        let query = by_path(&modules, "query");
        let mut all_sub = std::collections::BTreeSet::new();
        for kind in ["prop", "list", "meta"] {
            let p = param(&query, kind);
            assert!(p["limit"].is_number(), "{kind} limit");
            for (child, path) in p["submodules"].as_object().unwrap() {
                assert_eq!(path.as_str().unwrap(), format!("query+{child}"));
                all_sub.insert(child.clone());
            }
        }
        assert!(all_sub.contains("siteinfo"));
        let generator = param(&query, "generator");
        for g in generator["type"].as_array().unwrap() {
            assert!(all_sub.contains(g.as_str().unwrap()));
        }

        // Submodules and action modules are described; an unknown submodule is `missing`.
        assert_eq!(by_path(&modules, "query+siteinfo")["group"], "meta");
        assert_eq!(by_path(&modules, "query+siteinfo")["prefix"], "si");
        assert_eq!(by_path(&modules, "wbeditentity")["mustbeposted"], true);
        assert_eq!(by_path(&modules, "query+nothing")["missing"], true);
        param(&by_path(&modules, "paraminfo"), "modules");

        // What Pywikibot validates against before calling: token types and languages.
        let more: Vec<String> = ["query+tokens", "wbsearchentities"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let more = paraminfo_modules(&more);
        let tokens = by_path(&more, "query+tokens");
        let types = param(&tokens, "type");
        assert!(types["type"].as_array().unwrap().contains(&json!("csrf")));
        let search = by_path(&more, "wbsearchentities");
        let langs = param(&search, "language");
        assert!(langs["type"].as_array().unwrap().contains(&json!("en")));
    }
}
