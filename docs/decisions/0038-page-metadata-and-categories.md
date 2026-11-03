# 0038. Page metadata, legacy categories and articles

- **Status:** Proposed
- **Date:** 2026-09-29
- **Updated:** 2026-10-31 (A17)
- **Author:** James Hare / Claude Opus
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0003](0003-statement-ui.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0019](0019-discussions.md), [0026](0026-sitelinks.md), [0029](0029-resolver-namespaces.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0009](0009-keyed-entity-types-and-domain.md), [0016](0016-permissions-and-access-control.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0023](0023-moderation.md), [0030](0030-edit-filters.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0035](0035-adopting-a-wikibase.md), [0039](0039-files-and-media.md), [0041](0041-content-models.md), [0042](0042-template-expansion-and-parsoid.md), [0049](0049-boards.md)

## Context

On Wikipedia, metadata about **concepts** is rich: Wikidata gives every subject typed, referenced, queryable statements. Metadata about **pages** is stuck with systems that are decades old. Categories mix three different kinds of fact into one flat list of names. Short descriptions are a template read back as a page property. Quality assessments are templates on talk pages. And the one piece of page quality Wikidata does record, the featured-article badge, is stored on the concept's sitelink, because the page has nowhere to keep it.

Triplespace can give pages first-class metadata from the start. It already has most of the pieces: page IDs and entity page IDs share one sequence ([0015](0015-record-format-and-partition-registry.md) §2), pages live in a logged partition with full history ([0008](0008-namespaces-and-document-pages.md) §4), and the statement machinery (the statement UI of [0003](0003-statement-ui.md), constraints of [0031](0031-property-constraints.md), filters of [0030](0030-edit-filters.md), RDF and search) does not depend much on what the subject is.

[0008](0008-namespaces-and-document-pages.md) left two questions that bear on this: whether category links should ever become data, and, until the 2026-09-27 amendment reserved it, what the main namespace is for. Categories matter because **the main use case for legacy categories is ingesting page revisions from MediaWiki**, where category membership, sort keys and `__HIDDENCAT__` are stored in the page text. And pages that are paired with items are articles, which MediaWiki keeps in namespace 0.

James's direction, from the design discussion of 2026-09-29:

- Pages get first-class metadata, served over the Triplespace REST API only. No legacy MediaWiki or Wikibase API consumer expects statements on a wiki page, so page statements get no entity ID.
- **Legacy categories are defined only in wikitext.** A category that could also be defined as data would need reconciliation between two sources. The way forward is to migrate from categories to Triplespace-native page metadata and entity concepts.
- Configured mappings can turn existing categories into page statements, so a tenant gets the native shape without first rewriting its pages. For now, mappings produce statements about the page only. MediaWiki categories do cross into facts about the subject, and that boundary may be crossed later, once there is a good UI for telling page metadata from concept metadata.
- Inferring statements from template calls is premature. The focus is inferring triples from categories.
- A sitelink from an item to a page on the tenant makes the page's title a way to reach the item. Data about the underlying concept and data about the page are displayed separately, to reinforce that they describe two different things.
- Articles go in namespace 0, and its talk namespace comes with it.
- Threads get statements too. A thread's status is naturally a statement about the thread.

## Decision

### 1. Pages carry statements (extends 0008 §4)

*Changed by A2, A3, A5, A9, A11, A12, A16.*

*Current text: [06](../architecture/06-statements-and-properties.md) §5.1, §5.2.*

### 2. A page's statements: asserted and projected

*Current text: [06](../architecture/06-statements-and-properties.md) §5.3.*

### 3. Legacy categories are defined only in wikitext (amends 0008 §8; settles 0008 Q5)

*Changed by A4, A15.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §5.3.*

### 4. The `Category` namespace (amends 0008 §2)

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §3.6.*

### 5. Category mappings (extends 0015 §3)

*Current text: [10](../architecture/10-pages-and-content-models.md) §5.4.*

### 6. Pages paired with items (amends 0026 §1; extends 0026 §2)

*Changed by A7.*

*Current text: [06](../architecture/06-statements-and-properties.md) §4.6.*

### 7. One subject per frame (extends 0010 §1 and §2)

*Changed by A13.*

*Current text: [19](../architecture/19-site-ui.md) §1.1, §1.3, §1.4, §3.1.*

### 8. Namespace 0 holds articles (amends 0008 §2)

*Changed by A17.*

*Current text: [10](../architecture/10-pages-and-content-models.md) §1.4, §3.3.*

### 9. Threads (extends 0019 §6; amends 0019 §7)

*Changed by A10.*

*Current text: [14](../architecture/14-discussions.md) §2.3, §5.4.*

### 10. Storage (extends 0013 §5.6 and §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.4, §4.7, §5, §6.1.*

