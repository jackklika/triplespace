# 0042. Template expansion and the Parsoid renderer

- **Status:** Proposed
- **Date:** 2026-09-30
- **Updated:** 2026-11-04 (A22)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0024](0024-subsidiary-accounts.md), [0028](0028-tenancy-policy.md), [0030](0030-edit-filters.md), [0033](0033-backend-stack.md), [0034](0034-frontend-stack.md), [0038](0038-page-metadata-and-categories.md)
- **Uses:** [0023](0023-moderation.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0047](0047-special-pages.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

Triplespace renders a fixed subset of wikitext with a parser built into the binary ([0008](0008-namespaces-and-document-pages.md) §8, [0033](0033-backend-stack.md) §9.1). Everything outside the subset stays visible: a template call renders as a chip showing the call. Three earlier decisions depend on that:

| Where | What it assumes |
|---|---|
| [0008](0008-namespaces-and-document-pages.md) §9 | Imported pages are made renderable by one extra **flattening** revision, whose text the source wiki expanded with `action=expandtemplates` |
| [0038](0038-page-metadata-and-categories.md) §3 | "Templates are not parsed": a category that a template emits reaches a page only through flattening |
| `namespaces.toml` | Template (10) is reserved and not implemented |

That is enough for a wiki that starts native. It is not enough for one that wants to keep using MediaWiki's content as it is: an imported wiki that keeps editing its templates, or a mirror of a Wikipedia whose articles are mostly infoboxes, navboxes and citation templates.

**Parsoid on its own does not change this.** Parsoid is the wikitext-to-HTML engine inside MediaWiki. It does not expand templates. Every transclusion and parser function goes to its host's `DataAccess::preprocessWikitext`, which in MediaWiki is the legacy preprocessor, the place where ParserFunctions and Scribunto run. A Parsoid "instance" is therefore only as capable as the expansion behind it.

So the question splits in two:

1. **Expansion.** Turning source into expanded wikitext: templates, template arguments, parser functions, variables and, with [0043](0043-lua-modules.md), Lua.
2. **Rendering.** Turning expanded wikitext into HTML. Parsoid is the high-fidelity option; the built-in renderer is the light one.

James's direction, from the design discussion of 2026-09-30:

- **Expansion is native only.** No configuration hands expansion to another service.
- **A tenant can be configured to render through a Parsoid instance** for full wikitext rendering.
- **Links and categories come from expanded output.**
- **The Template namespace and its talk namespace are enabled when template expansion is enabled.**
- **Renders that depend on mutable remote content are acceptable,** because that is consistent behaviour: a page that uses a remote template shows what the template says now.

## Decision

### 1. Expansion, then rendering (amends 0008 §8 and 0033 §9.1)

*Changed by A19.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.1.*

### 2. Settings (extends 0015 §3; extends 0028 §1)

*Changed by A2, A5, A19.*

*Current text: [23](../architecture/23-configuration-and-registry.md) §1.4, §3.1, §3.2, §3.4, §3.6, §6.*

### 3. The Template namespace (amends 0008 §2)

*Changed by A7, A13, A14, A19.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.3.*

### 4. The expander

*Changed by A6, A9, A22.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.4.*

### 5. Variables, parser functions, tags and switches: a registry (extends 0015 §5)

*Changed by A6, A9, A10, A12, A20, A21.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §2.1, §2.2, §2.3.*

### 6. Pre-save transform (extends 0008 §8)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.5.*

### 7. The built-in renderer after expansion (amends 0010 §4)

*Changed by A6.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §1.2, §6.3.*

### 8. The Parsoid renderer

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §3.*

#### 8.1 Parsoid calls back into Triplespace

*Changed by A6, A21.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §3.1.*

#### 8.2 The render-scoped API

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §3.2.*

#### 8.3 The Parsoid service (amends 0033 §1)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §3.3.*

#### 8.4 Output and failure

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §3.4.*

### 9. Links, categories and other metadata come from expanded output (amends 0038 §3; amends 0008 §10)

*Changed by A6, A17, A19.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §4.1.*

### 10. The render manifest and refresh (extends 0013 §5.6 and 0014 §3–5)

*Changed by A2, A3, A6, A8, A9, A15, A17, A18.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §4.2, §4.3, §4.4, §4.5, §4.6.*

### 11. Foreign template repositories (uses 0039 §11)

*Changed by A5, A11.*

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §5.1, §5.2, §5.3.*

### 12. Rendering a revision as of its time (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §3.2, §3.4, §5.2.*

### 13. Import (amends 0008 §9)

*Changed by A19.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §6.*

### 14. API (extends 0012 §4 and §5)

*Changed by A8, A21.*

*Current text: [18](../architecture/18-api.md) §2.2, §2.3, §3.2.*

### 15. Editing and preview (amends 0010 §4; amends 0034 §6)

*Current text: [19](../architecture/19-site-ui.md) §3.1, §3.2, §5.4.*

### 16. Limits, rate limits and errors (extends 0024 §5)

*Current text: [07](../architecture/07-actors-and-accounts.md) §6.2, §6.4.*

### 17. Search and filters (extends 0014 §7; amends 0030 §2)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1; [09](../architecture/09-security-and-moderation.md) §7.2.*

### 18. Conformance (extends 0033 §15)

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

### 19. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Wikis built on templates can live here unchanged.** An imported wiki keeps editing its templates, and a Wikipedia mirror can render its infoboxes, through the built-in renderer at modest fidelity or through Parsoid at MediaWiki's.
- **Parsoid adds no second source of truth.** It reads everything from Triplespace, through a render-scoped API that records what it read.
- **Expansion has one implementation,** used for views, `expandtemplates`, pre-save transform, preview and Parsoid's callbacks, so the API and the page agree.
- **A render now depends on more than its page.** Template edits, entity changes, the clock and remote wikis move a page's output. The manifest, the epoch and the refresh queue make that explicit, and a template used on a million pages costs a million refreshes, as it does on MediaWiki.
- **Some `view` tables are no longer replayable.** Links, categories and transclusions on a tenant with expansion on are rebuilt by re-rendering, and with foreign repositories or volatile inputs the rebuild can differ. The log stays the source of truth for what was written; it is not the record of what was rendered.
- **Categories can come from templates.** 0038's projection now reads the expanded text, so categorizing by template works, and so do tracking categories for broken templates.
- **One optional service needs PHP.** Instances that never choose Parsoid never run it.
- ~~**Pages will look plainer than on Wikipedia** until site styles exist. Templates assume `MediaWiki:Common.css` and TemplateStyles, and neither is available (Q2, Q3).~~ *TemplateStyles and one site-styles page are available ([0055](0055-templatestyles-templatedata-and-page-properties.md) §3–4; A6).*
- **History can render as it was.** `as_of=revision` is something MediaWiki cannot offer, and it falls out of the append-only log.

## Open questions

- **Q1. Cascading protection.** With transclusion, protecting a page without protecting its templates protects little. `cascade=1` is still refused ([0023](0023-moderation.md) §8); whether to implement it as an enclosure over `view.transclusion`.
- **Q2.** ~~**TemplateStyles.** A `sanitized-css` model and a CSS sanitizer in Rust. 0008 §5 excluded it with the other style models; templates from Wikipedia depend on it.~~ *Settled by [0055](0055-templatestyles-templatedata-and-page-properties.md) §1–3: the `sanitized-css` model, the `scatter-css` sanitizer, and `<templatestyles>` implemented.*
- **Q3.** ~~**Site styles.** Whether a tenant may have any site CSS, given 0008 §5's exclusion of `css`.~~ *Settled by [0055](0055-templatestyles-templatedata-and-page-properties.md) §4: one designated `sanitized-css` page, named by `wikitext.site_styles`, scoped to rendered content.*
- **Q4.** ~~**Mirror mode for template repositories,** copying used foreign pages into an instance partition as 0039 §11's file mirrors do, for availability and verification.~~ *Settled by [0053](0053-mirrored-pages.md) §5: a page repository in `mirror` mode keeps its pages, templates and modules included, in the `pages/{repo}` instance partition.*
- **Q5.** ~~**Push invalidation from Wikimedia repositories.** Following EventStreams `recentchange` for the repository's Template and Module namespaces, instead of waiting for `repo.cache_ttl`.~~ *Settled by [0053](0053-mirrored-pages.md) §6: a repository's `events` stream bumps the render epoch of every page whose manifest names a changed foreign template.*
- **Q6. Editing through Parsoid HTML.** VisualEditor-style editing needs Parsoid's HTML-to-wikitext direction, page bundles stored per revision, and TemplateData.
- **Q7. Section editing of transcluded sections,** which MediaWiki numbers `T-1`.
- **Q8. Language variants.** `languagevariants` is empty and LanguageConverter is not implemented.
- **Q9. Labeled Section Transclusion** (`#lst`), which Wikisource depends on.

## References

- [Manual:Preprocessor](https://www.mediawiki.org/wiki/Manual:Preprocessor), [Help:Templates](https://www.mediawiki.org/wiki/Help:Templates), [Help:Magic words](https://www.mediawiki.org/wiki/Help:Magic_words), [Help:Extension:ParserFunctions](https://www.mediawiki.org/wiki/Help:Extension:ParserFunctions)
- [Parsoid](https://www.mediawiki.org/wiki/Parsoid); `wikimedia/parsoid` `src/Config/Api/{SiteConfig,DataAccess,PageConfig}.php` and `bin/parse.php` (`--apiURL`)
- [API:Expandtemplates](https://www.mediawiki.org/wiki/API:Expandtemplates), [API:Parse](https://www.mediawiki.org/wiki/API:Parse), [API:Siteinfo](https://www.mediawiki.org/wiki/API:Siteinfo)
- [Manual:Templatelinks table](https://www.mediawiki.org/wiki/Manual:Templatelinks_table), [Manual:Job queue](https://www.mediawiki.org/wiki/Manual:Job_queue) (`refreshLinks`, `htmlCacheUpdate`)
- [Help:Tracking categories](https://www.mediawiki.org/wiki/Help:Tracking_categories)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-30
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §19
- **Summary:** 0005 §2 is the one crate table CI checks, and carries the two new crates and every change this section listed (0005 A42).

Replaced text (§19):

> | Crate | Change |
> |---|---|
> | `scatter-wikitext-expand` *(new)* | Substrate, pure, no C dependencies. The preprocessor tree, frames, strip markers, pre-save `subst:`, core variables and parser functions, ParserFunctions, the registry embedding `docs/registry/wikitext-functions.toml`, the `ExpandHost` trait, and the manifest and limit-report types (§4–6, §10). Depends on the ICU4X crates already chosen ([0033](0033-backend-stack.md) §9.3) for dates, numbers and plurals |
> | `scatter-wikitext` | Renders expanded text with its strip state; extracts links, categories, file links, external links and indicators from expanded text (§7, §9); `poem` and `indicator` |
> | `triplespace-render` *(new)* | `ExpandHost` over the serving model; the render pipeline and renderer choice; the Parsoid client and the render-scoped endpoint's manifest collection (§8); the refresh job, `ops.render_refresh` and the invalidation fan-out (§10); foreign template repositories (§11). Depends on `triplespace-db`, `triplespace-projections`, `triplespace-titles`, `triplespace-cache`, `triplespace-upstream`, `scatter-wikitext`, `scatter-wikitext-expand` and `scatter-pages` |
> | `triplespace-titles` | Conditional namespaces (`enabled_by`) and the disabled-namespace refusal (§3) |
> | `triplespace-db` | Migrations for `view.transclusion`, `view.render_state` and `ops.render_refresh` |
> | `triplespace-cache` | The render epoch in `p:` keys; dependency `Cache-Tag`s (§10) |
> | `triplespace-search` | Rendered `text`, `source_text` and `hastemplate:` (§17) |
> | `triplespace-api-action`, `triplespace-api-rest` | The modules and routes of §14; the render-scoped endpoint (§8.2) |
> | `triplespace-cli` | `render refresh` (a tenant or the instance) and `render rebuild` (§10) |
>
> `services/parsoid/` is a PHP program, not a crate. The workspace goes from forty-nine crates to fifty-one.

### A2. Lua: the `wikitext.lua` setting and entity usage

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §1, §10
- **Change:** extends §2, §10
- **Summary:** `wikitext.lua` (`off`, `on`; default `off`; requires `wikitext.expansion = on`) joins the settings of §2, and the manifest records an entity with the usage aspects of 0043 §10, kept in `view.entity_usage`. Both were written into §2 and §10 before this ADR was first committed, together with 0043 (commit `644c2b6`).

### A3. Report deltas and site statistics

- **Date:** 2026-09-30
- **Source:** [0047](0047-special-pages.md) §4.3, §13
- **Change:** extends §10
- **Summary:** The refresh job applies report deltas (`view.report_entry`) in the same transaction as the `transclusion`, link, category and `entity_usage` rows it writes. Site statistics, a volatile input here, are read from `view.site_stats`.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §10, §19
- **Summary:** A1–A3 were folded into the Decision. The open questions were numbered, and the headings of §1, §11 and §17 now use the verbs of 0050 §3 (amends, uses). No decision changed. Before this, A3 was a blockquote, A2 was written in place with no note, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A5. Template repositories become page repositories

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §1, §3
- **Change:** amends §2, §11
- **Summary:** The `template-repo` config kind is retired into `page-repo`, a repository that may serve any `pages` namespace; `wikitext.template_repos` becomes `pages.repos` and `wikitext.share` becomes `pages.share`. §11's lookup is the title stack of 0052 §3 applied to transclusion, and is otherwise unchanged. Nothing had been written under the old names.

Replaced text (§2):

> | `wikitext.template_repos` | A list of repository names | Empty | Foreign template repositories, in lookup order (§11) |
> | `wikitext.share` | `off`, `on` | `off` | Whether this tenant's Template and Module pages may serve other tenants as a `tenant` repository (§11) |

Replaced text (§11):

> **A template repository is a source of Template and Module pages that a tenant uses without copying them,** configured exactly as file repositories are ([0039](0039-files-and-media.md) §11): a `config` record of kind `template-repo`, keyed `template-repo:{name}`, in a tenant's or the instance's `config`, and listed in lookup order in the tenant's `wikitext.template_repos`.

### A6. TemplateStyles, TemplateData and page properties

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §3, §5–7
- **Change:** amends §5, §8.1; extends §4, §7, §9, §10
- **Summary:** `templatestyles` and `templatedata` move from `chip` to `implemented`; `__DISAMBIG__` from `ignored` to `implemented`; `SHORTDESC` and `#REDIRECT` are registered; the page properties expansion returns are stored in `view.page_prop`; `action=templatedata` is served to Parsoid; stylesheets are manifest entries and have a tracking category. Q2 and Q3 settled by 0055; Q4 and Q5 by [0053](0053-mirrored-pages.md) §5–6.

Replaced text (§5):

> | **Extension tags:** `templatestyles`, `templatedata`, `math`, `chem`, `ce`, `score`, `timeline`, `graph`, `mapframe`, `maplink`, `categorytree`, `inputbox`, `imagemap`, `hiero`, `charinsert` | `chip` |

> | **Behaviour switches:** `__NOEDITSECTION__`, `__NEWSECTIONLINK__`, `__NONEWSECTIONLINK__`, `__NOGALLERY__`, `__INDEX__`, `__NOINDEX__`, `__STATICREDIRECT__`, `__EXPECTUNUSEDCATEGORY__`, `__DISAMBIG__`, `__NOCONTENTCONVERT__`, `__NOTITLECONVERT__` | `ignored` |

Replaced text (§8.1):

> | TemplateData | `action=templatedata` | Not implemented. Only Parsoid's HTML-to-wikitext direction uses it, and reading never calls it |

### A7. An include may not widen

- **Date:** 2026-10-01
- **Source:** [0056](0056-security-model.md) §6
- **Change:** amends §3
- **Summary:** Transclusion is subject to the flow rule: a page includes another only if the included page's `read` groups are a subset of the including page's; a refused include is a red link for every viewer. The §10 refresh on a `read` ACL change already re-evaluates it.

Replaced text (§3):

> **Any `wikitext` page can be transcluded**, as in MediaWiki:

### A8. A plain purge re-renders

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §14
- **Change:** amends §10, §14
- **Summary:** A plain `action=purge` on a local page bumps its render epoch, deletes its `p:` keys and purges its `Cache-Tag`, as MediaWiki's purge re-parses a page whose output depends on the clock or on expanded templates. The new epoch changes the page's `ETag`, which is how the web tier's revalidating cache and the CDN see it. Anyone who may read the page may purge it, as in MediaWiki.

Replaced text (§10):

> 6. `action=purge` with `forcelinkupdate` or `forcerecursivelinkupdate`.

Replaced text (§14):

> | `action=purge` | `forcelinkupdate` and `forcerecursivelinkupdate` queue refreshes (§10) |

### A9. Blocks

- **Date:** 2026-10-05
- **Source:** [0062](0062-workspaces.md) §2–3
- **Change:** extends §4, §5, §10
- **Summary:** `ExpandHost` gains scope, table, sprint and board reads; the registry gains the `#workspace` declaration and eight block functions, flagged `scoped`; the manifest gains the scoped-block entry and `view.render_dep_scoped`, with refreshes enqueued by the scope and task projections.

### A10. `#query` and `#conformance`

- **Date:** 2026-10-05
- **Source:** [0063](0063-query-namespace.md) §6; [0064](0064-entityschema-and-validation.md) §6
- **Change:** extends §5
- **Summary:** Two more scoped blocks: a saved query's result table, and a schema's conformance figures.

### A11. `repo.cache_ttl` is a per-repository field

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §11
- **Summary:** `repo.cache_ttl` is a per-repository field of the `page-repo`/`file-repo` record, not a setting beside it, with a per-kind default: 7 days for files ([0039](0039-files-and-media.md) §11), one hour for template source and page bundles ([0052](0052-page-repositories-and-title-inheritance.md) §1). §11's sentence on proxying names the field as a field of the `page-repo` record with the one-hour default. (PENDING C20)

Replaced text (§11):

> **`mediawiki` repositories proxy.** Fetched source is cached for `repo.cache_ttl` (default one hour), and each manifest entry carries the remote revision ID and that expiry, so a page using a remote template refreshes when the cache does (§10).

### A12. Implemented tags and switches render with expansion off

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** Tags and behaviour switches the registry marks `implemented` are rendered by the built-in renderer whether or not `wikitext.expansion` is on; expansion gates templates, parser functions and variables, not the extension tags and switches of §5's table. [0033](0033-backend-stack.md) §9.1's chip list is pruned to what stays `chip`. The row's verb is `amends`, but §5 says nothing that this contradicts: its statuses describe what the expander does with each entry and are silent on the renderer with expansion off, so it is logged as `extends`. (PENDING E20)

### A13. Template's models

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** The models of Template (10) are `wikitext` and `sanitized-css`, the latter by the `.css` suffix rule of [0055](0055-templatestyles-templatedata-and-page-properties.md) §1, which §3's table omitted after 0055 added it to [0041](0041-content-models.md) §3. (PENDING E21)

Replaced text (§3):

> | 10 Template | `pages` | `wikitext` | `first-letter` | Yes, as in MediaWiki | 11 |

### A14. Template talk is registered while expansion is off

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** While `wikitext.expansion` is off, 10 is `reserved` and 11 is registered but disabled, both in `namespaces.toml` with `enabled_by`, as [0043](0043-lua-modules.md) §2 already says for 828 and 829. §3 said 11 was not registered. (PENDING E22)

Replaced text (§3):

> **While it is off**, 10 is `reserved` and 11 is not registered, as now. This is a new rule of 0008 §2: **a reserved number can be implemented conditionally**, by a setting named in its `namespaces.toml` entry (`enabled_by = "wikitext.expansion"`).

### A15. `render_state.unheld_misses`

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §10
- **Summary:** `view.render_state` gains `unheld_misses integer`, the count of unheld-entity misses that [0043](0043-lua-modules.md) §11 describes `render_state` as holding. (PENDING E23)

### A16. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§19
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [07](../architecture/07-actors-and-accounts.md), [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [11](../architecture/11-rendering-templates-and-modules.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A17. Row classes with one writer each, and usage invalidation as a tier-3 consumer

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §3, §6
- **Change:** amends §9, §10
- **Summary:** Every projection declares its class, log-replayed or view-derived, and the render tables are neither: `transclusion`, `render_state` and the render-owned rows are render-derived, rebuilt by re-rendering every `wikitext` page, a rebuild that truncates the render-owned rows, enqueues every page on `ops.render_refresh` and re-runs no job. `view.page_link`, `view.page_category` and `view.page_prop` carry two row classes told apart by `from_render`, with one writer each: the page projection writes the source-derived rows when it applies the record, the refresh job writes the expansion-derived rows from the render; each owns its class exclusively, readers union them, the refresh job never deletes a row it did not write, and turning `wikitext.expansion` off truncates the render-owned rows and nothing else. Refresh trigger 3 is no longer "a change to an entity's `resolved_version`" but a composition event, "entity X composed to version N", consumed by the entity-usage consumer in its own queue, never inside the composing or appending transaction; the consumer checks `view.entity_usage` for the entity and every member of its cluster before diffing, is off while no tenant has `wikitext.expansion` on, and off during bootstrap and bulk modes. The chapter resolved what the ledger did not say: a page's source links are indexed as soon as its edit is projected, and only the links its templates emit wait for the refresh queue. (REVIEW G10, G13)

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §4.1, as it stood):

> **These rows are written by the refresh job (§4.3), not by the page projection.** Computing them may read other pages and run Lua, which does not belong inside the appending transaction ([0013](0013-postgres-storage.md) §7). A page's links follow its edit after the refresh queue, as MediaWiki's deferred links update does.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §4.3, as it stood):

> **These are the first `view` tables that are not a pure function of the log.** They are rebuilt by re-rendering every `wikitext` page, and where a render read a foreign repository or the clock, the rebuild may differ from the original. This is the exception to [0013](0013-postgres-storage.md) §5.6's rule that every `view` table is rebuilt from the log, for these three tables and for the links and categories they drive (§4.1); the catalogue is [03](../architecture/03-storage-caches-and-search.md)'s.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §4.4, as it stood):

> 3. A change to an entity's `resolved_version`, for usage rows whose aspects the change touches (§7.9).

### A18. Dependency-induced epoch bumps serve stale

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §10
- **Summary:** An epoch bump that a dependency induces (a template or module edited, an entity recomposed, a `wikitext.*` setting changed) serves the old `p:` entry stale-while-revalidate and lets the refresh queue re-render the page in its own time, so a template edit on a mirror of Wikipedia re-renders its pages behind the readers rather than in front of them. Synchronous renders are bounded per tenant by a render pool, deployment configuration beside the Parsoid concurrency; a miss that finds the pool full is served stale where a stale entry exists and enqueued, and waits only where there is none. The page's own edit and `action=purge` keep "no stale": they delete the `p:` keys and the next read renders afresh, so read-your-writes holds for the editor; erasure, deletion and hiding of a dependency never serve stale either, whatever the pool's state. (REVIEW G46)

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §4.5, as it stood):

> Its lifetime is the lower of [0014](0014-caches-and-search.md) §4's ceiling and `expires_at`. A miss renders synchronously under single flight ([0014](0014-caches-and-search.md) §4); there is no stale serving after an epoch bump, as MediaWiki renders afresh after `page_touched` moves.

### A19. Expansion is part of wikitext

- **Date:** 2026-10-20
- **Source:** Direct: James, review of 2026-10-20
- **Change:** amends §1, §2, §3, §9, §13
- **Summary:** `wikitext.expansion` is withdrawn. Template expansion is a feature of having `wikitext` pages, not a setting: a tenant with pages expands them, and a tenant with only entities has nothing to expand. The setting's off state described a wiki that has pages but no templates, which is not one MediaWiki's clients expect: Pywikibot checks every page save for `{{nobots}}` through `prop=templates&tlnamespace=10` and refuses a namespace siteinfo omits, so on such a tenant it could save no page at all. So Template (10) and Template talk (11) are `pages` namespaces on every tenant, without `enabled_by`; `subst:` always expands on save; links, categories and page properties are always read from the expanded text as well as the stored text; the template reports and `Special:ExpandTemplates` need no setting, and `ts-expansion-off` is withdrawn. The two row classes of `from_render` stay, since their reason is one writer per class, but "turning expansion off truncates the render-owned rows" goes with the setting. The import's flattening revision (§13) becomes the fallback for an export that lacks its templates, not a tenant's mode. `wikitext.lua` and `wikitext.renderer` are unchanged, and `wikitext.ceiling` keeps `expansion` and `lua` and now caps only Lua; a `subset` ceiling already written caps Lua as `expansion` does. A stored `wikitext.expansion` record is ignored, since the registry no longer resolves the key, so no tenant needs a migration. Chapters [11](../architecture/11-rendering-templates-and-modules.md) §1.1, §1.3, §2.3 and §4.1, [10](../architecture/10-pages-and-content-models.md) §1.4, §5.1, §5.3, §5.5 and §6, [23](../architecture/23-configuration-and-registry.md) §1.4, §3.2 and §3.4, and the sentences of [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [13](../architecture/13-mirrored-pages.md) §3.3, [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md) §3.1–3.2, [21](../architecture/21-special-pages.md) and [22](../architecture/22-crates-and-stack.md) that named the setting were rewritten; the registries `settings.toml`, `namespaces.toml`, `special-pages.toml`, `tenancy.toml`, `version.toml` and `wikitext-functions.toml` follow.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §1.1 and §1.3, as they stood):

> **With expansion off, stage 1 is the identity.** Template calls and parser functions render as chips, and categories are read from the source ([0038](../decisions/0038-page-metadata-and-categories.md) §3, in [10](../architecture/10-pages-and-content-models.md)).

> Whether a tenant expands, runs Lua and which renderer serves its HTML are the `site` settings `wikitext.expansion`, `wikitext.lua` and `wikitext.renderer`, capped by the instance's `wikitext.ceiling`; the Parsoid service's address, timeout and concurrency are deployment configuration. The settings, their defaults and the rule that turning `wikitext.expansion` off also turns `wikitext.lua` off and `wikitext.renderer` back to `builtin` are in [23](../architecture/23-configuration-and-registry.md) ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2).

> **While `wikitext.expansion` is on**, Template (10) is a `pages` namespace and Template talk (11) is enabled:

> **While it is off**, 10 is `reserved` and 11 is registered but disabled, both entered in `namespaces.toml` with `enabled_by = "wikitext.expansion"`, as §7.2 says for 828 and 829. This is a rule of [0008](../decisions/0008-namespaces-and-document-pages.md) §2: **a reserved number can be implemented conditionally**, by a setting named in its `namespaces.toml` entry. The namespace table is [10](../architecture/10-pages-and-content-models.md)'s.

> **Turning expansion off never removes pages.** Template pages that exist keep their records and history. The namespace refuses `create`, `edit` and `move` into it with `ts-namespace-disabled`, and its existing pages stay readable at their titles and can be deleted. Nothing transcludes them. Turning expansion on again restores everything. `meta=siteinfo` lists a disabled namespace while it still holds pages, so clients can read them.

Replaced text ([10](../architecture/10-pages-and-content-models.md) §6 step 3, as it stood):

> 3. **Flatten templates in one extra revision, where the tenant does not expand them.** Where the latest revision uses templates, a bot fetches the expanded text from the source wiki with `action=expandtemplates` and saves it as a new revision. History stays faithful to the source, and the current text needs no template engine. With `wikitext.expansion` on, this step is optional: the importer may bring the Template and Module pages with full history, as ordinary pages, instead of flattening. Flattening remains the procedure for a tenant with expansion off, or by choice.

Replaced text ([23](../architecture/23-configuration-and-registry.md) §3.2, as it stood):

> | `wikitext.expansion` | `site` | tenant ≤ instance ceiling (`wikitext.ceiling`) | `off`, `on` | `off` | Expand templates and parser functions; enables Template and Template talk | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 |

> Turning `wikitext.expansion` off also turns `wikitext.lua` off and `wikitext.renderer` back to `builtin`; changing any `wikitext.*` setting bumps the render epoch of every `wikitext` page in the tenant and queues them for refresh ([11](../architecture/11-rendering-templates-and-modules.md)).

### A20. The Interface namespace

- **Date:** 2026-10-28
- **Source:** [0095](0095-the-interface-namespace.md) §2
- **Change:** amends §5
- **Summary:** Messages that `{{int:}}` reads are overridden by the wiki's Interface pages, the instance's under the wiki's (0095 §2); the namespace is no longer reserved.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §2.3, as it stood):

> **Messages.** `{{int:}}`, tracking-category names (§4.1) and Parsoid's `meta=allmessages` calls (§3.1) read MediaWiki core's message files for the languages the instance serves, shipped as data. The MediaWiki namespace (8) stays reserved, so no wiki can override a message.

### A21. Messages from Interface pages

- **Date:** 2026-10-28
- **Source:** [0095](0095-the-interface-namespace.md) §8 (A1)
- **Change:** amends §5, §8.1, §14
- **Summary:** `{{int:}}` expands the wiki's Interface page for the key in the wiki's language, with its arguments, and stays a chip where there is none; `meta=allmessages` answers from the Interface pages over the site's shipped messages (0095 §2, §8).

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §2.3, as it stood):

> **Messages.** `{{int:}}`, tracking-category names (§4.1) and Parsoid's `meta=allmessages` calls (§3.1) read MediaWiki core's message files for the languages the instance serves, shipped as data, under the wiki's overrides: a page of the Interface namespace (8, canonical `MediaWiki`) overrides a message for the wiki, `Interface:{Key}` in its own language and `Interface:{Key}/{lang}` in one, the instance's from the primary tenant's pages and the wiki's over them, as [0095](../decisions/0095-the-interface-namespace.md) §2 has it for every interface message. Overrides are not read by the renderer yet.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §3.1, as it stood):

> | Tracking-category names | `meta=allmessages` | Shipped messages (§2.3) |

Replaced text ([18](../architecture/18-api.md) §2.3, as it stood):

> | `meta=allmessages` | The shipped MediaWiki messages ([0042](../decisions/0042-template-expansion-and-parsoid.md) §5) | [0042](../decisions/0042-template-expansion-and-parsoid.md) §14 |

### A22. Which template drew a block

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while building template debugging (SCA-230), 2026-11-04; unconfirmed
- **Change:** extends §4
- **Summary:** Expansion also maps each of the page's own template calls to its output, and a render can mark each top-level block a call drew with `data-ts-template`, the template's title, so "which template made this" is a hover away (the Show Templates script on Wikipedia), and so a census can read it from rendered HTML. The calls:
  - The map covers the calls written in the page that transcluded a page: not calls inside the templates they transcluded, not parser functions or calls left as written, and not a missing template's red link.
  - A block is a top-level element of the render lying wholly within one call's output, read from the spans of everything it holds, since a generated paragraph or list has no span of its own. A template's inline output inside a paragraph of the page's own text marks nothing; a template that draws two blocks marks both.
  - The position is `data-ts-template-at`, the byte offset of the call's `{{` in the page's text, which a tool can map back to the source; not an ordinal, which shifts with every call before it.
  - It is asked for, never the default: `action=parse` takes the Triplespace flag `tstemplates`, and the site passes it when the page is viewed with `?tstemplates=1`. A public render, which the edge caches by its address, never carries the marks unless that address asks.
