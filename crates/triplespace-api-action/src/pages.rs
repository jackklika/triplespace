//! Document pages for the Action API (0008 §4; 18 §2.3–2.4): the tenant's wikitext
//! settings, title resolution into the page namespaces, `view.page` lookups, and the
//! render pipeline that `action=parse` and the site's page views use.
//!
//! **Rendering** is the two stages of 11 §1.1: the text is expanded first by
//! `scatter-wikitext-expand`, whose host reads Template pages' latest text, since
//! expansion is part of wikitext wherever a tenant has wikitext pages (0042 A19); then
//! `scatter-wikitext` renders it. Both are pure, so the database is read
//! around them: expansion runs in rounds, the host recording the pages it did not have
//! and the next round having them, and before rendering the links the text makes are
//! extracted and their targets' existence, redirect flags and entity labels read in
//! three queries.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use scatter_titles::Namespaces;
use scatter_wikitext::html::url_encode_title;
use scatter_wikitext::{Config, PageInfo, Rendered, Resolver, Title, parse_target};
use scatter_wikitext_expand::{ExpandHost, Expansion, Transcluded};
use tokio_postgres::Client;
pub use triplespace_projections::acl_read::Hidden;

use crate::api::Ctx;
use crate::response::ApiError;

/// How many rounds of expansion fetch the pages the previous round lacked.
const EXPANSION_ROUNDS: usize = 8;

/// A tenant's site settings that pages depend on.
#[derive(Clone)]
pub struct Site {
    /// The wikitext configuration, with the site's name as Project's local name.
    pub config: Arc<Config>,
    /// `site.name`.
    pub name: String,
    /// `site.language`.
    pub lang: String,
    /// The tenant's table of names (0078 §3), which says which foreign IDs name an entity.
    pub registry: &'static scatter_providers::Registry,
}

impl Site {
    /// Loads the tenant's settings.
    pub async fn load(ctx: &Ctx) -> Result<Self, ApiError> {
        let mut site = Self::load_for(ctx.db(), &ctx.tenant.slug).await?;
        site.registry = ctx.registry().await?;
        Ok(site)
    }

    /// Loads a tenant's settings outside an Action API request, with the default table of
    /// names: its providers, without the tenant's entity sources.
    pub async fn load_for(db: &tokio_postgres::Client, tenant: &str) -> Result<Self, ApiError> {
        let (settings, _) = triplespace_accounts::settings::load(db, Some(tenant)).await?;
        let name = settings
            .value("site.name")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| tenant.to_string());
        let lang = settings
            .value("site.language")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "en".into());
        let mut names = triplespace_projections::pages::TenantNames::load(db, tenant)
            .await
            .map_err(ApiError::internal)?;
        names.site_name = Some(name.clone());
        Ok(Self {
            config: names.config(),
            name,
            lang,
            registry: scatter_providers::Registry::default_registry(),
        })
    }

    /// Whether a namespace holds pages on this tenant. Module and Module talk wait for
    /// Lua (`wikitext.lua`, 0043), which is not built, so they hold none yet.
    #[must_use]
    pub fn holds_pages(&self, ns: i32) -> bool {
        Namespaces::embedded()
            .get(ns)
            .is_some_and(|n| n.holds_pages(|_| false))
    }

    /// A title as `Project:Scope`, with the site's local names.
    #[must_use]
    pub fn prefixed(&self, ns: i32, title: &str) -> String {
        Title::new(ns, title).prefixed(&self.config)
    }
}

/// A site named Librarybase with no namespace records of its own: for tests.
#[cfg(test)]
pub(crate) fn tests_site() -> Site {
    Site {
        config: config_for("Librarybase"),
        name: "Librarybase".into(),
        lang: "en".into(),
        registry: scatter_providers::Registry::default_registry(),
    }
}

/// The wikitext configuration for a site name alone, without the tenant's namespace
/// records: for tests.
#[cfg(test)]
fn config_for(site: &str) -> Arc<Config> {
    triplespace_projections::pages::TenantNames {
        site_name: Some(site.to_string()),
        ..Default::default()
    }
    .config()
}