### 11. RDF (amends 0001 §3 and 0008 §10; uses 0032 §2)

*Changed by A8.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.4.*

### 12. Search (extends 0014 §7)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §11.1.*

### 13. API (extends 0012 §4 and §5)

*Changed by A3.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.2.*

### 14. Permissions and filters

*Current text: [09](../architecture/09-security-and-moderation.md) §7.2, §8.9.*

### 15. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **Pages and concepts have metadata of the same kind, kept apart.** Page statements use the same properties, UI, constraints, search and SPARQL as item statements, and the display rule (§7) keeps each on its own subject.
- **Legacy categories keep working with no second source of truth.** Imported pages keep their categories exactly as their text defines them, `prop=categories` and `list=categorymembers` work, and mappings give them native form without touching the text.
- **A category change in the text changes data.** Removing `[[Category:Articles needing cleanup]]` removes a statement. That is intended, and the statement's provenance says where it came from.
- ~~**Template-emitted categories are frozen at import.** After the flattening revision, adding `{{Cleanup}}` does not categorize a page. Editors add the category link, or the native statement.~~ *Only with expansion off; with a tenant's expansion on, `{{Cleanup}}` categorizes the page (A4).*
- **Namespace 0 now holds pages.** Two numbers leave the reserved set (0 and 14) and their talk namespaces (1 and 15) are enabled.
- **The main graph has non-entity subjects.** Tools that assume every subject of `p:` is a `wikibase:Item` or `wikibase:Property` will meet `schema:WebPage` subjects. The local-host sitelink's article node is a page IRI rather than a URL.
- **Page history mixes text and statement revisions.** MediaWiki clients see statement revisions as revisions that left the main text unchanged.

## Open questions

- **Q1. Crossing to concept data.** Mappings from categories to statements about the paired item, once a UI can show that a statement on an item came from an article's categories.
- **Q2. Subject-level categories.** A report comparing categories such as "1952 births" with the paired item's statements, as a way to migrate them without mapping them.
- **Q3.** ~~**Template calls after import.** Whether a narrow, non-parsing recognition of template calls (name and parameters only) should ever feed mappings.~~ *Settled by [0072](0072-template-mappings.md) §1: template calls are recognized by name and parameters, without expansion, and feed template mappings that write derived statements about items ([0071](0071-derived-statements-from-mirrored-pages.md) §1), not page statements.*
- **Q4.** ~~**Page terms.** Whether pages get a label or description, for a display title or short description, or whether those stay statements.~~ *Settled by [0065](0065-mediainfo-captions-and-commons.md) §1 for File pages: captions are the page's labels and descriptions. Open for other pages, where the short description is a page property ([0055](0055-templatestyles-templatedata-and-page-properties.md) §6); see Q7.*
- **Q5.** ~~**Category redirects.** MediaWiki's soft category redirects are templates; hard redirects wait on [0008](0008-namespaces-and-document-pages.md) Q8.~~ *Settled by [0051](0051-page-redirects.md) §4: a hard redirect on a category page is followed for viewing only, membership stays with the name in each member's text, and soft redirects stay templates.*
- **Q6. Collation.** Whether `uppercase` is enough, or tenants need ICU collations per language.
- **Q7.** (Rest of Q4.) Whether pages other than File pages get a label or description beyond the short description.

## References

