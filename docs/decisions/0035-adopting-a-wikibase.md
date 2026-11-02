# 0035. Adopting an existing Wikibase as a tenant

- **Status:** Proposed
- **Date:** 2026-09-28
- **Updated:** 2026-11-02 (A16)
- **Author:** James Hare / Claude Fable
- **Changes:** [0002](0002-source-graphs-and-mass-ingest.md), [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0008](0008-namespaces-and-document-pages.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0018](0018-tenants.md)
- **Uses:** [0024](0024-subsidiary-accounts.md), [Wikibase contract](../api/wikibase-compat.md)

## Context

The first deployment is Librarybase: a MediaWiki+Wikibase with about 350,000 entities whose `Q` and `P` numbers are cited outside it, in URLs, in citation data and, once other tenants read it, as `LBQ` and `LBP` ([0018](0018-tenants.md) §5); whose user IDs attribute its history; and whose revision IDs are in `oldid=` permalinks.

[0018](0018-tenants.md) §1 and §5 settle what those numbers *mean* on Triplespace: the tenant's `local` partition is the source of provider `LB`; `Q6` at home is `LBQ6` to every other tenant and instance; the rewrite happens on read; a provider the instance itself hosts has no mirror partition and no sync job. Nothing here changes that.

What no ADR gives is a way to *write* `Q6` into the tenant's `local` partition with that number. The three ways into a local partition all allocate:

- the editor path, where `wbeditentity` with `new=item` mints the next ID;
- bulk `create` ([0002](0002-source-graphs-and-mass-ingest.md) §8.5), where the server allocates IDs and the caller names new entities by temporary refs and match keys;
- wholesale replacement, allowed only against an explicit base revision ([0002](0002-source-graphs-and-mass-ingest.md) §8.2), which a first load has no base for.

[0018](0018-tenants.md) §10 moves a tenant between two Triplespace instances, as an export bundle whose headers already carry every ID; a MediaWiki wiki has no log, no headers and no key chain. [0008](0008-namespaces-and-document-pages.md) §9 imports *pages* from a MediaWiki XML export with full history and original attribution, and says nothing about entities.

One alternative needs no new operation and was rejected: load the old wiki into `mirror/librarybase` as if it were any foreign Wikibase, and rely on the read rewrite of 0018 §5 to show `LBQ6` as `Q6` at home. It puts the wiki's own data in a mirror partition, which 0018 §5 says a hosted provider tenant never has; every later edit becomes an overlay on a mirror of oneself; and it still needs a materialize-into-local step, which is the operation this ADR defines under another name.

## Decision

### 1. Adoption (extends 0018 §1 and §5)

*Changed by A11, A15.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.1, §8.3.*

### 2. Preconditions and the job

*Changed by A2, A11, A12, A13, A15.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.2.*

### 3. The `adopt` operation (amends 0002 §8.2 and §8.5)

*Changed by A4, A6, A7, A12, A14.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §3.2, §3.9, §7.3.*

### 4. Sequence floors (extends 0013 §6; amends 0015 §2 and 0007 §3)

*Changed by A12.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.4.*

### 5. Accounts (amends 0007 §3 and 0016 §3; uses 0018 §4 and §10)

*Changed by A5, A8, A10, A16.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §2.5.*

### 6. Provenance and log events (amends 0011 §6.1; extends 0012 §5)

*Changed by A12, A13.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.2, §2.3.*

### 7. History

*Changed by A12.*

*Current text: [05](../architecture/05-providers-and-ingest.md) §7.5, §8.3.*

### 8. Crates (amends 0005 §2 and §7)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1, §3.1.*

## Consequences

- **Librarybase keeps every number.** Entity, page, revision, log and user IDs survive, and every external citation of `Q6`, `pageid=12` or `oldid=41877` keeps its meaning.
- **Adoption is a one-way door.** The source must be read-only before the final dump, and afterwards the wiki changes only through edits here. There is no second adoption and no sync.
- **Floors leave gaps.** The first entity created after adoption takes a number above the source's counter, not above its highest surviving ID. That is the same rule Wikibase itself follows.
- **Adopted accounts exist without holders.** Every source user has a tenant account and a contributions page from the day of adoption, and none can log in until reclaimed. This is the same state a moved tenant's accounts are in under [0018](0018-tenants.md) §10.
- **Current-state adoption forecloses in-order history** (§7). A tenant that wants its history in `local`, revision by revision, must adopt with the history dump in hand.
- **The history comes with the wiki** (A12). Every revision the source kept arrives in the one job under its own number, time, author and summary, so histories, contributions, diffs and `oldid=` links read as they did on the source from the first day. An adoption writes one record per revision rather than one per entity, and takes longer by that ratio.
- **What the source hid stays hidden, and what it deleted stays out** (A14). A revision-deleted part is absent from the record and shown as suppressed; a deleted page is not adopted, and its numbers are still never reused.
- ~~**The log-granularity residue of 0005 narrows.** `adopt` is the whole-entity write to the local graph; whether an ordinary edit may replace an entity against a base revision remains open there.~~ *`create-or-add` with `overwrite` and a base revision replaces an entity; `adopt` is the one whole-entity write that needs no base revision (A6).*