/// A document page's row.
#[derive(Debug, Clone)]
pub struct Page {
    /// Its page ID.
    pub page_id: u64,
    /// Its namespace.
    pub ns: i32,
    /// Its title within the namespace.
    pub title: String,
    /// Its content model.
    pub model: String,
    /// The latest revision.
    pub latest_revid: Option<u64>,
    /// The latest record's offset, the base of an edit.
    pub latest_offset: Option<u64>,
    /// Bytes of the latest text.
    pub len: i32,
    /// When it last changed, µs since the epoch.
    pub latest_at: Option<u64>,
    /// Whether the latest text is a redirect.
    pub is_redirect: bool,
    /// How many revisions it has.
    pub revisions: i32,
}

const PAGE_COLUMNS: &str = "page_id, ns, title, content_model, latest_revid, latest_offset,
    COALESCE(len, 0), (EXTRACT(EPOCH FROM latest_at) * 1000000)::bigint, is_redirect, revisions";

fn page_of(r: &tokio_postgres::Row) -> Page {
    let u = |i: usize| {
        r.get::<_, Option<i64>>(i)
            .and_then(|v| u64::try_from(v).ok())
    };
    Page {
        page_id: u64::try_from(r.get::<_, i64>(0)).unwrap_or(0),
        ns: r.get(1),
        title: r.get(2),
        model: r.get(3),
        latest_revid: u(4),
        latest_offset: u(5),
        len: r.get(6),
        latest_at: u(7),
        is_redirect: r.get(8),
        revisions: r.get(9),
    }
}

/// What the caller may not read on the tenant (0056 §5; 09 §5.3): pass it to the lookups
/// below on every read, so a page a confidential restriction hides from the caller is
/// missing, and [`Hidden::NONE`] on a write, which decides with the target's own chain.
pub async fn hidden(ctx: &Ctx) -> Result<Hidden, ApiError> {
    // The L0 cache of 0056 §5 for the one answer that is safe to remember: "nothing is
    // hidden for this principal", keyed by the tenant's ACL frontier. A non-empty set can
    // change with a page or thread written under a hidden target, so it is not cached;
    // an empty one becomes non-empty only through an ACL record, which moves the frontier.
    // The frontier is the newest ACL's offset together with the row count, so that a
    // retirement (a row gone) moves it as a write does.
    let row = ctx
        .db()
        .query_one(
            "SELECT coalesce(max(\"offset\"), 0), count(*) FROM view.acl WHERE tenant = $1",
            &[&ctx.tenant.slug],
        )
        .await?;
    let frontier = (row.get::<_, i64>(0) << 20) ^ row.get::<_, i64>(1);
    let key = (
        ctx.tenant.slug.clone(),
        ctx.caller
            .effective
            .groups
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
        ctx.caller
            .effective
            .reads_as
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join(","),
        ctx.caller.effective.reads_every,
    );
    if ctx.app.nothing_hidden().is_fresh(&key, frontier) {
        return Ok(Hidden::NONE);
    }
    let h = triplespace_projections::acl_read::hidden(
        ctx.db(),
        &ctx.tenant.slug,
        &ctx.caller.effective,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)?;
    if h.is_empty() {
        ctx.app.nothing_hidden().remember(key, frontier);
    }
    Ok(h)
}

/// The key of a principal's hidden-set answer: tenant, groups, inclusions.
pub type HiddenKey = (String, String, String, bool);

/// Principals for whom nothing is hidden, as of an ACL frontier (see [`hidden`]).
#[derive(Debug, Default)]
pub struct NothingHidden {
    entries: std::sync::Mutex<std::collections::HashMap<HiddenKey, i64>>,
}

impl NothingHidden {
    /// Whether the principal was found to have nothing hidden at this frontier.
    pub fn is_fresh(&self, key: &HiddenKey, frontier: i64) -> bool {
        self.entries
            .lock()
            .is_ok_and(|e| e.get(key) == Some(&frontier))
    }

    /// Remembers that nothing is hidden for the principal at this frontier.
    pub fn remember(&self, key: HiddenKey, frontier: i64) {
        if let Ok(mut e) = self.entries.lock() {
            if e.len() > 4096 {
                e.clear();
            }
            e.insert(key, frontier);
        }
    }
}

/// Whether the tenant is private and the caller outside it (09 §4.7): every entity, page
/// and row of it is then absent for the caller.
pub async fn private_to(ctx: &Ctx) -> Result<bool, ApiError> {
    triplespace_projections::acl_read::tenant_hidden(
        ctx.db(),
        &ctx.tenant.slug,
        &ctx.caller.effective,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    )
    .await
    .map_err(ApiError::internal)
}