- [Help:Categories](https://www.mediawiki.org/wiki/Help:Categories), [Manual:categorylinks table](https://www.mediawiki.org/wiki/Manual:Categorylinks_table) and [Help:Magic words](https://www.mediawiki.org/wiki/Help:Magic_words) (`DEFAULTSORT`, `__HIDDENCAT__`)
- [API:Categories](https://www.mediawiki.org/wiki/API:Categories), [API:Categorymembers](https://www.mediawiki.org/wiki/API:Categorymembers) and [API:Categoryinfo](https://www.mediawiki.org/wiki/API:Categoryinfo)
- [Wikibase REST API](https://www.wikidata.org/wiki/Wikidata:REST_API) statement routes
- [Extension:WikibaseMediaInfo](https://www.mediawiki.org/wiki/Extension:WikibaseMediaInfo), for the design not taken (§1)
- [Wikidata: allowed entity types constraint (Q52004125)](https://www.wikidata.org/wiki/Q52004125)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-29
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §15
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A38).

Replaced text (§15):

> No new crate.
>
> | Crate | Adds |
> |---|---|
> | `scatter-wikitext` | Extraction of category links, sort keys, `DEFAULTSORT` and `__HIDDENCAT__` from the subset; the leading-colon link (§3). Still builds for `wasm32`, so the editor's preview shows categories |
> | `scatter-wikibase-model` | The page subject and `{page ID}$<UUID>` statement IDs (§1); sitelinks held by page ID for the tenant's own hosts (§6) |
> | `scatter-wikibase-changeset` | Change sets keyed by page ID, restricted to statements (§1) |
> | `triplespace-titles` | The main and `Category` namespaces; bare titles in the main namespace, and main-namespace titles last in the `/resolve` order with the `also` field (§8) |
> | `triplespace-projections` | `page_category`, `category`, the mapping and thread-status projections, `page_statements` resolution, page subjects in `statement_assertion`, `entity_ref` and constraint checking, the mapping re-run and make-it-real jobs (§2–5, §9–10) |
> | `triplespace-rdf` | Page statements and page nodes in the main graph; the sitelink article node for paired pages (§11) |
> | `triplespace-search` | `categories` and `statement_keywords` on `pages` (§12) |
> | `triplespace-api-rest`, `triplespace-api-action` | The routes and modules of §13 |

### A2. Uploads in the page's history

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §2
- **Change:** extends §1
- **Summary:** A file page's uploads are records in the same partition, keyed by the page ID, so text revisions, statement revisions and file versions share one history.

### A3. File pages' statements as MediaInfo entities

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §6–7
- **Change:** amends §1, §13
- **Summary:** By section:
  - §1: On **File pages** the statements have the MediaInfo ID `M{page ID}` and are read and written through `wbgetentities` and the Wikibase statement modules, as on Commons, because tools written for Structured Data on Commons expect that contract. They appear in the `mediainfo` slot (`wikibase-mediainfo`) in `prop=revisions`. Terms and sitelinks on `M` IDs are refused with `not-supported` until page terms are settled. Statements on every other page stay as written here: no entity ID and no Action API.
  - §13: File pages are the exception: their statements are in the Action API as MediaInfo entities.

Replaced text (§1):

> **No entity ID.** Page statements have no `M`-style or other entity ID, are not returned by `wbgetentities`, and cannot be edited through the Wikibase Action API modules. They are served by the REST routes of §13. In `prop=revisions`, a statement revision appears as MediaWiki shows a revision that changed only a secondary slot: the main text is unchanged, and the summary describes the change.

Replaced text (§13):

> `wbgetentities` by `sites` and `titles` finds a page's paired item (§6). Page statements are not in the Action API.

### A4. Categories from expanded text

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §9
- **Change:** amends §3
- **Summary:** While a tenant's `wikitext.expansion` is on, membership is read from the **expanded** text: categories that templates emit count, `<includeonly>` categories reach transcluding pages, and expansion adds MediaWiki's tracking categories for its own conditions. The paragraph above holds for tenants with expansion off. The consequence that template-emitted categories are frozen at import was struck for tenants with expansion on.

Replaced text (§3):

> **Templates are not parsed.** Categories that templates emit reach the text only through the flattening revision of [0008](0008-namespaces-and-document-pages.md) §9 step 3, whose `action=expandtemplates` runs on the source wiki and writes the category links and `__HIDDENCAT__` into the flattened text. A template call added after an import renders as a placeholder (0008 §8) and emits nothing. Categories MediaWiki's parser adds on its own, such as tracking categories for broken file links, are not in any text and are out of scope.

### A5. Boards carry statements

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §1
- **Change:** extends §1
- **Summary:** Boards (310) carry page statements, as document pages do. They describe the board, not its threads.

### A6. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–15
- **Summary:** A1–A5 were folded into the Decision. The open questions were numbered, and §11's heading, which said it extended 0032, now says it uses 0032 §2, as the Related line already said. No decision changed. Before this, A2–A5 were blockquotes, and A1 was recorded only in 0005. The file before conversion is commit `0b26a3a`.

### A7. Sitelinks cannot target inherited titles

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §6
- **Change:** extends §6
- **Summary:** A title whose primary is a page repository's page is refused as a sitelink target with `ts-sitelink-foreign`, because the page ID a sitelink stores changes when the title is forked.

### A8. Short descriptions in RDF

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §6
- **Change:** extends §11
- **Summary:** The page node gains `schema:description` from the `wikibase-shortdesc` page property, where set. Page terms (Q4) stay open.

### A9. Captions

- **Date:** 2026-10-05
- **Source:** [0065](0065-mediainfo-captions-and-commons.md) §1
- **Change:** amends §1
- **Summary:** File pages' change sets may carry labels and descriptions (captions) through a `terms` operation; Q4 settled for File pages, the rest as Q7.

Replaced text (§1):

> It may not carry labels, descriptions, aliases or sitelinks; a page has a title, and its sitelinks are held by items (§6).

### A10. `proposal-state`

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §5
- **Change:** extends §9
- **Summary:** A second projected thread statement, from the proposal projection.

### A11. Only aliases and sitelinks are refused on `M` IDs

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §1
- **Summary:** Only aliases and sitelinks on `M` IDs are refused with `not-supported` ([0065](0065-mediainfo-captions-and-commons.md) §1). Labels and descriptions on an `M` ID are captions, accepted since A9, so §1's sentence that terms and sitelinks are refused "until page terms are settled" was wrong about something already decided. (PENDING E11)

Replaced text (§1):

> they appear in the `mediainfo` slot (`wikibase-mediainfo`) in `prop=revisions`, and terms and sitelinks on `M` IDs are refused with `not-supported` until page terms are settled (Q4).

### A12. A proposal's fields are page metadata of the thread page

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §1
- **Summary:** A proposal's kind, destination, base, payload, omitted and flags are **page metadata of the thread page**, set by the `propose` operation and projected to `view.proposal`; they are not fields of the thread's `create`, whose `target` stays the thread's home ([0067](0067-proposals.md) §3; [0019](0019-discussions.md) §4). For this ADR the decision adds to §1's account of what a thread page carries and contradicts nothing in it, so the row's `amends` is logged here as `extends`. (PENDING F7)

### A13. One tabs table

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §7
- **Summary:** [0010](0010-site-ui.md) §2 is the one tabs table: Page data on the thread row, Talk on the document-page row. §7 refers to it instead of carrying a second table, which had lost Talk from the document-page row. (PENDING F15)

Replaced text (§7):

> | Page kind | Tabs |
> |---|---|
> | Document page | Read, Edit, Page data, History, Links here |
> | Thread | The thread (0019 §8), Page data, History |

### A14. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§15
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [06](../architecture/06-statements-and-properties.md), [09](../architecture/09-security-and-moderation.md), [10](../architecture/10-pages-and-content-models.md), [14](../architecture/14-discussions.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A15. Two row classes of `view.page_category`, one writer each

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §6
- **Change:** amends §3
- **Summary:** With expansion on, membership is read from the expanded text *as well as* the stored text, and the two readings are two row classes of `view.page_category` with one writer each, as the links projection has: every row carries `from_render`; the page projection writes the memberships the stored text declares, with `from_render = false`, when it applies the record; the refresh job of [0042](0042-template-expansion-and-parsoid.md) §10 writes those the expanded text adds, with `from_render = true`. Each writer owns its class exclusively, a category page lists the union, the refresh job never deletes a row it did not write, and turning `wikitext.expansion` off truncates the render-owned rows alone, leaving every membership the text itself declares. 0083's table names §2 (asserted and projected statements); the chapter section that changed, [10](../architecture/10-pages-and-content-models.md) §5.3, holds §3, so the row is corrected. (REVIEW G13)

Replaced text ([10](../architecture/10-pages-and-content-models.md) §5.3, as it stood):

> With expansion on ([0042](../decisions/0042-template-expansion-and-parsoid.md) §9; [11](../architecture/11-rendering-templates-and-modules.md)), membership is read from the **expanded** text: categories that templates emit count, `<includeonly>` categories reach transcluding pages, and expansion adds MediaWiki's tracking categories for its own conditions.

### A16. Help pages carry statements

- **Date:** 2026-10-29
- **Source:** [0096](0096-adopting-any-mediawiki.md) §3
- **Change:** extends §1
- **Summary:** Help (12) is a document namespace (0096 §3), so its pages carry statements, as Project's do. The chapter text is [06](../architecture/06-statements-and-properties.md) §5.1.

### A17. A page title names a page; an entity's page is in its namespace

- **Date:** 2026-10-31
- **Source:** Direct: James, design discussion of 2026-10-31
- **Change:** extends §8
- **Summary:** The Action API took a bare entity ID given as a page title (`action=edit&title=Q42`, `action=delete&title=Q42`, `titles=Q42`) for the entity's page, so an article titled `Q42`, which §8 allows, could not be reached through the API; and since the check went by an ID's shape, every main-namespace title beginning with three letters (`Stub`, parsed as the foreign ID `STUb`) was refused as an entity, which no wiki noticed only because none had articles yet. James: a bare `Q42` is valid where an entity is being identified, as a concept, "but insufficient when talking about an on-wiki page. Consider the distinction in example.wikibase.cloud/entity/Q6 and example.wikibase.cloud/wiki/Item:Q6. Q6 is the name of the entity, but when identifying it as a wiki page, it's Item:Q6." So a parameter that names a page takes a page title: a bare title is in the main namespace, and an entity's page is its namespace's title, `Item:Q42`; a parameter that names an entity (`ids`, `entity`, `/entity/Q42`, `Special:EntityPage/Q42`) takes the bare ID as before. A title names an entity's page when its namespace holds entities, by any of the tenant's names for it, and its ID is one the tenant serves. "On a Wikidata-style wiki where items are the main namespace, it would not coincide with titled pages, so you would be able to do strict enforcement in that context": the same rule makes a bare ID the entity's page there, since no article can share its title. The chapter text is 10 §3.3.
