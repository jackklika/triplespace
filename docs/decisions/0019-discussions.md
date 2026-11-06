# 0019. Discussions

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-05 (A31)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md)
- **Uses:** [0006](0006-log-integrity-and-erasure.md), [0007](0007-actor-identity.md), [0018](0018-tenants.md), [0022](0022-federation.md)

## Context

[0008](0008-namespaces-and-document-pages.md) §2 reserved a talk namespace beside every subject namespace and left discussions to their own ADR. This is that ADR.

The design principle of 0008 is that a namespace's kind says what a page is made of. `Item:` and `Property:` are composite views over the resolved graph; `Project:` and `User:` are blobs of stored text. A talk page is a third thing: **a composite view over threads**. Each thread is its own page with its own history. A post refers to the post it answers. A thread is attached to one talk page and can be moved to another. The talk page's history is the histories of its threads, presented as one.

MediaWiki has tried this shape twice. **LiquidThreads** put each thread in a `Thread:` namespace and moved threads between talk pages. **Flow** (Structured Discussions) put each topic on a `Topic:` page, made the talk page a board of topics, and let topics move between boards. Both are gone, and the reasons are design inputs here: topics had opaque UUID titles and URLs nobody could cite; the board's history was hard to read; watching a board and watching a topic did not compose; and because the content was not text, the rest of the wiki's tooling (history, diffs, contributions, backlinks, search) treated it as foreign. **DiscussionTools**, which is what Wikimedia wikis use now, keeps talk pages as wikitext and overlays structure by parsing signatures and indentation. That is not available here: Triplespace has no wikitext parser beyond the subset of 0008 §8, and it does not want its discussions to depend on `~~~~`.

Two goals from the earlier ADRs constrain the design. Every unit of contribution has to be a record with a global revision ID ([0015](0015-record-format-and-partition-registry.md) §2), so that history, contributions, recent changes, diffs, permalinks, hiding and erasure all work without anything new. And the instance loads *data*, not the discussion behind it: foreign talk pages are never mirrored.

[0011](0011-logs.md) §8 already types local log events with Activity Streams 2.0. Posts and threads are described with the same vocabulary.

## Decision

### 1. A thread is a page; a post is a record

*Changed by A4, A10, A14, A15.*

*Current text: [14](../architecture/14-discussions.md) §1.1, §1.2, §3.6.*

### 2. Talk pages are composite; attachment is by identifier (amends 0008 §2)

*Changed by A6, A9, A10, A12, A15, A23.*

*Current text: [14](../architecture/14-discussions.md) §1.3, §3.5.*

### 3. The `Thread` namespace and thread titles (extends 0008 §1 and §3)

*Changed by A9, A10, A13, A23, A26.*

*Current text: [14](../architecture/14-discussions.md) §1.4.*

### 4. The record: four parts (amends 0015 §1)

*Changed by A10, A13, A15, A17, A18, A19, A23, A24, A25, A26, A28.*

*Current text: [14](../architecture/14-discussions.md) §1.5, §3.6, §4.2.*

### 5. Text: markdown, mentions and links (extends 0008 §5, §8 and §10)

*Changed by A8, A13, A24, A25, A28.*

*Current text: [14](../architecture/14-discussions.md) §1.6.*

### 6. Status and visibility

*Changed by A3, A5, A7, A8, A13, A20, A21, A25, A26.*

*Current text: [14](../architecture/14-discussions.md) §2.1, §2.2, §2.3, §2.4.*

### 7. History, activity, logs and RDF (extends 0001 §1 and §6, 0011 §6.1 and §8)

*Changed by A4, A8, A10, A27, A29.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §2.3, §6.1, §6.3.*

### 8. Rendering and the UI (extends 0010)

*Changed by A2, A10, A15, A28, A30, A31.*

*Current text: [19](../architecture/19-site-ui.md) §6.1.*

### 9. API (extends 0012 §4 and §5)

*Changed by A4, A9, A10, A16, A30, A31.*

*Current text: [18](../architecture/18-api.md) §2.1, §2.3, §2.4, §3.2.*

### 10. The Activity Streams profile

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §6.2.*

### 11. Storage, caches and search (extends 0013 §5 and 0014)

*Changed by A9, A10.*

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.8, §5, §6.1, §11.1, §12.1.*

### 12. Permissions (extends 0016 §2; amends 0016 §4)

*Changed by A4, A10.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §4.4, §8.1.*

### 13. Crates (amends 0005 §2)

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- **A post is a revision, so everything that works on revisions works on posts.** History, diffs, contributions, recent changes, permalinks, hiding, erasure, inclusion proofs and export bundles need no discussion-specific code. This is the property Flow and LiquidThreads lacked.
- **Talk pages are not text.** Bots that edit, archive or template talk pages do not work; `action=edit` tells them so. Existing wikitext talk pages cannot be imported as they are (Q5).
- **The three-part uniformity of 0015 is loosened.** One payload type has four parts. Verification, erasure and the storage schema were already written for *n* parts, so the cost is one sentence and one more `parts` value.
- **Thread names are readable and stable across moves, and renames do not redirect.** A link to a renamed thread breaks, as a link to a renamed user page does. Permalinks by revision or page ID do not.
- **Status is social.** Anyone may mark a thread resolved, and anyone may reopen it, in public, with attribution. The instance decides only what the marks are called and how the UI folds them.
- **No archives, no `/Archive N` subpages, no lost history.** Old threads are collapsed, not moved.
- **Discussion stays local.** A mirrored entity's talk page is this wiki's; it is never fed upstream or to another tenant, and no upstream discussion is fetched.
- **The AS2 vocabulary costs nothing now and keeps federation possible later.**

