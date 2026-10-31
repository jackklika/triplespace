# 0047. Special pages

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-10-31 (A28)
- **Author:** James Hare / Claude Opus
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0024](0024-subsidiary-accounts.md), [0029](0029-resolver-namespaces.md), [0031](0031-property-constraints.md), [0042](0042-template-expansion-and-parsoid.md), [0046](0046-primary-tenant.md)
- **Uses:** [0019](0019-discussions.md), [0021](0021-notifications.md), [0028](0028-tenancy-policy.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md)

## Context

About sixty special pages are named across [0001](0001-revision-metadata-rdf.md) to [0046](0046-primary-tenant.md). Each ADR named one where MediaWiki users would look for it: 0010 §12's address table, 0016 §7, 0023 §9, 0028 §11, 0039 §18 and so on. [0005](0005-crate-organization.md) §2 says `triplespace-server` "serves the special pages the ADRs name". That makes the set whatever the ADRs happened to mention, and leaves four problems.

1. **Pages the ADRs rely on but never name.**
   - [0010](0010-site-ui.md) §2 puts a **Special pages** link in the global header, but specifies no index for it.
   - 0010 §3 describes "the full results page" of search without giving it an address.
   - 0010 §2's **New** menu and [0029](0029-resolver-namespaces.md) §3's "Create an item with this DOI" both open an item form that is not specified.
   - Move, revision deletion and unblocking exist as actions ([0008](0008-namespaces-and-document-pages.md) §4, [0023](0023-moderation.md) §8) but have no pages.
   - Identity conflicts are "listed for review" ([0004](0004-identity-clusters-and-equivalence.md) §10), as are local corrections ([0002](0002-source-graphs-and-mass-ingest.md) §7), but neither list has an address.
2. **Standard pages nobody has decided.** MediaWiki 1.43 core registers 138 special pages, 8 of them conditionally. Wikibase REL1_43 registers 20 in the repository and 3 in the client. Most of them are maintenance reports, which tools, gadgets and help pages link to by name. No ADR says which of them Triplespace serves.
3. **Renamed pages have lost their MediaWiki names.** Some examples:
   - `Special:EditFilter` replaces AbuseFilter's `Special:AbuseFilter` ([0030](0030-edit-filters.md)).
   - `Special:OAuthConsumers` and a section of `Special:Account` replace the OAuth extension's consumer pages ([0025](0025-oauth-server.md) §5).
   - Bot passwords became subsidiaries' keys ([0024](0024-subsidiary-accounts.md) §4).

   Only `Special:Preferences` was given a redirect (0010 §11). [0016](0016-permissions-and-access-control.md) Consequences promises that "MediaWiki clients and users see MediaWiki", and that promise does not hold at these addresses.
4. **Reports at mirror scale.** MediaWiki computes its expensive reports offline in miser mode (`$wgMiserMode`, `updateSpecialPages.php` into `querycache`). A report over everything an instance holds would include hundreds of millions of mirrored entities. Such a report costs a great deal and tells a wiki's editors little about their own wiki.

Wikibase REL1_43 no longer registers `Special:EntitiesWithoutLabel` or `Special:EntitiesWithoutDescription`, which earlier releases had. Tools still link to them.

James's direction, from the design discussion of 2026-09-30:

- Add the Wikibase compatibility forms.
- Reports are **local graphs only by default**, as the general rule for report pages.
- Reports are **live projections by default**, and otherwise configured as batch.
- Build a **native Special:Nuke** and a **Special:Export**.
- **LinkSearch** stays outstanding until the external-links table is designed.
- **No EmailUser**.
- **Mute** and the **redirect reports** are deferred.
- **Aliases are configured in the registry**.

## Decision

### 1. One registry of special pages (extends 0015 §5)

*Changed by A9, A15, A19, A22, A23, A27, A28.*

*Current text: [21](../architecture/21-special-pages.md) §1.1, §8.*

### 2. Names, aliases and section aliases

*Changed by A19, A22.*

*Current text: [21](../architecture/21-special-pages.md) §1.2.*

### 3. Where a page is served (extends 0018 §11 and 0046 §7)

*Changed by A22, A24.*

*Current text: [21](../architecture/21-special-pages.md) §1.3.*

### 4. Reports

*Current text: [21](../architecture/21-special-pages.md) §2.*

