//! Document pages (step 2; 10 §3.2; 03 §4.5, §4.7; 0051 §5; 0083 §4, §6): what a
//! `scatter:v0/page` record in a tenant's `pages` partition projects to.
//!
//! - **`view.page`**: the page's title, model and latest record, its length, time and
//!   revision count, and whether its latest text is a redirect.
//! - **`view.entity_revision`**: the text after every revision, keyed by the page ID in
//!   decimal (03 §4.4), so `prop=revisions` at any `oldid` reads one row (18 §2.4). A
//!   `move` stores the text it kept; a revision whose source withheld the text has no row.
//! - **`view.redirect`**: the target of a redirect's latest text, resolved to a page ID
//!   where one exists, and kept resolved as pages are created and moved.
//! - **`view.page_link`** and **`view.page_category`**: the rows the stored text makes,
//!   with `from_render = false`; the rows expanded text adds are the refresh job's
//!   (0083 §6). Links to titles are recorded by namespace and title (`6`, `{ns}:{title}`),
//!   so a link keeps its row when its target is created, moved or deleted; links into an
//!   entity namespace by entity ID (`1`); interwiki links are not rows.
//!
//! Links and categories are read with `scatter-wikitext`'s extraction from the stored
//! text, over the namespace registry. A tenant's local Project name (its site name,
//! 0008 A37) is not yet one of the names it reads.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use scatter_log::cbor::Value;
use scatter_log::header::Header;
use scatter_log::record::Record;
use scatter_pages::{PAYLOAD_PAGE, PageOp, TEXT_PART};
use scatter_projection::{Applied, Backend, BoxFuture, Projection, Step};
use scatter_titles::Namespaces;
use scatter_wikitext::{Config, ConfigSource, Namespace, Title};

use crate::backend::PgCx;
use crate::common::{content, partition_info, time_of, to_jsonb};
use scatter_log_postgres::PgClient;

fn sql(e: &tokio_postgres::Error) -> String {
    crate::common::describe(e)
}

/// The payload type of a document page's records.
pub const PAYLOAD: &str = PAYLOAD_PAGE;

/// The `target_kind` of a link row naming a title as `{ns}:{title}`.
pub const TARGET_TITLE: i16 = crate::common::target_kind::TITLE;

/// The wikitext configuration the registry's namespaces give, with their canonical names.
pub fn wikitext_config() -> &'static Config {
    static C: OnceLock<Config> = OnceLock::new();
    C.get_or_init(|| Config::new(TenantNames::default().source()))
}

/// The wikitext configuration source over the namespace registry, with a site name as
/// the Project namespace's local name where it is given.
#[must_use]
pub fn config_source(site_name: Option<&str>) -> ConfigSource {
    TenantNames {
        site_name: site_name.map(str::to_string),
        ..TenantNames::default()
    }
    .source()
}

/// A tenant's names for its namespaces (0008 §7, A37; 10 §1.3 rule 4, §3.5): the
/// registry's, Project under the site's name, and what the tenant's `namespace` records
/// give a registered number, a fixed local name and aliases. Rendering and link
/// extraction read titles through the same names, so a link the site renders as a
/// Project link is recorded as one.
///
/// A `namespace` record is the registry row as JSON (payloads.md §4), keyed
/// `namespace:{number}`; of its fields these are read: `id`, `name` (the local name, where
/// it is not the canonical one or, for Project, the site's) and `aliases`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TenantNames {
    /// `site.name`; the canonical `Project` where none is given.
    pub site_name: Option<String>,
    /// Per namespace number: the fixed local name, and the aliases.
    pub overrides: std::collections::BTreeMap<i32, (Option<String>, Vec<String>)>,
}

/// The `view.registry` rows [`TenantNames`] reads, for a tenant (`$1`): its `site.name`,
/// the instance's default for it, and its `namespace` records.
pub const TENANT_NAMES_SQL: &str = "SELECT tenant, kind, code, config FROM view.registry
    WHERE (tenant = $1 AND ((kind = 'site' AND code = 'site.name') OR kind = 'namespace'))
       OR (tenant = '' AND kind = 'template' AND code = 'site:site.name')";

