# 0020. Change feeds: recent changes, related changes and watchlists

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-05 (A19)
- **Author:** James Hare / Claude Fable
- **Changes:** [0005](0005-crate-organization.md), [0007](0007-actor-identity.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0016](0016-permissions-and-access-control.md), [0019](0019-discussions.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0004](0004-identity-clusters-and-equivalence.md), [0006](0006-log-integrity-and-erasure.md), [0011](0011-logs.md), [0018](0018-tenants.md), [0022](0022-federation.md)

## Context

MediaWiki has four features that list changes: page history, recent changes, related changes (`Special:RecentChangesLinked`) and the watchlist. It implements them with three tables and two joins: history reads `revision`, recent changes reads `recentchanges`, related changes joins `recentchanges` to `pagelinks`, and the watchlist joins it to `watchlist`. They are one feature with four target sets.

Triplespace has already folded the tables. [0012](0012-api-requirements.md) §3 gave history, recent changes, contributions and the log one row shape; [0013](0013-postgres-storage.md) §5 made `view.activity` one table, indexed by time, actor and target; [0010](0010-site-ui.md) §5 defined a history as every record keyed to a target or a member of its cluster. What is left is to name the abstraction, add the two target sets that do not exist yet, and answer the two questions the earlier ADRs left open: how watchlists work, and whether a watch on a mirrored entity covers its syncs ([0010](0010-site-ui.md)); and whether there is a change feed for downstream consumers ([0012](0012-api-requirements.md)). [0019](0019-discussions.md) §8 also promised that watching a talk page covers its threads.

One constraint is fixed by James: **whether a user is watching something is private and is not part of the graph.** It is not a record in any partition, logged or otherwise.

## Decision

### 1. A feed is activity rows filtered by a target set

*Changed by A3, A15, A19.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.1.*

### 2. Target sets

*Changed by A6, A7, A9, A11, A12, A16.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.2.*

### 3. The watch set is private state, not records (extends 0007 §8, 0013 §4)

*Changed by A2, A5, A11, A13, A17, A18.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.3.*

### 4. Delivery: page, Atom, stream (settles 0012 Q6)

*Changed by A6, A8, A15.*

*Current text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.4.*

### 5. API (extends 0012 §4 and §5)

*Changed by A2, A6, A18.*

*Current text: [18](../architecture/18-api.md) §2.3, §3.1, §3.2.*

### 6. Storage and cost (extends 0013 §5)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.6.*

### 7. Permissions (extends 0016 §2)

*Changed by A4.*

*Current text: [09](../architecture/09-security-and-moderation.md) §2.2, §8.2.*

### 8. Crates

*Changed by A1.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Consequences

- ~~**One query path.** History, recent changes, related changes, the watchlist, contributions and the downstream stream are one code path over one table with one row shape, and they cannot disagree about what a change is or who may see it.~~ *A mirrored entity's history and its sync rows in a watchlist are read from `view.entity_history`, not `view.activity`, and the stream is ordered by commit where a page is ordered by source time (A15, A16).*
- **A watch is a private filter, never data.** Nobody, including the instance's own graph, records who watches what. The price is that the watch set is not rebuildable from the log and does not follow a tenant when it moves.
- **Watching a mirrored entity includes its upstream changes,** by default, which is the answer 0010 was waiting for.
- **No watcher counts,** which MediaWiki tools occasionally use to gauge attention. Here attention is private.
- **The downstream feed exists without a new model**, and it carries erasures, so a consumer can be as faithful to this instance as this instance is to Wikidata.
- **Related changes for entities are new territory.** MediaWiki's are link-based; here the outward relation is the statement graph, which is what a Wikibase editor actually means by "related".

## Open questions

- **Q1.** ~~**Notifications.** Mentions, replies, new messages on one's talk page, email and digests: a feed with push delivery and per-user read state, deserving its own ADR now that the pull side exists.~~ *Settled by [0021](0021-notifications.md), which adds a `notify` flag to `private.watch`.*
- **Q2.** ~~**Patrolling.**~~ *Settled by [0023](0023-moderation.md) §6: a `patrolled` filter on every feed.*
- **Q3.** ~~**Preferences.** Where the rest of an account's preferences live; this ADR puts four in `private` beside the watch set.~~ *Settled by [0027](0027-preferences-and-portability.md) §1: one `private.preference` store with a registered key set; the four auto-watch flags move there.*
- **Q4.** ~~**Provider-to-tenant sync over the stream** instead of by job ([0018](0018-tenants.md) §5).~~ *Settled by [0028](0028-tenancy-policy.md) §5 and [0022](0022-federation.md) §2: same-instance reading is direct; cross-instance reading is a sync job that follows the stream filtered to `source = local`, verified against the provider's checkpoints by default.*
- **Q5.** ~~**Rate limits** on the stream and on Atom, and the default period window per feed.~~ *Settled by [0024](0024-subsidiary-accounts.md) §5 and [0027](0027-preferences-and-portability.md) §1: rate limits are the `stream` and `atom` classes; the period window is the `feeds.period` preference, with its default in `preferences.toml`; the default's value is tuning.*
- **Q6. Watching a namespace or a query.** Dynamic watch sets defined by a SPARQL query or a search, as some wikis have wanted.
- **Q7. Related changes for keyed types.** Whether a domain's related set is its DNS parents and children ([0009](0009-keyed-entity-types-and-domain.md)).

