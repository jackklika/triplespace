# 16. Logs, feeds and notifications

This chapter covers the log: the upstream logs a provider's adapter mirrors and the local events an instance's own actions produce, where each lives, how upstream is followed when it hides something, and the one catalogue of every log type and action the ADRs name. It then covers what is built on those events: the activity row that history, recent changes, contributions and the log share; change feeds, target sets, the watch set and the three deliveries; notifications, addressing, the inbox and its channels; and the Activity Streams profile. It assumes the log and its records from [01](01-log-and-records.md), the graph catalogue from [02](02-graphs-rdf-and-query.md) (the RDF of every log event is [02](02-graphs-rdf-and-query.md) §5.2), the `view.activity` and `private` tables from [03](03-storage-caches-and-search.md), actors from [07](07-actors-and-accounts.md), tenants and instance acts from [08](08-tenants-and-instances.md), and ACLs, visibility and edit filters from [09](09-security-and-moderation.md). Threads are [14](14-discussions.md); scopes, tables and sprints are [15](15-structured-pages.md); ActivityPub beyond the notifier is [17](17-federation-and-publication.md).

## 1. Upstream and local logs

*Sources: [0011](../decisions/0011-logs.md) §1, §2, §3, §4, §5, §6.2.*

### 1.1 Faithful to the public record

*Sources: [0011](../decisions/0011-logs.md) §1.*

Triplespace mirrors what a provider publishes, and nothing more.

- **Some logs are never public.** The `suppress` log and CheckUser logs never are.
- **Hidden fields stay hidden.** A public entry whose performer, comment or target is hidden arrives with those fields empty.
- **Later hiding is followed.** When upstream hides something after the instance has mirrored it, the instance removes it too (§1.5). This follows the rule of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5: a faithful mirror forgets what upstream withdraws.

### 1.2 Log graphs

*Sources: [0011](../decisions/0011-logs.md) §2.*

Log events that have no other home are written to log graphs. These are source graphs, like the actor graphs of [07](07-actors-and-accounts.md) §1.6; they are rows of the graph catalogue in [02](02-graphs-rdf-and-query.md) §1.2.

| Graph | Illustrative IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Local log** | `{base}/graph/log` | Source | Jobs, and actions with no other record (§2) | Full | `logged` | Internal only |
| **Provider log**, one per provider | `{base}/graph/log/wikidata` | Source | That provider's log sync jobs | Full | `hashed` | Internal only |
| **Instance log** (`log/{farm}`) | `{farm base}/graph/log` | Source | Instance jobs (§2.2), OAuth consumer records, `blob` ACLs and expunge records (§2.3), global filters | Full | `logged` | Internal only |

**The history policy is always `full`.** A log event does not replace an earlier state, so compacting by key would destroy events. A provider log keeps full history even when its entity mirror is `latest`. Records leave a log graph only through `erase` (§1.5).

**The integrity policies follow the existing pattern.** The local log is instance data, so it is `logged`, like the local graph. A provider log is `hashed`, like the provider's mirror ([01](01-log-and-records.md) §4); its completeness against upstream cannot be proved.

**Both are internal only.** Log events reach RDF only in the metadata graph ([02](02-graphs-rdf-and-query.md) §5.2), which the Wikibase-compatible dump never contains; a full dump, the instance's own export, carries it ([0013](../decisions/0013-postgres-storage.md) §8). The API serves them (§3.2).

The instance log is the instance partition of [08](08-tenants-and-instances.md) §7.4, written to by [0011](../decisions/0011-logs.md) §6.1 and §6.3, [0025](../decisions/0025-oauth-server.md) §5 and [0039](../decisions/0039-files-and-media.md) §10 and §16.

### 1.3 Log event records

*Sources: [0011](../decisions/0011-logs.md) §3.*

A record in a log graph has the payload type `scatter:v0/logevent`, or one of the others the log graphs admit: `upstream-revision` in a provider log ([01](01-log-and-records.md) §2.6); `acl`, `filter` and `filter-hit` in the local log ([09](09-security-and-moderation.md) §4.3, §7.5); `expunge` in the instance log only ([0039](../decisions/0039-files-and-media.md) §10); and `erase` everywhere. `docs/registry/graphs.toml` lists them. A `logevent`'s payload holds:

- the log type and action, as MediaWiki strings;
- the time of the event at its source;
- the upstream log ID, for provider logs;
- the target: an entity ID, page ID, actor key or job ID;
- the performer's actor key ([07](07-actors-and-accounts.md) §1.1);
- the comment, in the comment part ([01](01-log-and-records.md) §2.3);
- typed parameters (§2.3);
- the visibility bits, as upstream last reported them.

**The header key is the target's identifier, never a title or name** ([01](01-log-and-records.md) §2.2):

| Target | Key |
|---|---|
| Entity | The prefixed ID, such as `WDQ123` |
| Keyed entity, such as a Domain | Its surrogate ([04](04-entities-and-identifiers.md) §3.4) |
| Document page | The page ID ([0008](../decisions/0008-namespaces-and-document-pages.md) §4) |
| Account | The actor key, such as `wikidatawiki:12345` |
| Job | The job ID |
| Hidden or unresolvable | `null` |

Upstream titles are mapped to keys when the adapter reads them.

- **Entity pages** go through the provider registry, which maps each entity namespace to a type code. On Wikidata, `Q42` in the main namespace becomes `WDQ42`, `Property:P31` becomes `WDP31`, and `Lexeme:L1` becomes `WDL1`.
- **User titles** are resolved to numeric IDs with the issuer's API, as [07](07-actors-and-accounts.md) §1 does for imported edits.
- **Titles in namespaces the provider does not mirror** are skipped.

### 1.4 Which upstream logs are mirrored

*Sources: [0011](../decisions/0011-logs.md) §4.*

The provider registry lists the log types each provider mirrors. It is configuration, recorded in the log ([23](23-configuration-and-registry.md)). The default for every provider is `delete/*` and `protect/*`; for Wikidata:

| Log | Mirrored | Why |
|---|---|---|
| `delete/delete`, `delete/restore` | Yes | Who deleted or restored an entity, when and why |
| `delete/revision` | Yes | Triggers redaction of held revisions (§1.5) |
| `delete/event` | Yes | Triggers redaction of held log events (§1.5) |
| `protect/*` | Yes | Protection history |
| `create/create` | Derived | Not mirrored as events: the creation of a mirrored entity is derived from its first upstream revision record where history is backfilled ([01](01-log-and-records.md) §2.6), so the provider log never holds one event per entity ever created |
| `import/*`, `contentmodel/*` | No by default | Page provenance; a provider's registry entry may add them |
| `renameuser/*` | Consumed (§1.6) | Updates foreign actor records. Stored without names. |
| `move/*`, `merge/merge` | No | Entity pages cannot be moved. They become relevant only if non-entity pages are mirrored. |
| `block/*`, `rights/*`, `newusers/*`, `patrol/*`, `tag/*`, `managetags/*`, and extension logs such as thanks and AbuseFilter | No | They describe people and moderation, not the data. Mirroring them would raise the privacy stakes without adding to any entity's history. |

**The backfill covers events whose target the instance holds**, and nothing else. An event about an entity the instance has never held, such as the deletion of an item that was deleted before the mirror began, is not mirrored; a provider log therefore grows with the instance's mirror, never with the provider's whole history. The deletion of an entity the instance does hold is mirrored, which is what §3.2's deleted-entity page reads.

**Sources:**

| Purpose | Source |
|---|---|
| Backfill and periodic reconciliation | The provider's XML logging dump (`pages-logging`) |
| Filling gaps | `list=logevents`, paged by time |
| Live updates | The EventStreams `recentchange` stream, which carries log events with their IDs, types, actions and parameters. `revision-visibility-change` is also used. |

Parameters in dumps come in two formats: PHP-serialized arrays in newer entries, and newline-separated values in older ones. The adapter parses both.

**Each log sync is a job** ([05](05-providers-and-ingest.md)). An event is identified by its upstream log ID, so re-reading an event the instance already holds changes nothing.

### 1.5 Following upstream when it hides something

*Sources: [0011](../decisions/0011-logs.md) §5.*

Three upstream actions withdraw public data. Triplespace follows each of them.

**A log event's fields are hidden.** This is announced publicly as `delete/event`, or discovered during reconciliation. The sync job erases the hidden parts of the held record with an `erase` record ([01](01-log-and-records.md) §5.1) whose reason class is `upstream`: a hidden comment erases the comment part, and a hidden performer the attestation part ([01](01-log-and-records.md) §2.3, §2.4). The visibility bits a projection reports are derived from which parts are erased.

**A log event is suppressed outright.** It then vanishes from the API and the dumps without any public announcement.

- Each reconciliation against a new logging dump erases held events that the dump no longer contains, with reason class `upstream`, and appends no copy.
- The sweep aborts if the number of erasures exceeds a configured threshold, as the `snapshot` sweep does ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4). A truncated dump must not wipe the log.

**Parts of past revisions are hidden** (`delete/revision`, or a `revision-visibility-change` event). The affected revision IDs are in the event's parameters.