impl TenantNames {
    /// From [`TENANT_NAMES_SQL`]'s rows; a tenant without a `site.name` is named by its
    /// slug, as the setting's default is.
    #[must_use]
    pub fn from_rows(slug: &str, rows: &[(String, String, String, serde_json::Value)]) -> Self {
        let mut own = None;
        let mut default = None;
        let mut overrides = std::collections::BTreeMap::new();
        for (tenant, kind, code, config) in rows {
            match kind.as_str() {
                "site" => own = config["value"].as_str().map(str::to_string),
                "template" if tenant.is_empty() => {
                    default = config["record"]["value"].as_str().map(str::to_string);
                }
                "namespace" => {
                    let id = config["id"]
                        .as_i64()
                        .and_then(|n| i32::try_from(n).ok())
                        .or_else(|| code.parse().ok());
                    let Some(id) = id else { continue };
                    let name = config["name"]
                        .as_str()
                        .filter(|n| !n.is_empty())
                        .map(str::to_string);
                    let aliases = config["aliases"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default();
                    overrides.insert(id, (name, aliases));
                }
                _ => {}
            }
        }
        Self {
            site_name: own.or(default).or_else(|| Some(slug.to_string())),
            overrides,
        }
    }

    /// Reads a tenant's names.
    ///
    /// # Errors
    ///
    /// The database's.
    pub async fn load(db: &tokio_postgres::Client, tenant: &str) -> Result<Self, String> {
        let rows = db
            .query(TENANT_NAMES_SQL, &[&tenant])
            .await
            .map_err(|e| sql(&e))?;
        let rows: Vec<_> = rows
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
            .collect();
        Ok(Self::from_rows(tenant, &rows))
    }

    /// The wikitext configuration source these names give.
    #[must_use]
    pub fn source(&self) -> ConfigSource {
        let namespaces = Namespaces::embedded()
            .iter()
            .map(|n| {
                let mut ns = Namespace::new(n.id, &n.canonical);
                ns.case_sensitive = n.case.as_deref() == Some("case-sensitive");
                ns.subpages = n.subpages;
                if n.id == 6 {
                    ns = ns.alias("Image");
                }
                if n.id == 7 {
                    ns = ns.alias("Image talk");
                }
                let (fixed, aliases) = self.overrides.get(&n.id).cloned().unwrap_or_default();
                // Project and Project talk take the site's name unless a record fixes theirs;
                // a namespace the registry names (Interface, 0095 §1) takes that name.
                let local = fixed.or_else(|| {
                    let site = self.site_name.as_deref();
                    match n.id {
                        scatter_titles::PROJECT => site.map(str::to_string),
                        5 => site.map(|s| format!("{s} talk")),
                        _ => n.name.clone(),
                    }
                });
                if let Some(local) = local.filter(|l| *l != n.canonical) {
                    ns.name = local;
                    ns = ns.canonical(&n.canonical);
                }
                for a in &aliases {
                    ns = ns.alias(a);
                }
                ns
            })
            .collect();
        ConfigSource {
            namespaces,
            ..ConfigSource::default()
        }
    }

    /// The configuration these names give, built once per set of names.
    #[must_use]
    pub fn config(&self) -> std::sync::Arc<Config> {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        static CACHE: OnceLock<Mutex<HashMap<TenantNames, Arc<Config>>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let mut map = cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        map.entry(self.clone())
            .or_insert_with(|| Arc::new(Config::new(self.source())))
            .clone()
    }

    /// The aliases `meta=siteinfo&siprop=namespacealiases` lists: every name a namespace
    /// answers to beside its local and canonical names, as `(id, alias)`.
    #[must_use]
    pub fn aliases(&self) -> Vec<(i32, String)> {
        self.source()
            .namespaces
            .iter()
            .flat_map(|n| n.aliases.iter().map(move |a| (n.id, a.clone())))
            .collect()
    }
}

/// What a page record says, read once.
pub struct PageRecord {
    /// The operation; `None` when the content part is erased.
    pub op: Option<PageOp>,
    /// The text part, where the record carries one.
    pub text: Option<String>,
}

impl PageRecord {
    /// Reads a page record.
    pub fn read(record: &Record) -> Result<Self, String> {
        let op = match content(record)? {
            Some(v) => Some(PageOp::from_json(to_jsonb(&v))?),
            None => None,
        };
        let text = record
            .body()
            .part(TEXT_PART)
            .and_then(|p| p.value().ok().flatten())
            .and_then(|v| match v {
                Value::Text(t) => Some(t),
                _ => None,
            });
        Ok(Self { op, text })
    }
}

/// The time a page record shows: an adopted revision's source time, else the append time.
#[must_use]
pub fn shown_time(header: &Header, op: &PageOp) -> u64 {
    op.source()
        .filter(|s| header.revid == Some(s.source_revid))
        .and_then(|s| crate::feed::parse_timestamp(&s.source_time))
        .unwrap_or(header.appended_at)
}

/// The `view` rows of document pages.
#[derive(Debug, Default, Clone, Copy)]
pub struct PageProjection;

impl<B: Backend<Cx = PgCx>> Projection<B> for PageProjection {
    fn name(&self) -> &'static str {
        "page"
    }