## Open questions

- **Q1.** ~~**History for tenants that have not yet adopted** (§7): whether (a) or (b) is the default, or whether both are offered as modes of one job.~~ *Settled by A12: (a), history first and in order, is the default; `--current-only` keeps the current-state adoption, with (b) as its later path.*
- **Q2.** ~~**Pages in the same job.** Whether an adoption should carry the source's document pages ([0008](0008-namespaces-and-document-pages.md) §9) in the same run, sharing the page-ID floor, or leave them to a second job.~~ *Settled by A13: the same job, every page of the dump, keeping its page and revision IDs.*
- **Q3.** ~~**Vouching.** Whether a still-running source wiki can act as an identity provider for reclaiming, which needs an OAuth issuer on the source, or whether the manual `reclaim/reclaim` path is the only one a MediaWiki source offers.~~ *Settled by [0088](0088-external-identity-providers.md) §8: it can, through an issuer that reaches its users, `local` on the source itself or `central` checked with `usprop=centralids`, while `reclaim.vouching` names it.*
- **Q4. Sitelinks.** A source's `wb_items_per_site` rows are site IDs and titles; they become URLs through the site aliases of [0026](0026-sitelinks.md) §2, and a site ID the registry does not know is a reject. Whether adoption should register unknown site IDs as aliases on the fly is open.

## References