- For each affected revision the instance holds, the sync erases the hidden parts in the same way, from the revision's upstream revision record and from the observed `put` that carries its ID.
- This covers backfilled upstream revisions of retained entities ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5) and observed states that carry the revision's ID.
- If the revision's content is hidden, the mirrored state for that revision is erased. MediaWiki does not allow hiding the content of an entity's current revision, so the entity's current mirrored state is never affected.

**Retention does not override upstream hiding.** `retain` keeps an entity that upstream deletes. It does not keep fields that upstream hides for privacy or legal reasons. Once a retained entity has been deleted upstream and materialized into the local graph, it is local data. Upstream can no longer change it, and hiding within it becomes a local decision.

**`upstream` is one of the reason classes** of [01](01-log-and-records.md) §5.1.

**A remote `Delete` is followed the same way.** When a fediverse server deletes a post that reached a federated talk page, the post's text part is erased with reason class `upstream` ([17](17-federation-and-publication.md)).

### 1.6 Upstream renames

*Sources: [0011](../decisions/0011-logs.md) §6.2.*

An upstream `renameuser` event does two things.

1. **It updates the foreign actor's record** in `actors/{provider}`, with the new name. Compaction under `latest` then removes the old name, as [07](07-actors-and-accounts.md) §1.3 describes.
2. **It is kept in the provider log without names.** The key is the actor key, and the parameters that carry names are dropped. The UI shows "Renamed on Wikidata", with a link to the upstream log entry. Keeping the names would put every past name of every renamed account into a graph that is never compacted, which [0007](../decisions/0007-actor-identity.md) avoids on purpose.

## 2. Local log events

*Sources: [0011](../decisions/0011-logs.md) §6, §6.1, §6.3, §7.*

### 2.1 Projected, not written twice

*Sources: [0011](../decisions/0011-logs.md) §6, §6.1; [0040](../decisions/0040-instance-prerogatives.md) §7; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

Most local actions already have a record elsewhere: an erase record in its target's partition, a page record, an actor record, a change set. Writing a second log record for the same action would mean two writes that cannot be made atomic across partitions, and two places to erase.

**A log event for such an action is a projection of its record.** The log graph holds only actions that have no other record. The catalogue in §2.3 says, for each type and action, which record it is projected from.

**Instance acts keep their own types.** An event projected from an instance act ([08](08-tenants-and-instances.md) §8) keeps its type (`erase/erase`, `block/block`, `rights/rights`, `renameuser/renameuser`), names the operator actor `instance:{farm code}` as performer and carries `instance: true` in its parameters. `list=logevents` reports the performer as the operator actor's display name, and `leprop=details` includes the authority link. Contributions of `instance:{farm code}` at a tenant list the instance acts on that tenant; at the farm base, for `ts-viewoperator`, every instance act on every tenant with the operator who carried it out. The rest of [0040](../decisions/0040-instance-prerogatives.md) §7, the display of an instance act in history and its RDF, is in [19](19-site-ui.md) and [02](02-graphs-rdf-and-query.md) §5.1.

**Erasure propagates automatically, because events are projections.**

- When a vanish erases a user's actor records, every rename event that showed a past name loses it.
- When an unlink erases a `link-account` record, the `link/link` event goes too. What remains is the erase record, which only administrators see, as [07](07-actors-and-accounts.md) §3.1 intends.

### 2.2 Jobs live in the local log

*Sources: [0011](../decisions/0011-logs.md) §6.3; [0035](../decisions/0035-adopting-a-wikibase.md) §6.*