    fn step(&self) -> Step {
        Step::Sources
    }

    fn accepts(&self, header: &Header) -> bool {
        header.payload_type == PAYLOAD_PAGE
    }

    #[allow(clippy::too_many_lines)]
    fn apply<'a>(
        &'a self,
        cx: &'a mut PgCx,
        record: &'a Record,
    ) -> BoxFuture<'a, Result<Applied, String>> {
        Box::pin(async move {
            let h = record.header();
            let info = partition_info(cx, h.partition).await?;
            let page = PageRecord::read(record)?;
            let Some(op) = page.op else {
                return Ok(Applied::rows(0));
            };
            let page_id = h
                .page_id
                .and_then(|p| i64::try_from(p).ok())
                .ok_or("a page record carries its page ID")?;
            let offset = i64::try_from(h.offset).map_err(|e| e.to_string())?;
            let revid = h.revid.and_then(|r| i64::try_from(r).ok());
            let at = time_of(shown_time(h, &op));
            let tenant = info.tenant.as_str();
            // The report keys this record touches (0047 §4.3): its titles, and the link
            // targets and categories it drops or adds.
            let mut touched: BTreeSet<String> = BTreeSet::new();
            let len = page
                .text
                .as_ref()
                .map(|t| i32::try_from(t.len()).unwrap_or(i32::MAX));
            let mut rows = 0u64;

            // The title the latest text is read as, after this record.
            let (ns, title) = match &op {
                PageOp::Create {
                    ns, title, model, ..
                } => {
                    rows += cx
                        .execute(
                            concat!(
                                "INSERT INTO view.page (tenant, page_id, ns, title, content_model, latest_offset,
                               latest_revid, len, latest_at, revisions, reserved, random)
                             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1, false, ",
                                crate::page_random_sql!(),
                                ")
                             ON CONFLICT (tenant, page_id) DO UPDATE SET
                               ns = EXCLUDED.ns, title = EXCLUDED.title, content_model = EXCLUDED.content_model,
                               latest_offset = EXCLUDED.latest_offset, latest_revid = EXCLUDED.latest_revid,
                               len = EXCLUDED.len, latest_at = EXCLUDED.latest_at, revisions = 1, reserved = false"
                            ),
                            &[&tenant, &page_id, ns, title, model, &offset, &revid, &len, &at],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    (*ns, title.clone())
                }
                PageOp::Edit { model, .. } => {
                    let n = cx
                        .execute(
                            "UPDATE view.page SET latest_offset = $3, latest_revid = $4,
                               len = COALESCE($5, len), latest_at = $6,
                               content_model = COALESCE($7, content_model), revisions = revisions + 1
                             WHERE tenant = $1 AND page_id = $2 AND (latest_offset IS NULL OR latest_offset < $3)",
                            &[&tenant, &page_id, &offset, &revid, &len, &at, model],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    if n == 0 {
                        return Ok(Applied::rows(0));
                    }
                    rows += n;
                    current_title(cx, tenant, page_id).await?
                }
                PageOp::Move { ns, title, .. } => {
                    let (old_ns, old_title) = current_title(cx, tenant, page_id).await?;
                    touched.insert(crate::reports::title_key(old_ns, &old_title));
                    let n = cx
                        .execute(
                            "UPDATE view.page SET ns = $3, title = $4, latest_offset = $5, latest_revid = $6,
                               latest_at = $7, revisions = revisions + 1
                             WHERE tenant = $1 AND page_id = $2 AND (latest_offset IS NULL OR latest_offset < $5)",
                            &[&tenant, &page_id, ns, title, &offset, &revid, &at],
                        )
                        .await
                        .map_err(|e| sql(&e))?;
                    if n == 0 {
                        return Ok(Applied::rows(0));
                    }
                    rows += n;
                    // Redirects that named the page's old title no longer reach it.
                    cx.execute(
                        "UPDATE view.redirect SET target_page_id = NULL
                         WHERE tenant = $1 AND target_page_id = $2 AND NOT (target_ns = $3 AND target_title = $4)",
                        &[&tenant, &page_id, ns, title],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
                    (*ns, title.clone())
                }
            };

            // The text after the revision: its own, or for a move the text it kept.
            let text = match (&op, page.text) {
                (PageOp::Move { .. }, _) => latest_text(cx, tenant, page_id).await?,
                (_, t) => t,
            };
            if let (Some(text), Some(revid)) = (&text, revid) {
                rows += cx
                    .execute(
                        "INSERT INTO view.entity_revision (tenant, revid, entity_id, partition, \"offset\", state)
                         VALUES ($1, $2, $3, $4, $5, $6)
                         ON CONFLICT (tenant, revid) DO NOTHING",
                        &[
                            &tenant,
                            &revid,
                            &page_id.to_string(),
                            &scatter_log_postgres::ids::partition_to_db(h.partition),
                            &offset,
                            &text.as_bytes(),
                        ],
                    )
                    .await
                    .map_err(|e| sql(&e))?;
            }

            // A page that now holds a title resolves the redirects that name it.
            if matches!(op, PageOp::Create { .. } | PageOp::Move { .. }) {
                cx.execute(
                    "UPDATE view.redirect SET target_page_id = $2
                     WHERE tenant = $1 AND target_ns = $3 AND target_title = $4 AND interwiki IS NULL",
                    &[&tenant, &page_id, &ns, &title],
                )
                .await
                .map_err(|e| sql(&e))?;
            }

            touched.insert(crate::reports::title_key(ns, &title));
            if let Some(text) = &text {
                rows += index_text(cx, tenant, page_id, ns, &title, text, &mut touched).await?;
            }
            crate::reports::queue(cx, tenant, h.partition, h.offset, &touched).await?;
            Ok(Applied::rows(usize::try_from(rows).unwrap_or(usize::MAX)))
        })
    }

    fn reset<'a>(&'a self, cx: &'a mut PgCx, partition: u64) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let info = partition_info(cx, partition).await?;
            let p = scatter_log_postgres::ids::partition_to_db(partition);
            for q in [
                "DELETE FROM view.page_link WHERE tenant = $1 AND NOT from_render",
                "DELETE FROM view.page_category WHERE tenant = $1 AND NOT from_render",
                "DELETE FROM view.redirect WHERE tenant = $1",
                "DELETE FROM view.page WHERE tenant = $1 AND NOT reserved",
            ] {
                cx.execute(q, &[&info.tenant]).await.map_err(|e| sql(&e))?;
            }
            cx.execute(
                "DELETE FROM view.entity_revision e WHERE e.tenant = $1 AND e.partition = $2",
                &[&info.tenant, &p],
            )
            .await
            .map_err(|e| sql(&e))?;
            Ok(())
        })
    }
}

