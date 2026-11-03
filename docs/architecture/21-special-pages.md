# 21. Special pages

This chapter describes the pages in the `Special` namespace: the registry that lists every name the instance knows, how names and aliases resolve and where a page is served, the report pages and their backings, the Wikibase compatibility forms, the identity and correction pages, `Special:Nuke`, `Special:Export` and `Special:Import`, `Special:Version`, the pages later ADRs added, and the names that are deferred, declined or reserved. It ends with one catalogue of every entry in `docs/registry/special-pages.toml`. It assumes the `Special` namespace and title resolution of [10](10-pages-and-content-models.md), the `view` tables and projection order of [03](03-storage-caches-and-search.md), the permission catalogue of [09](09-security-and-moderation.md), the tenant and farm bases of [08](08-tenants-and-instances.md), and the Action and REST routes of [18](18-api.md), which hold the API that these pages share; the rate classes are in [07](07-actors-and-accounts.md) and the settings in [23](23-configuration-and-registry.md).

## 1. The registry

*Sources: [0047](../decisions/0047-special-pages.md) §1, §2, §3.*

### 1.1 One registry of special pages

*Sources: [0047](../decisions/0047-special-pages.md) §1.*

**`docs/registry/special-pages.toml` lists every special page name the instance knows**, including pages it serves, pages it defers or declines, and names it reserves. It is embedded by `scatter-titles`, beside the namespace registry, for the server and the site alike; `triplespace-titles`, the DB-backed resolver over it, resolves titles in the `Special` namespace ([0008](../decisions/0008-namespaces-and-document-pages.md) §3; [0047](../decisions/0047-special-pages.md) A28). Each entry has these fields:

| Field | Meaning |
|---|---|
| `name` | The canonical name. Where MediaWiki or an extension has the page, this is the first English alias MediaWiki gives it, so `RecentChanges` and not `Recentchanges`, and `MostLinkedFiles` and not `Mostimages`. |
| `mediawiki_name` | MediaWiki's internal name, where it differs from `name`. `meta=siteinfo&siprop=specialpagealiases` reports it as `realname`, and `list=querypage` accepts it as `qppage`. |
| `origin` | `mediawiki`, an extension's name as `version.toml` lists it (`Wikibase`, `WikibaseClient`, `Nuke`, `AbuseFilter`, `OAuth`, `Echo`, `CentralAuth`, `GlobalBlocking`, `WikibaseQualityConstraints`, `WikibaseLexeme`, `EntitySchema`, `GlobalContributions`; [0077](../decisions/0077-special-version.md) §8), or `triplespace` |
| `status` | `served`, `deferred` (with `until`), `declined` (with `reason`) or `reserved` |
| `scope` | `tenant`, `farm` or `both` (§1.3) |
| `group` | The heading under which `Special:SpecialPages` lists it. These are MediaWiki's group keys plus `wikibaserepo`, `wikibaseclient`, `identity` and `instance`. |
| `aliases` | Other names that reach the page (§1.2) |
| `section_aliases` | MediaWiki pages whose function is part of this page, each mapped to a subpage or an anchor (§1.2) |
| `restricted` | The permission without which the page is not listed and refuses to act, as MediaWiki's restricted special pages do |
| `report` | For reports only: `backing`, the tables it reads, and its default graph scope (§2) |
| `listed` | `false` for pages served but not listed (redirecting pages, per-object pages), under the group `unlisted` |
| `requires` | A `site` setting that must be on ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2) for the page to be listed and served |
| `note` | Free text for maintainers |
| `adr` | Where the page is specified |

**`Special:SpecialPages` lists the pages a reader is offered under their `group` headings**, in this order: `maintenance`, `pages`, `login`, `users`, `changes`, `media`, `wiki`, `redirects`, `highuse`, `pagetools`, `wikibaserepo`, `wikibaseclient`, `identity`, `instance`, and `other` last, as MediaWiki keeps it; under a heading, by the page's name in the reader's language. Each heading is MediaWiki's or Wikibase's message (`specialpages-group-*`), with Triplespace's own for `identity` (*Identity and corrections*) and `instance` (*Instance administration*), and carries MediaWiki's class and id (`mw-specialpagesgroup`, `mw-specialpagesgroup-{key}`), which gadgets and user scripts select. A page under `unlisted`, with `listed = false`, or `restricted` to a permission the reader does not hold, is not listed; a heading with no page is not drawn ([0047](../decisions/0047-special-pages.md) A27).

**Rules**, as for the other registry files:

- Names are never removed. A page that stops being served changes status.
- A name is never given a second meaning.
- **Every `Special:` name an ADR uses must be in the file**, as a `name`, an alias or a section alias. `check_adrs.py` has a fifth check that enforces this.
- Upstream page names that the ADRs do not decide can be added later as `deferred`.

**Statuses:**

- **`served`:** the decision that the instance serves the page. "Served" records a decision, not an implementation; implementation is tracked in the milestones, and a served page exists in a build only once it registers a handler. At startup `triplespace-titles` intersects the registry's `served` set with the handlers the binary compiled, and that intersection is what the instance reports as its special pages (§1.2) and lists on `Special:SpecialPages`; a served page without a handler is reported, with the difference, on `server.admin_listen`, and a request for it gets MediaWiki's "no such special page" response rather than a 404 or an empty page, so a client never learns of a page the build cannot serve.
- **`deferred`:** undecided, with the open question it waits on.
- **`declined`:** decided against, with the reason. Requests get MediaWiki's "no such special page" response.
- **`reserved`:** the name belongs to an entity type Triplespace has reserved but not implemented. Lexeme and EntitySchema were the cases until [0064](../decisions/0064-entityschema-and-validation.md) and [0066](../decisions/0066-lexemes.md) implemented them; no name is reserved today. The name will not be used for anything else.

### 1.2 Names, aliases and section aliases

*Sources: [0047](../decisions/0047-special-pages.md) §2; [0085](../decisions/0085-settings-and-preferences.md) §5.*

**An alias resolves to its page.**

- Aliases are matched case-insensitively, as MediaWiki matches them.
- A `GET` for an alias is redirected (301) to the canonical name, with the subpage and query string kept. A `POST` is served in place. This is how MediaWiki's `SpecialPageFactory::executePath` handles a name that is not the page's local name.
- Aliases cover MediaWiki's other English names (`Special:Random` and `Special:RandomPage`). They also cover the MediaWiki names of pages Triplespace renamed:

| Triplespace page | Aliases |
|---|---|
| `EditFilter` ([0030](../decisions/0030-edit-filters.md)) | `AbuseFilter` |
| `EditFilterLog` | `AbuseLog` |
| `OAuthConsumers` ([0025](../decisions/0025-oauth-server.md) §5) | `OAuthListConsumers`, `OAuthConsumerRegistration`, `OAuthManageConsumers` |
| `GlobalGroupMembership` ([0028](../decisions/0028-tenancy-policy.md)) | `GlobalUserRights` |
| `GlobalBlockList` | `ListGlobalBlocks` |

**A section alias redirects to the part of a page that does the job.** The target is a subpage or an anchor. These are mostly sections of `Special:Account` ([0010](../decisions/0010-site-ui.md) §11 and its extensions):

| MediaWiki page | Goes to |
|---|---|
| `ResetTokens` | `Account#watchlist-token` |
| `DisplayNotificationsConfiguration` | `Preferences#notifications`; `Preferences` is a page of its own ([0085](../decisions/0085-settings-and-preferences.md) §5) |
| `ChangeCredentials`, `RemoveCredentials`, `ChangePassword`, `LinkAccounts`, `UnlinkAccounts` | `Account#sign-in-methods`. MediaWiki's "linked accounts" are external sign-in providers, which are bindings here ([0007](../decisions/0007-actor-identity.md) §3). Triplespace's own **Linked accounts** section is a different thing (0007 §7). |
| `ChangeEmail`, `Confirmemail`, `Invalidateemail` | `Account#contacts`, where the email address and its verification live ([0021](../decisions/0021-notifications.md) §4, §6; [0085](../decisions/0085-settings-and-preferences.md) §5) |
| `BotPasswords` | `Account#subsidiaries` ([0024](../decisions/0024-subsidiary-accounts.md) §4) |
| `OAuthManageMyGrants` | `Account#connected-applications` ([0025](../decisions/0025-oauth-server.md) §5) |
| `NotificationsMarkRead` | `Notifications` |
| `Listadmins`, `Listbots` | `ListUsers/sysop`, `ListUsers/bot` |

**`siprop=specialpagealiases`** reports one entry per page the build serves: the registry's `served` set intersected with the compiled handlers (§1.1). Its `realname` is `mediawiki_name`, or `name` where there is no `mediawiki_name`. Its aliases are `name` followed by `aliases`. Each section alias is reported as an entry of its own, so a client asking for `Preferences` finds it. The Action API side is in [18](18-api.md).

**Localized aliases** are messages in `i18n/` ([0034](../decisions/0034-frontend-stack.md) §9), in the shape of MediaWiki's `*.alias.php` files. The first alias in the tenant's content language is the name the UI links to. Every alias in every language resolves. The registry holds the English names, which are the ones a tool can rely on.

### 1.3 Where a page is served

*Sources: [0047](../decisions/0047-special-pages.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §4; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §7.*

**A page is served where its subject lives.**

- **`farm`:** pages about instance-scope things. These are tenants and the tenancy policy, farm accounts, global groups and global blocks, instance acts and takedowns:
  - `Tenants`, `GlobalUsers`, `GlobalGroupMembership`, `GlobalBlock`, `GlobalBlockList`, `RemoveGlobalBlock`, `GlobalRecentChanges`;
  - `InstanceAction`, `InstanceSettings`, `Takedowns`, `Takedown`;
  - `OAuthConsumers`, whose approval is an instance right ([0046](../decisions/0046-primary-tenant.md) §8).
- **`both`:** pages with a tenant form and an instance form.
  - `Log`: a tenant's log, or the public records of the instance `log` (0046 §4).
  - `Jobs`: tenant bulk jobs, or instance jobs such as mirror syncs (0046 §4).
  - `Contributions`: at the farm base, the operator's acts for `ts-viewoperator` ([0040](../decisions/0040-instance-prerogatives.md) §7).
  - `Providers`: every provider at the farm base; at a tenant, the providers it has opted into, with its own lag.
  - `Tenancy`: a redirect to the Tenancy tab of `InstanceSettings` at the farm base and of `WikiSettings` at a tenant base, where 0046 §9's **Accept** now is ([0085](../decisions/0085-settings-and-preferences.md) §4).
  - `Version` and `SpecialPages`.
- **`tenant`:** everything else, including `GlobalContributions`, which [0018](../decisions/0018-tenants.md) §8 serves on a tenant for the accounts linked to it.

A `farm` page requested at a tenant base is redirected to the farm base. `Special:Tenants` is the page of the tenant list that `GET /tenancy` returns at the farm base (slug, host, name, status; [18](18-api.md)).

**Where the farm base is also a tenant's base**, the two sets of pages share a host. That is always so on a single-tenant instance, and it may be so on a farm ([0046](../decisions/0046-primary-tenant.md) §7). Names are unique in the registry, so nothing collides.

A `both` page at such a host shows both forms together, with a `scope=tenant|instance` filter that defaults to both. 0046 §7 says special pages "keep their paths". This is how two pages keep one path.

**The auth base** ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1, §7; [07](07-actors-and-accounts.md) §2.8) serves only the pages the registry marks `auth = true`: `UserLogin`, `CreateAccount`, `Account` and `UserLogout`, each in its auth form, acting on the instance login; every other page is a 404 there. Each is also a `tenant` page, in its tenant form, and on a single-tenant instance without an auth host the tenant's base is the auth base and serves both forms as one.