- **A job record is appended to the local log** when a job starts. It holds everything [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3 lists.
- **The job ID is minted by the instance** in sequence and is the record's key.
- **Further records under the same key** mark the job's finish, failure or revert.
  - The finish record carries the job's counts by outcome: created, merged, unchanged and rejected; and the count of upstream snak and reference hashes kept because the recomputation differed, by value type ([01](01-log-and-records.md)).
  - It also carries the checkpoint written at the end of the job ([01](01-log-and-records.md) §4.2).
- **Records a job writes in other partitions** carry the job ID in their attestation ([01](01-log-and-records.md) §2.4).

**Some jobs are the instance's.** A mirror sync is an instance action: its job record is appended to the instance `log`, keyed by job ID, so the instance's job history does not depend on which tenant is primary ([08](08-tenants-and-instances.md) §7.4). So is a job that writes instance acts into tenants, such as a reclamation run, an expunge's propagation, a deleted tenant's erasure or a cascade of renames; its job record is the authority its writes cite ([08](08-tenants-and-instances.md) §8.4). Tenant bulk jobs stay in the tenant's `log`.

An adoption job's record ([0035](../decisions/0035-adopting-a-wikibase.md) §2) projects as `job/start` and `job/finish` like any job; the provenance an adopted entity's first record carries is in §2.3 and in [05](05-providers-and-ingest.md).

**An adopted revision is an activity row at its source time,** not at the time of the job: an `edit` attributed to its author, with its own summary and minor flag, the `bot` flag, and the `job:{id}` and `adopt` tags; `new` is set on a page's first revision only ([05](05-providers-and-ingest.md) §7.3). Sorted by source time, the rows stand in the past, so recent changes shows an adoption as its job row and, within `rc.max_age`, the source's own last edits, and no addressing rule (§5.2) delivers an adopted row. A row whose parts the source hid carries the visibility bits of [09](09-security-and-moderation.md) §6.3.

### 2.3 The log-event catalogue

*Sources: [0011](../decisions/0011-logs.md) §6.1, §7; [0019](../decisions/0019-discussions.md) §7; [0023](../decisions/0023-moderation.md) §3; [0025](../decisions/0025-oauth-server.md) §5; [0030](../decisions/0030-edit-filters.md) §5; [0035](../decisions/0035-adopting-a-wikibase.md) §6; [0039](../decisions/0039-files-and-media.md) §16; [0040](../decisions/0040-instance-prerogatives.md) §7; [0049](../decisions/0049-boards.md) §9; [0051](../decisions/0051-page-redirects.md) §5; [0054](../decisions/0054-forking-a-mirrored-page.md) §1; [0061](../decisions/0061-sprints-and-tasks.md) §5; [0067](../decisions/0067-proposals.md) §3; [0069](../decisions/0069-synchronized-talk-pages.md) §4; [0018](../decisions/0018-tenants.md) §10; [0085](../decisions/0085-settings-and-preferences.md) §3; [0086](../decisions/0086-theming.md) §1; [0087](../decisions/0087-account-registration-and-challenges.md) §6.*

This is the one table of every log type and action the ADRs name, generated from [`../registry/log-events.toml`](../registry/log-events.toml) by `catalogues.py --write` and never edited by hand ([24](24-documentation.md) §11): the table of [0011](../decisions/0011-logs.md) §6.1 with each later ADR's log-events trailer folded in, and the typed parameters of [0011](../decisions/0011-logs.md) §7 beside each row. Each log type's parameters are typed in the payload; the raw parameters are never the only form stored. A parameter the adapter does not recognize is kept as a raw key–value pair. Nothing is dropped, except names (§1.6). The RDF type of each event is in [02](02-graphs-rdf-and-query.md) §5.2.

<!-- catalogues.py: log-events -->
| Log type/action | Comes from | Typed parameters | Visible to | Source |
|---|---|---|---|---|
| `create/create` | The first record of a local entity, document page or thread (a thread `create`), unless it was adopted (`import/*` below) | — | Everyone | 0011 §6.1; 0019 §7 |
| `delete/delete`, `delete/restore` | A `read` ACL on a page, thread, file page or local entity, and its retirement ([09](09-security-and-moderation.md) §4.3, §6.2) | `delete/restore`: the number of revisions restored; whether the talk page went with it; on restore, the record ACLs that stay | Everyone | 0011 §6.1, §7; 0019 §7; 0023 §3; 0039 §16 |
| `delete/revision`, `delete/event` | A `record` ACL on a record's parts ([09](09-security-and-moderation.md) §6.3); `delete/revision` with `type=oldimage` from a `record` ACL on an `upload` record | The affected revision or log IDs; the fields hidden and the fields revealed | Everyone | 0011 §6.1, §7; 0023 §3; 0039 §16 |
| `delete/statement` | A `read` ACL on a `statement` or `property` target, and its retirement ([09](09-security-and-moderation.md) §4.2): the statement is hidden for privacy or legal reasons, which is moderation, not an `override`. On a provider's activity stream it tells a reading instance to drop the statement it holds, as an `upstream` erasure ([17](17-federation-and-publication.md) §1.2) | The statement ID, or the property; whether hidden or revealed | Everyone, as `delete/revision`; the statement itself only the group | 0023 §2; 0022 §2 |
| `suppress/*`: `suppress/delete`, `suppress/revision`, `suppress/event`, `suppress/hide-user` | The same ACLs naming the suppression group ([09](09-security-and-moderation.md) §6.5); `suppress/hide-user` (MediaWiki writes this as `suppress/block` with `hideuser`) hides a username | As the `delete/*` row; `suppress/hide-user`: the actor key | The suppression group | 0011 §6.1; 0023 §3 |
| `patrol/patrol`, `patrol/unpatrol` | A patrol or unpatrol record in the local log ([09](09-security-and-moderation.md) §6.4) | — | Everyone | 0011 §6.1 |
| `abusefilter/create`, `abusefilter/modify` | A filter record in the local log ([09](09-security-and-moderation.md) §7.5) | — | Everyone, or the filter's viewers if it is private | 0011 §6.1; 0030 §5 |
| `abusefilter/hit` | A filter-hit record | — | Everyone, or `abusefilter-log-private` for a private filter's hits | 0011 §6.1; 0030 §5 |
| `move/move` | A page `move`, or a thread `rename` or `move` | The old and new titles, or the source and target talk pages; as MediaWiki's do, whether a redirect was left and its page ID; `noredirect: true` for a move that leaves no redirect (MediaWiki's `suppressredirect`); `keep` for a thread `move` with `keep` | Everyone | 0011 §6.1; 0019 §7; 0049 §9; 0051 §5 |
| `move/move_redir` | A page `move` onto an existing redirect, with MediaWiki's meaning | As `move/move` | Everyone | 0051 §5 |
| `thread/attach`, `thread/detach` | A thread `attach` or `detach` record, and each entry in a thread `create` record's `also` | Each event names the thread and the talk page or board, and appears in the logs of both | Everyone | 0011 §6.1; 0049 §9 |
| `thread/pin`, `thread/unpin` | A thread `pin` or `unpin` record, and a thread `create` with `pinned` | — | Everyone | 0011 §6.1; 0069 §4 |
| `task/claim`, `task/release` | A `scatter:v0/task` `claim` or `release` record on a sprint | — | Everyone | 0011 §6.1; 0061 §5 |
| `proposal/propose`, `proposal/submit`, `proposal/withdraw` | A thread `propose`, `submit` or `withdraw` record | — | Everyone | 0011 §6.1; 0067 §3 |
| `contentmodel/change` | A page `edit` that changes the model | The old and new models | Everyone | 0011 §6.1, §7 |
| `import/interwiki`, `import/upload` | A page import job ([10](10-pages-and-content-models.md)); the `create` of a fork, as `import/interwiki` with the page repository as the interwiki and the upstream page and revision in its parameters; the first record of an adopted entity or page, as `import/upload` from a dump or `import/interwiki` from the live API, never as `create/create`; an `upload` record with action `import`, as `import/upload` | The interwiki prefix or the upload, and the number of revisions | Everyone | 0011 §6.1, §7; 0035 §6; 0039 §16; 0054 §1 |
| `upload/upload`, `upload/overwrite`, `upload/revert` | An `upload` record with that action ([12](12-files-and-media.md)) | — | Everyone | 0011 §6.1; 0039 §16 |
| `takedown/takedown`, `takedown/reinstate` | The `blob` ACL in the instance `log` and its retirement, projected into the log of each tenant with an affected version, on the affected file pages | — | Everyone; without reason or notice when `confidential` | 0011 §6.1; 0039 §16 |
| `takedown/expunge` | An `expunge` record, in the instance's own log only | — | `ts-viewtakedown` | 0039 §16 |
| `newusers/create` | The first actor record of a registered local user. Temporary accounts are not logged. | — | Everyone | 0011 §6.1 |
| `newusers/create2` | The first actor record of an account someone else created: a subsidiary, whose performer is its operator ([07](07-actors-and-accounts.md) §4.2); an account created for another person, whose performer is its creator; or an approved account request, whose performer is the reviewer ([07](07-actors-and-accounts.md) §2.6). Requests and their rejection are not logged | — | Everyone | 0011 §6.1; 0087 §6 |
| `bot/transfer` | An actor record that changes a subsidiary's operator ([07](07-actors-and-accounts.md) §4.4) | — | Everyone | 0011 §6.1 |
| `renameuser/renameuser` | An actor record with a new name ([07](07-actors-and-accounts.md) §1.3) | — | Everyone | 0011 §6.1; 0040 §7 |
| `renameuser/vanish` | The placeholder record written by a vanish ([07](07-actors-and-accounts.md) §3.2) | — | Everyone, with no past name | 0011 §6.1 |
| `link/link` | A `link-account` record ([07](07-actors-and-accounts.md) §3.1) | — | Everyone, while the link exists | 0011 §6.1 |
| `reclaim/reclaim` | A `scatter:v0/reclaim` record in the tenant `log`, written beside the private binding when an account is reclaimed after a tenant move or an adoption: by a bureaucrat's manual reclaim, the bureaucrat its performer, or by the source vouching at login, the account its performer ([08](08-tenants-and-instances.md) §6.4; [07](07-actors-and-accounts.md) §2.5). Parameter `method`: `manual` or `vouched` | — | Everyone | 0018 §10; 0035 §5; 0088 §8 |
| `retention/set` | A `retain` operation | The old and new policies | Everyone | 0011 §6.1, §7 |
| `retention/apply` | The record the entity projection writes in a tenant's partition, under the instance attestation with an authority record, when it applies that tenant's retention policy to an entity an upstream tombstone removed ([05](05-providers-and-ingest.md) §2.1); a tier-3 job, one event per tenant and entity | The policy applied (`retain`, `cascade` or `orphan`) and the provider's tombstone | Everyone | 0002 §5 |
| `convert/convert` | A `convert` operation | The local ID that was minted | Everyone | 0011 §6.1, §7 |
| `erase/erase` | An `erase` record in any partition, the erasures of file deletion and takedown included | The reason class and the authority reference ([01](01-log-and-records.md) §5.1) | Everyone; the reason class and authority reference are visible to holders of `ts-viewerasures` ([09](09-security-and-moderation.md) §6.6) | 0011 §6.1, §7; 0039 §16; 0040 §7 |
| `job/start`, `job/finish`, `job/fail`, `job/revert` | Job records in the local log, or the instance `log` for an instance job (§2.2) | The job's source, version, mode and outcome counts | Everyone | 0011 §6.1, §7; 0035 §2 |
| `protect/protect`, `protect/modify`, `protect/unprotect` | An `acl` record in the local log: written, changed, retired ([09](09-security-and-moderation.md) §4.3) | For each restriction (such as edit or move), the group and the expiry; there is no cascade. A mirrored upstream `protect/*` event keeps MediaWiki's level | Everyone | 0011 §6.1, §7; 0023 §3 |
| `managetags/create`, `managetags/delete`, `managetags/activate`, `managetags/deactivate` | A `tag` config record ([09](09-security-and-moderation.md) §7.6) | — | Everyone | 0011 §6.1; 0030 §5 |
| `settings/change` | The records one `PATCH /settings` or settings-page save appends: `site` records, and at the farm base the instance's defaults, locks and tenancy switches; and a `palette` or `typeset` record written or deleted ([0086](../decisions/0086-theming.md) §1) ([23](23-configuration-and-registry.md) §5.2) | The keys set and unset, each with its old and new value, and the scope: `tenant`, `instance`, `defaults`, `locks` or `tenancy` | Everyone who may read the tenant | 0085 §3; 0086 §1 |
| `key/register`, `key/rotate`, `key/revoke` | A `key` record in `actors` ([07](07-actors-and-accounts.md) §5.4) | — | Everyone | 0011 §6.1 |
| `tag/update` | A record whose attestation tags were changed by `action=tag` | — | Everyone | 0011 §6.1; 0030 §5 |
| `block/*`, `rights/*` | `block` and `membership` records in the `actors` partition ([09](09-security-and-moderation.md) §3.3) | — | Everyone | 0011 §6.1; 0040 §7 |
| `oauth/propose`, `oauth/update`, `oauth/approve`, `oauth/reject`, `oauth/disable` | A consumer record in the instance `log`, with the consumer slug as target and the reason in the comment part ([07](07-actors-and-accounts.md) §7.6). Authorizations and revocations are not logged | — | Everyone | 0011 §6.1; 0025 §5 |
<!-- /catalogues.py -->

Several rows fold more than one source row of the same type: `import/interwiki` and `import/upload` are named for page imports, forks, adopted entities and file imports; `delete/delete`, `delete/revision`, `erase/erase`, `upload/*` and `takedown/*` appear in both [0011](../decisions/0011-logs.md) §6.1 and [0039](../decisions/0039-files-and-media.md) §16; `thread/attach`, `thread/detach`, `thread/pin`, `task/*`, `proposal/*` and `oauth/*` appear in [0011](../decisions/0011-logs.md) §6.1 and in their own ADR's trailer with the same meaning.

A log type with no row in the type table of [02](02-graphs-rdf-and-query.md) §5.2 is a `prov:Activity` carrying `scatter:logType` and `scatter:logAction` only; `thread/pin` and `thread/unpin` have their own row, `as:Add` and `as:Remove` with `as:target` the page's featured collection.

## 3. The activity row

*Sources: [0012](../decisions/0012-api-requirements.md) §3; [0011](../decisions/0011-logs.md) §9.*