/// A row, unless the caller may not read it.
fn visible(row: Option<&tokio_postgres::Row>, hidden: &Hidden) -> Option<Page> {
    row.map(page_of).filter(|p| !hidden.page(p.ns, p.page_id))
}

/// The page with a title, if one exists, is not deleted and is not hidden.
pub async fn by_title(
    db: &Client,
    tenant: &str,
    hidden: &Hidden,
    ns: i32,
    title: &str,
) -> Result<Option<Page>, ApiError> {
    let row = db
        .query_opt(
            &format!(
                "SELECT {PAGE_COLUMNS} FROM view.page
                 WHERE tenant = $1 AND ns = $2 AND title = $3 AND NOT deleted AND latest_offset IS NOT NULL"
            ),
            &[&tenant, &ns, &title],
        )
        .await?;
    Ok(visible(row.as_ref(), hidden))
}

/// The deleted pages that had a title, newest first, for a member of their deletion
/// group: those whose chain the caller passes as `Removed` with the deletion's group among
/// its own (09 §6.2); an absent one is left out (09 §5.3). Threads and talk pages included.
pub async fn deleted_by_title(ctx: &Ctx, ns: i32, title: &str) -> Result<Vec<Page>, ApiError> {
    let tenant = &ctx.tenant.slug;
    let rows = ctx
        .db()
        .query(
            &format!(
                "SELECT {PAGE_COLUMNS} FROM view.page
                 WHERE tenant = $1 AND ns = $2 AND title = $3 AND deleted
                 ORDER BY page_id DESC"
            ),
            &[tenant, &ns, &title],
        )
        .await?;
    let mut out = Vec::new();
    let at = scatter_actors::Timestamp(crate::edit::now_micros());
    for r in &rows {
        let page = page_of(r);
        let chain = triplespace_projections::acl_read::page_chain(ctx.db(), tenant, page.page_id)
            .await
            .map_err(ApiError::internal)?;
        let who = &ctx.caller.effective;
        match scatter_actors::read_decision(who, &chain, at) {
            scatter_actors::ReadDecision::Absent => {}
            scatter_actors::ReadDecision::Readable => out.push(page),
            scatter_actors::ReadDecision::Removed { .. } => {
                // The deletion's own group admits its members; a talk page deleted with
                // its subject is the subject's deletion.
                let admitted = chain.iter().any(|a| {
                    a.read_kind == Some(scatter_actors::ReadKind::Moderation)
                        && a.restrictions
                            .get(scatter_actors::acl::READ)
                            .and_then(|r| r.group.as_deref())
                            .is_some_and(|g| who.in_group(g))
                });
                if admitted {
                    out.push(page);
                }
            }
        }
    }
    Ok(out)
}

/// The page with an ID, if it is not deleted and not hidden.
pub async fn by_id(
    db: &Client,
    tenant: &str,
    hidden: &Hidden,
    page_id: u64,
) -> Result<Option<Page>, ApiError> {
    let row = db
        .query_opt(
            &format!(
                "SELECT {PAGE_COLUMNS} FROM view.page
                 WHERE tenant = $1 AND page_id = $2 AND NOT deleted AND latest_offset IS NOT NULL"
            ),
            &[&tenant, &i64::try_from(page_id).unwrap_or(i64::MAX)],
        )
        .await?;
    Ok(visible(row.as_ref(), hidden))
}

/// The page a revision belongs to, if the page is not deleted and not hidden.
pub async fn by_revid(
    db: &Client,
    tenant: &str,
    hidden: &Hidden,
    revid: u64,
) -> Result<Option<Page>, ApiError> {
    let row = db
        .query_opt(
            "SELECT target_id FROM view.activity
             WHERE tenant = $1 AND revid = $2 AND target_kind = 2",
            &[&tenant, &i64::try_from(revid).unwrap_or(i64::MAX)],
        )
        .await?;
    match row.and_then(|r| r.get::<_, Option<String>>(0)?.parse::<u64>().ok()) {
        Some(id) => by_id(db, tenant, hidden, id).await,
        None => Ok(None),
    }
}