## 2. Reports

*Sources: [0047](../decisions/0047-special-pages.md) §4; [0051](../decisions/0051-page-redirects.md) §6; [0072](../decisions/0072-template-mappings.md) §5.*

### 2.1 What a report is

*Sources: [0047](../decisions/0047-special-pages.md) §4.1.*

**A report lists the things that meet a condition, sorted and paged.** Reports include:

- MediaWiki's QueryPage pages: the `qppage` list of `list=querypage` and its unlisted siblings;
- Wikibase's lists (`ItemsWithoutSitelinks`, `ListProperties`, `UnconnectedPages`, `PagesWithBadges`, `EntityUsage`);
- Triplespace's own lists (§3.2).

Feeds ([0020](../decisions/0020-change-feeds.md)), logs, forms and pages about a single thing are not reports. Every report has a `report` table in the registry.

### 2.2 Local graphs by default

*Sources: [0047](../decisions/0047-special-pages.md) §4.2; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

**A report reads the tenant's own partitions** (`local`, `pages`, `log`, `actors`, `config` and its file versions) and nothing else by default. It does not read `mirror/*`, `log/{provider}`, `files/{repo}` or any other tenant's partitions. This is the general rule for report pages.

- **Page and file reports are unaffected.** Pages and uploads are always the tenant's own.
- **Entity reports list local subjects.** The rows are local entities, and local assertions where a report is about statements. Conditions are evaluated on the **resolved view**, as a reader sees it, which means on composed rows: an entity report consumes composition events ([03](03-storage-caches-and-search.md) §6) and re-evaluates an entry only when the canonical's composed row changes **and** the cluster has a local member, found by an index on `cluster_id` restricted to local IDs, so a mirror's term changes never fan through the report projection for clusters with no local member. For example:
  - `EntitiesWithoutLabel` lists local entities that have no label in the language. A local item fused with a labelled Wikidata item ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4) has a label, so it is not listed.
  - `ListProperties` lists local properties.
  - `ItemsWithoutSitelinks` lists local items.

  The report answers "what is missing among the things this wiki made, as its readers see them".
- **Widening takes a `graphs=` parameter** that names source graphs.
  - It is served live only where the report's backing already covers those graphs. `ConstraintReport` is the main case: `view.constraint_violation` covers the resolved view ([0031](../decisions/0031-property-constraints.md) §2, §5), and `IdentityConflicts` (§3.2) is another.
  - Elsewhere, widening must be configured by the instance's operators in its `reports` configuration (§2.3), and a report configured for mirror graphs is always **batch** for that graph set. No live projection is maintained at mirror scale.
- **`ConstraintReport` defaults to the local graph.** Its existing source-graph filter widens it.

### 2.3 Live projections by default; batch by configuration

*Sources: [0047](../decisions/0047-special-pages.md) §4.3; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §6.*

Every report has one of three backings:

| Backing | What serves it | Freshness |
|---|---|---|
| `index` | A keyset-paged query over a `view` table and one of its indexes. The report has no rows of its own. | Live, at the lag of the table it reads |
| `projection` | Rows in `view.report_entry` ([03](03-storage-caches-and-search.md) §4.10), maintained incrementally by the `report` projection as the tables it reads change | Live, at that projection's lag; for an entity report, bounded by composition lag |
| `batch` | Rows in `view.report_entry`, recomputed by a scheduled `ops` job | As of the last run |

**No report is batch by default.** The registry gives each report `index` or `projection`, and §2.4 lists them.

**Whether a report is batch is an instance setting, not a tenant's.** The mode exists to manage the instance's load. A small instance runs every report live and needs no setup. A large instance plans batch runs for its expensive reports. That is the operators' call, made where they manage the rest of the instance's capacity.

**Settings live in an instance-scope `config` kind, `reports`** ([23](23-configuration-and-registry.md)). The code is `default`, or a tenant's slug for an override that applies to that tenant only. For example, a farm can run one large tenant's reports in batch while every other tenant stays live. An override replaces the default key by key. The entry holds:

| Key | Meaning | Default |
|---|---|---|
| `batch` | Report names to run as batch. Any `projection` report can be listed. | None |
| `widen` | Report name → the mirror graphs it also covers (§2.2). Each widening is a batch run for that graph set. | None |
| `schedule` | When batch runs start | Daily |
| `batch_limit` | Rows kept per batch report, as MediaWiki's `$wgQueryCacheLimit` | 1,000 |

The settings are written with `ts-config` at the farm base, which is an instance right ([0040](../decisions/0040-instance-prerogatives.md) §9). A tenant's administrators cannot change them. They see each report's mode on the report page and in `GET /reports` ([18](18-api.md)). On a single-tenant instance the operators and the wiki's owners are usually the same people, so nothing changes for them.

Changing a mode writes nothing into any tenant's partitions, so it is not an instance act ([0040](../decisions/0040-instance-prerogatives.md) §1). When a report moves from live to batch, its next run fills its rows. When it moves from batch to live, the `report` projection rebuilds its rows for that tenant by the named operation "rebuild projection P for tenant T from `view`" ([03](03-storage-caches-and-search.md) §6), which a view-derived projection has and a log-replayed one does not.

A batch report's page says when it was computed, as MediaWiki's cached reports do, and a live entity report says beside its "computed at" how far composition is behind, since its freshness is bounded by composition lag. `list=querypage` returns `cached`, `cachedtimestamp` and `maxresults` for a batch report.

**How the projection keeps up.**

- The `report` projection is a **view-derived** projection and a **tier-3 consumer** ([03](03-storage-caches-and-search.md) §6): it is populated by a scan of the `view` tables it reads, never by a log replay, and it consumes events — "entity X composed to version N" for entity reports, the page projection's and the refresh job's row changes for page reports — in its own queue, at its own rate, in no write's transaction, and it is off during bootstrap and bulk modes until the operator turns it on. Each event recomputes only the entries its key affects. For example, a new `page_link` row to a missing title adds one to that title's `WantedPages` count, and a composition event for a canonical with no local member touches nothing (§2.2).
- Report entries are never part of a write's own rows. Their lag is reported with the other consumers', on the report page and in `GET /reports`.
- Some inputs are written by the refresh job rather than by the page projection ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10): `transclusion`, `entity_usage`, and the render-owned rows of links and categories under expansion. The refresh job's rows are events to the report projection like any other.

**Rebuilds.**

- `projection` entries are a function of the `view` tables they read, and are rebuilt from those tables, per tenant where asked, never from the log.
- `batch` entries are a dated snapshot. A rebuild empties them, and the next run fills them.

As with 0042 §10's tables, this is stated rather than left to be assumed.

### 2.4 The reports

*Sources: [0047](../decisions/0047-special-pages.md) §4.4; [0051](../decisions/0051-page-redirects.md) §6.*

The columns `len`, `latest_at`, `revisions` and `random` of `view.page` and `last_active` of `view.actor` that the index-backed reports read are in [03](03-storage-caches-and-search.md) §4.5.

**Index-backed:**

| Report | Reads |
|---|---|
| `AllPages`, `PrefixIndex` | `view.page (ns, title)` |
| `Categories`, `TrackingCategories` | `view.category`; 0042 §9's tracking-category names |
| `ShortPages`, `LongPages` | `view.page.len` |
| `AncientPages` | `view.page.latest_at` |
| `FewestRevisions`, `MostRevisions` | `view.page.revisions` |
| `NewPages` | Creations in `view.activity`, with the `patrolled` filter ([0023](../decisions/0023-moderation.md) §6) |
| `ProtectedPages`, `ProtectedTitles` | `view.acl`; reserved rows in `view.page` (0023 §10) |
| `ListFiles`, `NewFiles`, `MIMESearch`, `FileDuplicateSearch` | `view.file`, `view.file_version` ([0039](../decisions/0039-files-and-media.md) §20) |
| `ListUsers`, `ActiveUsers` | `view.actor`; `view.actor.last_active`; active means within `$wgActiveUserDays`, default 30 |
| `ListProperties` | `view.entity` by type and datatype |
| `EntityUsage` | `view.entity_usage` ([0043](../decisions/0043-lua-modules.md) §10) |
| `PagesWithBadges` | `view.sitelink` for the tenant's own host ([0038](../decisions/0038-page-metadata-and-categories.md) §6) |
| `ConstraintReport` | `view.constraint_violation` ([0031](../decisions/0031-property-constraints.md) §5) |
| `Corrections`, `IdentityConflicts` | `view.correction`; `view.link` (§3.2) |
| `Random`, `RandomInCategory`, `RandomRootpage` | `view.page.random`, as MediaWiki's `page_random` |
| `ListRedirects`, `BrokenRedirects`, `DoubleRedirects`, `RandomRedirect` | `view.redirect` joined to `view.page` ([0051](../decisions/0051-page-redirects.md) §6) |
| `PagesWithProp` | `view.page_prop` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6) |

`check_adrs.py`'s check 5 (§1.1) is case-sensitive on canonical names.