## Open questions

- **Q1.** ~~**Namespace numbers** for `Thread` and `Thread talk` (registry README, pending allocations).~~ *Settled by `docs/registry/namespaces.toml`, under the numbering policy of 0008 A6: 214/215.*
- **Q2. Temporary accounts.** Whether a `temp` account gets a `User talk` page for messages, as MediaWiki gives one. [0021](0021-notifications.md) asks the same of their notifications.
- **Q3.** ~~**Notifications**: whether `[[User:Example]]` in a post notifies that user, and what a reply to one's post delivers. Watching is settled by [0020](0020-change-feeds.md).~~ *Settled by [0021](0021-notifications.md) §2: `mention`, `reply`, `talk` and `thread-status` reasons.*
- **Q4. DiscussionTools compatibility.** Whether to implement `discussiontoolsedit` and `discussiontoolspageinfo` on these records, and how far Convenient Discussions and similar scripts can be made to work.
- **Q5.** ~~**Importing existing talk pages.** A Wikibase's `Item_talk:` and a project's `Project_talk:` pages are wikitext. One option is a job that splits a page by `==` heading into threads with one post each, attributed to the section's signatures where they parse and to an `imported` surrogate otherwise.~~ *Settled by [0054](0054-forking-a-mirrored-page.md) §5: split by level-two heading with the frontmatter first, one wikitext post per section attributed to the importer and the job, each thread created `archived` and dated from its earliest signature.*
- **Q6.** ~~**Cross-tenant discussion.** Whether a tenant may show, read-only, another tenant's threads about an entity it mirrors from that tenant.~~ *Settled by [0028](0028-tenancy-policy.md) §6: by policy, read-only.*
- **Q7.** ~~**Federation.** Inbox, outbox and actor keys, if ever.~~ *Settled by [0022](0022-federation.md) Part B: opt-in `Person` actors, talk pages as `Group` actors that `Announce` threads, inbound replies as posts by `federated` surrogates; the inbound protocol details stay open there.*
- **Q8.** ~~**Limits.** The post size limit, the depth default, and rate limits on posting.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5, as to where they live: the size limit and `thread.max_depth` are `site` settings (§5, §8); posting is rate-limited in the `edit` and `create` classes. The values are tuning.*

## References