/// The text of a page after a revision; `None` when the revision's text is not stored
/// (withheld or erased) or the revision is not the page's.
pub async fn text_at(
    db: &Client,
    tenant: &str,
    page_id: u64,
    revid: u64,
) -> Result<Option<String>, ApiError> {
    let found = triplespace_projections::pages::text_at(db, tenant, revid)
        .await
        .map_err(ApiError::internal)?;
    Ok(found
        .filter(|(key, _)| *key == page_id.to_string())
        .map(|(_, text)| text))
}

/// A page's latest text.
pub async fn latest_text(
    db: &Client,
    tenant: &str,
    page: &Page,
) -> Result<Option<String>, ApiError> {
    match page.latest_revid {
        Some(r) => text_at(db, tenant, page.page_id, r).await,
        None => Ok(None),
    }
}

/// A title as the API names one, resolved for the page namespaces: `None` when it is not a
/// valid title or names an entity namespace.
#[must_use]
pub fn resolve_title(site: &Site, title: &str) -> Option<Title> {
    let t = parse_target(title, &site.config, &Title::new(0, ""))?.title;
    if t.interwiki.is_some() || t.text.is_empty() {
        return None;
    }
    let ns = Namespaces::embedded().get(t.namespace)?;
    if ns.is_entity_namespace() {
        return None;
    }
    Some(t.without_fragment())
}

/// Whether a title names an entity page rather than a document page: a title in a
/// namespace that holds entities (`Item:Q42`, `Property:P3`, `Domain:wikipedia.org`), by
/// any of the tenant's names for it, whose ID is one the tenant serves: a local ID, a
/// keyed type's key, or a foreign ID of a registered provider or entity source, as an
/// entity read requires. A bare title is in the main namespace, which holds articles, so
/// `Q42` is the article titled `Q42`, not the item (10 §3.3; 0038 A17). Where a wiki's
/// main namespace held entities, as Wikidata's does, a bare ID would name the entity by
/// the same rule, since no article could share its title.
#[must_use]
pub fn names_entity(site: &Site, title: &str) -> bool {
    entity_of_title(site, title).is_some()
}

/// The entity a page title names, by [`names_entity`]'s rule: its ID, from the title's
/// text in an entity namespace however the tenant names that namespace (`Item:Q42` is
/// `Q42`, `Domain:Wikipedia.org` is `domain:wikipedia.org`).
#[must_use]
pub fn entity_of_title(site: &Site, title: &str) -> Option<scatter_wikibase_model::id::EntityId> {
    let t = parse_target(title, &site.config, &Title::new(0, ""))?.title;
    if t.interwiki.is_some() || t.text.is_empty() {
        return None;
    }
    let id = Namespaces::embedded()
        .get(t.namespace)?
        .entity_id(&t.text)?;
    crate::modules::entities::parse_id(&id)
        .ok()
        .filter(|id| crate::modules::entities::check_form(id, site.registry).is_ok())
}

// ---- rendering -----------------------------------------------------------------------

/// The expansion host: Template pages' latest text, from what the previous rounds read,
/// and the wiki's Interface pages for `{{int:}}` once a round has asked for one.
struct Host<'a> {
    site: &'a Site,
    known: HashMap<String, Option<String>>,
    misses: RefCell<Vec<(i32, String)>>,
    messages: Option<crate::interface::Overrides>,
    wants_messages: std::cell::Cell<bool>,
}

/// The page a call's name transcludes: Template is the default namespace, and a leading
/// colon means the main one; `None` for a name that is no title, or an interwiki one.
fn transcluded_title(site: &Site, name: &str) -> Option<Title> {
    let page = Title::new(scatter_titles::TEMPLATE, "");
    let t = if name.starts_with(':') {
        parse_target(name, &site.config, &page)?
    } else {
        let t = parse_target(name, &site.config, &page)?;
        if t.title.namespace == 0 {
            parse_target(&format!("Template:{name}"), &site.config, &page)?
        } else {
            t
        }
    };
    let title = t.title;
    title.interwiki.is_none().then_some(title)
}