async fn current_title(cx: &PgCx, tenant: &str, page_id: i64) -> Result<(i32, String), String> {
    let row = cx
        .query_one(
            "SELECT ns, title FROM view.page WHERE tenant = $1 AND page_id = $2",
            &[&tenant, &page_id],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok((row.get(0), row.get(1)))
}

/// The page's newest stored text.
pub async fn latest_text<C: PgClient>(
    client: &C,
    tenant: &str,
    page_id: i64,
) -> Result<Option<String>, String> {
    let row = client
        .query_opt(
            "SELECT state FROM view.entity_revision WHERE tenant = $1 AND entity_id = $2
             ORDER BY \"offset\" DESC LIMIT 1",
            &[&tenant, &page_id.to_string()],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| String::from_utf8_lossy(&r.get::<_, Vec<u8>>(0)).into_owned()))
}

/// At most how many Interface pages a tenant's overrides take, and the longest text one may
/// have (0095 §5): sensible bounds on what every page's `meta=siteinfo` carries.
pub const INTERFACE_MAX_PAGES: i64 = 1000;
/// The longest Interface page text, in characters, that is taken as an override.
pub const INTERFACE_MAX_CHARS: usize = 2000;

/// A tenant's Interface pages (0095 §2, §5) that anyone may read, as `(title, text)` by
/// title, the title without its namespace: the overrides of interface messages and the
/// names of IDs. A page under a read restriction, a redirect, or one longer than
/// [`INTERFACE_MAX_CHARS`] is left out. Whether the tenant itself is private is not asked
/// here: `meta=siteinfo` gives outsiders of a private tenant none of this.
///
/// # Errors
///
/// The database's.
pub async fn interface_pages<C: PgClient>(
    client: &C,
    tenant: &str,
) -> Result<Vec<(String, String)>, String> {
    let rows = client
        .query(
            "SELECT p.page_id, p.title, r.state FROM view.page p
             JOIN LATERAL (SELECT state FROM view.entity_revision
                           WHERE tenant = p.tenant AND entity_id = p.page_id::text
                           ORDER BY \"offset\" DESC LIMIT 1) r ON true
             WHERE p.tenant = $1 AND p.ns = $2 AND NOT p.deleted AND NOT p.is_redirect
               AND p.latest_offset IS NOT NULL
             ORDER BY p.title LIMIT $3",
            &[&tenant, &scatter_titles::INTERFACE, &INTERFACE_MAX_PAGES],
        )
        .await
        .map_err(|e| sql(&e))?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let hidden = crate::acl_read::hidden(
        client,
        tenant,
        &scatter_actors::Effective::default(),
        scatter_actors::Timestamp(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX)),
        ),
    )
    .await?;
    Ok(rows
        .iter()
        .filter(|r| {
            let id: i64 = r.get(0);
            !hidden.namespaces.contains(&scatter_titles::INTERFACE)
                && !hidden.pages.contains(&id.cast_unsigned())
        })
        .map(|r| {
            (
                r.get::<_, String>(1),
                String::from_utf8_lossy(&r.get::<_, Vec<u8>>(2)).into_owned(),
            )
        })
        .filter(|(_, text)| text.chars().count() <= INTERFACE_MAX_CHARS)
        .collect())
}