- [0018 — Tenants](0018-tenants.md) §5 (a tenant as a provider) and §10 (moving a tenant)
- [0008 — Namespaces and document pages](0008-namespaces-and-document-pages.md) §9 (importing pages)
- [Manual:Importing XML dumps](https://www.mediawiki.org/wiki/Manual:Importing_XML_dumps) and [Manual:Log actions](https://www.mediawiki.org/wiki/Manual:Log_actions) (`import/upload`, `import/interwiki`)
- [Wikibase `wb_id_counters`](https://www.mediawiki.org/wiki/Wikibase/Schema/wb_id_counters)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-28
- **Source:** [0005](0005-crate-organization.md) §2, §7
- **Change:** supersedes §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed; its §7 build order places the adoption job (0005 A34).

Replaced text (§8):

> | Crate | Change |
> |---|---|
> | `scatter-wikibase-changeset` | The `adopt` operation and its source fields (§3) |
> | `scatter-ingest` | The adoption job: preconditions, idempotent resume, floors, actor records (§2, §4, §5) |
> | `scatter-adapter-wikidata` | Adoption mode: reading a MediaWiki+Wikibase dump or API parameterised by the tenant's own provider entry, emitting IDs in home form and rewriting only the source's entity-source prefixes (§3) |
> | `scatter-log-postgres`, `triplespace-db` | The entity-ID sequences; setting floors; an explicit page ID on append (§4) |
> | `scatter-mwlog` | `import/upload` and `import/interwiki` for entities (§6) |
> | `triplespace-rdf` | `prov:wasRevisionOf` and `pav:importedOn` on adopted records (§6) |
> | `triplespace-cli` | `instance create --adopt --owner`; `adopt` (§2, §5) |
>
> No crate is added. In the build order of [0005](0005-crate-organization.md) §7, the adoption job sits in step 4 beside the mirror bootstrap: a tenant continuing an existing Wikibase needs it before step 5.

### A2. Adoption brings the wiki's files

- **Date:** 2026-09-30
- **Source:** [0039](0039-files-and-media.md) §14
- **Change:** extends §2
- **Summary:** Adopting a wiki with files imports every file version from its file tables and upload directory, deleted versions included, onto file pages that keep their source page IDs.

### A3. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §2, §8
- **Summary:** A1–A2 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A2 was a blockquote. The file before conversion is commit `0b26a3a`.

### A4. Unregistered entity sources

- **Date:** 2026-10-08
- **Source:** [0078](0078-entity-sources.md) §11
- **Change:** extends §3
- **Summary:** An adopted wiki's entity-source prefix that names no registry provider is declared as an entity source of the tenant under the same name, and its IDs are kept as written. A prefix whose name the registry already has fails the preconditions. Before this, such a prefix had no rule.

### A5. Hidden source accounts get the `actor` ACL

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** For a source account whose name is hidden, adoption writes the `actor` ACL restricted to `suppress` beside the actor record, so that `hidden` has one origin, the ACL of [0007](0007-actor-identity.md) §4, on an adopted tenant as on any other. (PENDING C2)

### A6. `adopt` needs no base revision

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** `adopt` is "the one whole-entity write that needs no base revision", not the one whole-entity write to the local graph: `create-or-add` with `overwrite` and a base revision still replaces an entity whole ([0002](0002-source-graphs-and-mass-ingest.md) §8.2). The consequence that said the residue of 0005 narrows is struck. (PENDING E4)

Replaced text (§3):

> This also narrows the residue [0005](0005-crate-organization.md) leaves open on log granularity: `adopt` is the one whole-entity write to the local graph, and it is confined to adoption jobs.

### A7. Lexemes are adoptable

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §3
- **Summary:** Strike "once the Lexeme namespace is implemented": `L` is adoptable, since [0066](0066-lexemes.md) decided lexemes. (PENDING E7)

Replaced text (§3):

> Any minted entity type the source has may be adopted (`Q`, `P`, and `L` once the Lexeme namespace is implemented).

### A8. `reclaim/reclaim` is a log event

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §5
- **Summary:** The `reclaim/reclaim` event §5 names is a row of [0011](0011-logs.md) §6.1's table, beside `takedown/expunge` and the no-redirect move: it projects from the reclaiming binding record in the tenant `log` and is visible to everyone. (PENDING E27)

### A9. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [05](../architecture/05-providers-and-ingest.md), [07](../architecture/07-actors-and-accounts.md), [16](../architecture/16-logs-feeds-and-notifications.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A10. Adopted accounts under the issuer code

- **Date:** 2026-10-09
- **Source:** [0079](0079-derived-issuer-codes.md) §6, §9
- **Change:** amends §5
- **Summary:** An adopted wiki's accounts are written under the adopting tenant's issuer code, derived from its founding record ([0079](0079-derived-issuer-codes.md) §1), and their own numbers: the source's user 42 is `{code}:42`. Where the registry already knows the source wiki as an issuer, its entry gains `tenant`, the adopting tenant's code, and the registered key becomes an input form of the tenant's: `librarybase:42` is read as `{code}:42`.

Replaced text (§5, in [07](../architecture/07-actors-and-accounts.md) §2.5):

> …so the source's user 42 is `{slug}:42`.

### A11. No adoption beside a live mirror of the same code

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §1, §2
- **Summary:** An instance refuses a sync job, shallow or full, for a provider whose registry `issuer` is a tenant of this instance: that tenant's `local` partition is the provider's graph, read directly, and a mirror of it would be a stale copy. The adoption job gains a fifth precondition: no live mirror partition exists for the tenant's code. Where the instance had been mirroring the source as a registry provider (`mirror/librarybase` for `LB`), that partition is frozen first, its `entity_source` rows dropped and composition switched to reading the tenant's `local` partition, so that readers see one graph for the code and not a mirror beside the tenant; adoption under a code with a live mirror partition is refused until then. Whether `LB` is a remote wiki to sync or a tenant here is therefore instance configuration, read from the registry's `issuer`. The ledger row names [0018](0018-tenants.md) §5, [0080](0080-tenants-as-entity-sources.md) §3 and [0070](0070-shallow-entity-mirroring.md) §7; the precondition is this ADR's, folded in [05](../architecture/05-providers-and-ingest.md) §7.2 and §8.3, and nothing is contradicted. (REVIEW G31)

### A12. Adoption carries the source's history

- **Date:** 2026-10-19
- **Source:** Direct: James, design discussion of 2026-10-19
- **Change:** amends §2, §3, §4, §6, §7; retitles §7
- **Summary:** An adoption brings the source's full revision history by default, as shape (a) of §7. Its input is a full-history XML export, and every revision of every entity page becomes one `adopt` record in `local`, in source-revision order within the page. The record's revision ID, header field 7, is the source's own, below the revision floor, so none is taken from the tenant's sequence. Its content adds `source_parentid` and `minor` to `source_revid`, `source_time` and `source_pageid`; its comment part is the revision's edit summary; its attestation names the revision's author with the job beside it: the adopted account `{code}:{n}`, a surrogate `actor:{n}` ([0007](0007-actor-identity.md) A27) for an IP or `imported>Name` author, or no actor and the `hidden` marker for a user the source hid. A revision that made the page a redirect carries `redirect` in place of `entity`, so a merged entity redirects as it did on the source, and a later revision that restores it carries `entity` again. The newest revision is the current state, with no adoption revision after it, so `lastrevid` is the source's and every `oldid=` the source issued resolves here. An `adopt` follows only an `adopt` of the same entity with a lower source revision ID; a re-run skips a revision present with the same content and rejects one present with other content. Each adopted revision is an activity row at its source time, so it stands in the past, and no addressing rule delivers one. A dump without history (every page one revision while revisions name parents) is refused unless the operator passes `--current-only`, which keeps the current-state adoption: one `adopt` per entity under a fresh revision ID, attributed to the job's subsidiary, with (b) as the later path to history. The job record carries `history` (`full` or `current`). Librarybase adopts with its history. The chapter text is [05](../architecture/05-providers-and-ingest.md) §3.2, §3.9, §7.2–§7.5 and §8.3, and [16](../architecture/16-logs-feeds-and-notifications.md) §2.2, §2.3 and §3.1; the change to revision-ID assignment is logged as [0015](0015-record-format-and-partition-registry.md) A62 and [0013](0013-postgres-storage.md) A68.

Replaced text ([05](../architecture/05-providers-and-ingest.md) §7.3, as it stood):

> - **`source_revid`, `source_time`, `source_pageid`** are the source's revision ID and timestamp for the adopted state and the page ID of the entity's page on the source. They are content, kept in the content part, and they give the record its provenance ([0035](../decisions/0035-adopting-a-wikibase.md) §6; [07](../architecture/07-actors-and-accounts.md)) and its page ID (§7.4).

> The record is an ordinary local revision: it takes the next revision ID from the tenant's sequence ([0015](../decisions/0015-record-format-and-partition-registry.md) §2), its comment part is empty, and its attestation names the job's subsidiary and the job ([0015](../decisions/0015-record-format-and-partition-registry.md) §1). The entity's `lastrevid` after adoption is that new local revision ID; `source_revid` is shown beside it as "adopted from revision N".

> **Re-running** an adoption job against the same dump skips every entity already present and is therefore how a partial adoption is completed. A newer dump is not a second adoption: once adopted, the wiki changes through edits.

Replaced text ([05](../architecture/05-providers-and-ingest.md) §3.2, as it stood):

> | `adopt` | Local | Writes an entity's whole state as its first local record, under the ID the source minted. Accepted only from an adoption job (§7.2). An entity already present is skipped when the content hash matches and rejected otherwise; `adopt` never updates. |

> `adopt` is accepted only from an adoption job on a tenant that is that wiki continuing on Triplespace, on a `local` partition that holds no other entity records; it never updates, and re-running it skips an entity whose content hash matches and rejects one that differs.

Replaced text ([05](../architecture/05-providers-and-ingest.md) §7.4, as it stood):

> | **Revision IDs** (`log.revision_id`). Adopted records take fresh IDs above the floor | The source's highest revision ID | No local revision ever shares a number with a source revision, so `oldid={source revid}` resolves to the source revision once history is imported (§7.5) and to nothing before, never to an unrelated local revision |

Replaced text ([05](../architecture/05-providers-and-ingest.md) §7.5, "History: shape fixed, content deferred", as it stood):

> Adoption brings **current state only**. What fixes the shape of a later history import is a constraint, not a preference: a later job must not displace the current state.

> - **(a) History first, in order.** Each source revision is an `adopt` record with an explicit revision ID (the source's, below the floor), its original timestamp and its original actor, as [0008](../decisions/0008-namespaces-and-document-pages.md) §9 steps 1–2 do for pages, and the last revision *is* the current state. It needs the XML history dump at adoption time, and it makes adoption one job.

> Adopting current state only forecloses (a) for that tenant. Librarybase accepts that: its first milestone is API-only and current-state, and (b) is then its path.
>
> Not yet: the choice between the two shapes for tenants that have not yet adopted is open in [0035](../decisions/0035-adopting-a-wikibase.md).

Replaced text ([05](../architecture/05-providers-and-ingest.md) §8.3, as it stood):

> its first milestone is API-only and current-state, with history as a later backfill (§7.5).

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §3.1, as it stood):

> An adopted entity's first record is an `edit` row with `new` set, the `bot` flag, the `job:{id}` tag and an `adopt` tag ([0035](../decisions/0035-adopting-a-wikibase.md) §6);

### A13. Every page comes in the same job

- **Date:** 2026-10-19
- **Source:** Direct: James, design discussion of 2026-10-19
- **Change:** amends §2; extends §6
- **Summary:** The adoption job carries every page of the dump, not only entity pages. A page outside the entity namespaces is imported by the page-import procedure of [0008](0008-namespaces-and-document-pages.md) §9 within the job, into `pages`, keeping its source page ID and, revision by revision, its source revision IDs, attributed as entity revisions are (A12); one revision sequence then serves both, as it did on the source. Precondition 3 applies to each partition the job writes, `local` for entities and `pages` for pages, so a job that adopted the entities can be resumed to carry the pages while no page has been edited. A page the job cannot import is counted as `deferred`. The `import/*` event is written for the first record of an adopted page as for an adopted entity. The chapter text is [05](../architecture/05-providers-and-ingest.md) §7.2 and [16](../architecture/16-logs-feeds-and-notifications.md) §2.3; the page-import side is logged as [0008](0008-namespaces-and-document-pages.md) A38.

Replaced text ([05](../architecture/05-providers-and-ingest.md) §7.2, as it stood):

> 3. **the tenant's `local` partition holds no entity records** other than those written by earlier adoption jobs for the same source. This is what makes a failed adoption resumable (§7.3) and everything else an ordinary edit;

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §2.3, as it stood):

> the first record of an adopted entity, as `import/upload` from a dump or `import/interwiki` from the live API, never as `create/create`;

### A14. Parts the source hid

- **Date:** 2026-10-19
- **Source:** Direct: James, design discussion of 2026-10-19
- **Change:** extends §3
- **Summary:** A revision the source revision-deleted reaches a history export with its text, comment or contributor replaced by `deleted="deleted"`, and the export does not say whether it was deleted or suppressed. The adopted record carries nothing for a withheld part, and the `adopt`'s `withheld` names the parts it lacks, as a `record` ACL names parts: `content`, so the `adopt` has neither `entity` nor `redirect` and the revision has no state for `oldid` to show to anyone; `comment`, so the comment part is empty; `attestation`, so it names no actor. Beside each such record the job writes a `record` ACL with those parts restricted to `suppress`, and the history views show them as suppressed. The current revision's text is never hidden on MediaWiki, so the current state is never withheld. Pages the source deleted are not in the export and are not adopted; the floors still cover their IDs. Adoption reads the export only, not the source's `archive` table or revision-deleted content. The chapter text is [05](../architecture/05-providers-and-ingest.md) §7.3.

### A15. Any MediaWiki, and the source's namespaces

- **Date:** 2026-10-29
- **Source:** [0096](0096-adopting-any-mediawiki.md) §1, §2
- **Change:** extends §1, §2
- **Summary:** A source without Wikibase may be adopted: it has no entities, so the job writes nothing to `local`, takes no `--counters` and sets no entity floor, and adopts its pages, accounts and files as any source's (0096 §1). The job registers the source's namespaces: a number the registry lists for a `pages` namespace keeps its pages there; a number it does not list becomes a tenant `namespace` record with its talk number; a number it lists for a namespace that holds no pages is archived or deferred. A taken name is renamed or archived into Project under a prefix, by `--namespace`, and the survey pass refuses the job until every taken name is decided (0096 §2). The chapter text is [05](../architecture/05-providers-and-ingest.md) §7.1, §7.2 and §7.6.

### A16. The source's accounts must agree with the tenant's

- **Date:** 2026-11-02
- **Source:** Direct: Claude, while building the Librarybase full-history adoption check, 2026-11-02; unconfirmed
- **Change:** extends §5
- **Summary:** Before it writes anything, the survey pass compares the dump's accounts with the accounts the tenant already has (in practice the owner that `instance create` made). It refuses the job when a source account's name is held by an account under another user ID, or when a source user ID is held under another name, and names each disagreement. Account names are unique on a tenant, as on MediaWiki, and the job leaves an account it finds present as it is, so either case would leave two accounts with one name or the source's account under the wrong name. The Librarybase run of 2026-11-01 met the first case: the owner was created under another user ID with the name of the source's account `Librarybase`, and the two same-named accounts stopped the `addressing` projection at the user page. `--no-accounts` skips the check with the accounts. The chapter text is [07](../architecture/07-actors-and-accounts.md) §2.5.