The four redirect reports list, in turn: every redirect with its target (`view.redirect` joined to `view.page`); redirects whose `target_page_id` is null and that are not external (the same); redirects whose target is itself a redirect, with the final target (`view.redirect` self-joined); and one redirect at random, as `Special:Random` picks a page (`view.redirect`). All four read local graphs by default, as every report does (§2.2), and each is index-backed and live (§2.3's `index` row).

**Projection-backed:**

| Report | Entry key and sort |
|---|---|
| `WantedPages`, `WantedCategories`, `WantedTemplates`, `WantedFiles` | A missing target; links, members, transclusions or uses to it |
| `MostLinkedPages`, `MostLinkedCategories`, `MostTranscludedPages`, `MostLinkedFiles`, `MostCategories` | A page or file; the count |
| `DeadendPages`, `LonelyPages` | A page with no outgoing links; a page with no incoming links and no transclusions |
| `UncategorizedPages`, `UncategorizedCategories`, `UncategorizedTemplates`, `UncategorizedFiles` | A page in no category |
| `UnusedCategories`, `UnusedTemplates`, `UnusedFiles` | A category with no members; a template nothing transcludes; a file nothing uses |
| `ListDuplicatedFiles`, `MediaStatistics` | A hash with more than one current file; a MIME type and its count and bytes |
| `ItemsWithoutSitelinks` | A local item with no sitelink |
| `EntitiesWithoutLabel`, `EntitiesWithoutDescription` | A local entity and a language, for each language in `reports.term_languages` (default: the content language). Other languages are added to the setting, not computed on request. |
| `UnconnectedPages` | A main-namespace page with no item paired through 0038 §6's own-host sitelink |

`reports.term_languages` is a tenant `site` setting, by the rule for a dotted key named without a scope; it is not one of the keys of the instance `reports` kind in §2.3.

Reports about templates are listed on every tenant, since the Template namespace always holds pages (0042 §3, A19).

### 2.5 Who sees what

*Sources: [0047](../decisions/0047-special-pages.md) §4.5.*

**Report entries are stored in their public form** ([0014](../decisions/0014-caches-and-search.md) §1, principle 3). Nothing behind a `read` ACL becomes an entry: deleted pages and entities, hidden names, and pages in a namespace with a read ACL.

A report never lists what an anonymous reader could not see. Every viewer sees the same report, as on MediaWiki. On a tenant whose graphs are private, the report page itself is behind the graph ACL ([0016](../decisions/0016-permissions-and-access-control.md) §4).

### 2.6 Reports that are not special pages: template-mapping runs

*Sources: [0072](../decisions/0072-template-mappings.md) §5.*

Each template-mapping run ([13](13-mirrored-pages.md)) reports, per mapping: calls matched, statements written, values a parser could not read, unmapped strings, malformed calls and duplicate parameters, each with examples. They are on the job's page and in `GET /extraction/{source}`, not on a report page of the kind §2.1 describes.

## 3. Wikibase compatibility forms, identity and correction pages

*Sources: [0047](../decisions/0047-special-pages.md) §5, §6.*

### 3.1 Wikibase compatibility forms

*Sources: [0047](../decisions/0047-special-pages.md) §5.*

The forms are `NewItem`, `NewProperty`, `SetLabel`, `SetDescription`, `SetAliases`, `SetLabelDescriptionAliases`, `SetSiteLink`, `MergeItems` and `RedirectEntity`. Each is served at its Wikibase address, with Wikibase's subpage and query forms (`Special:SetLabel/Q42/en`, `Special:SetSiteLink/Q42/enwiki`, `?id=…&language=…`), because tools and gadgets link to them.

**They are server-rendered forms that need no JavaScript.** [0034](../decisions/0034-frontend-stack.md) §10 gives older browsers the server-rendered pages without components. That leaves those browsers without the statement UI's editing, and these forms are their editing path.

**They write through the same handlers as `wbsetlabel`, `wbsetsitelink`, `wbmergeitems` and the rest** (0034 §1.5). Permissions, edit filters, rate limits and base offsets are therefore the same as for the API. The forms show the current value and nothing else, so they are not a second renderer of entities (0034 §1.3).

- **On a foreign entity,** `SetLabel`, `SetDescription` and `SetAliases` write a local term correction ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; kind `term` in `view.correction`). `SetSiteLink` writes a local sitelink ([0026](../decisions/0026-sitelinks.md)). Both are what the statement UI writes for the same edit.
- **`NewItem` and `NewProperty`** are the targets of the **New** menu (0010 §2).
  - They take Wikibase's `label`, `description`, `aliases` and `lang`.
  - They also take a Triplespace parameter, `statement={property}:{value}`, repeatable, which prefills statements. [0029](../decisions/0029-resolver-namespaces.md) §3's "Create an item with this DOI" opens `NewItem` with the resolver's property and key filled in.
  - `NewProperty` needs `property-create` and a `datatype`, one of those the API registers (`siteinfo`'s `wikibase-propertytypes`). It offers exactly those, and `Special:ListDatatypes` lists the same ones in the same order (0047 A26).
- **`MergeItems`** (`fromid`, `toid`, `ignoreconflicts`):
  - **Two local entities:** Wikibase's merge. Content moves to the target, the source is emptied and becomes a redirect, and both get ordinary revisions ([0011](../decisions/0011-logs.md) Context).
  - **Two foreign entities in one namespace:** a local `redirect` and nothing more ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §2). Upstream's content is not moved.
  - **Across namespaces:** refused with `ts-cross-namespace-merge`, which links to `Special:LinkEntities` with both IDs filled in (§3.2).
  - Needs `item-merge`.
- **`RedirectEntity`** writes a `redirect` (0004 §9), including the cross-namespace property redirect amended there on 2026-09-27. Needs `item-redirect`.

### 3.2 Identity and correction pages

*Sources: [0047](../decisions/0047-special-pages.md) §6.*

Three pages give addresses to things 0002 and 0004 already describe, and a fourth lists proposals:

- **`Special:LinkEntities`** is the form for `same-as`, `different-from` and `equivalent-property` ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). It needs `ts-link`, and `ts-linkproperty` for `equivalent-property`, and counts in the `link` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5). It is prefilled from a refused merge and from a conflict row.
- **`Special:IdentityConflicts`** is 0004 §10's review list. It shows held links (`view.link` with `status = 'held'`) and the duplicate-key conflicts of [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9.
  - Each row offers 0004 §10's resolutions as actions: a local `same-as`, `different-from`, a merging `redirect`, or deprecating the source statement.
  - By §2.2's rule it lists conflicts that involve the tenant's own records or entities it has local assertions on. `graphs=all` lists every held link in the tenant's view. That is live, because the table already holds them.
- **`Special:Corrections`** is 0002 §7's maintenance list of local corrections.
  - It can be filtered by state (`active`, `redundant`, `dangling`; [0003](../decisions/0003-statement-ui.md) §4), kind (`rank`, `suppress`, `term`) and upstream graph.
  - It offers **Retire** for redundant corrections, singly or in bulk, and review for dangling ones.
  - 0002 §7 notes that this list "doubles as a list of fixes to report upstream": its export per upstream graph is **Propose**, which opens a proposal thread per subject for the selection ([0067](../decisions/0067-proposals.md) §4; [14](14-discussions.md)).
- **`Special:Proposals`** lists the tenant's proposals by state, destination wiki and proposer ([0067](../decisions/0067-proposals.md) §7).

All four are in the `identity` group of `Special:SpecialPages`.

## 4. Special:Nuke, Special:Export and Special:Import

*Sources: [0047](../decisions/0047-special-pages.md) §7, §8.*

### 4.1 Special:Nuke

*Sources: [0047](../decisions/0047-special-pages.md) §7.*

**MediaWiki's Nuke** lists pages that one user created, taken from recent changes, and deletes the selected pages one deletion at a time. It takes `target`, `pattern`, `namespace` and `limit`, and needs the `nuke` right, held by `sysop` ([09](09-security-and-moderation.md) §8.6). Triplespace serves `Special:Nuke` at that address with that right and those parameters. It extends Nuke in three ways that the log makes cheap.

**What it covers.** Everything the target did on the tenant, by kind:

| The target's contribution | What Nuke does |
|---|---|
| A page, thread or local entity it created | Deletes it: a `read` ACL naming the deletion group ([0023](../decisions/0023-moderation.md) §4). With `deletetalk`, the talk page too. |
| A file it uploaded first | Deletes the file page (0039 §9) |
| A new version of someone else's file | Reverts the file to the previous version ([0039](../decisions/0039-files-and-media.md) §2, `upload/revert`) |
| Change sets on others' entities, or on page statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1) | Appends the inverse change sets. Later edits by others are kept, as a job revert keeps them ([0010](../decisions/0010-site-ui.md) §9). |
| Text edits to others' pages | Rolls back, restoring the latest revision by another actor, where the target's edits are the latest. Otherwise the page is listed and left alone, as rollback does. |
| Posts in others' threads | Hides the post's `text` part with a `record` ACL (0023 §5), since only a whole thread is deleted ([0019](../decisions/0019-discussions.md) §1) and a post's text is its record's `text` part (0019 §4) |
| Edit summaries, optionally | Hides the `comment` part of each of the target's records. Needs `deleterevision`. |

Foreign entities are never deleted (0023 §1). The target's local assertions about them are reverted, and upstream's content is untouched.

**Who it targets.**

- An account, by default together with its subsidiaries ([0024](../decisions/0024-subsidiary-accounts.md) §1), so that a spammer's bots go with it. Temporary accounts are accounts.
- Alternatively a **change tag**. An `oauth:{slug}` tag ([0025](../decisions/0025-oauth-server.md) §4) cleans up after a tool that misbehaved.
- IP addresses are private state ([0016](../decisions/0016-permissions-and-access-control.md) §3) and are not a target.

**When.** The window defaults to the recent-changes window, `rc.max_age` (0010 §7). MediaWiki's Nuke can see no further back than that, because it reads `recentchanges`. Here the log is complete, so the window can be extended to the target's whole history: for an account, `view.activity` by actor over the window; for a change-tag target, `view.activity` through the GIN index on its `tags` column ([03](03-storage-caches-and-search.md) §4.6), so a tag's whole history is one index scan. `pattern` (a title pattern) and `namespace` narrow it further.

**How it runs.**

1. **Preview.** The page lists every candidate, grouped by the kinds above, with counts and a checkbox each, all checked as in MediaWiki. Candidates that will be left alone are marked with the reason, such as a page edited since.
2. **Confirm.** Above `nuke.confirm_threshold` items (`site` setting, default 500), the form asks for the target's name to be typed, as the snapshot safety threshold does ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4).
3. **One job.** Submitting starts a job with mode `nuke` ([0011](../decisions/0011-logs.md) §6.3; [16](16-logs-feeds-and-notifications.md)).
   - Its actor is a subsidiary the administrator operates, as every job's actor is ([07](07-actors-and-accounts.md) §4.6). Its parameters are the target, the filters, the window, the selection and the reason.
   - Every record it writes carries the job ID in its attestation: ACLs in the tenant `log`, inverse change sets, page records and record ACLs.
   - Each write goes through the write path as the administrator's own would.

**What others see.**

- Recent changes shows **one row** for the job (0010 §7). A patroller marks it once (0023 §6).
- The deletion log still gets one `delete/delete` event per deleted page, each tagged `job:{id}`. A tool reading `list=logevents` sees what it would see after MediaWiki's Nuke.

**Undo.** **Revert this job…** on the job's page (0010 §9) does three things:

- retires the ACLs the job wrote;
- appends the inverse of its inverse change sets;
- restores the revisions it rolled back where nobody has edited since, and lists the rest.

It needs `ts-revertjob` and, because it retires deletions, `undelete`.

**Blocking is separate.** A block is an actor record with its own form ([0016](../decisions/0016-permissions-and-access-control.md) §3). The Nuke page links to `Special:Block` for the target and offers **Block after nuking**, which opens that form prefilled.

The REST routes `GET /nuke/preview` and `POST /nuke`, and the revert `POST /jobs/{id}/revert`, are in [18](18-api.md).

### 4.2 Special:Export

*Sources: [0047](../decisions/0047-special-pages.md) §8.*

**`Special:Export` keeps MediaWiki's form and parameters:**

- a list of titles;
- `addcat` with a category, and, where the `site` setting `export.from_namespaces` is on (MediaWiki's `$wgExportFromNamespaces`), `addns` with a namespace;
- `curonly` or full history;
- `templates`;
- `wpDownload`.

`action=query&export` and `exportnowrap` give the same output.

**The default format is MediaWiki XML**, export schema version 0.11, which carries content models and slots.

- **Pages.** Each page record is a `<revision>`, with its content model and format ([0041](../decisions/0041-content-models.md)).
  - A part hidden from the viewer is written as MediaWiki writes revision-deleted content (`<text deleted="deleted"/>`, and likewise `comment` and `contributor`). An erased part is written the same way.
  - Full history is governed by `export.history` (default on) and `export.max_history` (revisions per page, default 1,000), MediaWiki's `$wgExportAllowHistory` and `$wgExportMaxHistory`.
- **Templates and modules.** With `templates`, the pages that `view.transclusion` lists for the selection ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10) are added.
- **Local entities** are exported as Wikibase exports entity pages. The model is `wikibase-item` or `wikibase-property`. Each revision is one local change set, and its text is the canonical JSON of the entity's **local-graph** state at that revision ([wikibase-compat.md](../api/wikibase-compat.md)), read by the export job as one row of `view.entity_revision` per revision ([18](18-api.md) §1.3) and folded into the XML there, never on the request path. The revisions exported per entity are capped by a separate limit, lower than `export.max_history` for pages, since each entity revision is a whole state rather than a text. A MediaWiki Wikibase with free IDs can import them, which is the reverse of adoption ([0035](../decisions/0035-adopting-a-wikibase.md); [05](05-providers-and-ingest.md)).
- **Foreign entities are not exported.** Their content is upstream's. `Special:EntityData` serves their resolved view.
- **Files.** With `files`, file versions are added as `<upload>` elements with their contents, which 0039 §14's import accepts ([12](12-files-and-media.md)). The total is capped by `export.max_bytes`. Over the cap, URLs are written in place of contents.
- **Not in the XML:**
  - page statements ([0038](../decisions/0038-page-metadata-and-categories.md) §1), which no Wikibase slot carries;
  - threads and composite talk pages. They are left out by default, and with `threads` they are written with their own content models, which a MediaWiki importer refuses.