impl ExpandHost for Host<'_> {
    fn transclude(&self, name: &str) -> Option<Transcluded> {
        let title = transcluded_title(self.site, name)?;
        let key = title.prefixed(&self.site.config);
        match self.known.get(&key) {
            Some(Some(text)) => Some(Transcluded::Found(text.clone())),
            Some(None) => Some(Transcluded::Missing(key)),
            None => {
                self.misses
                    .borrow_mut()
                    .push((title.namespace, title.text.clone()));
                Some(Transcluded::Missing(key))
            }
        }
    }

    /// `{{int:key}}` (0095 §2): the wiki's override in its own language. A page renders
    /// once for every reader, so it is the wiki's language, not the reader's; the build
    /// ships none of MediaWiki core's messages, so a key the wiki does not override is
    /// left for the renderer's chip.
    fn message(&self, key: &str) -> Option<String> {
        let Some(o) = &self.messages else {
            self.wants_messages.set(true);
            return None;
        };
        let key = crate::interface::key(key).0;
        o.message(&key, &self.site.lang, &self.site.lang)
            .map(|(text, _)| text.to_string())
    }
}

/// A page an expansion transcluded, or tried to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transclusion {
    /// Its namespace.
    pub ns: i32,
    /// Its title within it.
    pub title: String,
    /// Whether it exists.
    pub exists: bool,
}

/// Expands text for viewing the page that holds it, reading the pages it transcludes, and
/// says which pages those were, and where the page's own calls put their output. A page
/// hidden from the viewer transcludes as missing (09 §5.3).
pub async fn expand(
    db: &Client,
    tenant: &str,
    site: &Site,
    hidden: &Hidden,
    text: &str,
) -> Result<(Expansion, Vec<Transclusion>), ApiError> {
    let mut host = Host {
        site,
        known: HashMap::new(),
        misses: RefCell::new(Vec::new()),
        messages: None,
        wants_messages: std::cell::Cell::new(false),
    };
    let mut expanded = scatter_wikitext_expand::expand_for_view_mapped(text, &host);
    let mut used: Vec<Transclusion> = Vec::new();
    for _ in 0..EXPANSION_ROUNDS {
        let misses: Vec<(i32, String)> = std::mem::take(&mut *host.misses.borrow_mut());
        let wants = host.wants_messages.get() && host.messages.is_none();
        if misses.is_empty() && !wants {
            break;
        }
        if wants {
            let pages = crate::interface::pages(db, tenant).await?;
            host.messages = Some(crate::interface::Overrides::from_pages(&pages));
        }
        for (ns, title) in misses {
            let key = site.prefixed(ns, &title);
            if host.known.contains_key(&key) {
                continue;
            }
            // A page of a namespace that holds no pages here transcludes as missing.
            let text = if site.holds_pages(ns) || ns == scatter_titles::TEMPLATE {
                match by_title(db, tenant, hidden, ns, &title).await? {
                    Some(p) => latest_text(db, tenant, &p).await?,
                    None => None,
                }
            } else {
                None
            };
            used.push(Transclusion {
                ns,
                title,
                exists: text.is_some(),
            });
            host.known.insert(key, text);
        }
        expanded = scatter_wikitext_expand::expand_for_view_mapped(text, &host);
    }
    Ok((expanded, used))
}

/// The resolver over what was read for one render.
pub struct Links {
    config: Arc<Config>,
    base: String,
    pages: HashMap<(i32, String), bool>,
    entities: HashSet<String>,
    labels: HashMap<String, String>,
}

impl Resolver for Links {
    fn info(&self, t: &Title) -> PageInfo {
        if t.namespace == -1 {
            return PageInfo {
                exists: true,
                ..PageInfo::default()
            };
        }
        if let Some(id) = Namespaces::embedded()
            .get(t.namespace)
            .and_then(|n| n.entity_id(&t.text))
        {
            return PageInfo {
                exists: self.entities.contains(&id),
                redirect: false,
                label: self.labels.get(&id).cloned(),
            };
        }
        match self.pages.get(&(t.namespace, t.text.clone())) {
            Some(redirect) => PageInfo {
                exists: true,
                redirect: *redirect,
                label: None,
            },
            None => PageInfo::default(),
        }
    }
    fn url(&self, t: &Title) -> String {
        format!("{}/wiki/{}", self.base, url_encode_title(&self.prefixed(t)))
    }
    fn missing_url(&self, t: &Title) -> String {
        format!(
            "{}/w/index.php?title={}&action=edit&redlink=1",
            self.base,
            url_encode_title(&self.prefixed(t))
        )
    }
}

impl Links {
    fn prefixed(&self, t: &Title) -> String {
        t.prefixed(&self.config)
    }