- [Extension:LiquidThreads](https://www.mediawiki.org/wiki/Extension:LiquidThreads) and [Extension:StructuredDiscussions](https://www.mediawiki.org/wiki/Extension:StructuredDiscussions) (Flow)
- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools) and [API:Discussiontoolsedit](https://www.mediawiki.org/wiki/Extension:DiscussionTools/API)
- [Extension default namespaces](https://www.mediawiki.org/wiki/Extension_default_namespaces)
- [Activity Streams 2.0](https://www.w3.org/TR/activitystreams-core/) and the [Activity Vocabulary](https://www.w3.org/TR/activitystreams-vocabulary/)
- [ActivityPub](https://www.w3.org/TR/activitypub/)
- [CommonMark](https://commonmark.org/)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §13
- **Summary:** 0005 §2 is the one crate table CI checks, and carries `scatter-threads` and every change this section listed (0005 A15).

Replaced text (§13):

> | Layer | Crate | Contents | Depends on |
> |---|---|---|---|
> | Substrate | `scatter-threads` | The `scatter:v0/thread` payload type and its four parts; the fold from records to thread state (tree, subject, attachment, status); title minting (§3); validation of operations against thread state. Pure | `scatter-log`, `scatter-pages` |
> | | `scatter-markdown` | Gains wiki links and the HTML sanitizer (§5), for every use of the model | — |
> | Triplespace | `triplespace-titles` | The `thread` and `composite` namespace kinds and talk-title resolution (§2–3) | as 0008 §11 |
> | | `triplespace-projections` | `view.thread`, `view.post`, `view.talk_page`, and `page_link` rows from posts | as 0013 |
> | | `triplespace-rdf` | The AS2 nodes of §7 | as 0013 |

### A2. Watching

- **Date:** 2026-09-27
- **Source:** [0020](0020-change-feeds.md) §3
- **Change:** extends §8
- **Summary:** Watching a subject watches its talk page and the threads attached to it; a thread can be watched on its own. §8 had been given the sentence in place.

### A3. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §2
- **Change:** extends §6
- **Summary:** `mention`, `reply`, `talk` and `thread-status` reasons; the `thread-status` notification fires when a thread moves from an open to a closed status. This settled Q3.

### A4. Deletion is an ACL; posts are patrollable

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §4, §6
- **Change:** amends §1, §7, §12; extends §9
- **Summary:** `delete` and `undelete` are no longer thread operations. Deleting a thread is a `read` ACL on its page ID, written to the tenant `log` partition, and undeletion retires it; hiding one post is a `record` ACL on that post's record with `parts: [text]`. The `thread` payload type carries `create`, `post`, `edit`, `rename` and `move`, and the sentence below about "any operation on a deleted thread other than `undelete`" reads as "any operation on a deleted thread". Posts and thread operations are patrollable.

Replaced text (§1):

> | `create` | Mints the thread's page ID and title, attaches it to a talk page, and is the opening post | The opening post | — |
> | `post` | A post in the thread, top-level or in reply to another post | The post | — |
> | `edit` | Replaces the **complete text** of one post | The new text | Required |
> | `rename` | Changes the subject, and so the title (§3) | — | Required |
> | `move` | Attaches the thread to a different talk page (§2) | — | Required |
> | `delete` | Hides the thread and every post in it from everyone but administrators, as 0008 §4 hides a page | — | Required |
> | `undelete` | Reverses a `delete` | — | Required |
>
> **Replies need no base offset.** Two people answering the same post at once both succeed. `edit`, `rename`, `move`, `delete` and `undelete` carry the base offset of [0006](0006-log-integrity-and-erasure.md) §8, and a mismatch is an `editconflict`. A `post` whose parent is not a post of the same thread, and any operation on a deleted thread other than `undelete`, is rejected.

Replaced text (§7):

> **Log events**, projected from the records as 0011 §6.1 requires: `create` → `create/create`; `rename` and `move` → `move/move`, with the old and new title or the source and target talk pages as parameters; `delete` and `undelete` → `delete/delete` and `delete/restore`. `post` and `edit` are revisions, not log events.

Replaced text (§12):

> | `delete`, `undelete` | `delete`, `undelete` | `sysop` |

### A5. Visibility as a preference

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1
- **Change:** amends §6
- **Summary:** The visibility rule of §6 is the `threads.visibility` preference.

Replaced text (§6):

> **Visibility is a viewer preference, not data.** Each thread on a talk page is shown *visible*, *collapsed* or *hidden*, chosen by a rule over its status and the age of its last post. The instance sets the defaults (for example: `resolved`, `rejected` and `stale` collapsed; anything quiet for ninety days collapsed; nothing hidden), and a viewer overrides them.

### A6. Cross-tenant discussion

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §6
- **Change:** extends §2
- **Summary:** A tenant may show, read-only and by policy, a provider tenant's threads about an entity it mirrors. This settled Q6.

### A7. Thread statuses are configuration

- **Date:** 2026-09-27
- **Source:** Direct: James, review of 2026-09-27 (third pass)
- **Change:** amends §6
- **Summary:** Statuses are `config` records of kind `thread-status` in the tenant `config` ([0015](0015-record-format-and-partition-registry.md) §3), each with a label, a **category** (`open` or `closed`) and a display order; the defaults ship in `docs/registry/thread-statuses.toml` and follow the labels of bug trackers and project tools: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined** and **Stale** (closed). `rejected` is renamed `declined`. The category, not the name, drives the default visibility rule below (closed threads collapse) and the `thread-status` notification of [0021](0021-notifications.md) §2, which fires when a thread moves from an open to a closed status. `open` is required and cannot be retired.

Replaced text (§6):

> The values are registry data, so an instance can add its own; the defaults are `open`, `resolved`, `rejected` and `stale`. A status is therefore a claim made in public,

### A8. Articles and thread statements

- **Date:** 2026-09-29
- **Source:** [0038](0038-page-metadata-and-categories.md) §8–9, §11
- **Change:** amends §5, §7; extends §6
- **Summary:** By section:
  - §5: With articles in the main namespace, `[[Q42]]` links to the main-namespace page titled `Q42`, in markdown as in wikitext. An entity is mentioned with its namespace: `[[Item:Q42]]`, which still renders with the entity's label.
  - §6: Threads carry statements, as document pages do. When the `thread-status` role is bound, a thread's status is also a projected, read-only statement, derived from the latest status-bearing post; this section stays the one place a status is set, and asserting that property on a thread is refused.
  - §7: Except thread statements, which are output in the main graph as page statements are.

Replaced text (§5):

> - **Wiki links.** `[[Title]]`, `[[Title|text]]` and `[[Title#Section]]` are resolved by the title resolver of 0008 §3, as in the wikitext subset. `[[Q42]]` renders with the entity's label, as Wikibase does. This is the mention syntax; it is what every editor already knows.

Replaced text (§7):

> The post's revision node of [0001](0001-revision-metadata-rdf.md) §1 is the same IRI; the AS2 properties are added to it. Nothing about threads enters the main or resolved graph.

### A9. Content models

- **Date:** 2026-09-30
- **Source:** [0041](0041-content-models.md) §4, §8, §10
- **Change:** amends §2, §3, §9, §11
- **Summary:** By section:
  - §2: The kind `composite` is gone: a talk namespace is a `pages` namespace whose model is `triplespace-talk`, a model whose source is *composite* (no records of its own).
  - §3: The kind `thread` is gone: `Thread` is a `pages` namespace whose model is `triplespace-thread`.
  - §9: `action=edit` on a `Thread:` or talk title is refused with MediaWiki's own `no-direct-editing` (`apierror-no-direct-editing`), because neither model supports direct editing. `ts-structured` is retired.
  - §11: The `content_model` values are the registry IDs `triplespace-thread` and `triplespace-talk`, not `thread` and `talk`.

Replaced text (§2):

> **Talk namespaces change kind from `reserved` to `composite`.** The talk page for a target is the ordered set of threads currently attached to it. It has no stored text and no records of its own.

Replaced text (§3):

> **`Thread` is a registered namespace of a new kind, `thread`:** pages composed from thread records. Its paired `Thread talk` is `reserved` and empty, because clients assume every subject namespace has one. Subpages are not allowed. The numbers are 214 and 215, in the Triplespace range of [0008](0008-namespaces-and-document-pages.md) §2 as amended on 2026-09-27; LiquidThreads' 90 and 91 are not reused.

Replaced text (§9):

> `action=edit` on a `Thread:` or talk title is refused with a new error code, `ts-structured`, so that a bot learns at once that these pages are not text.

### A10. Boards: one home, any number of listings

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §2, §5–9, §11–13
- **Change:** amends §1, §2, §3, §7, §8, §12; extends §4, §9, §11
- **Summary:** By section:
  - §1: Two operations join the table: `attach` lists the thread on a further talk page or board, and `detach` removes a listing. Both need a base offset. `create` may carry `also`, further targets listed at creation, and `move` may carry `keep`, which keeps the old home as a listing.
  - §2: A thread has exactly one **home**, the attachment this section describes, and may have **listings** on further talk pages and boards, up to `thread.max_attachments`. "A thread cannot be attached to two pages; cross-posting is a link" is replaced. Only the home encloses the thread, is its `as:context`, and may be a user talk page; listing, composite history and watches treat every attachment alike. A board ([0049](0049-boards.md) §1) is a `page` target whose talk page is itself.
  - §3: `Thread talk` (215) is a `virtual` namespace that forwards to `Thread`: `Thread talk:X` resolves to `Thread:X`. A thread is its own talk page.
  - §4: `also` on `create`; `target` and `talk` on `attach` and `detach`; `keep` on `move`.
  - §7: A talk page's or board's history includes every thread attached to it, home or listing, and the `thread/attach` and `thread/detach` events. A thread's `as:context` is its home, and it carries `scatter:listedOn` for each listing.
  - §8: A talk page lists its threads in the order they were attached to it, which is creation order unless a thread was moved or listed there. A thread shows its home and its listings.
  - §9: `POST /thread/{page ID}/attach` and `/detach`; `also` on `POST /thread` and `keep` on `/move`; `home` and `listings` in `GET /thread/{page ID}`; `home` on each thread of `GET /page/{id}/threads`.
  - §11: `view.thread_attachment` holds every attachment, home or listing, and serves the listing query in place of the `thread_talk` index; `view.thread.talk_page_id` is the home. Boards have `view.talk_page` rows.
  - §12: A talk page encloses the threads whose *home* it is, not those listed on it. `attach` and `detach` need `move` on the thread and `edit` on the target.

Replaced text (§1):

> **A thread is a page in the `Thread` namespace (§3).** It has a page ID from the tenant's sequence ([0015](0015-record-format-and-partition-registry.md) §2, [0018](0018-tenants.md) §2), a subject, an attachment (§2), a status (§6) and a history.

Replaced text (§2):

> **A thread is attached to exactly one target.** The target is named by identifier,
>
> It is minted from the page-ID sequence by the first `create` or `move` record that attaches a thread to that target,
>
> **A `move` moves the whole thread.** Its posts leave the old talk page's history and join the new one's, and one `move/move` log event, with the source and target talk pages as parameters, appears in both pages' logs (§7). A thread cannot be attached to two pages; cross-posting is a link.

Replaced text (§7):

> **A talk page's history is composite:** the rows of every thread currently attached to it, plus the attach and move events, in one sequence ordered by time.
>
> - **A thread** is `{base}/page/{page ID}`, an `as:OrderedCollection` of its posts, with `as:name` the subject, `as:context` the talk page's node, `as:published`, and `scatter:status`.

Replaced text (§8):

> - **A talk page** lists its threads, by default in creation order with the newest last, as MediaWiki readers expect, with a switch to order by last activity. Each thread shows its subject, status, participant count, last activity, and its posts or a collapsed summary per §6.

Replaced text (§9):

> | `POST /thread/{page ID}/rename`, `/move`, `/delete`, `/undelete` | The remaining operations, each with a base offset |

Replaced text (§11):

> CREATE INDEX thread_talk ON view.thread (talk_page_id, created);

Replaced text (§12):

> **Enclosure gains one rule:** a talk page encloses the threads attached to it. Protecting `Item talk:Q42` with `acl:page:{talk page ID}` therefore restricts every thread on it, and a thread moved elsewhere leaves that protection behind.

### A11. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A10 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A4 (in part), A7, A8, A9 and A10 were eighteen blockquotes, and A2, A3, A5 and A6 were recorded in the header or in other ADRs. The file before conversion is commit `0b26a3a`.

### A12. Talk pages of inherited pages

- **Date:** 2026-10-01
- **Source:** [0052](0052-page-repositories-and-title-inheritance.md) §7
- **Change:** extends §2
- **Summary:** A page a repository serves has a local talk page on its provider-ranged page ID, as a mirrored entity has one; the fork job moves those threads to the fork's talk page.

### A13. Imported threads: wikitext posts, closed on creation, dated by the import

- **Date:** 2026-10-01
- **Source:** [0054](0054-forking-a-mirrored-page.md) §5
- **Change:** amends §3, §4, §5, §6
- **Summary:** `status` may appear on `create`; the content part gains `mediaType` (`text/markdown` or `text/x-wiki`, the latter written only by imports and rendered by the wikitext pipeline) and `imported_from`; a thread an import creates takes the date the import supplies for its title; the default statuses gain **Archived** (closed). Q5 settled.

Replaced text (§3):

> The date is the UTC date of the `create` record and never changes.

Replaced text (§4):

> | `status` | `post` | A status value (§6), when the post sets one |

Replaced text (§5):

> **Posts are markdown**, rendered by the `markdown` content model of 0008 §5, with two additions that apply to that model everywhere:

Replaced text (§6):

> **A thread's status is set by posting.** A `post` may carry a `status` value.

> **Resolved**, **Won't fix**, **Declined** and **Stale** (closed).

### A14. Proposal operations

- **Date:** 2026-10-05
- **Source:** [0067](0067-proposals.md) §3
- **Change:** extends §1
- **Summary:** `propose`, `submit` and `withdraw` join the operation table; a `create` may carry `propose`'s fields.

### A15. Followed talk pages and pinned threads

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §3, §4, §7
- **Change:** amends §2; extends §1, §4, §8
- **Summary:** "Foreign talk pages are never loaded" is narrowed to mirrored entities: a page repository with `talk = sync` has its talk pages followed, and their sections are shown as foreign threads beside local ones, as are those of a fork that follows upstream. `pin` and `unpin` join the operations; `create` may carry `pinned`; `imported_from` may carry a section's name hash. Talk pages list pinned threads first and merge foreign threads by time.

Replaced text (§2):

> **Foreign talk pages are never loaded.** The talk page of a mirrored entity holds local threads only.

### A16. `GET /page/{id}/history` is defined in 0012 §5

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** extends §9
- **Summary:** `GET /page/{id}/history` is defined in [0012](0012-api-requirements.md) §5's page table, which took it from §7; §9's route table cites that definition and no longer calls the route "Unchanged", since it is this ADR that gave the route its composite behaviour for a talk page ID. (PENDING E36)

### A17. The Comment part on every operation that carries a summary

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** The Comment row of §4's parts table lists every operation that carries a summary: `edit`, `rename`, `move`, `attach`, `detach`, `pin`, `unpin`, `propose`, `submit` and `withdraw`; it stays null for `create` and `post`, whose text is the contribution. The row had not been extended when A10, A14 and A15 added the operations. (PENDING F5)

Replaced text (§4):

> | 1 | **Comment** | The summary for `edit`, `rename`, `move`, `attach` and `detach`; null for `create` and `post`, whose text *is* the contribution | comment |

### A18. `pin` and `unpin` name any attachment

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §4
- **Summary:** In §4's content-part table, `target` is a `page` target only for `attach` and `detach`; `pin` and `unpin` name any attachment of the thread, an `actor` target included ([0049](0049-boards.md) §6, [0069](0069-synchronized-talk-pages.md) §4). (PENDING F6)

Replaced text (§4):

> | `target`, `talk` | `attach`, `detach`, `pin`, `unpin` | The target (a `page` target only) and its talk page ID, minted by an `attach` if the target had none ([0049](0049-boards.md) §6); for `pin` and `unpin`, the attachment pinned or unpinned ([0069](0069-synchronized-talk-pages.md) §4) |

### A19. A proposal's fields are page metadata set by `propose`

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §4
- **Summary:** A proposal's kind, destination, base, payload, omitted and flags are **page metadata of the thread page**, set by the `propose` operation's content part and projected to `view.proposal` ([0067](0067-proposals.md) §3, [0038](0038-page-metadata-and-categories.md) §1); they are not fields of `create`, whose `target` stays the thread's home. The ledger verb is amends, but §4's table never listed those fields on `create` (only A14's Summary said a `create` may carry them), so this entry extends §4 with the `propose` row and contradicts no text of it. (PENDING F7)

### A20. Who may set `declined` on a proposal thread

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** amends §6
- **Summary:** On a proposal thread the `declined` status may be set only by the proposer or by a holder of `edit` on the subject's talk page; any other post carrying `declined` is refused with `ts-proposal-status` ([0067](0067-proposals.md) §5). §6's rule that anyone who may post may set a status holds for every other status and every other thread, and notes this exception. (PENDING F8)

Replaced text (§6):

> Anyone who may post may set one.

### A21. `thread-statuses.toml` holds the shipped statuses

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** extends §6
- **Summary:** `docs/registry/thread-statuses.toml` is the registry file of the default statuses §6 names, with name, label, category and order per status and `required = true` on `open`: **Open**, **In progress** and **Blocked** (open), and **Resolved**, **Won't fix**, **Declined**, **Stale** and **Archived** (closed). The consolidation audit had recorded the file as missing because the chapter writers were given a partial copy of the registry; it existed. Nothing is created. (PENDING F29)

### A22. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [14](../architecture/14-discussions.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A23. Server-minted values move to the attestation map

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2, §3, §4
- **Summary:** A value the server mints into a thread record while appending it never enters a client-signed part. The talk page ID a `create`, `move`, `attach`, `detach`, `pin` or `unpin` carries for its target, the talk page IDs of each `also` entry, and the minted title of a `create` or `rename`, suffix included, are carried in the record's attestation map rather than its content part, so that the content and comment parts stay the bytes the client submitted and a client signature over them verifies; the client submits the subject, the server mints the title; a rebuild reads the IDs and titles from the attestation maps; the content part carries `target` alone. A page `move`, by contrast, carries its new title in the content part, because the mover chose it. The ledger names §2; the title rule is §3's and the parts table is §4's, so all three are changed. (REVIEW G23)

Replaced text ([14](../architecture/14-discussions.md) §1.3, as it stood):

> It is minted from the page-ID sequence by the first `create`, `move` or `attach` record that attaches a thread to that target, and that record carries it in its content part (§1.5), so a rebuild reads it rather than deriving it.

Replaced text ([14](../architecture/14-discussions.md) §1.4, as it stood):

> The `create` and `rename` records carry the minted title, as a page `move` carries its new title, so the title index is a projection and a rebuild reproduces it.

Replaced text ([14](../architecture/14-discussions.md) §1.5, as it stood):

> | `title` | `create`, `rename` | The minted title (§1.4) |
> | `talk` | `create`, `move` | The talk page's page ID, minted by this record if the target had none |
> | `also` | `create` (optional) | Further targets listed at creation, each `{target, talk}` (§3.6) |
> | `target`, `talk` | `attach`, `detach`, `pin`, `unpin` | The target (a `page` target only, for `attach` and `detach`) and its talk page ID, minted by an `attach` if the target had none (§3.6); for `pin` and `unpin`, the attachment pinned or unpinned, any attachment, `actor` included (§4) |
>
> Mentions, links and the rendered HTML are never stored; they are derived from the text part (§1.6). Nothing in the content part names the record's own ID, which is assigned at append and read from the header.

Replaced text ([14](../architecture/14-discussions.md) §3.6, as it stood):

> Both carry a summary in the comment part, as `move` does, and `target` and `talk` in the content part; `also` on `create` and `keep` on `move` are in the field table of §1.5.

Replaced text ([14](../architecture/14-discussions.md) §4.2, as it stood):

> Each carries `target` and `talk` in the content part, as `attach` does, and a summary in the comment part.

### A24. Heading posts, wrapper posts and imported posts' provenance

- **Date:** 2026-10-20
- **Source:** [0090](0090-converting-talk-pages-comment-by-comment.md) §3, §8
- **Change:** extends §4, §5
- **Summary:** The thread payload gains `heading` and `wrap` on `post`, for the heading posts and wrapper posts a talk-page conversion writes; `imported_from` may appear on `post` as well as `create`, carrying each imported post's DiscussionTools name and ID, `signed_as`, `added_by`, original timestamp and fallback reason; `inReplyTo` may name a heading post. All are written only by imports, and refused on an editor's `post`. Heading posts render as headings and wrapper posts around the posts they enclose. Chapter text: [14](../architecture/14-discussions.md) §1.5, §1.6.

Replaced text ([14](../architecture/14-discussions.md) §1.5, as it stood):

> | `inReplyTo` | `post` | The parent post's revision ID, or null for a top-level post |
>
> | `imported_from` | `create` (optional) | The repository, talk page, revision and archive subpage an imported thread came from, and the hash of the section's DiscussionTools name where there is one ([0054](../decisions/0054-forking-a-mirrored-page.md) §5, [0069](../decisions/0069-synchronized-talk-pages.md) §7) |
>
> The fields of `propose`, `submit` and `withdraw` are in §5.3.

Replaced text ([14](../architecture/14-discussions.md) §1.6): none; a paragraph on heading and wrapper posts is added before the one on `~~~~`.

### A25. The `closed` status, and collapsing a range of posts

- **Date:** 2026-10-21
- **Source:** Direct: James, design discussion of 2026-10-21
- **Change:** amends §4, §5; extends §6
- **Summary:** A default thread status `closed` (category `closed`) is added to `thread-statuses.toml`, for a discussion someone formally closed with a statement; a talk-page conversion writes it for a closing template ([0090](0090-converting-talk-pages-comment-by-comment.md) A1). A post may carry `collapse: {first, last}`, the revision IDs of two posts of the same thread, and the range is shown collapsed beneath the post's text, its posts keeping their places and parents; a later post carrying `uncollapse` with the collapsing post's revision ID undoes it. Collapsing is native, a claim in public like a status: anyone who may post may collapse, and imports write collapsing posts for wrapper templates. The wrapper posts and `wrap` field of A24 are withdrawn. Chapter text: [14](../architecture/14-discussions.md) §1.5, §1.6, §2.1, §2.4.

Replaced text ([14](../architecture/14-discussions.md) §1.5, as it stood):

> | `wrap` | `post` (optional) | The closing call of a wrapper post, whose text is the opening call of the template pair that closed or collapsed its section or subsection ([13](../architecture/13-mirrored-pages.md) §5.14) |
>
> `imported_from` on `post`, `heading` and `wrap` are written only by imports; a `post` from an editor that carries any of them is refused ([0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §8).

Replaced text ([14](../architecture/14-discussions.md) §1.6, as it stood):

> **Heading posts and wrapper posts**, which only an import writes, render as what they were: a heading post as a heading at its depth, and a wrapper post by expanding its opening call, a placeholder and its closing call once and putting the enclosed posts' HTML at the placeholder ([0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §3; [13](../architecture/13-mirrored-pages.md) §5.14).

### A26. Rules settled while building `scatter-threads`

- **Date:** 2026-10-22
- **Source:** Direct: James, design discussion of 2026-10-22
- **Change:** extends §3, §4, §6
- **Summary:** Calls the pure crate made where the text was silent, each confirmed by James. **Subjects** (§3): underscores and runs of whitespace become one space, the ends are trimmed and the first letter is upper-cased; control characters are refused beside MediaWiki's forbidden ones, and a subject must leave room for the date and a ` (n)` suffix within the 255-byte title limit. **`imported_from` keys** (§4): `repository`, `page`, `revision`, `archive` and `section_hash` for a thread's source; `name`, `id`, `signed_as`, `added_by` (`revision`, `actor`), `timestamp`, `fallback` and `wrapper` for an imported comment; any other key is refused (James: "reasonable"). **Import-only fields** (§4): `imported_from`, `heading` and the wikitext media type are refused on an original `create` or `post` and accepted on an imported one (James: "If a field is only applicable for an import it should not be allowed for an original post, but if it genuinely is an imported post it should be fine"). **Media type** (§4): an `edit` carries its post's own media type, so a post keeps its markup for good. **Pins** (§4): `pinned: true` on `create` counts against `thread.max_pinned` on the home, as a `pin` does. **Statuses** (§6): a retired status can no longer be set, threads that carry it keep it, and a status the registry no longer lists reads as open; `closed` (A25) is a current default, not a retired one (James: "it was a gap in the original model"). **Collapses** (§6): ranges may nest but not cross; a partly overlapping range is refused. Chapter text: [14](../architecture/14-discussions.md) §1.4, §1.5, §2.1, §2.4, §4.2.

### A27. A thread record that is a log event carries a log ID

- **Date:** 2026-10-22
- **Source:** Direct: James, "proceed to what you feel the best next step is", 2026-10-22; implementation calls for his review
- **Change:** extends §7
- **Summary:** Made while projecting thread records as activity rows. A thread record that is also a log event (`create`, `rename`, `move`, `attach`, `detach`, `pin`, `unpin`) takes a log ID from the tenant's sequence beside its revision ID, as a MediaWiki page creation has both a revision and a log entry; `post` and `edit` take none. Its one activity row carries the log type, action and parameters beside the edit, and `list=logevents` reads those rows with the `log` rows; the parameters name the targets and the talk page IDs the record minted or found. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §6.1.

### A28. Post markup is a tenant setting; a trailing signature is removed on save

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-24
- **Change:** amends §4, §5; extends §8
- **Summary:** James: "I like the idea of quietly removing the tildes upon save. There isn't really a need to scold the user. Whether posts use markdown or wikitext can be an instance or tenant setting." With his choices of 2026-10-25 on the details: **Markup.** The tenant `site` setting `thread.markup` (`markdown` or `wikitext`, default `markdown`; a tenant sets it or inherits its instance's default, 0085 §2.1) names the media type of every original `create` and `post` written while it holds. A request that omits `mediaType` takes it; one that names the other is refused with `ts-bad-media-type` (James chose refusal over accepting either). `text/x-wiki` is therefore no longer import-only, which amends A26's list of import-only fields; `heading` and `imported_from` stay import-only. An `edit` keeps its post's media type (A26), so changing the setting rewrites nothing and a thread may hold both. **Signatures.** When an original `create`, `post` or `edit` is written, a signature at the very end of its text (`~~~`, `~~~~` or `~~~~~`, with any whitespace and one `--`, `—` or `–` before it) is removed silently; tildes elsewhere are kept (James chose the trailing signature only). A wikitext post then receives the rest of the pre-save transform as a wikitext page does: `subst:`, `safesubst:` and signatures elsewhere in the text expand (James chose this over no transform). A markdown post receives nothing else. An original `create` or `edit` whose text is empty once its signature is removed is refused with `ts-empty-post`, and so is a `post` unless it sets a status or collapses or uncollapses a range (an implementation call for James's review). Imports keep their text verbatim. **UI** (§8): the composer shows the markup in use beneath its text area and removes the signature without comment. Chapter text: [14](../architecture/14-discussions.md) §1.5, §1.6; [11](../architecture/11-rendering-templates-and-modules.md) §1.2, §1.5; [13](../architecture/13-mirrored-pages.md) §5.13 (0054 A7); [19](../architecture/19-site-ui.md) §6.1; [23](../architecture/23-configuration-and-registry.md) §3.2, §6 (`siprop=triplespace` reports it).

Replaced text ([14](../architecture/14-discussions.md) §1.5, as it stood):

> | `mediaType` | `create`, `post`, `edit` (optional) | The text part's media type: `text/markdown` by default, `text/x-wiki` for a post an import writes ([0054](../decisions/0054-forking-a-mirrored-page.md) §5). An `edit` carries its post's own media type, so a post keeps its markup for good ([0019](../decisions/0019-discussions.md) A26) |
>
> The rule is general: a field that applies only to an import, `imported_from`, `heading` or the `text/x-wiki` media type, is refused on an original `create` or `post` and accepted on an imported one ([0019](../decisions/0019-discussions.md) A26).

Replaced text ([14](../architecture/14-discussions.md) §1.6, as it stood):

> **Posts are markdown** by default, rendered by the `markdown` content model of 0008 §5 ([10](../architecture/10-pages-and-content-models.md) §4.4), with two additions that apply to that model everywhere; a post whose `mediaType` is `text/x-wiki`, which only an import writes, is rendered by the wikitext pipeline instead, with expansion, so the banners and templates of an imported talk page render through the title stack ([0054](../decisions/0054-forking-a-mirrored-page.md) §5; [11](../architecture/11-rendering-templates-and-modules.md)):
>
> `~~~~` is not expanded. A post is attributed by its attestation, and the signature has nothing to add.

Replaced text ([11](../architecture/11-rendering-templates-and-modules.md) §1.2 and §1.5, as it stood):

> | Pre-save transform | `~~~`, `~~~~`, `~~~~~` on wikitext pages (not posts, [0019](../decisions/0019-discussions.md) §5) |
>
> The pre-save transform expands `~~~`, `~~~~` and `~~~~~` on wikitext pages, not posts ([0008](../decisions/0008-namespaces-and-document-pages.md) §8; [0019](../decisions/0019-discussions.md) §5).

### A29. A talk page's revisions have content: its threads as wikitext

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §7
- **Summary:** James chose, of three forms, a talk page's revision content as wikitext ("Talk page as wikitext"), and of two memberships, "attached now, as they stood". `rvprop=content` on a talk title gives the threads attached to it now, in the talk page's order, each as it stood after the revision (a thread not yet started is left out), written as a MediaWiki talk page: `== Subject ==` sections, posts indented with `:` by depth and ending in the signature `~~~~` would have written, posts in the markup they were written in, imported posts with their own signatures, heading posts as headings, `(comment removed)` and `(username removed)` for what the viewer may not see. Model `triplespace-talk`, format `text/x-wiki`, read-only. `action=compare` diffs it by lines; a revision named with the talk page (`fromtitle`/`fromid` and `fromrev`) is the talk page's as of that revision, and named alone, its thread's. A thread that leaves takes its revisions with it, consistent with the composite history. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §6.1; [10](../architecture/10-pages-and-content-models.md) §4.3, §4.4 (0041 A17); `docs/registry/content-models.toml`.

### A30. A topic on a user talk page, by the user's name

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** extends §8, §9
- **Summary:** James asked for new topics on user talk pages in the site, with thread notifications ([0021](0021-notifications.md) A20). `POST /thread` takes an actor target by its account's name, `{kind: "actor", name}`, besides its key, resolved to a registered account or a bot on the tenant; a name none holds is `ts-no-target`, a 404. A user talk page offers **Add topic** as any talk page does, whether or not the user page exists. Chapter text: [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §6.1.

### A31. A thread's actions from the page: the calls made building them

- **Date:** 2026-11-05
- **Source:** Direct: Claude, while building the thread actions (SCA-168), 2026-11-05; unconfirmed
- **Change:** amends §8; extends §9
- **Summary:** Folded into [19](../architecture/19-site-ui.md) §6.1 and [18](../architecture/18-api.md) §3.2. The calls:
  - Every operation the records support besides posting is reached from the thread page's overflow menu, as `?action=thread&op={rename|move|attach|detach|pin|unpin|close|reopen}`: a form with a confirm button, which posts to the same address. The menu offers an item where the viewer's tenant-wide rights allow it (`move`, or `edit` for Close and Reopen) and the thread gives it something to do; the API decides the rest, page protection included, and a refusal comes back on the form.
  - Pin, Unpin and Remove a listing choose the attachment from a select, rather than a control beside each attachment on the thread page; a talk page's thread heads carry **Pin here** and **Unpin here**, which open the pin form with that page chosen and return to the talk page.
  - Move and List on take any title that leads to a talk page: the talk page, a board, or the subject itself. A title that cannot hold a thread is refused on the form.
  - **Close…** posts a status of the closed category (Resolved by default), with an optional closing statement; **Reopen…** posts `open`, with an optional comment. Both are posts, so a status remains a claim in the thread by a named actor.
  - The change routes (`rename`, `move`, `attach`, `detach`, `pin`, `unpin`) name a user talk page by its account, `{kind: "actor", name}`, as `POST /thread` does (A30), which is how the site's forms name one.

  Replaced text ([19](../architecture/19-site-ui.md) §6.1, as it stood):

  > A thread page has **Pin here** and **Unpin** beside each attachment, for those who hold the rights ([0049](../decisions/0049-boards.md) §13).
  >
  > A thread page has **List on…** and, beside each listing, **Remove from here**, for those who hold the rights ([0049](../decisions/0049-boards.md) §13). Moving a thread offers "Keep listed on the old page".