The `export.*` settings are tenant `site` settings, by the rule for a dotted key named without a scope ([23](23-configuration-and-registry.md)).

**A second format, `records`, is verifiable.**

- It contains the selected pages' and local entities' records from `pages` and `local`, each with an inclusion proof against the latest checkpoint. It also carries that checkpoint and the key-chain records needed to check it ([0006](../decisions/0006-log-integrity-and-erasure.md) §9; [01](01-log-and-records.md)).
- A part withheld from the viewer appears as its commitment only, as an erased part does, so every proof still verifies.
- `log`-partition records are left out, because the `log` partition is internal (0023 §7).
- `triplespace-cli verify --records` checks such an export. A third party can then confirm that an export is exactly what the wiki holds, which no MediaWiki export can show.

**Size.**

- An export up to `export.sync_revisions` (default 5,000 revisions) streams.
- A larger one becomes an `ops` export job, as [0027](../decisions/0027-preferences-and-portability.md) §3's data bundle does. The page then links to it when it is ready.
- Exports count in a rate class of their own, `export` ([0024](../decisions/0024-subsidiary-accounts.md) §5; [07](07-actors-and-accounts.md) §6.2), default 30 / 300 per hour for `user` / `bot`.
- Export needs only `read`, and every response is redacted per viewer ([0012](../decisions/0012-api-requirements.md) §8).

`POST /export` and `GET /export/{id}` are in [18](18-api.md).

### 4.3 Special:Import

*Sources: [0047](../decisions/0047-special-pages.md) §8.*

**`Special:Import` is the form for [0008](../decisions/0008-namespaces-and-document-pages.md) §9's import job** ([10](10-pages-and-content-models.md)).

- **Upload an XML file** (`importupload`), or **import from another wiki** named by site alias ([0026](../decisions/0026-sitelinks.md) §2) in place of MediaWiki's interwiki sources (`import`).
- It runs the template census first, shows its report, and on confirmation starts the job and goes to `Special:Jobs/{id}`.
- It accepts MediaWiki XML only. Data moves between instances by a tenant move ([0018](../decisions/0018-tenants.md) §10) or verified sync ([0022](../decisions/0022-federation.md)), not by importing `records`.

## 5. Special:Version

*Sources: [0077](../decisions/0077-special-version.md) §1, §2, §3, §4, §5, §7, §8, §9, §11; [0047](../decisions/0047-special-pages.md) §9.*

### 5.1 One page, two forms

*Sources: [0077](../decisions/0077-special-version.md) §1; [0047](../decisions/0047-special-pages.md) §9.*

`Special:Version` shows credits and licence notice, installed software and services, features, entry points, crates and third-party components, wikitext, the extensions that inspired features, and the AI agents named in the ADRs. **It has these sections, in this order:**

1. The credit line and licence notice (§5.2, §5.3).
2. Installed software: Triplespace's build, then the services and embedded components (§5.4).
3. Features: the tenant's on its form, the instance's on the farm form (§5.10).
4. Entry points (§5.5).
5. Triplespace crates, then third-party components (§5.6).
6. Wikitext: extension tags, parser functions, variables and switches (§5.7).
7. Inspired by: MediaWiki and its extensions (§5.8).
8. AI agents (§5.9).
9. The instance: its name, and its owner of record where `instance.owner_of_record` is set (0016 A11).

Subpages: `Special:Version/Credits` (§5.2), `Special:Version/License` and `Special:Version/License/{component}` (§5.3).

**The page names no ADRs.** The `adr` fields in `version.toml` are for maintainers and are not shown.

**The tenant form and the farm form** differ only in §5.10's features and in §5.5's entry points, which are the tenant's at a tenant base and the instance's at the farm base. Where the two bases share a host, both forms show behind §1.3's `scope` filter.

**What is listed and what is derived.** `docs/registry/version.toml` ([23](23-configuration-and-registry.md) §4.6) holds what nothing else records: the developer, contributors, funders, AI agents, services, feature switches and extension pages. Everything else is derived: the build and its components from the component manifest ([22](22-crates-and-stack.md)), lineage from the registries' `origin` fields, wikitext from `wikitext-functions.toml`, and features from configuration.

**The page is rendered by `triplespace-ui` from `GET /version`** ([18](18-api.md)), since the site's crates reach the instance only through its API ([0005](../decisions/0005-crate-organization.md) §3 rule 10). Where `triplespace-web` serves the site, it adds its own build to §5.4, so a web tier and an API server built from different commits are both shown.

### 5.2 The credit line, contributors and funders

*Sources: [0077](../decisions/0077-special-version.md) §2.*

**The credit line** is one sentence:

> This wiki is powered by [Triplespace](https://github.com/scatterbase/triplespace), developed by Scatter LLC in Portland, Oregon, with contributions from Alice, Bob and others.

- **The product, developer and place are message parameters,** from `[developer]` in `version.toml`, so translators never translate them. The messages are `ts-version-poweredby` (no contributors) and `ts-version-poweredby-contributors`.
- **Contributors are `[[contributor]]` entries in `version.toml`,** in order of first merged contribution. Each has a name as its holder wishes to be credited, the date of the first merged contribution, and the date the contributor licence agreement was signed; nothing is merged without one (0005 §6). Git history is not read.
- **The line names the first ten.** With more, it ends "and [[Special:Version/Credits|others]]", as MediaWiki's does. The list is formatted by the reader's language's list rules.
- **`Special:Version/Credits` lists every contributor**, then every funder, then the AI agents of §5.9.
- **Scatter LLC is the developer, not a contributor,** and AI agents are never contributors: they sign no agreement and are credited separately (§5.9).

**Funders are `[[funder]]` entries**: a name, what the support was for, and the dates it began and, if it has, ended. When there is at least one, a second sentence follows the credit line: "Development has been supported by {funders}." (`ts-version-funders`). A funder whose support has ended stays listed. There are none today.

### 5.3 The licence notice and licence texts

*Sources: [0077](../decisions/0077-special-version.md) §3.*

**The notice follows the credit line.** It is the FSF's recommended notice for GPLv3-or-later, with "Triplespace" for "this program", in the message `ts-version-license-info`:

> Triplespace is free software; you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation; either version 3 of the License, or (at your option) any later version.
>
> Triplespace is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for more details.
>
> You should have received [[Special:Version/License|a copy of the GNU General Public License]] along with this program; if not, see https://www.gnu.org/licenses/.

MediaWiki's `version-license-info` is not reused, because it names version 2.

Two more sentences follow: the `scatter-*` crates are also available from Scatter LLC under a commercial licence (0005 §6), and the documentation and registry data the binary embeds are dedicated to the public domain under CC0 1.0 (`docs/LICENSE`).

**The licence text is in the binary.** The repository root's `LICENSE` is embedded at build time (`include_str!`), so every build carries its own copy, whatever was or was not shipped beside it.

- **`Special:Version/License` serves it verbatim, in English only,** in a `<pre>` block with no interface translation. The FSF treats only the English text as the licence; the notice above is translated like any message.
- **`Special:Version/License/{component}`** serves a third-party component's licence texts and copyright notices from the manifest ([22](22-crates-and-stack.md)): the texts a crate or package ships, not a generic text for its SPDX identifier. Each row of §5.6 links to its component's page.

**The source.** §5.4's build row links to the corresponding source:

- For a build from a clean tree, `{developer.source}/tree/{commit}`.
- `instance.source_url` ([23](23-configuration-and-registry.md) §3.3) replaces it, for an operator who publishes a modified build elsewhere.
- A build from a modified tree (the manifest's `modified` flag) without `instance.source_url` shows "modified build; source location not configured" to everyone, and `triplespace instance check` warns. The JavaScript and wasm sent to browsers are conveyed to every reader, so their source must be offered whatever the server's licence would otherwise require of a hosted service ([0033](../decisions/0033-backend-stack.md) §16).
- The built JavaScript, CSS and wasm files carry a header comment with `@license GPL-3.0-or-later` and the path `Special:Version`.

### 5.4 Installed software and services

*Sources: [0077](../decisions/0077-special-version.md) §4.*

**The build row** shows Triplespace's version, the commit, whether the build is modified, the commit's date (not the build time, so builds stay reproducible), the rustc version, the target and the cargo features compiled in, from the manifest. Where `triplespace-web` serves the site, a second row shows the web tier's build (§5.1).

**Then each service the instance is configured to use, and each embedded component in use,** from `version.toml`'s `[[service]]` list. A service that is not configured, or an embedded component not in use, is not shown. Each row has:

| Column | Shows |
|---|---|
| Name and role | From the registry |
| State | `connected` or `unreachable`. An embedded component has no state |
| Version | Read by the registry's `version` probe for a service (`SHOW server_version` for PostgreSQL; Valkey's `INFO server`; OpenSearch's `GET /`), from the manifest for an embedded component (Oxigraph), or a fixed string (Lua 5.1). QLever and Parsoid are shown without a version for now |

The services are PostgreSQL, Valkey, OpenSearch, QLever, Parsoid and object storage, and the embedded components are Oxigraph and Lua; adding a service to the stack adds a registry entry.

**Probes run at startup and with each health check** and are cached in the process. Rendering the page never contacts a service, so it cannot be used to make the instance probe its services on demand.

**Disclosure** is the instance setting `version.services` ([23](23-configuration-and-registry.md) §3.3):

- **`full`** (default), as MediaWiki shows its database server's version to everyone;
- **`names`**: names, roles and states, without versions;
- **`off`**: the build row only.

Holders of `ts-config` at the farm base always see `full`. Whatever the setting, the page never shows a host, port, URL, bucket, index name, database name or credential.

### 5.5 Entry points

*Sources: [0077](../decisions/0077-special-version.md) §5.*

**The URLs a client needs, as MediaWiki's "Entry point URLs" lists them:** the article path, script path, `api.php` and `rest.php`, then Triplespace's: the `triplespace/v0` REST base, `/entity/` and `Special:EntityData`, the update stream ([0032](../decisions/0032-sparql-update-stream.md)), the SPARQL endpoint while `query.enabled` is on ([0059](../decisions/0059-query-service.md) §6), the MCP endpoint while `mcp.enabled` is on ([0075](../decisions/0075-mcp-server.md) §1), and the OAuth endpoints ([0025](../decisions/0025-oauth-server.md)). Each is shown only where it is served, and at the base that serves it; a private tenant's SPARQL endpoint is served to its own principals and shown to them, and its stream and dumps are not served at all ([18](18-api.md) §6).

### 5.6 Crates and third-party components

*Sources: [0077](../decisions/0077-special-version.md) §1, §6; [0097](../decisions/0097-versioning-and-skins.md) §7, §8.*

The fifth section lists Triplespace's crates, then the third-party components: crates, frontend packages and vendored code, each with its version, licence and upstream, and a link to its licence page (§5.3). What the section lists, and the component manifest it is read from, are in [22](22-crates-and-stack.md).

**Versions** ([0097](../decisions/0097-versioning-and-skins.md) §7, §8). Beside the product version, the page gives the version of each contract the instance serves: the record format and payload majors, `api_version`, the template contract, the Codex token release, and each message bundle; then each installed skin with its version, licence and requirements, and any skin not loaded, with the reason.

### 5.7 Wikitext

*Sources: [0077](../decisions/0077-special-version.md) §7.*

**Extension tags, parser functions, variables and behaviour switches,** from `wikitext-functions.toml` ([11](11-rendering-templates-and-modules.md) §2), as MediaWiki's "Parser extension tags" and "Parser function hooks" list them. Each shows its origin and status (`implemented`, `chip` or `ignored`) and, on the tenant form, whether it is active there, given the settings its `requires` and `enabled_by` name ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2).

### 5.8 Inspired by

*Sources: [0077](../decisions/0077-special-version.md) §8.*

**Triplespace runs no MediaWiki extension.** This section lists the extensions whose features, names or APIs it implements, so that their users, tools and pages work here. It is headed "Inspired by", introduced by `ts-version-inspiredby-intro`, and never says "installed".

**It is derived from every registry that records an `origin`**: `special-pages.toml`, `wikitext-functions.toml` and `content-models.toml`. MediaWiki core comes first, then the extensions in alphabetical order. Each row links to the extension's page on mediawiki.org (`[[extension]]` in `version.toml`) and lists what came from it, by kind:

- **Special pages** that are `served`;
- **Content models** that are `implemented`;
- **Wikitext** entries, `implemented` ones first, then those that are only recognized (`chip` or `ignored`), labelled as such.

Entries that are `deferred`, `declined` or `reserved` are not shown. An extension with no shown entry is not listed.

**Extensions that leave no `origin`** in any registry, but from which Triplespace took something, carry `inspired_by` in `version.toml`: what was taken, which the page shows, and the ADR, which it does not. The seed is CirrusSearch and WikibaseCirrusSearch (search fields and keywords, [0014](../decisions/0014-caches-and-search.md)), CodeMirror (the source editor, 0034 §4) and DiscussionTools (stable names for sections and comments, [0069](../decisions/0069-synchronized-talk-pages.md)). An ADR that takes something from an extension adds an `origin` to the registry it fills or, failing that, an `inspired_by` entry.

**`origin` values** are `mediawiki`, `triplespace`, `generic` (content models only) or an extension's name as `version.toml` lists it. The three registries agree: `wikibase` became `Wikibase` in `content-models.toml`, and `timeline` and `wikihiero` became `EasyTimeline` and `WikiHiero` in `wikitext-functions.toml`. The checker that keeps the registries and `version.toml` in step is in [22](22-crates-and-stack.md).

### 5.9 AI agents

*Sources: [0077](../decisions/0077-special-version.md) §9.*

**The AI agents named in the ADRs' Author lines,** from `[[agent]]` in `version.toml`: Claude Opus, Claude Fable and Claude Sonnet, made by Anthropic. Each shows the model versions recorded in commit trailers, where there are any (Claude Opus 5.5 and Claude Fable 5.1; none for Claude Sonnet). The section is introduced by `ts-version-agents-intro`: "Triplespace's design records and code were written with these AI agents."

- **The list is the ADRs' Author lines,** not the commit history, and the checker keeps them in step ([22](22-crates-and-stack.md)).
- **0033 and 0034 name "Claude" without the model.** They were written by Claude Opus or Claude Fable; `[agent_attribution]` records them as unattributed rather than guessing.

### 5.10 Features

*Sources: [0077](../decisions/0077-special-version.md) §11.*

**The tenant form shows the tenant's feature switches:** the value of each `[[feature]]` setting of scope `tenant` in `version.toml` (`content.licence`, `wikitext.lua`, `wikitext.renderer`, `pages.repos`, `search.inherited`, `query.enabled`, `mcp.enabled` and `proposals.push`). It also shows the providers the tenant has opted into, linked to `Special:Providers`, its tenancy preset ([0028](../decisions/0028-tenancy-policy.md) §1), and whether it is a private tenant ([0056](../decisions/0056-security-model.md) §3).

**The farm form shows the instance's:** the settings of scope `instance` (`search.farm_wide` and `files.separation`), the tenancy policy's preset and the number of tenants.

An ADR that adds a setting turning a feature on or off adds it to `[[feature]]`.

## 6. Other pages

*Sources: [0047](../decisions/0047-special-pages.md) §9; [0061](../decisions/0061-sprints-and-tasks.md) §9; [0062](../decisions/0062-workspaces.md) §7; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0066](../decisions/0066-lexemes.md) §7.*

### 6.1 Pages served with MediaWiki's or Wikibase's meaning

*Sources: [0047](../decisions/0047-special-pages.md) §9; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §9.*

These are served with MediaWiki's or Wikibase's meaning and parameters. The notes say what they read or write.

| Page | Notes |
|---|---|
| `SpecialPages` | The index the header links to (0010 §2), grouped by `group`. Restricted pages are listed only to holders of the right. At the farm base it also lists the `farm` pages. |
| `Search` | The full results page of 0010 §3: `search`, `fulltext`, `ns{n}`, `profile`, `offset`, `limit`. `go` follows 0010 §3's **Go to** rule. |
| `MovePage` | The overflow menu's **Move** (0010 §2); a `move` record ([0008](../decisions/0008-namespaces-and-document-pages.md) §4) |
| `GoToComment` | DiscussionTools' page: finds an imported post by its DiscussionTools name or ID and redirects to it on its thread through `Special:PermanentLink/{revid}`, listing the posts where several share a name ([0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §9; [13](13-mirrored-pages.md) §5.17). Origin `DiscussionTools`; group `redirects` |
| `RevisionDelete` | The history dialog of 0023 §9 as a page: `type`, `target`, `ids` |
| `Unblock`, `RemoveGlobalBlock` | Retire a block, or a global block at the farm base ([0028](../decisions/0028-tenancy-policy.md)) |
| `EditWatchlist` | The watch set, with `/raw` and `/clear` ([0020](../decisions/0020-change-feeds.md) §3) |
| `ChangeContentModel` | [0041](../decisions/0041-content-models.md)'s `action=changecontentmodel` |
| `Fork` | Forks a title whose primary is a page repository's page, without an edit: shows the stack, the licence and the fork options, and appends the fork's `create` ([0054](../decisions/0054-forking-a-mirrored-page.md) §2; [13](13-mirrored-pages.md)). Origin `triplespace`; restricted to `createpage` |
| `Query` | A SPARQL editor and result table over `/sparql`, with the tenant's prefix set, labels resolved, a shareable `?query=` URL and a "Try with scope" button ([0059](../decisions/0059-query-service.md) §6; [02](02-graphs-rdf-and-query.md)). Origin `triplespace`; listed while `query.enabled` is on; group `wikibaserepo`. On a private tenant it is served to the principals that satisfy the tenant's ACL, from the tenant's own store, since a private tenant keeps its query service and lacks only the anonymous endpoint, the update stream and the dumps ([18](18-api.md) §6) |
| `CreateSprint` | The form that writes a `triplespace-sprint` subpage under a project page, with its scope, rules, window and an optional board (§6.2). Origin `triplespace`; restricted to `createpage`; group `pagetools` |
| `CreateWorkspace` | The wizard that gives a project page a scope and a kit's pages: name, scope through the builder, kit with a preview, then the creates in dependency order as one job (§6.3). Origin `triplespace`; restricted to `createpage`; group `pagetools` |
| `NewEntitySchema`, `EntitySchemaText`, `SetEntitySchemaLabelDescriptionAliases` | EntitySchema's pages, with its parameters: create a schema, serve its ShExC as `text/shex`, set its terms (§6.4). Origin `EntitySchema` |
| `CheckEntitySchema` | A schema and a subject; shows the result-shape map (§6.4). Origin `triplespace`; rate class `query` |
| `NewLexeme`, `MergeLexemes` | WikibaseLexeme's pages: create a lexeme from lemma, language and category; merge two lexemes as `same-as` plus conversion (§6.5). Origin `WikibaseLexeme` |
| `MergeUpstream` | The guided three-way merge of a fork with its repository's current text ([0068](../decisions/0068-merging-with-upstream.md) §6; [13](13-mirrored-pages.md)). Origin `triplespace`; restricted to `edit` on the page; group `pagetools` |
| `ExpandTemplates` | [0042](../decisions/0042-template-expansion-and-parsoid.md)'s expander, in the `parse` rate class |
| `ComparePages` | Any two pages or revisions, with 0010 §6's diff |
| `ConfirmAccounts` |  |  | `ConfirmAccount` | served | `tenant` | `users` | restricted `confirmaccount` |  | 0087 §6 | — |
| `Redirect` | `user/{id}`, `revision/{id}`, `page/{id}`, `file/{name}` and `logid/{id}`, resolved locally. `file` was already served by 0039 §7. |
| `RenameUser` | Renaming another account ([0007](../decisions/0007-actor-identity.md) §4; `renameuser`) |
| `UserLogout`, `PasswordReset` | Session end; a reset for the built-in `password` issuer (0007 §3), sent by the email channel of 0021 |
| `UploadStash` | One's stashed uploads (0039) |
| `ItemDisambiguation` | Wikibase's label lookup (`/{language}/{label}`). 0029 §3's resolver disambiguation uses its layout. |
| `Statistics` | `view.site_stats` ([03](03-storage-caches-and-search.md) §4.10), which also serves `siprop=statistics`, the `NUMBEROF*` variables ([0042](../decisions/0042-template-expansion-and-parsoid.md) §5) and `mw.site.stats` ([0043](../decisions/0043-lua-modules.md)) |
| `Version` | §5 ([0077](../decisions/0077-special-version.md)) |
| `ApiSandbox`, `ApiHelp`, `AllMessages`, `NamespaceInfo`, `PasswordPolicies`, `ListDatatypes`, `AvailableBadges`, `MyLanguageFallbackChain`, `Blankpage` | From the registry, configuration and i18n |
| `MyPage`, `MyTalk`, `MyContributions`, `MyLanguage`, `MyLog`, `MyUploads`, `AllMyUploads`, `EditPage`, `PageHistory`, `PageInfo`, `Purge`, `DeletePage`, `ProtectPage`, `NewSection`, `TalkPage`, `EditTags` | MediaWiki's redirecting pages, to the matching page or action. `NewSection` opens the talk page's new-thread form ([0019](../decisions/0019-discussions.md); [14](14-discussions.md)). |

### 6.2 `Special:CreateSprint`

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §9; [0062](../decisions/0062-workspaces.md) §7.*

A form, restricted to `createpage`: the parent project page (any namespace-4 page, defaulting to the page the form was opened from), the sprint's name, the scope (an existing scope by title, or a new inline `statement`/`category` kind), the rules from a picker with their parameters, the window, and an optional board to create alongside. It writes the sprint page with the model set, and the board if asked, and opens the sprint page. Listed under `pagetools`, origin `triplespace`. It is the essay's "form-based interface" for a sprint; the workspace form that also creates a scope, tables and a project page is `Special:CreateWorkspace` (§6.3). `Special:CreateSprint` stays the form for one more sprint under an existing workspace, and defaults its scope to the workspace's `subjects`. Sprints and workspaces themselves are in [15](15-structured-pages.md).

### 6.3 `Special:CreateWorkspace`

*Sources: [0062](../decisions/0062-workspaces.md) §7.*

A wizard, restricted to `createpage`:

1. **Name**, and the project page to create or extend (an existing project page may be given a workspace by this form, which then only adds the declaration and blocks).
2. **Scope**, through the scope builder of [0062](../decisions/0062-workspaces.md) §5, saved as `Scope:{name}` or kept inline.
3. **Kit**, from the kits the tenant can see, with a preview of every page it will create and the blocks the project page will carry; pages that already exist are shown as "kept" and not overwritten.
4. **Create.** Each page is an ordinary `create` by the user, in the `create` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5), written in dependency order (scope, then tables and board, then sprint, then the project page) by one job so that a refused page stops the rest and reports why. The wizard then opens the project page.