/// The text of a page after a revision.
pub async fn text_at<C: PgClient>(
    client: &C,
    tenant: &str,
    revid: u64,
) -> Result<Option<(String, String)>, String> {
    let row = client
        .query_opt(
            "SELECT entity_id, state FROM view.entity_revision WHERE tenant = $1 AND revid = $2",
            &[&tenant, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await
        .map_err(|e| sql(&e))?;
    Ok(row.map(|r| {
        (
            r.get::<_, String>(0),
            String::from_utf8_lossy(&r.get::<_, Vec<u8>>(1)).into_owned(),
        )
    }))
}

/// Rewrites the rows the page's latest text makes: its redirect, links and categories,
/// adding to `touched` the report keys of every title link and category it removes or
/// writes (0047 §4.3).
async fn index_text(
    cx: &PgCx,
    tenant: &str,
    page_id: i64,
    ns: i32,
    title: &str,
    text: &str,
    touched: &mut BTreeSet<String>,
) -> Result<u64, String> {
    let config = TenantNames::load(cx.conn(), tenant).await?.config();
    let page = Title::new(ns, title);
    let ex = scatter_wikitext::extract(text, &config, &page);
    let namespaces = Namespaces::embedded();
    let mut rows = 0u64;

    cx.execute(
        "UPDATE view.page SET is_redirect = $3 WHERE tenant = $1 AND page_id = $2",
        &[&tenant, &page_id, &ex.redirect.is_some()],
    )
    .await
    .map_err(|e| sql(&e))?;
    match &ex.redirect {
        Some(t) => {
            let target_ns = t.interwiki.is_none().then_some(t.namespace);
            rows += cx
                .execute(
                    "INSERT INTO view.redirect (tenant, page_id, target_ns, target_title, fragment, interwiki, target_page_id)
                     VALUES ($1, $2, $3, $4, $5, $6,
                       (SELECT page_id FROM view.page p WHERE p.tenant = $1 AND p.ns = $3 AND p.title = $4 AND NOT p.deleted))
                     ON CONFLICT (tenant, page_id) DO UPDATE SET target_ns = EXCLUDED.target_ns,
                       target_title = EXCLUDED.target_title, fragment = EXCLUDED.fragment,
                       interwiki = EXCLUDED.interwiki, target_page_id = EXCLUDED.target_page_id",
                    &[&tenant, &page_id, &target_ns, &t.text, &t.fragment, &t.interwiki],
                )
                .await
                .map_err(|e| sql(&e))?;
        }
        None => {
            cx.execute(
                "DELETE FROM view.redirect WHERE tenant = $1 AND page_id = $2",
                &[&tenant, &page_id],
            )
            .await
            .map_err(|e| sql(&e))?;
        }
    }

    let mut links: BTreeSet<(i16, String)> = BTreeSet::new();
    let targets = ex
        .links
        .iter()
        .map(|l| &l.target)
        .chain(ex.redirect.as_ref());
    for t in targets {
        if t.interwiki.is_some() || t.text.is_empty() {
            continue;
        }
        let entity = namespaces
            .get(t.namespace)
            .and_then(|n| n.entity_id(&t.text));
        if let Some(id) = entity {
            links.insert((crate::common::target_kind::ENTITY, id));
        } else {
            links.insert((TARGET_TITLE, format!("{}:{}", t.namespace, t.text)));
        }
    }
    for r in cx
        .query(
            "DELETE FROM view.page_link WHERE tenant = $1 AND from_page = $2 AND NOT from_render
             RETURNING target_kind, target_id",
            &[&tenant, &page_id],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        if r.get::<_, i16>(0) == TARGET_TITLE {
            touched.insert(format!("title:{}", r.get::<_, String>(1)));
        }
    }
    for (kind, id) in &links {
        if *kind == TARGET_TITLE {
            touched.insert(format!("title:{id}"));
        }
        rows += cx
            .execute(
                "INSERT INTO view.page_link (tenant, from_page, target_kind, target_id, from_render)
                 VALUES ($1, $2, $3, $4, false) ON CONFLICT DO NOTHING",
                &[&tenant, &page_id, kind, id],
            )
            .await
            .map_err(|e| sql(&e))?;
    }

    rows += index_categories(cx, tenant, page_id, title, &ex.categories, touched).await?;
    Ok(rows)
}

/// Rewrites a page's own category rows, adding to `touched` the report key of every
/// category it leaves or joins (0047 §4.3).
async fn index_categories(
    cx: &PgCx,
    tenant: &str,
    page_id: i64,
    title: &str,
    categories: &[scatter_wikitext::Category],
    touched: &mut BTreeSet<String>,
) -> Result<u64, String> {
    let mut rows = 0u64;
    for r in cx
        .query(
            "DELETE FROM view.page_category WHERE tenant = $1 AND page_id = $2 AND NOT from_render
             RETURNING category",
            &[&tenant, &page_id],
        )
        .await
        .map_err(|e| sql(&e))?
    {
        touched.insert(crate::reports::category_key(&r.get::<_, String>(0)));
    }
    let mut seen = BTreeSet::new();
    for c in categories {
        if !seen.insert(c.name.clone()) {
            continue;
        }
        touched.insert(crate::reports::category_key(&c.name));
        let sortkey = c.sort_key.clone().unwrap_or_else(|| title.to_string());
        rows += cx
            .execute(
                "INSERT INTO view.page_category (tenant, page_id, category, sortkey, from_render)
                 VALUES ($1, $2, $3, $4, false) ON CONFLICT DO NOTHING",
                &[&tenant, &page_id, &c.name, &sortkey],
            )
            .await
            .map_err(|e| sql(&e))?;
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tenant_names_its_namespaces() {
        use serde_json::json;
        let row = |tenant: &str, kind: &str, code: &str, config: serde_json::Value| {
            (
                tenant.to_string(),
                kind.to_string(),
                code.to_string(),
                config,
            )
        };
        // No record: the slug names Project and Project talk.
        let names = TenantNames::from_rows("librarybase", &[]);
        let c = names.config();
        assert_eq!(
            c.namespace_by_name("Librarybase talk").map(|n| n.id),
            Some(5)
        );
        assert_eq!(c.namespace_by_name("project").map(|n| n.id), Some(4));
        // The instance default, then the tenant's own name.
        let names = TenantNames::from_rows(
            "lb",
            &[row(
                "",
                "template",
                "site:site.name",
                json!({"kind": "template", "record": {"value": "Default"}}),
            )],
        );
        assert_eq!(names.site_name.as_deref(), Some("Default"));
        let names = TenantNames::from_rows(
            "lb",
            &[
                row(
                    "",
                    "template",
                    "site:site.name",
                    json!({"record": {"value": "Default"}}),
                ),
                row(
                    "lb",
                    "site",
                    "site.name",
                    json!({"kind": "site", "value": "Librarybase"}),
                ),
                row(
                    "lb",
                    "namespace",
                    "4",
                    json!({"kind": "namespace", "id": 4, "aliases": ["LB"]}),
                ),
                row(
                    "lb",
                    "namespace",
                    "2",
                    json!({"kind": "namespace", "id": 2, "name": "Person", "aliases": ["U"]}),
                ),
            ],
        );
        let c = names.config();
        assert_eq!(c.namespace_by_name("LB").map(|n| n.id), Some(4));
        assert_eq!(c.namespace_by_name("Librarybase").map(|n| n.id), Some(4));
        assert_eq!(c.namespace_by_name("person").map(|n| n.id), Some(2));
        assert_eq!(
            c.namespace_by_name("User").map(|n| n.id),
            Some(2),
            "canonical kept"
        );
        assert_eq!(Title::new(2, "Harej").prefixed(&c), "Person:Harej");
        let ex = scatter_wikitext::extract("[[LB:Scope]] [[U:Harej]]", &c, &Title::new(0, "X"));
        assert_eq!(ex.links[0].target, Title::new(4, "Scope"));
        assert_eq!(ex.links[1].target, Title::new(2, "Harej"));
        let aliases = names.aliases();
        assert!(aliases.contains(&(4, "LB".into())));
        assert!(aliases.contains(&(6, "Image".into())));
    }

    #[test]
    fn the_registry_configures_wikitext() {
        let c = wikitext_config();
        assert_eq!(c.namespace(4).map(|n| n.name.as_str()), Some("Project"));
        assert_eq!(c.namespace_by_name("image").map(|n| n.id), Some(6));
        let page = Title::new(4, "Home");
        let ex = scatter_wikitext::extract(
            "[[Item:Q5]] [[project:scope]] [[Category:X|k]]\n#REDIRECT [[Not first]]",
            c,
            &page,
        );
        // A `#REDIRECT` after the first line is a list item, and its link an ordinary one.
        assert!(ex.redirect.is_none());
        assert_eq!(ex.links.len(), 3);
        assert_eq!(ex.links[1].target, Title::new(4, "Scope"));
        assert_eq!(ex.links[2].target, Title::new(0, "Not first"));
        assert_eq!(ex.categories[0].name, "X");
        let with_site = Config::new(config_source(Some("Librarybase")));
        assert_eq!(
            with_site.namespace_by_name("Librarybase").map(|n| n.id),
            Some(4)
        );
        assert_eq!(
            with_site.namespace_by_name("Project").map(|n| n.id),
            Some(4)
        );
    }
}