    /// Reads what the links of a text need: their targets' existence, redirects and labels.
    /// A page hidden from the viewer is a red link, as a missing page is (09 §5.3).
    pub async fn read(
        db: &Client,
        tenant: &str,
        site: &Site,
        hidden: &Hidden,
        text: &str,
        page: &Title,
    ) -> Result<Self, ApiError> {
        let ex = scatter_wikitext::extract(text, &site.config, page);
        let namespaces = Namespaces::embedded();
        let mut nss: Vec<i32> = Vec::new();
        let mut titles: Vec<String> = Vec::new();
        let mut ids: Vec<String> = Vec::new();
        for t in ex
            .links
            .iter()
            .map(|l| &l.target)
            .chain(ex.redirect.as_ref())
        {
            if t.interwiki.is_some() {
                continue;
            }
            if let Some(id) = namespaces
                .get(t.namespace)
                .and_then(|n| n.entity_id(&t.text))
            {
                ids.push(id);
            } else {
                nss.push(t.namespace);
                titles.push(t.text.clone());
            }
        }
        let mut pages = HashMap::new();
        if !titles.is_empty() {
            for r in db
                .query(
                    "SELECT p.ns, p.title, p.is_redirect, p.page_id FROM view.page p
                     JOIN unnest($2::int[], $3::text[]) AS w(ns, title) ON p.ns = w.ns AND p.title = w.title
                     WHERE p.tenant = $1 AND NOT p.deleted AND p.latest_offset IS NOT NULL",
                    &[&tenant, &nss, &titles],
                )
                .await?
            {
                let ns: i32 = r.get(0);
                if hidden.page(ns, u64::try_from(r.get::<_, i64>(3)).unwrap_or(0)) {
                    continue;
                }
                pages.insert((ns, r.get::<_, String>(1)), r.get::<_, bool>(2));
            }
        }
        let mut entities = HashSet::new();
        let mut labels = HashMap::new();
        if !ids.is_empty() {
            for r in db
                .query(
                    "SELECT id FROM view.entity WHERE tenant IN ($1, '') AND id = ANY($2) AND NOT deleted",
                    &[&tenant, &ids],
                )
                .await?
            {
                entities.insert(r.get::<_, String>(0));
            }
            for r in db
                .query(
                    "SELECT entity_id, text FROM view.term
                     WHERE tenant IN ($1, '') AND entity_id = ANY($2) AND kind = 1 AND lang = $3
                     ORDER BY (tenant = $1)",
                    &[&tenant, &ids, &site.lang],
                )
                .await?
            {
                let id: String = r.get(0);
                let label: String = r.get(1);
                // Wikibase writes an entity link without its own text as `label (ID)`.
                labels.insert(id.clone(), format!("{label} ({id})"));
            }
        }
        Ok(Self {
            config: site.config.clone(),
            base: String::new(),
            pages,
            entities,
            labels,
        })
    }
}

/// A render, with what was read to make it.
pub struct Render {
    /// The rendered page.
    pub rendered: Rendered,
    /// The pages expansion transcluded.
    pub transcluded: Vec<Transclusion>,
    /// The resolver the links were rendered with.
    pub links: Links,
}