### 6.4 EntitySchema pages

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §7 (shared with [18](18-api.md) and [03](03-storage-caches-and-search.md)).*

The EntitySchema pages moved from `reserved` to `served`: `NewEntitySchema` (labels, description, aliases and text; appends the first record), `EntitySchemaText/{id}` (the ShExC as `text/shex`, which external validators fetch), `SetEntitySchemaLabelDescriptionAliases`. The schema page's own **Edit schema** form appends a new record with the changed text. A further page, **`CheckEntitySchema`** (origin `triplespace`), takes a schema and a subject and shows the result-shape map. The schema API and search are in [18](18-api.md) and [03](03-storage-caches-and-search.md); schemas themselves are in [15](15-structured-pages.md).

### 6.5 Lexeme pages

*Sources: [0066](../decisions/0066-lexemes.md) §7 (shared with [19](19-site-ui.md)).*

The WikibaseLexeme pages moved from `reserved` to `served`: `NewLexeme` (lemma, language, category; appends the first record) and `MergeLexemes` ([0066](../decisions/0066-lexemes.md) §4; [04](04-entities-and-identifiers.md) §5.1). The entity suggester, `Special:Search`, `Special:EntityPage` and `Special:EntityData` take lexeme and part IDs. The lexeme page's layout is in [19](19-site-ui.md).

## 7. Deferred, declined and reserved

*Sources: [0047](../decisions/0047-special-pages.md) §10; [0051](../decisions/0051-page-redirects.md) §6; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0066](../decisions/0066-lexemes.md) §7.*

| Page | Status | Until, or why |
|---|---|---|
| `LinkSearch` | deferred | An external-links table. [0042](../decisions/0042-template-expansion-and-parsoid.md) §9 extracts external links, but no table holds them. |
| `Mute` | deferred | Undecided |
| `BookSources` | deferred | A bound ISBN resolver (`resolvers.toml` has a draft). It would then be an alias of that resolver. |
| `AutoblockList` | deferred | Autoblocks, which [0016](../decisions/0016-permissions-and-access-control.md) does not specify |
| `PageLanguage` | deferred | Per-page content language, which nothing specifies |
| `EditRecovery` | deferred | The source editor's own recovery ([0034](../decisions/0034-frontend-stack.md) §7) |
| `PageData` | deferred | Whether to alias it to the document node `{base}/page/{id}` (0008 §10) |
| `EmailUser` | declined | No user-to-user email |
| `Unwatchedpages` | declined | The watch set is private state ([0020](../decisions/0020-change-feeds.md) §3). An aggregate over it would make the report projection a reader of `private`. |
| `WithoutInterwiki`, `MostInterwikis`, `GoToInterwiki` | declined | Interlanguage links are sitelinks ([0026](../decisions/0026-sitelinks.md)). Interwiki prefixes come from site aliases, and their links render as their targets. |
| `MergeHistory` | declined | It would move revisions between pages, which the log cannot do |
| `Lockdb`, `Unlockdb` | declined | Read-only is a tenant freeze ([0018](../decisions/0018-tenants.md) §10) or an instance act ([0040](../decisions/0040-instance-prerogatives.md)) |
| `RunJobs`, `JavaScriptTest`, `AuthenticationPopupSuccess`, `Contribute` | declined | Internal to MediaWiki, a development tool, MediaWiki's popup login flow, and a skin feature |
| `DispatchStats` | declined | Wikibase's client-change dispatching has no counterpart. `Special:Providers` reports sync lag. |

Four pages that were deferred are served: `ListRedirects`, `BrokenRedirects`, `DoubleRedirects` and `RandomRedirect` (§2.4), and `PagesWithProp` (§2.4). Five names that were `reserved` while Lexeme and EntitySchema were are served since [0064](../decisions/0064-entityschema-and-validation.md) §7 and [0066](../decisions/0066-lexemes.md) §7: `NewLexeme`, `MergeLexemes`, `NewEntitySchema`, `EntitySchemaText`, `SetEntitySchemaLabelDescriptionAliases` (§6.4, §6.5). No name is reserved today (§1.1).

## 8. The catalogue

*Sources: [0047](../decisions/0047-special-pages.md) §1, §4.4, §9, §10; [0051](../decisions/0051-page-redirects.md) §6; [0061](../decisions/0061-sprints-and-tasks.md) §9; [0062](../decisions/0062-workspaces.md) §7; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0066](../decisions/0066-lexemes.md) §7; [0077](../decisions/0077-special-version.md) §1; [0085](../decisions/0085-settings-and-preferences.md) §4, §5; [0087](../decisions/0087-account-registration-and-challenges.md) §6; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §9.*

Every entry of `docs/registry/special-pages.toml` (194 pages: 174 served, 7 deferred, 13 declined, none reserved), in alphabetical order. The columns are the registry's fields (§1.1); **Section** is where this chapter describes the page. The ADR column is the registry's `adr` field as written.

Every `Special:` name that [0047](../decisions/0047-special-pages.md) §1–§10, [0051](../decisions/0051-page-redirects.md) §6, [0061](../decisions/0061-sprints-and-tasks.md) §9, [0062](../decisions/0062-workspaces.md) §7, [0064](../decisions/0064-entityschema-and-validation.md) §7, [0066](../decisions/0066-lexemes.md) §7 and [0077](../decisions/0077-special-version.md) §1 use is in the registry as a name, alias or section alias; none is missing. Twenty-six served pages in the registry are specified by other ADRs and are not described in those sections or in this chapter; the catalogue marks them "—" and names their ADR: `Block`, `BlockList`, `DeletedContributions`, `ListGroupRights`, `Undelete` and `UserRights` ([09](09-security-and-moderation.md)); `UserLogin`, `CreateAccount`, `ListGrants`, `OAuth`, `PendingSubsidiaries` and `TransferAccount` ([07](07-actors-and-accounts.md)); `RecentChanges`, `RecentChangesLinked`, `Watchlist` and `Tags` ([16](16-logs-feeds-and-notifications.md), [09](09-security-and-moderation.md)); `FilePath` and `Upload` ([12](12-files-and-media.md)); `WhatLinksHere`, `Diff` and `PermanentLink` ([19](19-site-ui.md)); `EntityData`, `EntityPage`, `ItemByTitle` and `GoToLinkedPage` ([10](10-pages-and-content-models.md), [06](06-statements-and-properties.md)).