#### 4.1 What a report is

*Current text: [21](../architecture/21-special-pages.md) §2.1.*

#### 4.2 Local graphs by default

*Changed by A18.*

*Current text: [21](../architecture/21-special-pages.md) §2.2.*

#### 4.3 Live projections by default; batch by configuration

*Changed by A18.*

*Current text: [21](../architecture/21-special-pages.md) §2.3.*

#### 4.4 The reports

*Changed by A3, A5, A16.*

*Current text: [21](../architecture/21-special-pages.md) §2.4, §8.*

#### 4.5 Who sees what

*Current text: [21](../architecture/21-special-pages.md) §2.5.*

### 5. Wikibase compatibility forms

*Changed by A26.*

*Current text: [21](../architecture/21-special-pages.md) §3.1.*

### 6. Identity and correction pages

*Changed by A11, A21.*

*Current text: [21](../architecture/21-special-pages.md) §3.2.*

### 7. Special:Nuke

*Changed by A20.*

*Current text: [21](../architecture/21-special-pages.md) §4.1.*

### 8. Special:Export and Special:Import

*Changed by A20.*

*Current text: [21](../architecture/21-special-pages.md) §4.2, §4.3.*

### 9. Other pages (extends 0010 §2, §3 and §12)

*Changed by A4, A6, A7, A8, A9, A10, A11, A12, A25.*

*Current text: [21](../architecture/21-special-pages.md) §5.1, §6.1, §8.*

### 10. Deferred, declined and reserved

*Changed by A3, A5, A9.*

*Current text: [21](../architecture/21-special-pages.md) §7, §8.*

### 11. API (extends 0012 §4 and §5)

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2.*

### 12. Permissions (extends 0016 §2)

*Changed by A14.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.6.*

### 13. Storage (amends 0013 §5 and §7)

*Changed by A13, A20.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.5, §4.10, §5, §6.1.*

### 14. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **One list answers whether Triplespace has a given special page,** and, where it does not, why not. The checker keeps the ADRs and the list from drifting apart.
- **MediaWiki's names keep working** for renamed pages and for pages folded into `Special:Account`. A help page or tool that links to `Special:AbuseFilter` or `Special:BotPasswords` lands in the right place.
- **Reports describe the wiki, not its mirrors.** A tenant mirroring Wikidata gets `ItemsWithoutSitelinks` for its own items, not for a hundred million of Wikidata's. Mirror-scale reports are possible, but only as configured batch runs.
- **Reports are live without miser mode.** The price is one more projection and some fan-out per link and category change. The instance's `reports` configuration is the escape hatch where a tenant's write rate makes that too costly. Operators can apply it to the whole instance or to one tenant.
- **Nuke is a job, so it is one row, one patrol mark and one undo.** It reaches entities, statements, files and posts as well as pages, and it can go back further than recent changes.
- **Exports can be verified.** The `records` format lets someone who holds an export prove what the wiki held, which suits Scatterbase's purposes as well.
- ~~**Some familiar pages wait on other designs.** `LinkSearch` and `PagesWithProp` wait on tables that 0042 implies but never defines. The redirect reports wait on [0008](0008-namespaces-and-document-pages.md) Q8.~~ *The redirect reports are served ([0051](0051-page-redirects.md) §6; A3) and `PagesWithProp` is ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6; A5). `LinkSearch` still waits on an external-links table.*

## Open questions