/// Expands and renders a page's text. With `templates`, each top-level block one of the
/// page's own template calls drew carries `data-ts-template`, the template's title, and
/// `data-ts-template-at`, where the call starts in the text, in bytes (0042 §4, A22).
pub async fn render(
    db: &Client,
    tenant: &str,
    site: &Site,
    hidden: &Hidden,
    text: &str,
    page: &Title,
    templates: bool,
) -> Result<Render, ApiError> {
    let (expansion, transcluded) = expand(db, tenant, site, hidden, text).await?;
    let text = expansion.text;
    let links = Links::read(db, tenant, site, hidden, &text, page).await?;
    let marks: Vec<scatter_wikitext::Mark> = if templates {
        expansion
            .calls
            .iter()
            .filter_map(|c| {
                let title = transcluded_title(site, &c.name)?;
                Some(scatter_wikitext::Mark {
                    range: c.output.clone(),
                    attrs: vec![
                        ("data-ts-template".to_string(), title.prefixed(&site.config)),
                        ("data-ts-template-at".to_string(), c.at.to_string()),
                    ],
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    let rendered = scatter_wikitext::render_marked(&text, &site.config, &links, page, &marks);
    Ok(Render {
        rendered,
        transcluded,
        links,
    })
}

/// What a read of an entity may show the caller (09 §5.3, §6.2; 0056 §5): its chain
/// (the entity, its namespace, the graph serving it, its sets, the tenant) decided as a
/// page's is. `Absent` answers as a missing entity; `Deleted` likewise for everyone
/// outside the deletion group, while members see it, as `view.entity.deleted` tells
/// lists to leave it out; `Readable` serves it. `graph` is the graph whose record serves
/// the entity, when known.
pub async fn entity_decision(
    ctx: &Ctx,
    id: &str,
    graph: Option<&str>,
) -> Result<scatter_actors::ReadDecision, ApiError> {
    let chain =
        triplespace_projections::acl_read::entity_chain(ctx.db(), &ctx.tenant.slug, id, graph)
            .await
            .map_err(ApiError::internal)?;
    Ok(scatter_actors::read_decision(
        &ctx.caller.effective,
        &chain,
        scatter_actors::Timestamp(crate::edit::now_micros()),
    ))
}

/// Whether the caller may read the entity at all: readable, or deleted and the caller in
/// the group (a member sees a deleted entity marked, as of a deleted page).
pub async fn entity_readable(ctx: &Ctx, id: &str, graph: Option<&str>) -> Result<bool, ApiError> {
    Ok(entity_decision(ctx, id, graph).await? == scatter_actors::ReadDecision::Readable)
}

/// A target's visibility as the site's **Restricted** pill shows it (19 §1.3; 0056 §13):
/// `None` where everyone may read, else the groups a `read` restriction along the chain
/// names and the names of the sets the target is in that restrict it. Asked only for a
/// target the caller already reads, so it tells them nothing new of what it hides.
#[must_use]
pub fn visibility_of(chain: &[scatter_actors::Acl]) -> Option<serde_json::Value> {
    use scatter_actors::acl::Target;
    let at = scatter_actors::Timestamp(crate::edit::now_micros());
    let v = scatter_actors::acl::visibility(chain, at);
    if v.is_public() {
        return None;
    }
    let sets: Vec<String> = chain
        .iter()
        .filter(|a| {
            matches!(a.target, Target::Set(_)) && a.read_restrictions_at(at).next().is_some()
        })
        .filter_map(|a| a.name.clone())
        .collect();
    Some(serde_json::json!({
        "groups": v.groups.iter().collect::<Vec<_>>(),
        "sets": sets,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> Site {
        tests_site()
    }

    /// A title names an entity only in a namespace that holds entities, and only for an
    /// ID the tenant serves; a bare title is a main-namespace article, whatever its shape
    /// (10 §3.3; 0038 A17).
    #[test]
    fn only_an_entity_namespace_names_an_entity() {
        let s = site();
        for t in [
            "Item:Q42",
            "item:Q42",
            "Property:P3",
            "Domain:wikipedia.org",
            "Item:WDQ42",
        ] {
            assert!(names_entity(&s, t), "{t} names an entity");
        }
        for t in [
            "Q42",
            "P3",
            "WDQ42",
            "Stub",
            "Citing sources",
            "Main Page",
            "Project:Home",
            "Talk:Q42",
            "Item_talk:Q42",
            "Item:Paris",
            "Item:STUb",
        ] {
            assert!(!names_entity(&s, t), "{t} is not an entity title");
        }
    }

    #[test]
    fn titles_resolve_into_page_namespaces() {
        let s = site();
        assert_eq!(
            resolve_title(&s, "project:scope"),
            Some(Title::new(4, "Scope"))
        );
        assert_eq!(
            resolve_title(&s, "Librarybase:Scope"),
            Some(Title::new(4, "Scope"))
        );
        assert_eq!(
            resolve_title(&s, "User_talk:Harej#x"),
            Some(Title::new(3, "Harej"))
        );
        assert_eq!(resolve_title(&s, "Item:Q5"), None);
        assert_eq!(resolve_title(&s, "a|b"), None);
        assert_eq!(s.prefixed(4, "Scope"), "Librarybase:Scope");
        assert!(s.holds_pages(10), "Template holds pages");
        assert!(s.holds_pages(4));
        assert!(!s.holds_pages(828), "Module waits for Lua");
    }
}