| Name | MediaWiki name | Aliases | Origin | Status | Scope | Group | Restricted, requires, listed | Report | ADR | Section |
|---|---|---|---|---|---|---|---|---|---|---|
| `Account` |  | section aliases: `ResetTokens`→`#watchlist-token`, `ChangeCredentials`→`#sign-in-methods`, `RemoveCredentials`→`#sign-in-methods`, `ChangePassword`→`#sign-in-methods`, `LinkAccounts`→`#sign-in-methods`, `UnlinkAccounts`→`#sign-in-methods`, `ChangeEmail`→`#contacts`, `Confirmemail`→`#contacts`, `Invalidateemail`→`#contacts`, `BotPasswords`→`#subsidiaries`, `OAuthManageMyGrants`→`#connected-applications` | `triplespace` | served | `tenant` | `users` |  |  | 0010 §11, 0085 §5 | §1.2 |
| `ActiveUsers` | `Activeusers` |  | `mediawiki` | served | `tenant` | `users` |  | `index`: view.actor (last_active); graphs local | 0047 §4.4 | §2.4 |
| `AllMessages` | `Allmessages` |  | `mediawiki` | served | `tenant` | `wiki` |  |  | 0047 §9 | §6.1 |
| `AllMyUploads` |  | `AllMyFiles` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `AllPages` | `Allpages` |  | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.page (ns, title); graphs local | 0047 §4.4 | §2.4 |
| `AncientPages` | `Ancientpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.page (ns, latest_at); graphs local | 0047 §4.4 | §2.4 |
| `ApiHelp` |  |  | `mediawiki` | served | `tenant` | `wiki` |  |  | 0047 §9 | §6.1 |
| `ApiSandbox` |  |  | `mediawiki` | served | `tenant` | `wiki` |  |  | 0047 §9 | §6.1 |
| `AuthenticationPopupSuccess` |  |  | `mediawiki` | declined: internal to MediaWiki's popup login flow | `tenant` | `login` |  |  | — | §7 |
| `AutoblockList` |  | `ListAutoblocks` | `mediawiki` | deferred: autoblocks, which 0016 does not specify | `tenant` | `users` |  |  | — | §7 |
| `AvailableBadges` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0026 §1, 0047 §9 | §6.1 |
| `BlankPage` | `Blankpage` |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Block` |  | `BlockIP`, `BlockUser` | `mediawiki` | served | `tenant` | `users` | restricted `block` |  | 0016 §7, 0023 §9 | — |
| `BlockList` |  | `ListBlocks`, `IPBlockList` | `mediawiki` | served | `tenant` | `users` |  |  | 0016 §7, 0023 §9 | — |
| `BookSources` | `Booksources` |  | `mediawiki` | deferred: a bound ISBN resolver (resolvers.toml draft); then an alias of it | `tenant` | `other` |  |  | — | §7 |
| `BrokenRedirects` |  |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.redirect where target_page_id is null and not external, joined to view.page; graphs local | 0051 §6 | §2.4 |
| `Categories` |  |  | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.category; graphs local | 0047 §4.4 | §2.4 |
| `ChangeContentModel` |  |  | `mediawiki` | served | `tenant` | `pagetools` | restricted `editcontentmodel` |  | 0047 §9 | §6.1 |
| `CheckEntitySchema` |  |  | `triplespace` | served | `tenant` | `wikibaserepo` |  |  | 0064 §7 | §6.4 |
| `ComparePages` |  |  | `mediawiki` | served | `tenant` | `pagetools` |  |  | 0047 §9 | §6.1 |
| `ConstraintReport` |  |  | `WikibaseQualityConstraints` | served | `tenant` | `wikibaserepo` |  | `index`: view.constraint_violation; graphs local (widened live by graphs=; 0047 §4.2) | 0031 §3 | §2.4 |
| `Contribute` |  |  | `mediawiki` | declined: a skin feature | `tenant` | `unlisted` |  |  | — | §7 |
| `Contributions` |  | `Contribs` | `mediawiki` | served | `both` | `users` |  |  | 0010 §8, 0018 §8, 0040 §7 | §1.3 |
| `Corrections` |  |  | `triplespace` | served | `tenant` | `identity` |  | `index`: view.correction (state); graphs local | 0002 §7, 0047 §6 | §2.4 |
| `CreateAccount` |  | `RequestAccount` | `mediawiki` | served | `tenant` | `login` |  |  | 0010 §10, 0087 §6 | — |
| `CreateSprint` |  |  | `triplespace` | served | `tenant` | `pagetools` | restricted `createpage` |  | 0061 §9 | §6.2 |
| `CreateWorkspace` |  |  | `triplespace` | served | `tenant` | `pagetools` | restricted `createpage` |  | 0062 §7 | §6.3 |
| `DeadendPages` | `Deadendpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_link; graphs local | 0047 §4.4 | §2.4 |
| `DeletedContributions` |  |  | `mediawiki` | served | `tenant` | `users` | restricted `deletedhistory` |  | 0023 §9 | — |
| `DeletePage` |  | `Delete` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Diff` |  |  | `mediawiki` | served | `tenant` | `redirects` | unlisted |  | 0010 §12, 0015 §2 | — |
| `DispatchStats` |  |  | `Wikibase` | declined: Wikibase's client-change dispatching has no counterpart; Special:Providers reports sync lag | `tenant` | `wikibaserepo` |  |  | — | §7 |
| `DoubleRedirects` |  |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.redirect self-joined on its target; graphs local | 0051 §6 | §2.4 |
| `EditFilter` | `AbuseFilter` | `AbuseFilter` | `AbuseFilter` | served | `tenant` | `wiki` |  |  | 0030 §10 | §1.2 |
| `EditFilterLog` | `AbuseLog` | `AbuseLog` | `AbuseFilter` | served | `tenant` | `changes` |  |  | 0030 §10 | §1.2 |
| `EditPage` |  | `Edit` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `EditRecovery` |  |  | `mediawiki` | deferred: the source editor's own recovery (0034 §7) | `tenant` | `pagetools` |  |  | — | §7 |
| `EditTags` |  |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `EditWatchlist` |  |  | `mediawiki` | served | `tenant` | `users` | restricted `editmywatchlist` |  | 0047 §9 | §6.1 |
| `EmailUser` | `Emailuser` | `Email` | `mediawiki` | declined: no user-to-user email | `tenant` | `users` |  |  | — | §7 |
| `EntitiesWithoutDescription` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  | `projection`: view.entity, view.term; graphs local | 0047 §4.2, §4.4 | §2.4 |
| `EntitiesWithoutLabel` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  | `projection`: view.entity, view.term; graphs local | 0047 §4.2, §4.4 | §2.4 |
| `EntityData` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0001, 0008 §3, wikibase-compat.md | — |
| `EntityPage` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0008 §3 | — |
| `EntitySchemaText` |  |  | `EntitySchema` | served | `tenant` | `wikibaserepo` |  |  | 0064 §7 | §6.4 |
| `EntityUsage` |  | `EntityUsageData` | `WikibaseClient` | served | `tenant` | `wikibaseclient` |  | `index`: view.entity_usage; graphs local | 0043 §10, 0047 §4.4 | §2.4 |
| `ExpandTemplates` |  |  | `mediawiki` | served | `tenant` | `pagetools` |  |  | 0047 §9 | §6.1 |
| `Export` |  |  | `mediawiki` | served | `tenant` | `pagetools` |  |  | 0047 §8 | §4.2 |
| `FewestRevisions` | `Fewestrevisions` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.page (ns, revisions); graphs local | 0047 §4.4 | §2.4 |
| `FileDuplicateSearch` |  |  | `mediawiki` | served | `tenant` | `media` |  | `index`: view.file_version (sha1); graphs local | 0039 §18 | §2.4 |
| `FilePath` | `Filepath` |  | `mediawiki` | served | `tenant` | `media` |  |  | 0039 §7 | — |
| `Fork` |  |  | `triplespace` | served | `tenant` | `pagetools` | restricted `createpage`; requires `pages.repos` |  | 0054 §2 | §6.1 |
| `GlobalBlock` |  |  | `GlobalBlocking` | served | `farm` | `instance` |  |  | 0028 §11 | §1.3 |
| `GlobalBlockList` |  | `ListGlobalBlocks` | `GlobalBlocking` | served | `farm` | `instance` |  |  | 0028 §11 | §1.3 |
| `GlobalContributions` |  |  | `GlobalContributions` | served | `tenant` | `users` |  |  | 0018 §8 | §1.3 |
| `GlobalGroupMembership` |  | `GlobalUserRights` | `CentralAuth` | served | `farm` | `instance` |  |  | 0028 §11 | §1.3 |
| `GlobalRecentChanges` |  |  | `triplespace` | served | `farm` | `instance` |  |  | 0028 §9 | §1.3 |
| `GlobalUsers` |  |  | `CentralAuth` | served | `farm` | `instance` |  |  | 0028 §11 | §1.3 |
| `GoToComment` |  |  | `DiscussionTools` | served | `tenant` | `redirects` |  |  | 0090 §9 | §6.1 |
| `GoToInterwiki` |  |  | `mediawiki` | declined: interwiki prefixes come from site aliases (0026 §2) and their links render as their targets | `tenant` | `redirects` |  |  | — | §7 |
| `GoToLinkedPage` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0029 §5 | — |
| `IdentityConflicts` |  |  | `triplespace` | served | `tenant` | `identity` |  | `index`: view.link (status held); graphs local (widened live by graphs=all; 0047 §6) | 0004 §10, 0047 §6 | §2.4 |
| `Import` |  |  | `mediawiki` | served | `tenant` | `pagetools` | restricted `import` |  | 0047 §8 | §4.2 |
| `InstanceAction` |  |  | `triplespace` | served | `farm` | `instance` | unlisted |  | 0040 §7 | §1.3 |
| `InstanceSettings` |  |  | `triplespace` | served | `farm` | `instance` |  |  | 0085 §4 | §1.3 |
| `ItemByTitle` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0026 §7, 0029 §5 | — |
| `ItemDisambiguation` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0029 §3, 0047 §9 | §6.1 |
| `ItemsWithoutSitelinks` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  | `projection`: view.entity, view.sitelink; graphs local | 0047 §4.4 | §2.4 |
| `JavaScriptTest` |  |  | `mediawiki` | declined: a MediaWiki development tool | `tenant` | `wiki` |  |  | — | §7 |
| `Jobs` |  |  | `triplespace` | served | `both` | `changes` |  |  | 0010 §9, 0046 §4 | §1.3 |
| `LinkEntities` |  |  | `triplespace` | served | `tenant` | `identity` | restricted `ts-link` |  | 0047 §6 | §3.2 |
| `LinkSearch` |  |  | `mediawiki` | deferred: an external-links table (0047 open questions) | `tenant` | `redirects` |  |  | — | §7 |
| `ListDatatypes` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §9 | §6.1 |
| `ListDuplicatedFiles` |  | `ListFileDuplicates` | `mediawiki` | served | `tenant` | `media` |  | `projection`: view.file_version; graphs local | 0039 §18 | §2.4 |
| `ListFiles` | `Listfiles` | `FileList`, `ImageList` | `mediawiki` | served | `tenant` | `media` |  | `index`: view.file; graphs local | 0039 §18 | §2.4 |
| `ListGrants` | `Listgrants` |  | `mediawiki` | served | `tenant` | `users` |  |  | 0024 §9 | — |
| `ListGroupRights` | `Listgrouprights` | `UserGroupRights` | `mediawiki` | served | `tenant` | `users` |  |  | 0016 §7 | — |
| `ListProperties` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  | `index`: view.entity (type, datatype); graphs local | 0047 §4.4 | §2.4 |
| `ListRedirects` | `Listredirects` |  | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.redirect joined to view.page; graphs local | 0051 §6 | §2.4 |
| `ListUsers` | `Listusers` | `UserList`, `Users`; section aliases: `Listadmins`→`/sysop`, `Listbots`→`/bot` | `mediawiki` | served | `tenant` | `users` |  | `index`: view.actor; graphs local | 0024 §9 | §2.4 |
| `LockDB` | `Lockdb` |  | `mediawiki` | declined: read-only is a tenant freeze (0018 §10) or an instance act (0040) | `tenant` | `wiki` |  |  | — | §7 |
| `Log` |  | `Logs` | `mediawiki` | served | `both` | `changes` |  |  | 0010 §12, 0023 §9, 0046 §4 | §1.3 |
| `LonelyPages` | `Lonelypages` | `OrphanedPages` | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_link, view.transclusion; graphs local | 0047 §4.4 | §2.4 |
| `LongPages` | `Longpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.page (ns, len); graphs local | 0047 §4.4 | §2.4 |
| `MediaStatistics` |  |  | `mediawiki` | served | `tenant` | `media` |  | `projection`: view.file_version; graphs local | 0039 §18 | §2.4 |
| `MergeHistory` |  |  | `mediawiki` | declined: it would move revisions between pages, which the log cannot do | `tenant` | `pagetools` |  |  | — | §7 |
| `MergeItems` |  | `MergeItem` | `Wikibase` | served | `tenant` | `wikibaserepo` | restricted `item-merge` |  | 0047 §5 | §3.1 |
| `MergeLexemes` |  |  | `WikibaseLexeme` | served | `tenant` | `wikibaserepo` |  |  | 0066 §7 | §6.5 |
| `MergeUpstream` |  |  | `triplespace` | served | `tenant` | `pagetools` | restricted `edit` |  | 0068 §6 | §6.1 |
| `MIMESearch` | `MIMEsearch` |  | `mediawiki` | served | `tenant` | `media` |  | `index`: view.file_version; graphs local | 0039 §18 | §2.4 |
| `MostCategories` | `Mostcategories` |  | `mediawiki` | served | `tenant` | `highuse` |  | `projection`: view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `MostInterwikis` | `Mostinterwikis` |  | `mediawiki` | declined: interlanguage links are sitelinks (0026) | `tenant` | `highuse` |  |  | — | §7 |
| `MostLinkedCategories` | `Mostlinkedcategories` | `MostUsedCategories` | `mediawiki` | served | `tenant` | `highuse` |  | `projection`: view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `MostLinkedFiles` | `Mostimages` | `MostFiles`, `MostImages` | `mediawiki` | served | `tenant` | `highuse` |  | `projection`: view.file_link; graphs local | 0047 §4.4 | §2.4 |
| `MostLinkedPages` | `Mostlinked` | `MostLinked` | `mediawiki` | served | `tenant` | `highuse` |  | `projection`: view.page_link; graphs local | 0047 §4.4 | §2.4 |
| `MostRevisions` | `Mostrevisions` |  | `mediawiki` | served | `tenant` | `highuse` |  | `index`: view.page (revisions); graphs local | 0047 §4.4 | §2.4 |
| `MostTranscludedPages` | `Mostlinkedtemplates` | `MostLinkedTemplates`, `MostUsedTemplates` | `mediawiki` | served | `tenant` | `highuse` |  | `projection`: view.transclusion; graphs local | 0047 §4.4 | §2.4 |
| `MovePage` | `Movepage` |  | `mediawiki` | served | `tenant` | `pagetools` | restricted `move` |  | 0047 §9 | §6.1 |
| `Mute` |  |  | `mediawiki` | deferred: undecided (0047 open questions) | `tenant` | `users` |  |  | — | §7 |
| `MyContributions` | `Mycontributions` | `MyContribs` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `MyLanguage` |  |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `MyLanguageFallbackChain` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §9 | §6.1 |
| `MyLog` | `Mylog` |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `MyPage` | `Mypage` |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `MyTalk` | `Mytalk` |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `MyUploads` | `Myuploads` | `MyFiles` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `NamespaceInfo` |  |  | `mediawiki` | served | `tenant` | `wiki` |  |  | 0047 §9 | §6.1 |
| `NewEntitySchema` |  |  | `EntitySchema` | served | `tenant` | `wikibaserepo` |  |  | 0064 §7 | §6.4 |
| `NewFiles` | `Newimages` | `NewImages` | `mediawiki` | served | `tenant` | `changes` |  | `index`: view.file_version; graphs local | 0039 §18 | §2.4 |
| `NewItem` |  | `CreateItem` | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0010 §2, 0029 §3, 0047 §5 | §3.1 |
| `NewLexeme` |  |  | `WikibaseLexeme` | served | `tenant` | `wikibaserepo` |  |  | 0066 §7 | §6.5 |
| `NewPages` | `Newpages` |  | `mediawiki` | served | `tenant` | `changes` |  | `index`: view.activity (creations, patrolled); graphs local | 0047 §4.4 | §2.4 |
| `NewProperty` |  | `CreateProperty` | `Wikibase` | served | `tenant` | `wikibaserepo` | restricted `property-create` |  | 0047 §5 | §3.1 |
| `NewSection` |  |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Notifications` |  | section aliases: `NotificationsMarkRead`→`(the page)` | `Echo` | served | `tenant` | `users` |  |  | 0021 §6 | §1.2 |
| `Nuke` |  |  | `Nuke` | served | `tenant` | `pagetools` | restricted `nuke` |  | 0047 §7 | §4.1 |
| `OAuth` |  |  | `OAuth` | served | `tenant` | `login` | unlisted |  | 0025 §1 | — |
| `OAuthConsumers` | `OAuthListConsumers` | `OAuthListConsumers`, `OAuthConsumerRegistration`, `OAuthManageConsumers` | `OAuth` | served | `farm` | `users` |  |  | 0025 §5, 0046 §8 | §1.3 |
| `PageData` |  |  | `mediawiki` | deferred: whether to alias it to the document node {base}/page/{id} (0008 §10) | `tenant` | `unlisted` |  |  | — | §7 |
| `PageHistory` |  | `History` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `PageInfo` |  | `Info` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `PageLanguage` |  |  | `mediawiki` | deferred: per-page content language, which nothing specifies | `tenant` | `pagetools` |  |  | — | §7 |
| `PagesWithBadges` |  | `QueryBadges` | `WikibaseClient` | served | `tenant` | `wikibaseclient` |  | `index`: view.sitelink (own host); graphs local | 0047 §4.4 | §2.4 |
| `PagesWithProp` |  | `PagesByProp` | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.page_prop (name, sortkey, page_id); graphs local | 0055 §6 | §2.4 |
| `PasswordPolicies` |  |  | `mediawiki` | served | `tenant` | `users` |  |  | 0047 §9 | §6.1 |
| `PasswordReset` |  |  | `mediawiki` | served | `tenant` | `users` |  |  | 0047 §9 | §6.1 |
| `PendingSubsidiaries` |  |  | `triplespace` | served | `tenant` | `users` | restricted `userrights` |  | 0025 §5 | — |
| `PermanentLink` |  | `PermaLink` | `mediawiki` | served | `tenant` | `redirects` | unlisted |  | 0019 §8, 0021 §4 | — |
| `Preferences` |  | section aliases: `DisplayNotificationsConfiguration`→`#notifications` | `mediawiki` | served | `tenant` | `users` | restricted `editmyoptions` |  | 0085 §5 | §1.2 |
| `PrefixIndex` | `Prefixindex` |  | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.page (ns, title); graphs local | 0047 §4.4 | §2.4 |
| `Proposals` |  |  | `triplespace` | served | `tenant` | `identity` |  |  | 0067 §7 | §3.2 |
| `ProtectedPages` | `Protectedpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.acl; graphs local | 0016 §7, 0023 §9 | §2.4 |
| `ProtectedTitles` | `Protectedtitles` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.page (reserved); graphs local | 0023 §9 | §2.4 |
| `ProtectPage` |  | `Protect` | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Providers` |  |  | `triplespace` | served | `both` | `wiki` |  |  | 0022 §11 | §1.3 |
| `Purge` |  |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Query` |  |  | `triplespace` | served | `tenant` | `wikibaserepo` | requires `query.enabled` |  | 0059 §6 | §6.1 |
| `Random` | `Randompage` | `RandomPage` | `mediawiki` | served | `tenant` | `redirects` |  | `index`: view.page (ns, random); graphs local | 0047 §4.4 | §2.4 |
| `RandomInCategory` |  |  | `mediawiki` | served | `tenant` | `redirects` |  | `index`: view.page_category, view.page (random); graphs local | 0047 §4.4 | §2.4 |
| `RandomRedirect` | `Randomredirect` |  | `mediawiki` | served | `tenant` | `redirects` |  | `index`: view.redirect joined to view.page (random); graphs local | 0051 §6 | §2.4 |
| `RandomRootpage` | `Randomrootpage` |  | `mediawiki` | served | `tenant` | `redirects` |  | `index`: view.page (ns, random); graphs local | 0047 §4.4 | §2.4 |
| `RecentChanges` | `Recentchanges` |  | `mediawiki` | served | `tenant` | `changes` |  |  | 0010 §7, 0020 §4 | — |
| `RecentChangesLinked` | `Recentchangeslinked` | `RelatedChanges` | `mediawiki` | served | `tenant` | `changes` |  |  | 0020 §4 | — |
| `Redirect` |  |  | `mediawiki` | served | `tenant` | `redirects` |  |  | 0007 §2, 0039 §7, 0047 §9 | §6.1 |
| `RedirectEntity` |  | `EntityRedirect`, `ItemRedirect`, `RedirectItem` | `Wikibase` | served | `tenant` | `wikibaserepo` | restricted `item-redirect` |  | 0047 §5 | §3.1 |
| `RemoveGlobalBlock` |  | `GlobalUnblock` | `GlobalBlocking` | served | `farm` | `instance` |  |  | 0047 §9 | §6.1 |
| `RenameUser` | `Renameuser` |  | `mediawiki` | served | `tenant` | `users` | restricted `renameuser` |  | 0047 §9 | §6.1 |
| `RevisionDelete` | `Revisiondelete` |  | `mediawiki` | served | `tenant` | `pagetools` | restricted `deleterevision` |  | 0023 §9, 0047 §9 | §6.1 |
| `RunJobs` |  |  | `mediawiki` | declined: internal to MediaWiki | `tenant` | `unlisted` |  |  | — | §7 |
| `Search` |  |  | `mediawiki` | served | `tenant` | `redirects` |  |  | 0010 §3, 0047 §9 | §6.1 |
| `SetAliases` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §5 | §3.1 |
| `SetDescription` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §5 | §3.1 |
| `SetEntitySchemaLabelDescriptionAliases` |  |  | `EntitySchema` | served | `tenant` | `wikibaserepo` |  |  | 0064 §7 | §6.4 |
| `SetLabel` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §5 | §3.1 |
| `SetLabelDescriptionAliases` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §5 | §3.1 |
| `SetSiteLink` |  |  | `Wikibase` | served | `tenant` | `wikibaserepo` |  |  | 0047 §5 | §3.1 |
| `ShortPages` | `Shortpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `index`: view.page (ns, len); graphs local | 0047 §4.4 | §2.4 |
| `SpecialPages` | `Specialpages` |  | `mediawiki` | served | `both` | `other` |  |  | 0010 §2, 0047 §9 | §6.1 |
| `Statistics` |  | `Stats` | `mediawiki` | served | `tenant` | `wiki` |  |  | 0047 §9, §13 | §6.1 |
| `Tags` |  |  | `mediawiki` | served | `tenant` | `changes` |  |  | 0030 §10 | — |
| `Takedown` |  |  | `triplespace` | served | `farm` | `instance` | unlisted |  | 0039 §18 | §1.3 |
| `Takedowns` |  |  | `triplespace` | served | `farm` | `instance` | restricted `ts-takedown` |  | 0039 §18 | §1.3 |
| `TalkPage` |  |  | `mediawiki` | served | `tenant` | `unlisted` | unlisted |  | 0047 §9 | §6.1 |
| `Tenancy` |  |  | `triplespace` | served | `both` | `redirects` | unlisted |  | 0028 §11, 0046 §9, 0085 §4 | §1.3 |
| `Tenants` |  |  | `triplespace` | served | `farm` | `instance` |  |  | 0018 §11, 0046 §9 | §1.3 |
| `TrackingCategories` |  |  | `mediawiki` | served | `tenant` | `pages` |  | `index`: view.category; 0042 §9 tracking categories; graphs local | 0047 §4.4 | §2.4 |
| `TransferAccount` |  |  | `triplespace` | served | `tenant` | `users` | restricted `ts-transferaccount` |  | 0024 §9 | — |
| `Unblock` |  |  | `mediawiki` | served | `tenant` | `users` | restricted `block` |  | 0047 §9 | §6.1 |
| `UncategorizedCategories` | `Uncategorizedcategories` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `UncategorizedFiles` | `Uncategorizedimages` | `UncategorizedImages` | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_category, view.file; graphs local | 0039 §18 | §2.4 |
| `UncategorizedPages` | `Uncategorizedpages` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `UncategorizedTemplates` | `Uncategorizedtemplates` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `UnconnectedPages` |  | `WithoutConnection`, `WithoutSitelinks` | `WikibaseClient` | served | `tenant` | `wikibaseclient` |  | `projection`: view.page, view.sitelink (own host); graphs local | 0038 §6, 0047 §4.4 | §2.4 |
| `Undelete` |  |  | `mediawiki` | served | `tenant` | `pagetools` | restricted `browsearchive` |  | 0023 §9 | — |
| `UnlockDB` | `Unlockdb` |  | `mediawiki` | declined: read-only is a tenant freeze (0018 §10) or an instance act (0040) | `tenant` | `wiki` |  |  | — | §7 |
| `UnusedCategories` | `Unusedcategories` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.category, view.page_category; graphs local | 0047 §4.4 | §2.4 |
| `UnusedFiles` | `Unusedimages` | `UnusedImages` | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.file, view.file_link; graphs local | 0039 §18 | §2.4 |
| `UnusedTemplates` | `Unusedtemplates` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.transclusion; graphs local | 0047 §4.4 | §2.4 |
| `UnwatchedPages` | `Unwatchedpages` |  | `mediawiki` | declined: the watch set is private state (0020 §3); an aggregate over it would make the report projection a reader of private | `tenant` | `maintenance` |  |  | — | §7 |
| `Upload` |  |  | `mediawiki` | served | `tenant` | `media` | restricted `upload` |  | 0039 §18 | — |
| `UploadStash` |  |  | `mediawiki` | served | `tenant` | `media` | restricted `upload` |  | 0047 §9 | §6.1 |
| `UserLogin` | `Userlogin` | `Login` | `mediawiki` | served | `tenant` | `login` |  |  | 0010 §10 | — |
| `UserLogout` | `Userlogout` | `Logout` | `mediawiki` | served | `tenant` | `login` |  |  | 0047 §9 | §6.1 |
| `UserRights` | `Userrights` | `MakeSysop`, `MakeBot` | `mediawiki` | served | `tenant` | `users` |  |  | 0016 §7, 0024 §9 | — |
| `Version` |  |  | `mediawiki` | served | `both` | `wiki` |  |  | 0077 | §5 |
| `WantedCategories` | `Wantedcategories` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_category, view.page; graphs local | 0047 §4.4 | §2.4 |
| `WantedFiles` | `Wantedfiles` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.file_link, view.file; graphs local | 0047 §4.4 | §2.4 |
| `WantedPages` | `Wantedpages` | `BrokenLinks` | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.page_link, view.page; graphs local | 0047 §4.4 | §2.4 |
| `WantedTemplates` | `Wantedtemplates` |  | `mediawiki` | served | `tenant` | `maintenance` |  | `projection`: view.transclusion, view.page; graphs local | 0047 §4.4 | §2.4 |
| `Watchlist` |  |  | `mediawiki` | served | `tenant` | `changes` | restricted `viewmywatchlist` |  | 0020 §4 | — |
| `WhatLinksHere` | `Whatlinkshere` |  | `mediawiki` | served | `tenant` | `pagetools` |  |  | 0010 §12 | — |
| `WikiSettings` |  |  | `triplespace` | served | `tenant` | `wiki` |  |  | 0085 §4 | — |
| `WithoutInterwiki` | `Withoutinterwiki` |  | `mediawiki` | declined: interlanguage links are sitelinks (0026) | `tenant` | `maintenance` |  |  | — | §7 |