- **Q1. The external-links table.** Its shape (MediaWiki's `externallinks` stores reversed domains for prefix search) decides `LinkSearch`, `list=exturlusage`, and edit-filter variables over added links.
- **Q2.** ~~**A page-properties table.** It would serve `PagesWithProp`, `list=pageswithprop` and `prop=pageprops`.~~ *Settled by [0055](0055-templatestyles-templatedata-and-page-properties.md) §6: `view.page_prop`.*
- **Q3. Global Nuke.** Whether a global group at the farm base ([0028](0028-tenancy-policy.md) §3) may nuke a farm account across every tenant it is linked to, as one job per tenant or as an instance act ([0040](0040-instance-prerogatives.md)).
- **Q4. Page statements in exports.** Whether to carry them in MediaWiki XML as an extra slot, which only Triplespace would read, or only in `records`.
- **Q5. Mute.** Whether `Special:Mute` and user-level muting of notifications ([0021](0021-notifications.md)) and email should exist at all.
- **Q6. Reports over mirror graphs.** Which batch reports over a mirror are worth offering by default, for example mirrored properties without a local constraint, and whether their snapshots should be shared across tenants that mirror the same provider.
- **Q7. Thresholds.** The defaults are placeholders: `nuke.confirm_threshold` (500), `export.sync_revisions` (5,000), `export.max_history` (1,000) and the `reports` `batch_limit` (1,000).

## References

- [MediaWiki `SpecialPageFactory`, REL1_43](https://github.com/wikimedia/mediawiki/blob/REL1_43/includes/specialpage/SpecialPageFactory.php) (the core list and its groups) and [`MessagesEn.php`](https://github.com/wikimedia/mediawiki/blob/REL1_43/languages/messages/MessagesEn.php) (English aliases)
- [Wikibase `extension-repo.json` and `extension-client.json`, REL1_43](https://github.com/wikimedia/mediawiki-extensions-Wikibase/tree/REL1_43) (repository and client special pages)
- [Help:Special pages](https://www.mediawiki.org/wiki/Help:Special_pages), [Manual:$wgMiserMode](https://www.mediawiki.org/wiki/Manual:$wgMiserMode), [Manual:updateSpecialPages.php](https://www.mediawiki.org/wiki/Manual:UpdateSpecialPages.php), [Manual:$wgQueryCacheLimit](https://www.mediawiki.org/wiki/Manual:$wgQueryCacheLimit)
- [API:Querypage](https://www.mediawiki.org/wiki/API:Querypage), [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo) (`specialpagealiases`, `statistics`)
- [Extension:Nuke](https://www.mediawiki.org/wiki/Extension:Nuke)
- [Help:Export](https://www.mediawiki.org/wiki/Help:Export), [Manual:$wgExportMaxHistory](https://www.mediawiki.org/wiki/Manual:$wgExportMaxHistory)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §14
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A47).

Replaced text (§14):

> | Crate | Change |
> |---|---|
> | `triplespace-titles` | Embeds `special-pages.toml`. Resolves `Special:` names, aliases (case-folded) and section aliases, and applies the farm/tenant scope rules (§1–3). |
> | `triplespace-projections` | The `report` and `site_stats` projections, the new `page` and `actor` columns, and the batch report runner (§4, §13) |
> | `scatter-integrity` | Building and verifying a `records` export: a selection of records with inclusion proofs (§8) |
> | `triplespace-api-action`, `triplespace-api-rest` | `specialpagealiases`, `list=querypage`, `export`, and the routes of §11 |
> | `triplespace-server` | Serves the pages the registry marks `served`, including the forms of §5 and the pages of §6–9 |
> | `triplespace-cli` | `verify --records` |
>
> No crate is added.

### A2. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §14–15
- **Summary:** A1 was folded into the Decision. The open questions were numbered, and the list this ADR kept as §15, "Changes to other ADRs" (quoted below), was replaced by the generated table, so the Decision now ends at §14. No decision changed. Before this, A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

Replaced text (§15):

> - **[0002](0002-source-graphs-and-mass-ingest.md) §7 and [0004](0004-identity-clusters-and-equivalence.md) §10:** the correction list and the conflict list are `Special:Corrections` and `Special:IdentityConflicts` (§6).
> - **[0005](0005-crate-organization.md) §2:** the crate changes listed above.
> - **[0007](0007-actor-identity.md):** the open question on serving `Special:Redirect/user/{id}` locally is settled: it is served, with the other `Redirect` forms (§9).
> - **[0008](0008-namespaces-and-document-pages.md) §9:** `Special:Import` is the form for the import job, and `Special:Export` its counterpart (§8).
> - **[0010](0010-site-ui.md) §2, §3 and §12:** the Special pages link goes to `Special:SpecialPages`, the **New** menu to `NewItem` and `NewProperty`, and the full results page is `Special:Search`. The address table is now the registry.
> - **[0013](0013-postgres-storage.md) §5.6 and §7:** the tables, columns and projection order of §13.
> - **[0015](0015-record-format-and-partition-registry.md) §3 and §5, and [0016](0016-permissions-and-access-control.md) §2:** the `reports` config kind, a new registry file, and `nuke`.
> - **[0018](0018-tenants.md) §11 and [0046](0046-primary-tenant.md) §7:** the scope rule of §3.
> - **[0023](0023-moderation.md) §9 and §11:** `Special:Nuke` and `nuke`.
> - **[0024](0024-subsidiary-accounts.md) §5:** the `export` class and the `sysop` row of the `job` class.
> - **[0029](0029-resolver-namespaces.md) §3:** "Create an item with this DOI" opens `Special:NewItem` with the statement filled in.
> - **[0031](0031-property-constraints.md) §3:** `ConstraintReport` defaults to the local graph.
> - **[0042](0042-template-expansion-and-parsoid.md) §10:** the refresh job applies report deltas for the tables it writes (§4.3); `view.site_stats` serves the `NUMBEROF*` variables (§13).

### A3. The redirect reports

- **Date:** 2026-10-01
- **Source:** [0051](0051-page-redirects.md) §6
- **Change:** amends §10; extends §4.4
- **Summary:** `ListRedirects`, `BrokenRedirects`, `DoubleRedirects` and `RandomRedirect` leave the deferred table and are served as index-backed reports over `view.redirect`.

Replaced text (§10):

> | `ListRedirects`, `BrokenRedirects`, `DoubleRedirects`, `RandomRedirect` | deferred | Page redirects ([0008](0008-namespaces-and-document-pages.md) Q8) |

### A4. `Special:Fork`

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §2
- **Change:** extends §9
- **Summary:** A Triplespace page that forks an inherited title without an edit, restricted to `createpage`.

### A5. `PagesWithProp`

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §6
- **Change:** amends §10; extends §4.4
- **Summary:** `view.page_prop` exists, so `PagesWithProp` leaves the deferred table and is served over it. Q2 settled.

Replaced text (§10):

> | `PagesWithProp` | deferred | A page-properties table. 0042 §4 returns page properties, but none are stored. |

### A6. `Special:Query`

- **Date:** 2026-10-04
- **Source:** [0059](0059-query-service.md) §6
- **Change:** extends §9
- **Summary:** `Special:Query`, the SPARQL editor over `/sparql`, served while `query.enabled` is on. Registered in `special-pages.toml` with `requires = "query.enabled"`.

### A7. `Special:CreateSprint`

- **Date:** 2026-10-04
- **Source:** [0061](0061-sprints-and-tasks.md) §9
- **Change:** extends §9
- **Summary:** `Special:CreateSprint`, restricted to `createpage`, registered in `special-pages.toml`.

### A8. `Special:CreateWorkspace`

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §7
- **Change:** extends §9
- **Summary:** `Special:CreateWorkspace`, restricted to `createpage`, registered in `special-pages.toml`.

### A9. EntitySchema pages

- **Date:** 2026-10-05
- **Source:** [0064](0064-entityschema-and-validation.md) §7
- **Change:** amends §10; extends §1, §9
- **Summary:** The three EntitySchema pages move from reserved to served, with `CheckEntitySchema` added; §1's `reserved` status has no current members.

Replaced text (§10):

> - **`reserved`:** the name belongs to an entity type Triplespace has reserved but not implemented. These are Lexeme and EntitySchema ([0009](0009-keyed-entity-types-and-domain.md), [0041](0041-content-models.md)).

### A10. Lexeme pages

- **Date:** 2026-10-05
- **Source:** [0066](0066-lexemes.md) §7
- **Change:** extends §9
- **Summary:** `NewLexeme` and `MergeLexemes` move from reserved to served.

### A11. Proposals and merging

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §4, §7; [0068](0068-merging-with-upstream.md) §6
- **Change:** extends §6, §9
- **Summary:** `Special:Corrections`' export becomes Propose; `Special:Proposals` is new; `Special:MergeUpstream` is new.

### A12. Special:Version

- **Date:** 2026-10-07
- **Source:** [0077](0077-special-version.md) §1
- **Change:** amends §9
- **Summary:** `Special:Version` gets its own row, pointing to 0077, which specifies its sections, subpages and sources; it is no longer described as built from the registry, configuration and i18n alone.

Replaced text (§9):

> | `Version`, `ApiSandbox`, `ApiHelp`, `AllMessages`, `NamespaceInfo`, `PasswordPolicies`, `ListDatatypes`, `AvailableBadges`, `MyLanguageFallbackChain`, `Blankpage` | From the registry, configuration and i18n |

### A13. `tenant` is the slug

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §13
- **Summary:** The `tenant` column of `view.report_entry`, `view.report_state` and `view.site_stats` is `tenant text NOT NULL DEFAULT ''`, the column as [0013](0013-postgres-storage.md) §5 (0013 A11) defines it for every `view` table, not `tenant bigint`. (PENDING A6)

Replaced text (§13):

> CREATE TABLE view.report_entry (                -- §4.3: projection and batch reports
>   tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL DEFAULT 'local',  -- graph set (§4.2)

> CREATE TABLE view.report_state (
>   tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL,

> CREATE TABLE view.site_stats (                  -- §9: MediaWiki's site_stats
>   tenant bigint PRIMARY KEY,

### A14. No `sysop` job rate row

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §12
- **Summary:** Jobs run under subsidiaries: `ts-runjob` defaults to `bot` everywhere ([0016](0016-permissions-and-access-control.md) §2 drops `sysop`; [0046](0046-primary-tenant.md) §8), and an administrator runs a job, a Nuke or a job revert included, by creating or approving a subsidiary they operate. The `sysop` row this section added to the `job` rate class of [0024](0024-subsidiary-accounts.md) §5 goes with it. James: "administrators should be able to run jobs under subsidiaries", read as this. (PENDING C14)

Replaced text (§12):

> The `job` rate class gains a `sysop` row, 10 per hour (amends [0024](0024-subsidiary-accounts.md) §5, which gave only `user` and `bot`). Without that row an administrator who is not a bot could not start a Nuke or a job revert.

### A15. `listed`, `requires`, `note` and extension origins

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §1
- **Summary:** The field table of §1 gains `listed` (whether `Special:SpecialPages` lists the page), `requires` (the setting the page is served under, as A6 used for `Special:Query`), both of which the registry file already carries, and `note`; and `origin` may be any extension's name as `version.toml` lists it ([0077](0077-special-version.md) §8), not only the names §1 enumerates. (PENDING F20)

### A16. `RandomRootpage`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4.4
- **Summary:** The report is `RandomRootpage`, the registry's spelling and MediaWiki's first English alias; §4.4 had `RandomRootPage`. `check_adrs.py` check 5 becomes case-sensitive on canonical names so the difference is caught ([0050](0050-adr-format.md) §14, check 15). (PENDING F21)

Replaced text (§4.4):

> | `Random`, `RandomInCategory`, `RandomRootPage` | `view.page.random` (§13), as MediaWiki's `page_random` |

### A17. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§14
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [18](../architecture/18-api.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A18. Reports are a view-derived, tier-3 consumer of composition

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3, §6
- **Change:** amends §4.2, §4.3
- **Summary:** The `report` projection declares its class: it is view-derived, populated by a scan of the `view` tables it reads and never by a log replay, and "rebuild projection P for tenant T from `view`" is the named operation a batch-to-live switch uses. It is a tier-3 consumer: it consumes events — "entity X composed to version N" for entity reports, the page projection's and the refresh job's row changes for page reports — in its own queue, at its own rate, in no write's transaction, off during bootstrap and bulk modes until the operator turns it on. An entity report re-evaluates an entry only when the canonical's composed row changes and the cluster has a local member, found by an index on `cluster_id` restricted to local IDs, so a mirror's changes never fan through the report projection for clusters with no local member; a live entity report's freshness is bounded by composition lag, shown beside "computed at". "Step 5 of 0013 §7" and "the synchronous budget" are withdrawn; the refresh job's rows are events to the report projection like any other, not deltas it applies in the refresh transaction. (REVIEW G10, G13, G50)

Replaced text ([21](../architecture/21-special-pages.md) §2.2, as it stood):

> - **Entity reports list local subjects.** The rows are local entities, and local assertions where a report is about statements. Conditions are evaluated on the **resolved view**, as a reader sees it. For example:

Replaced text ([21](../architecture/21-special-pages.md) §2.3, as it stood):

> | `projection` | Rows in `view.report_entry` ([03](../architecture/03-storage-caches-and-search.md) §4.10), maintained incrementally by the `report` projection as the tables it reads change | Live, at that projection's lag |

> Changing a mode writes nothing into any tenant's partitions, so it is not an instance act ([0040](0040-instance-prerogatives.md) §1). When a report moves from live to batch, its next run fills its rows. When it moves from batch to live, the `report` projection rebuilds its rows for that tenant.

> A batch report's page says when it was computed, as MediaWiki's cached reports do. `list=querypage` returns `cached`, `cachedtimestamp` and `maxresults` for it.

> - The `report` projection runs after the tables it reads, in step 5 of [0013](0013-postgres-storage.md) §7 ([03](../architecture/03-storage-caches-and-search.md) §6.1). Each change to an input key recomputes only the entries that key affects. For example, a new `page_link` row to a missing title adds one to that title's `WantedPages` count.
> - Report entries are **fan-out**, not part of a write's own rows. They are applied by the projection worker under 0013 §7's synchronous budget, and their lag is reported with the other projections'.
> - Some inputs are written by the refresh job rather than by replay ([0042](0042-template-expansion-and-parsoid.md) §10): `transclusion`, `entity_usage`, and links and categories under expansion. For those, the refresh job applies the report deltas in the same transaction as the rows it writes.

> - `projection` entries are a function of the `view` tables they read, and are rebuilt with them.

### A19. A served page exists only once the build registers its handler

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** corrects §1, §2
- **Summary:** `served` records a decision, not an implementation, and a served page exists in a build only once it registers a handler. At startup `triplespace-titles` intersects the registry's `served` set with the handlers the binary compiled; that intersection is what the instance reports as its special pages, in `siprop=specialpagealiases` and on `Special:SpecialPages`. A served page without a handler is reported, with the difference, on `server.admin_listen`, and a request for it gets MediaWiki's "no such special page" response, so a client never learns of a page the build cannot serve. The rest of the row (`private.watch`'s `watch_target` index, encrypted upstream grants, `siprop=triplespace` as a function of configuration with the live counters moved to `GET /version`, `/provider/{code}/mirror` and `/repo/{name}`) lands in [0020](0020-change-feeds.md), [0022](0022-federation.md) and [0012](0012-api-requirements.md). (REVIEW G47)

Replaced text ([21](../architecture/21-special-pages.md) §1.1, as it stood):

> - **`served`:** the instance serves the page. "Served" records a decision, not an implementation. Implementation is tracked in the milestones.

Replaced text ([21](../architecture/21-special-pages.md) §1.2, as it stood):

> **`siprop=specialpagealiases`** reports one entry per served page.

### A20. Nuke by tag, and per-entity export folded by the export job

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §8; extends §7, §13
- **Summary:** `view.activity.tags` gets a GIN index (`activity_tags`, [03](../architecture/03-storage-caches-and-search.md) §4.6), so a change-tag target's whole history is one index scan: Nuke's window over an account reads `view.activity` by actor, and over a tag reads it through that index. Per-revision entity export is folded by the export job, which reads one row of `view.entity_revision` per revision and folds it into the XML there, never on the request path; the revisions exported per entity are capped by `export.max_entity_history` (default 100; [23](../architecture/23-configuration-and-registry.md) §3), lower than `export.max_history` for pages, since each entity revision is a whole state rather than a text. The report half of the row (entity reports consume composition events, trigger only for a canonical with a local member, and show their composition lag) is A18's. The foreign-actor contributions view, bounded to `view.activity` with a note that upstream revisions are not indexed by actor, is [0010](0010-site-ui.md) §8's text ([19](../architecture/19-site-ui.md) §2.5), not this ADR's. (REVIEW G50)

Replaced text ([21](../architecture/21-special-pages.md) §4.2, as it stood):

> - **Local entities** are exported as Wikibase exports entity pages. The model is `wikibase-item` or `wikibase-property`. Each revision is one local change set, and its text is the canonical JSON of the entity's **local-graph** state at that revision ([wikibase-compat.md](../api/wikibase-compat.md)). A MediaWiki Wikibase with free IDs can import them, which is the reverse of adoption ([0035](0035-adopting-a-wikibase.md); [05](../architecture/05-providers-and-ingest.md)).

### A21. `Special:LinkEntities` needs `ts-linkproperty` for `equivalent-property`

- **Date:** 2026-10-09
- **Source:** [0016](0016-permissions-and-access-control.md) A32
- **Change:** amends §6
- **Summary:** `Special:LinkEntities` needs `ts-link` for `same-as` and `different-from` and also `ts-linkproperty` for `equivalent-property`, the right that now gates linking properties.

Replaced text ([21](../architecture/21-special-pages.md) §3.2, as it stood):

> It needs `ts-link` and counts in the `link` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5).

### A22. Special:Preferences, Special:WikiSettings and Special:InstanceSettings

- **Date:** 2026-10-17
- **Source:** [0085](0085-settings-and-preferences.md) §4, §5
- **Change:** extends §1; amends §2; amends §3
- **Summary:** The registry gains three served pages: `Preferences` (MediaWiki's name, scope `tenant`, restricted to `editmyoptions`, with `DisplayNotificationsConfiguration` as its section alias for the notifications tab), `WikiSettings` (scope `tenant`) and `InstanceSettings` (scope `farm`). `Preferences` stops being a section alias of `Account`, whose `ResetTokens` alias now leads to the watchlist token and whose email aliases lead to the new Contacts section. `Tenancy` keeps its name, unlisted, as a redirect to the Tenancy tabs of the two settings pages. The chapter text is [21](../architecture/21-special-pages.md) §1.2, §1.3 and §8.

Replaced text ([21](../architecture/21-special-pages.md) §1.2, as it stood):

> | `Preferences`, `ResetTokens`, `DisplayNotificationsConfiguration` | `Account#preferences`, `Account#preferences`, `Account#notifications` |

Replaced text ([21](../architecture/21-special-pages.md) §1.2, as it stood):

> | `ChangeEmail`, `Confirmemail`, `Invalidateemail` | `Account#notifications`, where the email address and its verification live ([0021](0021-notifications.md) §4, §6) |

Replaced text ([21](../architecture/21-special-pages.md) §1.3, as it stood):

>   - `Tenancy`: the policy at the farm base, and a tenant's own view of it with 0046 §9's **Accept**.

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> | `Account` |  | section aliases: `Preferences`→`#preferences`, `ResetTokens`→`#preferences`, `DisplayNotificationsConfiguration`→`#notifications`, `ChangeCredentials`→`#sign-in-methods`, `RemoveCredentials`→`#sign-in-methods`, `ChangePassword`→`#sign-in-methods`, `LinkAccounts`→`#sign-in-methods`, `UnlinkAccounts`→`#sign-in-methods`, `ChangeEmail`→`#notifications`, `Confirmemail`→`#notifications`, `Invalidateemail`→`#notifications`, `BotPasswords`→`#subsidiaries`, `OAuthManageMyGrants`→`#connected-applications` | `triplespace` | served | `tenant` | `users` |  |  | 0010 §11 | §1.2 |

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> | `Tenancy` |  |  | `triplespace` | served | `both` | `instance` |  |  | 0028 §11, 0046 §9 | §1.3 |

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> (189 pages: 169 served, 7 deferred, 13 declined, none reserved)

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> Twenty-five served pages in the registry are specified by other ADRs and are not described in those sections or in this chapter; the catalogue marks them "—" and names their ADR: `Block`, `BlockList`, `DeletedContributions`, `ListGroupRights`, `Undelete` and `UserRights` ([09](09-security-and-moderation

### A23. Special:ConfirmAccounts, and RequestAccount as an alias

- **Date:** 2026-10-17
- **Source:** [0087](0087-account-registration-and-challenges.md) §6
- **Change:** extends §1
- **Summary:** The registry gains `ConfirmAccounts`, origin `ConfirmAccount`, served at tenant scope in the `users` group and restricted to `confirmaccount`; and `CreateAccount` gains the alias `RequestAccount`, ConfirmAccount's request form, which under `account.approval` is the same form. `version.toml` gains the `ConfirmAccount` extension, and `ConfirmEdit` with what Triplespace took from it. The chapter text is [21](../architecture/21-special-pages.md) §8.

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> | `CreateAccount` |  |  | `mediawiki` | served | `tenant` | `login` |  |  | 0010 §10 | — |

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> Every entry of `docs/registry/special-pages.toml` (192 pages: 172 served, 7 deferred, 13 declined, none reserved), in alphabetical order.

### A24. Pages at the auth base

- **Date:** 2026-10-18
- **Source:** [0089](0089-the-auth-host-and-instance-logins.md) §7
- **Change:** extends §3
- **Summary:** The auth base serves only the pages the registry marks `auth = true`, `UserLogin`, `CreateAccount`, `Account` and `UserLogout`, each in an auth form that acts on the instance login; every other page is a 404 there. Each is also a `tenant` page in its tenant form, and on a single-tenant instance without an auth host the tenant's base serves both forms as one. The field is pending until `triplespace-titles` accepts it. The chapter text is [21](../architecture/21-special-pages.md) §1.3.

### A25. Special:GoToComment

- **Date:** 2026-10-20
- **Source:** [0090](0090-converting-talk-pages-comment-by-comment.md) §9
- **Change:** extends §9
- **Summary:** `Special:GoToComment`, DiscussionTools' redirect page, is served with its meaning: it finds an imported post by its DiscussionTools name or ID and redirects to it through `Special:PermanentLink/{revid}`, listing the posts where several share a name. Origin `DiscussionTools`, scope `tenant`, group `redirects`; registered in `special-pages.toml`. Chapter text: [21](../architecture/21-special-pages.md) §6.1, §8.

Replaced text ([21](../architecture/21-special-pages.md) §8, as it stood):

> (193 pages: 173 served,

### A26. `NewProperty` offers the data types the API registers

- **Date:** 2026-10-29
- **Source:** Direct: James, design discussion of 2026-10-29
- **Change:** amends §5
- **Summary:** `Special:NewProperty` offered fifteen data types, those the entity pages had a name for, while `siteinfo`'s `wikibase-propertytypes` reported thirteen: Wikibase's twelve built-in types and `wikibase-domain` ([0009](0009-keyed-entity-types-and-domain.md)). The two extra were `math` and `musical-notation`, which Wikibase registers only with the Math and Score extensions, and which no ADR adopts: Triplespace validates neither TeX nor LilyPond, renders neither, and has no RDF type for them; the entity pages name them only so that mirrored Wikidata properties of those types display. James asked for the right answer and for the two lists to agree. `wikibase-propertytypes` is the authority, as it is in Wikibase, where it is the set a property may be created with: `NewProperty` offers exactly the registered types, and `Special:ListDatatypes` lists the same ones in the same order. A data type joins the list when an ADR adopts it and the API serves it.

Replaced text ([21](../architecture/21-special-pages.md) §3.1, as it stood):

> - `NewProperty` needs `property-create` and a `datatype`.

### A27. Special:SpecialPages lists by the registry's headings

- **Date:** 2026-10-31
- **Source:** Direct: James, confirmed 2026-10-31; Claude's call while building the grouped Special:SpecialPages, 2026-10-31
- **Change:** extends §1
- **Summary:** §1 gives every page a `group`, "the heading under which `Special:SpecialPages` lists it", and makes `listed` and `restricted` decide whether it is listed, but leaves the order of the headings, their names and the order within one to the page. The page lists its headings in a fixed order, MediaWiki's groups in the order Wikimedia wikis show them, then Wikibase's and Triplespace's, with `other` last as MediaWiki keeps it; names each with MediaWiki's or Wikibase's message, and Triplespace's two as *Identity and corrections* and *Instance administration*; orders the pages under a heading by their names in the reader's language; and gives each heading MediaWiki's class and id (`mw-specialpagesgroup`, `mw-specialpagesgroup-{key}`), which gadgets and user scripts select. Until this entry the page was one list in the order its code gave, and listed restricted pages to every reader. `requires` is not applied by the page yet, since the site does not receive a tenant's `site` settings. The chapter text is 21 §1.1.

### A28. The special-page registry is embedded by `scatter-titles`

- **Date:** 2026-10-31
- **Source:** Direct: James, design discussion of 2026-10-31
- **Change:** corrects §1
- **Summary:** §1's text said the registry is embedded by `triplespace-titles`. The crate map (22 §2.1) gives the registry to `scatter-titles`, the pure crate that embeds the namespace registry and builds for `wasm32`, and `triplespace-titles` is the DB-backed resolver over it, not yet built; `scatter-titles` has embedded `special-pages.toml` since the grouped `Special:SpecialPages` (A27). The decision is unchanged; the chapter now names the crate that holds it. The chapter text is 21 §1.1.

Replaced text ([21](../architecture/21-special-pages.md) §1.1, as it stood):

> It is embedded by `triplespace-titles`, which already resolves titles in the `Special` namespace ([0008](../decisions/0008-namespaces-and-document-pages.md) §3).