### 3.1 One row shape

*Sources: [0012](../decisions/0012-api-requirements.md) §3.*

History, recent changes, contributions and the log all use one row shape in the REST API. Its table is `view.activity` ([03](03-storage-caches-and-search.md) §4.6).

| Field | Contents |
|---|---|
| `kind` | `edit`, `sync`, `upstream-edit`, `log`, `job` or `erased` |
| `time`, `appended` | When the event happened at its source; when this instance recorded it |
| `source` | The graph: `local`, `pages`, `mirror/wikidata`, `log`, `log/wikidata`, and so on |
| `record` | `{partition, offset}`. Absent for upstream edits fetched live. |
| `revid`, `logid` | The global revision and log IDs ([01](01-log-and-records.md) §2.5) |
| `upstream` | The upstream revision or log ID, and its URL |
| `target` | The entity ID or page ID; the title; the label in the viewer's language; the cluster member the record is keyed to |
| `actor` | Actor key, IRI, current name, kind, and a hidden flag ([07](07-actors-and-accounts.md) §1) |
| `operator`, `requested_by` | For bots and jobs ([07](07-actors-and-accounts.md) §4.6; §2.2) |
| `job` | The job ID |
| `summary`, `comment` | The parsed summary: operation, arguments and automatic text ([0001](../decisions/0001-revision-metadata-rdf.md) §6). The user's own comment is a separate field. |
| `tags`, `flags` | Change tags, which a record stores in its attestation part ([09](09-security-and-moderation.md) §7.6); the minor, bot and new flags |
| `patrolled` | Whether the row has been patrolled ([09](09-security-and-moderation.md) §6.4) |
| `filters` | The edit filters the record matched, with their actions, for rows the viewer may see ([09](09-security-and-moderation.md) §7.8) |
| `size`, `delta` | [0012](../decisions/0012-api-requirements.md) §2.4 |
| `changes` | For syncs: the summary of [0012](../decisions/0012-api-requirements.md) §2.2 |
| `correction` | For corrections: the state in [0003](../decisions/0003-statement-ui.md) §4 (active, redundant or dangling) |
| `visibility` | Which fields are hidden. For `erased` rows, the reason class, if the viewer holds `ts-viewerasures` ([09](09-security-and-moderation.md) §6.6). |
| `links` | Diff, history, job, and upstream URLs |

An adopted revision is an `edit` row at its source time, with the `bot` flag, the `job:{id}` tag and an `adopt` tag, and `new` set on its page's first revision ([0035](../decisions/0035-adopting-a-wikibase.md) §6; §2.2); a fork's `create` is an `edit` with `new` set ([0054](../decisions/0054-forking-a-mirrored-page.md) §1). The routes that serve the row are in [18](18-api.md).

### 3.2 What reads log events

*Sources: [0011](../decisions/0011-logs.md) §9.*

- **The activity projection** (`view.activity`, [03](03-storage-caches-and-search.md) §4.6) indexes the local log graph's events and the projected local events. It never holds an upstream log event, so it grows with local activity and the number of jobs, not with the mirrors.
- **Upstream log events are rows of `view.entity_history`**, not of `view.activity`. `view.entity_history (tenant, canonical_id, time, partition, offset, kind, member)` is written by composition ([03](03-storage-caches-and-search.md)) and is the one backing of a mirrored entity's history: one row per record keyed to any member of the entity's cluster, in any source partition, and one per provider-log event keyed to a member, each naming its kind and the member it is keyed to. An entity's history page ([19](19-site-ui.md) §3.3) and `GET /entity/{id}/history` read it, so the upstream deletion, protection and redaction events of an entity appear beside its mirror records and local edits in one keyset query.
  - Recent changes shows local events only. Upstream log events stay out of it, as mirror syncs do.
- **`list=logevents`** serves local events from `view.activity` and, with `leprovider`, a provider's events from `view.entity_history`. MediaWiki's type and action strings are kept, and the new types in §2.3 are added. The provider filter defaults to local events. The API details are in [18](18-api.md).
- **A deleted entity's page** can still say what happened, for example "Deleted on Wikidata on {date} by {admin}: {reason}". The event is in the provider log, which `cascade` does not touch, and its row in `view.entity_history` is keyed to the entity.

## 4. Change feeds

*Sources: [0020](../decisions/0020-change-feeds.md) §1, §2, §3, §4.*

### 4.1 A feed is activity rows filtered by a target set

*Sources: [0020](../decisions/0020-change-feeds.md) §1.*

A **feed** is defined by three things:

- a **target set** (§4.2): which entities, pages, threads and talk pages it covers;
- **filters**: the row kinds of [0010](../decisions/0010-site-ui.md) §7 (edits, page edits, jobs, log actions, mirror syncs), namespace, actor, the bot, minor and new flags, the `patrolled` flag ([09](09-security-and-moderation.md) §6.4), a period, and for target-scoped feeds a **syncs** switch (§4.2);
- a **delivery** (§4.4): a page, an Atom document or a live stream.

Its rows are the activity rows of §3.1, redacted for the viewer as [0012](../decisions/0012-api-requirements.md) §8 requires ([09](09-security-and-moderation.md) §5.7), ordered by (time, partition, offset) with the continuation tokens of [0012](../decisions/0012-api-requirements.md) §2.3 when paged, and in commit order when streamed (§4.4). Contributions are the actor filter over the everything set, so they are a feed too.

**A deleted target's changes leave the lists** ([0020](../decisions/0020-change-feeds.md) §1, A19). Recent changes, contributions and watchlists, the everything, actor and watched sets, leave out the change rows (`edit`, `sync`, `erased`) of a page, thread or local entity that is deleted now, for every viewer, the deletion group included, as MediaWiki moves a deleted page's revisions out of `recentchanges`. Its log events stay, its deletion's among them. A one-target history and a talk page's composite history keep the rows, which is where `prop=deletedrevisions` and the deletion group read them ([09](09-security-and-moderation.md) §6.2). The test is made when the list is read, so an undeletion brings the rows back.

**Every list of changes the UI or the API shows is one of these.** No feature builds its own list. That is the rule that keeps a history, a watchlist and the downstream stream from drifting apart.

### 4.2 Target sets