## References

- [Help:Watchlist](https://www.mediawiki.org/wiki/Help:Watchlist), [Help:Watchlist expiry](https://www.mediawiki.org/wiki/Help:Watchlist_expiry) and [Help:Related changes](https://www.mediawiki.org/wiki/Help:Related_changes)
- [API:Watchlist](https://www.mediawiki.org/wiki/API:Watchlist), [API:Watch](https://www.mediawiki.org/wiki/API:Watch) and [API:Watchlistraw](https://www.mediawiki.org/wiki/API:Watchlistraw)
- [Manual:Watchlist table](https://www.mediawiki.org/wiki/Manual:Watchlist_table)
- [Wikimedia EventStreams](https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams)
- [Server-sent events](https://html.spec.whatwg.org/multipage/server-sent-events.html) and [Atom (RFC 4287)](https://www.rfc-editor.org/rfc/rfc4287)

## Amendment log

### A1. Crate table

- **Date:** 2026-09-27
- **Source:** [0005](0005-crate-organization.md) §2
- **Change:** supersedes §8
- **Summary:** 0005 §2 is the one crate table CI checks, and carries every change this section listed (0005 A16).

Replaced text (§8):

> | Crate | Change |
> |---|---|
> | `triplespace-projections` ([0013](0013-postgres-storage.md) §10) | Feed specifications, target-set expansion, the activity query for every set, and related-changes joins |
> | `triplespace-accounts` | `private.watch` and `private.watch_token`, the expiry sweep, auto-watch, and resolving a holder's watch set to targets |
> | `triplespace-api-rest`, `triplespace-api-action` | The routes and modules of §5; Atom and server-sent events |

### A2. Notifications

- **Date:** 2026-09-27
- **Source:** [0021](0021-notifications.md) §2, §7
- **Change:** extends §3, §5
- **Summary:** `private.watch` gains `notify`, which delivers a watched target's changes as notifications; `uiprop=hasmsg` is defined there. The SQL had been given the column in place. This settled Q1.

### A3. Patrolling

- **Date:** 2026-09-27
- **Source:** [0023](0023-moderation.md) §6
- **Change:** extends §1
- **Summary:** A `patrolled` filter on every feed. This settled Q2.

### A4. Rate limits

- **Date:** 2026-09-27
- **Source:** [0024](0024-subsidiary-accounts.md) §5
- **Change:** amends §7
- **Summary:** Streams and Atom fetches are rate-limited in the `stream` and `atom` classes. This settled the rate-limit half of Q5.

Replaced text (§7):

> No permission reads another account's watch set; there is no `viewwatchlist`-of-others right and no route for one. Rate limits on the stream are open.

### A5. Preferences and portability

- **Date:** 2026-09-27
- **Source:** [0027](0027-preferences-and-portability.md) §1, §3–4
- **Change:** amends §3
- **Summary:** The four auto-watch flags move into the preference store, with `autowatch.notify`; the watch set is portable in the user data bundle and the private extract of a move; the period window is the `feeds.period` preference. This settled Q3 and the period half of Q5.

Replaced text (§3):

> - **Auto-watch** is a preference: pages and entities the account edits, pages it creates, threads it posts in, and threads it starts, each on or off. Preferences are private state too and belong beside the watch set; only these four are decided here (open questions).
>
> It does not travel with a tenant ([0018](0018-tenants.md) §10), because nothing private does; a user who moves takes their list with `list=watchlistraw` and re-imports it.

### A6. Farm-wide feeds and the stream across instances

- **Date:** 2026-09-27
- **Source:** [0028](0028-tenancy-policy.md) §5, §9
- **Change:** extends §2, §5; amends §4
- **Summary:** An all-tenants target set, served at the farm base under `feeds.farm_wide`. Cross-instance provider-to-tenant sync follows the stream, filtered to `source = local` and verified ([0022](0022-federation.md) §2); same-instance reading is direct. This settled Q4.

Replaced text (§4):

> It is the same mechanism as `GET /jobs/{id}/events` (0012 §5), and a consumer that missed events resumes from its last token. Whether a consumer tenant should sync from a provider tenant this way instead of by job ([0018](0018-tenants.md) §5) is an open question.

### A7. The filter log is a feed

- **Date:** 2026-09-27
- **Source:** [0030](0030-edit-filters.md) §6
- **Change:** extends §2
- **Summary:** Filter hits are a target set, delivered as a page, Atom or stream like any feed.

### A8. The SPARQL Update stream

- **Date:** 2026-09-27
- **Source:** [0032](0032-sparql-update-stream.md) §6
- **Change:** extends §4
- **Summary:** The activity stream is for consumers of change sets and events. A triplestore that wants to follow the resolved view as triples uses the SPARQL Update stream, `GET /updates/stream`, which shares this route's transport and `Last-Event-ID` discipline and carries `DELETE DATA`/`INSERT DATA` bodies instead of activity rows.

### A9. Boards

- **Date:** 2026-10-01
- **Source:** [0049](0049-boards.md) §7
- **Change:** amends §2
- **Summary:** A talk page's one-target set includes every thread attached to it, home or listing, and a board's is the board and its threads. A thread that is listed or detached joins or leaves the set at query time, as a moved one does.

Replaced text (§2):

> **A subject and its talk page are one target.** The one-target set for `Item:Q42` includes `Item talk:Q42` and its threads; for `User:Example`, the user page and `User talk:Example`. A thread is its own target, and a thread that moves to another talk page leaves the first target's set and joins the second's at query time, so history and watches follow the talk page, not the thread ([0019](0019-discussions.md) §2).

### A10. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–8
- **Summary:** A1–A9 were folded into the Decision. The open questions were numbered. No decision changed. Before this, A8 and A9 were blockquotes, A2's column had been added in place, and the other entries were recorded in the header, the open questions or other ADRs. The file before conversion is commit `0b26a3a`.

### A11. Scopes as target sets and watches

- **Date:** 2026-10-04
- **Source:** [0060](0060-scopes.md) §7
- **Change:** extends §2, §3
- **Summary:** A Scope target set, joined against `view.scope_member` and bounded by `scopes.max_members`; `scope` and `rows` watch kinds, expanded at query time.

Replaced text (§3, in part):

> - **Target kinds** are `entity`, `page`, `thread` and `actor`.

### A12. Followed talk pages

- **Date:** 2026-10-06
- **Source:** [0069](0069-synchronized-talk-pages.md) §8
- **Change:** extends §2
- **Summary:** A talk page that follows upstream includes the followed page's `put` rows in its one-target set; a foreign thread is a target of its own.

### A13. `notify` is refused on `scope` and `rows` watches

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** amends §3
- **Summary:** `notify` is refused on a `scope` or `rows` watch, with the error `ts-watch-notify-unsupported`: those watches are feed filters that expand at query time, not subscriptions to a target, so the generalization of DiscussionTools' topic subscription stops at the four target kinds that name one thing (`entity`, `page`, `thread`, `actor`). [0060](0060-scopes.md) §7 says the same of its side. (PENDING E31)

Replaced text (§3):

> - **`notify`** marks a watch whose changes are also delivered as notifications ([0021](0021-notifications.md) §2): DiscussionTools' topic subscription, generalized to every watchable target.

### A14. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§8
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [09](../architecture/09-security-and-moderation.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A15. The stream's resume token is a vector of per-partition high-water marks

- **Date:** 2026-10-09
- **Source:** [0083](0083-write-path-in-three-tiers.md) §7
- **Change:** amends §4; extends §1
- **Summary:** A page and a stream carry the same rows in different orders. A paged list, its Atom form included, keeps `(time, partition, offset)` as its display order, where the time is the event's time at its source; a consumer that resumed from such a key would miss a row whose source time sorts before it and which committed after it, which imported revisions, adopted history, seeded fork history and upstream events all do, the edge MediaWiki's `rccontinue` has too. The stream is therefore ordered by commit, and its resume token is a vector of per-partition high-water marks, `{partition: offset, …}`, since within a partition offsets are commit-ordered under the append lock; `Last-Event-ID` carries that vector, opaquely encoded, and the continuation token of a page carries the sort key, the two never interchanged. A cross-instance sync job resumes from the per-partition token, and the stream filtered to `source = local` may carry each row's record body. (REVIEW G14)

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §4.4, as it stood):

> | **Stream** | `GET /activity/stream` for any feed specification, as server-sent events, with `Last-Event-ID` carrying the continuation token | | | authenticated, never by token |

### A16. A history's sync rows come from `view.entity_history`

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** amends §2
- **Summary:** A history and a watchlist draw a mirrored entity's sync rows from `view.entity_history (tenant, canonical_id, time, partition, offset, kind, member)`, the one backing of a mirrored entity's history, written by composition; upstream log events are rows of it and never of `view.activity`. The ledger names [0013](0013-postgres-storage.md) §5, [0011](0011-logs.md) §3 and [0010](0010-site-ui.md) §3; the fold also changed §2's chapter text ([16](../architecture/16-logs-feeds-and-notifications.md) §4.2), so it is logged here. (REVIEW G42)

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §4.2, as it stood):

> A history does show them, because it is scoped to one target and draws its sync rows from that target's mirror records.

### A17. `private.watch` gains a target index for the addressing projection

- **Date:** 2026-10-09
- **Source:** Direct: James, design review of 2026-10-09
- **Change:** extends §3
- **Summary:** `private.watch` gains `CREATE INDEX watch_target ON private.watch (target_kind, target_id) WHERE notify`, which exists for one reader: the addressing projection, which for every activity row must find every account that watches the row's target, its cluster members or its paired talk page with `notify` set, without scanning across accounts; the watchlist itself is read by account through the primary key. The ledger names §6; the watch table's DDL is §3's text ([16](../architecture/16-logs-feeds-and-notifications.md) §4.3), and §6's ([03](../architecture/03-storage-caches-and-search.md) §4.6) is unchanged. (REVIEW G47)

### A18. The watch set, built

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** amends §3, §5
- **Summary:** `private.watch` is built (migration `0021_watch`), with `action=watch`, `action=setnotificationtimestamp`, `prop=info` `watched` and `notificationtimestamp`, `list=watchlist`, `list=watchlistraw`, `GET /activity?set=watchlist`, the `/watchlist` routes, the `watchlist` parameter of the writes and the site's star, **Watch for…** menu, `Special:Watchlist` and `Special:EditWatchlist`. James chose: the star plus a **Watch for…** disclosure ("Star plus a 'Watch for…' menu"), the seen marks in this round, and the Atom feed with its token, and `scope` and `rows` watches, left for later. Settled in the building, for James to confirm: a row stores its target in `view.activity`'s numbering, with a new kind, `title` (`{ns}:{title}` in the subject namespace), for a title with no page yet, which covers the page once there is one; a page the watcher may not see is watched as its title, so watching it answers as watching a page to come, and a page ID they may not see is refused as missing; what a page is watched as (an entity's page and talk page as the entity; a user page, user talk page and a thread homed there as the user; a talk page as its subject); `seen` is the newest change seen, a change being unseen when newer than `seen` or the watch's addition and not the holder's own, and visiting a watched page marks it seen; `setnotificationtimestamp` only moves `seen` to now; `wlallrev` off shows a page's newest change within each page of results; the `watchlist` parameter applies to `action=edit`, Wikibase's writes and the thread routes (body fields `watchlist`, `watchlistexpiry`), not yet to `action=move`; `action=watch` takes Triplespace's `tsnotify`, and `prop=info` reports `ts-notify`. Chapter text: [16](../architecture/16-logs-feeds-and-notifications.md) §4.3; [18](../architecture/18-api.md) §2.3, §2.4, §3.2.

Replaced text ([16](../architecture/16-logs-feeds-and-notifications.md) §4.3, as it stood):

> **Target kinds** are `entity`, `page`, `thread`, `actor`, `scope` and `rows`.

> **`seen`** is updated when the holder reads the target or the watchlist, and drives the "changed since you last looked" mark. It is written often and matters little, which is why it lives here and not in a record.

> **Serving respects the schema boundary.** `triplespace-accounts` is the only code that reads `private.watch`. It resolves the holder's watch set to a list of targets and hands that list to the activity query, which runs under the public role against `view` alone. The join happens in the query as a target-set parameter, never as a cross-schema join.

Replaced text ([18](../architecture/18-api.md) §2.3, §2.4, §3.2, as it stood):

> | `action=watch`, `action=unwatch` | With `expiry` | [0020](0020-change-feeds.md) §5 |

> | `action=setnotificationtimestamp` | As in MediaWiki | [0020](0020-change-feeds.md) §5 |

> | `GET /watchlist` | The holder's watch set with expiries | [0020](0020-change-feeds.md) §5 |

### A19. A deleted target's changes leave the lists

- **Date:** 2026-11-05
- **Source:** Direct: James, design discussion of 2026-11-05
- **Change:** extends §1
- **Summary:** Per MediaWiki, a deleted page does not appear in recent changes. The everything, actor and watched sets (recent changes, contributions, watchlists) leave out the `edit`, `sync` and `erased` rows of a page, thread or local entity that is deleted now, for every viewer, the deletion group included; its log events, the deletion's among them, stay. One-target and talk-page histories keep the rows, for `prop=deletedrevisions` and the deletion group. The test is made at read time, so an undeletion brings the rows back. Found as a bug: an adopted page, deleted, still showed in recent changes.