*Sources: [0020](../decisions/0020-change-feeds.md) §2; [0028](../decisions/0028-tenancy-policy.md) §9; [0030](../decisions/0030-edit-filters.md) §6; [0060](../decisions/0060-scopes.md) §7; [0045](../decisions/0045-table-content-model.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §8; [0051](../decisions/0051-page-redirects.md) §9.*

| Set | Members | Source |
|---|---|---|
| **Everything** | every local record | — |
| **One target** | an entity and the members of its cluster; a document page; a thread; a talk page and the threads attached to it; an actor's user page and user talk page | `cluster_member` ([03](03-storage-caches-and-search.md) §4.4), `view.thread` ([03](03-storage-caches-and-search.md) §4.8) |
| **Related, outward** | for a document page or thread, what it links to; for an entity, the entities its statements, qualifiers and references point at, and the properties they use | `page_link`; the resolved JSON |
| **Related, inward** | for a page or entity, what links to or references it | `page_link_target`, `entity_ref` |
| **Watched** | the account's watch set (§4.3), each member expanded as a *one target* set | `private.watch` |
| **All tenants** | the union of every tenant's everything set, each row carrying its tenant, served at the farm base under `feeds.farm_wide` (§4.5) | every tenant's `view.activity` |
| **Filter hits** | the hits of one edit filter, of all filters, or of one actor, filtered by action taken (§4.6) | `view.filter_hit` |
| **Scope** | the one-target sets of every member of a scope, from its materialized members; bounded by `scopes.max_members` rather than `feeds.related_limit`, and carrying the scope's truncation notice | `view.scope_member` |

**A subject and its talk page are one target.** The one-target set for `Item:Q42` includes `Item talk:Q42` and every thread attached to it, home or listing; for `User:Example`, the user page and `User talk:Example`; for a board, the board and its threads ([14](14-discussions.md)). A thread is its own target, and a thread that moves, is listed or is detached leaves or joins a talk page's set at query time, so history and watches follow the talk page, not the thread.

**Related changes are a join, not a list.** The relation is evaluated inside the query against the period window, as MediaWiki joins `pagelinks`, and is never materialized. A relation with more than `feeds.related_limit` members (site configuration, default 5,000) is truncated to that many, and the feed says so. This is the one place the model has a scale edge, and it is MediaWiki's edge too: inward relations of `Q5` are unbounded, and nobody's related-changes page can be.

**A scope is a target set** ([0060](../decisions/0060-scopes.md) §7; [15](15-structured-pages.md)). Related changes of `Scope:Women writers` is the project's recent changes: a join against materialized rows, not a relation evaluated in the query, which is why `feeds.related_limit` does not apply and `scopes.max_members` is its bound. `Special:RecentChangesLinked/Scope:X` and the `rcscope` parameter of `list=recentchanges` serve it.

**A table's data is a feed** ([0045](../decisions/0045-table-content-model.md) §8; [15](15-structured-pages.md)). The links projection (`view.page_link`) records a link from the table to each row entity and each column property, after resolution, so "What links here" on an entity or property lists the tables that show it, `list=backlinks` returns them, and related changes from a table (*related, outward*) is the recent changes to its rows and its column properties. `tables.max_rows` defaults to `feeds.related_limit`, so the feed covers every row.

**Followed talk pages** ([0069](../decisions/0069-synchronized-talk-pages.md) §8; [13](13-mirrored-pages.md)). The one-target set of a talk page that follows a repository's talk page includes that page's `put` records in `pages/{repo}`, each a row "{n} upstream revisions to Talk:…" naming the foreign threads it changed (the `put`'s content part lists them); a foreign thread is its own target, the `put` rows that changed it. They are sync rows for the `syncs` filter below.

**Mirror syncs in target-scoped feeds.** Recent changes has no per-entity sync rows, because a sync of a million entities is one job row ([0010](../decisions/0010-site-ui.md) §7). A history does show them, because it is scoped to one target and draws its sync rows from that target's rows in `view.entity_history` (§3.2). A watchlist is scoped to the watch set, so its cost is bounded the same way, and it shows them too. The `syncs` filter therefore defaults **on** for a watchlist and a history, **off** for recent changes and related changes: a watch on a mirrored entity covers its syncs, because the person chose that entity, and "Wikidata changed it" is the thing they most want to know. They can turn it off.

**A move's redirect** ([0051](../decisions/0051-page-redirects.md) §9): the redirect `create` a move writes is one more row in recent changes, tagged `move-redirect`, grouped with the move in the UI as [0010](../decisions/0010-site-ui.md) §1 groups a job's rows. The filter side of that section is [09](09-security-and-moderation.md) §8.7.

### 4.3 The watch set is private state, not records

*Sources: [0020](../decisions/0020-change-feeds.md) §3, A18; [0060](../decisions/0060-scopes.md) §7; [0045](../decisions/0045-table-content-model.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §8.*

**A watch is a row in the `private` schema**, not a record in any partition. It is the same kind of thing as a session or an OAuth token: state that belongs to an account, that nobody else may see, and that has no public history. It follows every rule [07](07-actors-and-accounts.md) §1.6 and [03](03-storage-caches-and-search.md) §1.2 set for that schema: never projected, never exported, never placed on a feed, readable only by the accounts role, excluded from every bundle.

```sql
CREATE TABLE private.watch (
  actor_key text NOT NULL, target_kind smallint NOT NULL, target_id text NOT NULL,
  added timestamptz NOT NULL, expires timestamptz,
  seen timestamptz,                -- MediaWiki's wl_notificationtimestamp: newest change the holder has seen
  notify boolean NOT NULL DEFAULT false,   -- deliver changes as notifications (0021 §2)
  PRIMARY KEY (actor_key, target_kind, target_id)
);
CREATE INDEX watch_expiry ON private.watch (expires) WHERE expires IS NOT NULL;
CREATE INDEX watch_target ON private.watch (target_kind, target_id) WHERE notify;   -- the addressing projection's lookup (§5.2)
CREATE TABLE private.watch_token (actor_key text PRIMARY KEY, token text NOT NULL, issued timestamptz NOT NULL);
```

`watch_target` exists for one reader: the addressing projection (§5.1), which for every activity row must find every account that watches the row's target, its cluster members or its paired talk page with `notify` set, without scanning across accounts. The watchlist itself is read by account through the primary key.

- **Target kinds** are `entity`, `page`, `thread`, `actor`, `title`, `scope` and `rows`. A row stores them in `view.activity`'s numbering: an entity by ID, a page by page ID (a thread and a board are pages), an actor by key, and a **`title`** as `{ns}:{title}` in the subject namespace, for a title no page holds yet or one the watcher may not see, which covers the page once there is one and the watcher may read it ([0020](../decisions/0020-change-feeds.md) A18). A `scope` watch expands at query time to the Scope set of §4.2, one row however many members the scope has, and it follows the members as they change; a `rows` watch on a table expands the table's `rows` the same way, as a scope would, whatever its kind ([0060](../decisions/0060-scopes.md) §7). An entity watch stores the entity ID and is expanded through its cluster when the feed is read, so a merge or a redirect never loses it. A `page` watch is a document page, so watching a table watches its definition, as watching any page does ([0045](../decisions/0045-table-content-model.md) §8); a `thread` watch is a thread; an `actor` watch is the actor's user page and user talk page. Watching a talk page is watching its subject, and watching a subject watches its talk page (§4.2): the UI's watch star on either page toggles the same row.
- **What a page is watched as.** An entity's page and its talk page as the entity; a user page, a user talk page and a thread homed there as the user; a talk page, a board's or a subject's, as its subject; a document page, a board or a thread as itself. A page the watcher may not see is watched as its title, so watching it answers exactly as watching a page to come ([09](09-security-and-moderation.md) §5.3; [0020](../decisions/0020-change-feeds.md) A18).
- **Expiry** is optional, with MediaWiki's choices (a week, a month, three months, six months, permanent). A daily sweep deletes expired rows; until it runs, a lapsed row is treated as gone.
- **`seen`** is the newest change the holder has seen, and drives the "changed since you last looked" mark: a change is unseen when it is newer than `seen`, or than when the watch was added, and is not the holder's own. Visiting the page, **Mark all changes as seen** on `Special:Watchlist`, `action=setnotificationtimestamp` and `POST /watchlist/seen` move it to now, never back ([0020](../decisions/0020-change-feeds.md) A18). It is written often and matters little, which is why it lives here and not in a record.
- **Auto-watch** is a preference: pages and entities the account edits, pages it creates, threads it posts in, and threads it starts, each on or off, and whether auto-watches carry `notify`. These are the `autowatch.*` keys of the preference store ([07](07-actors-and-accounts.md) §8.1), which holds every preference beside the watch set. They apply through MediaWiki's `watchlist` parameter, whose default is `preferences`, on `action=edit`, Wikibase's writes and the thread routes: a target already watched keeps its watch, and a new auto-watch carries `notify` when `autowatch.notify` is on ([0020](../decisions/0020-change-feeds.md) A18).
- **`notify`** marks a watch whose changes are also delivered as notifications (§5.2): DiscussionTools' topic subscription, generalized to every watchable target. It is refused on a `scope` or `rows` watch (`ts-watch-notify-unsupported`): those watches are feed filters, not subscriptions.
- **Watcher counts are never shown**, to anyone, at any threshold. MediaWiki shows a count once it passes thirty; the accounts graph's promise is stricter and this follows it. `prop=info` returns `watched` and `notificationtimestamp` for the viewer's own watch and no `watchers` field.

**Watching a foreign page's title, a fork that follows, or a foreign thread** keeps its talk page mirrored ([0069](../decisions/0069-synchronized-talk-pages.md) §8; [13](13-mirrored-pages.md)).

**What this costs.** The watch set is not in the log, so it is not rebuilt from it: it survives on the strength of `private`'s backups, like sessions and bindings. It does not travel in a tenant's public bundle ([08](08-tenants-and-instances.md) §6.2), because nothing private does; on a cooperative move it travels in the private extract and is applied when the account is reclaimed, and a user can always take their list with `GET /account/export` or `list=watchlistraw` and re-import it ([07](07-actors-and-accounts.md) §8.4, §8.5). Rehosting an instance whole carries it ([08](08-tenants-and-instances.md) §6.7).

**Not yet** ([0020](../decisions/0020-change-feeds.md) A18): `private.watch_token` and the Atom watchlist, with `wlowner`/`wltoken`; `scope` and `rows` watches; the `watchlist` parameter of `action=move`.

**Serving respects the schema boundary.** `triplespace-accounts` is the only code that reads `private.watch`. It resolves the holder's watch set to a list of targets; `triplespace-projections` expands the list from `view` to the entities and pages it covers, each with the watch it is there for, and hands that to the activity query, which runs under the public role against `view` alone. The join happens in the query as a target-set parameter, never as a cross-schema join. The notifier asks `triplespace-accounts` who watches a change's targets with `notify` ([0020](../decisions/0020-change-feeds.md) A18).

### 4.4 Delivery: page, Atom, stream

*Sources: [0020](../decisions/0020-change-feeds.md) §4; [0083](../decisions/0083-write-path-in-three-tiers.md) §7.*

Every feed can be delivered three ways.

| Delivery | Everything | One target | Related | Watched |
|---|---|---|---|---|
| **Page** | `Special:RecentChanges` | the History tab | `Special:RecentChangesLinked/{title}`, with a *changes to pages linking here* switch | `Special:Watchlist`, labelled **Private** ([0010](../decisions/0010-site-ui.md) §1, principle 6) |
| **Atom** | `?feed=atom` on each, as MediaWiki offers | | | with a per-account token (`private.watch_token`), shown and resettable in `Special:Account` |
| **Stream** | `GET /activity/stream` for any feed specification, as server-sent events, with `Last-Event-ID` carrying the resume token | | | authenticated, never by token |

**A page and a stream carry the same rows in different orders.** A paged list, its Atom form included, is ordered by (time, partition, offset), the display order of §4.1, where the time is the event's time at its source; a consumer that resumed from such a key would never see a row whose source time sorts before it and which committed after it, which imported revisions, adopted history, seeded fork history and upstream events all do. The stream is therefore ordered by commit, and its **resume token is a vector of per-partition high-water marks**, `{partition: offset, …}`: within a partition, offsets are commit-ordered under the append lock ([01](01-log-and-records.md)), so a consumer holding the vector has seen exactly the rows at or below each mark and resumes from it without loss or repetition. `Last-Event-ID` carries that vector, opaquely encoded; the continuation token of a page carries the sort key of §4.1, and the two are never interchanged. MediaWiki's `rccontinue` has the same edge, and paged lists keep it.

**The stream is the downstream change feed.** It is the everything feed delivered live, its events are activity rows redacted exactly as any response is, and it carries `erased` rows and visibility changes so that a consumer can follow an erasure as §1.5 has this instance follow upstream. It is the same mechanism as `GET /jobs/{id}/events` ([0012](../decisions/0012-api-requirements.md) §5), and a consumer that missed events resumes from its last token. A consumer tenant on another instance syncs from a provider tenant this way: a sync job follows the stream filtered to `source = local`, which may then carry each row's record body, resuming from the per-partition token and verifying against the provider's checkpoints ([08](08-tenants-and-instances.md) §4.4, [17](17-federation-and-publication.md) §1.2); on the same instance, reading is direct ([08](08-tenants-and-instances.md) §4.1).

**The activity stream is for consumers of change sets and events.** A triplestore that wants to follow the resolved view as triples uses the SPARQL Update stream, `GET /updates/stream`, which shares this route's transport and `Last-Event-ID` discipline and carries `DELETE DATA`/`INSERT DATA` bodies instead of activity rows ([02](02-graphs-rdf-and-query.md) §6.4).

**Pages may go live.** A feed page has a *live updates* switch that subscribes to its own stream, as MediaWiki's recent changes does.

### 4.5 Farm-wide feeds

*Sources: [0028](../decisions/0028-tenancy-policy.md) §9.*

With `feeds.farm_wide` other than `off`, the feed model gains the target set **all tenants** of §4.2: the union of every tenant's everything set, each row carrying its tenant, served at the farm base as `Special:GlobalRecentChanges`, `?feed=atom` and `/activity/stream?set=farm`, with a tenant filter. `global-groups` limits it to members of a global group, which is what support volunteers use; `everyone` opens it. The same two gates apply to farm-wide search, `search.farm_wide` ([03](03-storage-caches-and-search.md) §11.4). Redaction is per viewer on the tenant each row belongs to, so a deletion on one tenant hides the row for everyone outside that tenant's deletion group, whatever farm-wide view it appears in. The tenancy switches themselves are in [08](08-tenants-and-instances.md) §5.

### 4.6 The filter log is a feed

*Sources: [0030](../decisions/0030-edit-filters.md) §6.*

**Filter hits** are a target set of §4.2: the hits of one filter, of all filters, or of one actor, filtered by action taken, delivered as a page (`Special:EditFilterLog`), Atom or stream like any feed, and redacted per viewer as the privacy rules of [09](09-security-and-moderation.md) §7.5 require. A hit row links to the filter, the actor, the record if one exists and the diff, and shows the public variables. `list=abuselog` serves the same rows in MediaWiki's shape ([18](18-api.md)).

**Testing.** `Special:EditFilter/test` evaluates a draft rule against the activity rows of a period, rebuilding each row's context from its record and the actor's state as of its time, the same as-of evaluation autopatrol uses ([09](09-security-and-moderation.md) §6.4). It reports which rows would have matched. It runs asynchronously as a job in `ops`, since a period may hold many rows, and it never takes an action.

## 5. Notifications

*Sources: [0021](../decisions/0021-notifications.md) §1, §2, §3, §4, §5.*

### 5.1 A notification is an activity delivered to an inbox

*Sources: [0021](../decisions/0021-notifications.md) §1.*

Every local account has an **inbox**. An activity row (§3.1, §4.1) is delivered to an inbox when an **addressing rule** (§5.2) says the account is a recipient. A notification is one inbox row: the activity, the recipient, the reason it was addressed, and its seen and read state. In AS2 terms, the reasons of §5.2 compute the activity's `to` and `cc`; the inbox is the account's `as:inbox`.

**Addressing is a projection.** It runs over activity rows after they are written, like every other projection ([03](03-storage-caches-and-search.md) §6.1), and is idempotent on (activity, recipient, reason). Nothing about who was notified is a record: an inbox is private state (§5.3), for the reason the watch set is.

**The inbox is a feed** in §4.1's sense: rows ordered by (time, partition, offset), with a per-row read state and a per-account seen time, delivered as a page, a bell, an email, or a fediverse message (§5.4).

### 5.2 Addressing rules

*Sources: [0021](../decisions/0021-notifications.md) §2; [0061](../decisions/0061-sprints-and-tasks.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §8; [0060](../decisions/0060-scopes.md) §7; [0040](../decisions/0040-instance-prerogatives.md) §7.*

| Reason | An account is addressed when | Echo's name |
|---|---|---|
| `mention` | A post ([14](14-discussions.md)) or a document page revision links to its user page with `[[User:Name]]`, resolved by the title resolver; in AS2 terms, an `as:Mention` tag whose `href` is the actor IRI ([07](07-actors-and-accounts.md) §1.2) | mention |
| `reply` | A post's `inReplyTo` names a post attributed to it; or a synced comment on a followed talk page answers a comment by the upstream account the account's upstream grant acts as ([0069](../decisions/0069-synchronized-talk-pages.md) §8) | (DiscussionTools) |
| `talk` | A thread is created on, or a post is added to a thread attached to, its user talk page: the thread's target is `actor:{its key}` ([14](14-discussions.md)) | edit-user-talk |
| `watch` | A change reaches a target it watches with **`notify`** set (`private.watch`, §4.3). This is DiscussionTools' topic subscription, generalized to every watchable target except `scope` and `rows` watches, on which `notify` is refused, and it inherits the subject-and-talk pairing and the sync default of §4.2 | (DiscussionTools subscription) |
| `job` | A job it ran, or that ran on its behalf as operator ([07](07-actors-and-accounts.md) §4.6), finished, failed or was reverted (§2.2) | — |
| `rights` | Its group memberships changed, or it was blocked or unblocked ([09](09-security-and-moderation.md) §3.3) | user-rights |
| `group-join` | It is a member of a group whose record sets `notify_joins` (09 §3.3), and another account joined that group by any route, a self-grant under `userrights` or a delegated `adds` list included. Joining a group that includes it does not count: only the joined group's own setting decides ([0021](../decisions/0021-notifications.md) A19) | — |
| `rename` | Its actor record changed its name; the performer is the renamer or the instance operator ([0040](../decisions/0040-instance-prerogatives.md) §7) | — |
| `thread-status` | A thread it started moved from an open to a closed status ([14](14-discussions.md)) | — |
| `filter` | It is a member of a group an edit filter's `notify` action names, and the filter matched ([09](09-security-and-moderation.md) §7.4) | — |
| `task-resolved` | It holds a live claim on a sprint task that someone else's write resolved; the activity row is the resolving record, and the item carries the subject, the rule and the resolver ([0061](../decisions/0061-sprints-and-tasks.md) §8; [15](15-structured-pages.md)) | — |
| `proposal-state` | It proposed a change whose state became `adopted`, `partly adopted`, `reverted` or `declined`; the activity row is the mirror record or status post that changed it ([0067](../decisions/0067-proposals.md) §5; [14](14-discussions.md)) | — |

- **Nobody is notified of their own action**, nor of their subsidiary's, which is theirs ([0021](../decisions/0021-notifications.md) A20). A subsidiary has no inbox of its own: its operator is notified of its jobs, of mentions of it, of its `rights` changes, and of `talk` messages on its user talk page, since that is who can answer them ([07](07-actors-and-accounts.md) §4.6).
- **Only local accounts have inboxes.** A mention of a foreign actor, such as a Wikidata editor's IRI, addresses nobody here. A thread notification goes to a registered account, or to a bot's operator; a temporary account receives none while [0021](../decisions/0021-notifications.md) Q3 is open ([0021](../decisions/0021-notifications.md) A20).
- **Across tenants**, under `notifications.cross_tenant = home`, a person's bell aggregates the inboxes of every tenant account linked to their farm account, and a mention that resolves to a farm account is delivered to the inbox of the person's home tenant ([08](08-tenants-and-instances.md) §10.2). Under `off`, the rule above stands.
- **Auto-subscription** follows §4.3's auto-watch preferences: an account may choose that threads it starts and threads it posts in are watched with `notify`. Sending a reply or a new section through the instance to a followed talk page auto-watches the foreign thread with `notify` when the person's auto-watch preference for threads they post in is on ([0069](../decisions/0069-synchronized-talk-pages.md) §8).
- **Hidden and erased content is not delivered**, and an already-delivered notification whose activity is later hidden or erased leaves every inbox: the addressing projection deletes the rows of a record that a `record` ACL hides in any part, the inbox's own step of the purge that clears caches ([03](03-storage-caches-and-search.md) §9.6), and every read of an inbox leaves out an item whose record is hidden in any part or erased, its counts included ([0021](../decisions/0021-notifications.md) A20).
- **A target the addressed account may not read addresses nobody.** A mention, `talk` message or `watch` change on a page, thread or entity under a confidential `read` restriction the account does not satisfy is not delivered, and a delivered notification whose target becomes restricted leaves the inbox ([09](09-security-and-moderation.md) §5.3). Addressing checks the recipient's reading as it delivers; a `read` ACL on a thread deletes its rows from the inboxes of those who may no longer read it; and every read of an inbox checks again, leaving out an item about a thread the reader may not read now or whose talk page is hidden from them, a confidential page's or a hidden user's, so a restriction that reaches a thread through its talk page, namespace or tenant takes effect without a purge ([0021](../decisions/0021-notifications.md) A20). A watch on such a target stays in `private` and produces nothing until the account may read it again.
- **Thread records** ([0021](../decisions/0021-notifications.md) A20) are read against the thread as it stood before them. A **mention** is a link to a user page itself, not a subpage, in a post's text: a markdown post's `[[wiki links]]`, so a link in code is not one, and a wikitext post's links as written, templates unexpanded; a post's first 50 are mentions, Echo's limit, and the rest are not. An **edit** addresses only the mentions it adds, and no other reason. **`talk`** is a new thread or post on a user talk page, not an edit there. **`thread-status`** addresses whoever wrote the thread's first post, when a post moves its status from an open category to a closed one ([14](14-discussions.md) §2.1). **Imported** threads and posts address nobody, and a reply to an imported post is no `reply` to the importer. **An account is addressed once per record**, for the first reason that applies in the order `talk`, `thread-status`, `reply`, `mention`. `watch` waits for the watch set of §4.3.
- **`watch`** ([0021](../decisions/0021-notifications.md) A21) is delivered for a new thread or post, to the watchers of the thread and of the subject of every page it is attached to, and for any record of a document page, to the watchers of the page and of its title; an entity's own changes wait for a decision on mirror syncs. It comes last: an account a thread record addresses for another reason is not addressed again.
- **Bundling** is presentational: rows with the same (reason, target) within a window are shown as one item with a count, as Echo bundles. The rows stay separate.
- **A scope watch is not fanned out per member**: it is a watchlist filter, not a subscription to each member, and `notify` is refused on it with `ts-watch-notify-unsupported` ([0060](../decisions/0060-scopes.md) §7).
- **Notifications that an instance act triggers on a guest** (a rename, a block) name the instance operator ([0040](../decisions/0040-instance-prerogatives.md) §7; [08](08-tenants-and-instances.md) §8.2).

### 5.3 The inbox is private state

*Sources: [0021](../decisions/0021-notifications.md) §3.*

```sql
CREATE TABLE private.inbox (
  actor_key text NOT NULL, id bigint GENERATED ALWAYS AS IDENTITY,
  partition bigint NOT NULL, "offset" bigint NOT NULL,      -- the activity row
  reason text NOT NULL, target_kind smallint, target_id text,
  published timestamptz NOT NULL, read_at timestamptz,
  PRIMARY KEY (actor_key, id), UNIQUE (actor_key, partition, "offset", reason)
);
CREATE TABLE private.inbox_state (actor_key text PRIMARY KEY, seen_at timestamptz);
```

The reason-by-channel **preference matrix** is the set of `notifications.{reason}.{channel}` keys in `private.preference`, which the addressing projection reads ([07](07-actors-and-accounts.md) §8.1; [23](23-configuration-and-registry.md) §3.7). A reason a later ADR adds, such as `task-resolved`, joins the matrix under the `*` defaults ([0061](../decisions/0061-sprints-and-tasks.md) §8). The rules of [03](03-storage-caches-and-search.md) §1.2 apply to the inbox: readable by the accounts role only, never exported, never in a tenant's public bundle. It is portable state ([07](07-actors-and-accounts.md) §8.3): it travels in the user data bundle and in the private extract of a cooperative tenant move. Rows are deleted after `notifications.retention` (site configuration, default 90 days; read rows sooner). An inbox is rebuildable from the activity rows and the addressing rules for the retention window, except for read state, which is lost on a rebuild and costs only bold text, as `seen` does in §4.3.

### 5.4 Channels

*Sources: [0021](../decisions/0021-notifications.md) §4.*

Each reason can be delivered on each channel, subject to the account's preference matrix (§5.3) and the instance's defaults.

| Channel | Delivery |
|---|---|
| **Web** | The bell in the global header ([19](19-site-ui.md)), with the unseen count; `Special:Notifications`, which is the inbox as a page with mark-read and mark-all-read |
| **Email** | Immediate, or a daily or weekly digest, to an address held in `private` and verified by a token link. No address is ever shown or exported |
| **Fediverse** | A direct message to the account's registered fediverse account (§5.5) |

Every channel renders the same item: who, did what, where, when, and a permalink (`Special:PermanentLink/{revid}` for a post, the job or log page otherwise), in the recipient's interface language.

**Not yet.** Web Push is not in [0021](../decisions/0021-notifications.md) (its Q1).

### 5.5 The fediverse channel

*Sources: [0021](../decisions/0021-notifications.md) §5.*

This is the smallest step ActivityPub allows: the instance **sends** activities and accepts only what it needs to be allowed to send. Nothing here makes any local user, page or thread visible to the fediverse; the actors a tenant may have beyond the notifier, and ActivityPub proper, are [17](17-federation-and-publication.md).

**The notifier actor.** Each tenant has one ActivityPub actor, of type `as:Service`, at `{base}/notifier`, discoverable by WebFinger as `acct:notifier@{host}`. Its document carries `inbox`, `outbox` (empty), `preferredUsername`, `name`, `summary` explaining what it is, and `publicKey`. Its keypair is generated at tenant creation and held in `private.ap_key`; it is unrelated to the instance key of [01](01-log-and-records.md) §4.3, whose job is checkpoints, and it does not travel when a tenant moves, since nothing private does ([08](08-tenants-and-instances.md) §6.2): a moved tenant's notifier is a new actor, and its users re-register. The notifier is one actor among those a tenant may have: local accounts and talk pages may opt in as `as:Person` and `as:Group` actors, each with its own keypair in `private.ap_key`, keyed by actor, and ActivityPub delivery for all of them is `triplespace-federation`'s ([0022](../decisions/0022-federation.md) §6, §13).

**What the notifier accepts.** Its inbox handles `Follow` (answered with `Accept`, and the follower recorded in `private.ap_follower`), `Undo` of a `Follow`, and nothing else: every other activity is acknowledged with 202 and dropped. Following the notifier is what lets a Mastodon account receive its messages when the account filters messages from strangers, and the registration flow says so.

**Registering an account.** In `Special:Account`, under a **Notifications** section labelled Private ([19](19-site-ui.md)), a user enters a fediverse handle. The instance resolves it by WebFinger, fetches the actor document (with a signed request, since many servers require one), stores the actor and inbox IRIs in `private.fediverse_handle`, and sends one direct message containing a confirmation link with a token. Clicking it marks the handle verified. Until then nothing else is sent.

**What is sent.** One `Create` of an `as:Note` per notification or bundle, addressed only to the recipient: `to` is their actor IRI, there is no `as:Public` and no followers collection, and the Note carries an `as:Mention` tag for them, which is what Mastodon requires to show a message as a direct message. `content` is the rendered item of §5.4 as HTML with a permalink; `attributedTo` is the notifier; `published` is the notification's time. There is no `inReplyTo`, and the Note is not a post on this wiki: it is the notifier speaking. Delivery is an HTTP-signed `POST` to the recipient's inbox with a `Digest` header, using the HTTP Signatures profile Mastodon accepts (draft-cavage; RFC 9421 and FEP-8b32 integrity proofs are implemented behind `federation.accept_*` switches, off until Mastodon accepts them, [0022](../decisions/0022-federation.md) §8), through the delivery queue `ops.delivery` ([03](03-storage-caches-and-search.md) §4.16) with exponential backoff, giving up after seven days; a handle that fails repeatedly is marked broken and the account page says so. Bundled notifications are sent as one message, and no account is sent more than one message per `notifications.fediverse_interval` (default five minutes).

**Privacy.** The handle is private in this instance's sense: never shown, projected or exported, and unrelated to the public account links of [07](07-actors-and-accounts.md) §3.1. But a message that says "Example replied to you on Librarybase" is delivered to another server, which thereby learns that this fediverse account belongs to a Librarybase user, and stores the message under its own rules. The registration form says this in plain words. The channel is off until a user registers a handle, and an instance may disable it, or restrict it to a domain allow-list, in site configuration.

**Discovery endpoints**, all public and read-only: `/.well-known/webfinger` (answering only for the notifier), `/.well-known/nodeinfo` with a minimal NodeInfo 2.1 document naming the software, and the notifier's actor document, served as `application/activity+json`. WebFinger answers for a local user, and their actor document is served, once they have opted in ([0022](../decisions/0022-federation.md) §6); otherwise a lookup of `acct:example@{host}` returns 404.

### 5.6 Sprint notifications and the sprint page

*Sources: [0061](../decisions/0061-sprints-and-tasks.md) §8.*

A sprint ([15](15-structured-pages.md)) adds one reason, `task-resolved` (§5.2), addressed to the claimant of a task that someone else's write resolved; the activity row is the resolving record, and the item carries the subject, the rule and the resolver. Nothing else notifies; a sprint's progress is read from its page and its scope's feed. The reason joins the `notifications.{reason}.{channel}` matrix of §5.3 under the `*` defaults.

**The page** shows, above the tabs: the description, the window and its state, a progress bar (resolved in window / total), one count per rule, and the **leaderboard**: actors by resolutions in the window, with their counts per rule, linking to their contributions filtered to the sprint's scope. Then tabs: **Tasks** (the board: Open, Claimed, Done; filter by rule, by claimant, by subject label; each task showing the subject's label and description, the rule's title, the claimant, and the rule's **action**), **Definition** (the JSON source editor), **Related changes** (the scope's, §4.2), and the usual talk and history. A `board` named in the definition is linked beside the talk tab.

**Actions** open the existing editing surface positioned on the task:

| Rule | Action |
|---|---|
| `missing-statement` | The subject's page with the statement UI ([0003](../decisions/0003-statement-ui.md)) adding a statement of the property, the value pre-filled when the rule names one |
| `constraint-violation` | The subject's page with the violating statement's popover open ([0031](../decisions/0031-property-constraints.md) §3) |
| `missing-page` | The page editor for a new page in the rule's namespace, titled from the subject's label in the content language, with the sitelink to the subject written on save ([0038](../decisions/0038-page-metadata-and-categories.md) §6), so the pairing that resolves the task is made in the same flow. A **Link existing page** alternative writes the sitelink alone |
| `query` | The subject's page |
| `schema-violation` | The subject's page at the first failing constraint, with the reason ([0064](../decisions/0064-entityschema-and-validation.md) §6) |

**Claim** and **Release** buttons on each task write the records of [0061](../decisions/0061-sprints-and-tasks.md) §5, which project as `task/claim` and `task/release` (§2.3). A claimed task shows who holds it and when the claim expires.

**In a project page.** Transcluding a sprint's progress bar or task board into `Project:WikiProject Women` is the workspaces ADR's job, with tables and board listings ([15](15-structured-pages.md)); nothing here prevents it.

## 6. The Activity Streams profile and thread activity

*Sources: [0019](../decisions/0019-discussions.md) §7, §10; [0049](../decisions/0049-boards.md) §9.*

### 6.1 Thread records as activity rows

*Sources: [0019](../decisions/0019-discussions.md) §7; [0049](../decisions/0049-boards.md) §9.*

**Every thread record is an activity row** (§3.1) of kind `edit` on the thread's page ID, with the operation in the parsed summary. A thread's history is its rows. `Special:Contributions` and `list=recentchanges` show posts as edits to `Thread:` pages, with the talk page in the row's `target`.

**A talk page's or board's history is composite:** the rows of every thread currently attached to it, home or listing, plus the `thread/attach`, `thread/detach` and move events, in one sequence ordered by time ([14](14-discussions.md)). It is served by `GET /page/{id}/history` for the talk page's ID and by `prop=revisions` on the talk title, where each row is a revision with its own global revision ID. A thread that moves away takes its rows with it.

**A talk page's revisions have content** ([0019](../decisions/0019-discussions.md) A29). `rvprop=content` on a talk title gives the talk page **as wikitext as of that revision**: the threads attached to it now, in the order the page lists them (pinned first, then by attachment), each as it stood after the revision, a thread not yet started being left out. Each thread is a `== Subject ==` section; each post's lines are indented with one `:` per level and end with the signature `~~~~` would have written, from the post's attestation; a post keeps the markup it was written in; an imported post already carries its signature and a heading post is a heading. What the viewer may not see reads as MediaWiki writes it: `(comment removed)`, `(username removed)`. The content model stays `triplespace-talk`, the format is `text/x-wiki`, and the text is never a page source. `action=compare` diffs it line by line. A revision named alone (`fromrev`) is its thread's; named together with the talk page (`fromtitle` or `fromid`), it is the talk page's as of that revision, and `torelative` moves along the talk page's revisions.

**Log events** are projected from thread records as §2.3 says: `create` → `create/create`; `rename` and `move` → `move/move`; `attach` and `detach` → `thread/attach` and `thread/detach`; `pin` and `unpin` → `thread/pin` and `thread/unpin`; `delete/delete` and `delete/restore` from the thread's `read` ACL. `post` and `edit` are revisions, not log events. **A record that is also a log event takes a log ID beside its revision ID**, as a MediaWiki page creation has both, and its one activity row carries the log type, action and parameters beside the edit: `list=logevents` reads those rows with the `log` rows, and a history or recent changes shows them as revisions. The parameters name the targets and the talk page IDs the record minted or found. Posts and thread operations are patrollable, as every `edit` row in `pages` is ([09](09-security-and-moderation.md) §6.4).

**Hiding and erasure** are the existing mechanisms. `deleterevision` hides a post's text, comment or actor; `erase` with `parts: [text]` removes the text for good. In either case the post stays in the tree as a tombstone: its place, its replies and its timestamp remain, its text does not.

### 6.2 The Activity Streams profile

*Sources: [0019](../decisions/0019-discussions.md) §10.*

Posts, threads and talk pages are described with the Activity Streams 2.0 vocabulary because the RDF of log events already uses it ([02](02-graphs-rdf-and-query.md) §5.2) and because it is the natural JSON shape for a post. It is used as a **fixed profile**, as Mastodon uses it: the REST API and the RDF export emit these properties and no others, the OpenAPI document of [0012](../decisions/0012-api-requirements.md) §1 pins the shape, and nothing performs JSON-LD expansion. The rules:

- `id` is assembled from the header (the record IRI) and never appears in the content part.
- `attributedTo` and `published` come from the attestation part and the header, never from the content part.
- `content` is HTML with `mediaType` explicit; `source` carries the markdown. AS2's default of HTML for `content` is never relied on.
- `inReplyTo` and `context` are IRIs of records and pages of this tenant.
- `tag` holds `Mention` objects with `href` and `name`, derived from the text.
- `status` is `scatter:status`, a `scatter:` extension in the manner of [02](02-graphs-rdf-and-query.md) §3.1, since AS2 has no term for it.

**Federation is out of scope of the profile.** [0019](../decisions/0019-discussions.md) adopts the vocabulary, not the protocol; the inbox and outbox that [0022](../decisions/0022-federation.md) later added are [17](17-federation-and-publication.md).

### 6.3 Threads, posts and events in RDF

*Sources: [0019](../decisions/0019-discussions.md) §7; [0049](../decisions/0049-boards.md) §9.*

In the metadata graph only, using the profile of §6.2:

- **A thread** is `{base}/page/{page ID}`, an `as:OrderedCollection` of its posts, with `as:name` the subject, `as:context` its home's node, `scatter:listedOn` for each listing, `as:published`, and `scatter:status`.
- **A post** is the record's IRI `{base}/record/{partition}/{offset}`, an `as:Note` with `as:inReplyTo`, `as:context` (the thread), `as:attributedTo` (omitted when hidden, with `scatter:attributionHidden` as [07](07-actors-and-accounts.md) §1.2), `as:published`, `as:updated` when edited, `as:content` as HTML with `as:mediaType "text/html"`, `as:source` with the markdown and `as:mediaType "text/markdown"`, and `as:tag` nodes of type `as:Mention` for each resolved wiki link. A hidden or erased post is an `as:Tombstone` with `as:formerType as:Note`.
- **A talk page** is `{base}/page/{talk page ID}`, an `as:OrderedCollection` of its threads, with `scatter:subject` the document node it discusses.
- **Events** are typed as [02](02-graphs-rdf-and-query.md) §5.2 types them: `as:Create`, `as:Move`, `as:Delete`, `as:Undo`; `thread/attach` is `as:Add`, with `as:object` the thread and `as:target` the talk page or board, and `thread/detach` is `as:Remove`, with `as:origin` the page it left. Activity Streams has exactly these terms. `thread/pin` and `thread/unpin` are `as:Add` and `as:Remove` with `as:target` the page's featured collection ([0011](../decisions/0011-logs.md) §6.1; [0069](../decisions/0069-synchronized-talk-pages.md) §4).

The post's revision node of [02](02-graphs-rdf-and-query.md) §2.1 is the same IRI; the AS2 properties are added to it. Nothing about threads enters the main or resolved graph, except thread statements, which are output in the main graph as page statements are ([02](02-graphs-rdf-and-query.md) §5.4). The nodes of boards and of threads with several listings are in [02](02-graphs-rdf-and-query.md) §5.3.
