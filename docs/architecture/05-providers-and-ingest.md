# 05. Providers and ingest

This chapter describes where mirrored data comes from and how it enters the log: the split between who minted an ID and who asserts a triple, the source graphs and the resolved view built over them, what happens when upstream deletes, corrects or extends an entity, the one change-set format every write uses and the three paths that submit it, import jobs, adapters, bulk creation, bootstrap loading, key-mapped providers, shallow mirroring, the entity sources a tenant declares for itself, the adoption of an existing Wikibase, and the catalogue of registered providers. It assumes the log and its records from [01](01-log-and-records.md), the graph catalogue and graph IRIs from [02](02-graphs-rdf-and-query.md), the `view` tables and the bootstrap storage of [03](03-storage-caches-and-search.md), the ID forms, keyed types and identity clusters of [04](04-entities-and-identifiers.md), subsidiary accounts from [07](07-actors-and-accounts.md), and a tenant as a provider from [08](08-tenants-and-instances.md). The `scatter-adapter-*` crates are in [22](22-crates-and-stack.md); the settings named here are catalogued in [23](23-configuration-and-registry.md).

## 1. Origin and assertion

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §1–4, §9; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5.*

### 1.1 Where an ID was minted is separate from who asserts a triple

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §1.*

The three-letter prefix of a foreign entity records two independent facts, split apart:

- **Identity origin.** The prefix records who *minted* the ID. `WDQ123` was minted by Wikidata. `Q5` was minted by this instance.
- **Assertion source.** The named graph a triple lives in records who *asserts* it (§1.2).

An entity with ID `WDQ123` can have triples in the Wikidata mirror graph, in the local graph, or in both. A three-letter prefix means "minted by provider X". It does not guarantee that provider X still holds the entity (§2.1).

### 1.2 Source graphs and projections

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2.*

Every log record names the graph it writes to.

| Graph | Illustrative IRI | Written by | History in the log |
|---|---|---|---|
| **Local** | `{base}/graph/local` | Editors, local bulk jobs, and retention when it materializes data (§2.1) | Full |
| **Mirror**, one per provider | `{base}/graph/mirror/wikidata`, `{base}/graph/mirror/openalex` | Only that provider's sync jobs | Set per provider: `latest` or `full`. Retained entities always keep full history (§2.1). |
| **Derived**, one per extraction source of a tenant ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1) | `{base}/graph/derived/familysearch` | Only that source's extraction job | `latest` by default; a tenant may set `full` |
| **Metadata** ([0001](../decisions/0001-revision-metadata-rdf.md)) | `{base}/graph/metadata` | Projection | Derived |

The local, mirror and derived graphs are *sources*. The metadata graph and the resolved graph (§1.4) are *projections*. A tenant's entity sources add one further source graph each, `source/{name}` (§6.4). The full catalogue of graphs, with their names and IRIs, is in [02](02-graphs-rdf-and-query.md).

No editor or local bulk job can write to a mirror graph.

### 1.3 History policies and the two histories of mirrored data

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2.*

**How much mirror history the log keeps is a storage policy.** It does not limit what the model can represent. Each mirror graph has a history policy:

- **`latest`** compacts the log to the latest state of each entity, as Kafka's log compaction does. This is the practical default for full mirrors of very large graphs.
- **`full`** keeps every state the instance has received.

Individual entities can be exempted from compaction, and retained entities always are (§2.1). Under either policy, a tombstone for an entity that is not retained erases its mirrored history once it is compacted.

**The metadata graph describes mirrored data just as it describes local data.** There are two kinds of history for mirrored data:

- **Observed history** is the sequence of states the instance received from sync jobs. How complete it is depends on the sync. A weekly dump sync skips upstream revisions made between dumps, while a sync from an edit stream sees each one. For Wikidata, every observed state carries its upstream revision ID. Observed history is therefore a sparse subset of upstream history.
- **Upstream history** is the provider's own revision history. For Wikidata, that is each revision's ID, timestamp, user, edit summary and tags. It can be backfilled for an entity from the provider's API or its history dumps, which fills in the gaps in observed history. OpenAlex publishes no revision history, so only observed history exists for it.

### 1.4 The main graph is a resolved view

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5.*

The main graph, the Wikibase-compatible RDF of [0001](../decisions/0001-revision-metadata-rdf.md) §2, is **computed**. A reconciliation policy is applied over the source graphs to produce it. The result is still Wikibase-shaped, it is still what gets exported to QLever, and it is still rebuildable from the log. The source graphs may also be exported, for consumers who want to compare what the upstream source asserts with what the instance asserts.

**The resolved view is in source form.** It keeps every ID as the graph that asserted it wrote it: `WDQ5` where Wikidata wrote `Q5`, `Q9` where a local editor wrote `Q9`. Identity clusters do not rewrite it; the canonical ID is an attribute of the cluster, the cluster's fused body is composed once per cluster, and the form in which a consumer sees the IDs is decided on the way out ([04](04-entities-and-identifiers.md) §4.4, §4.12). Composition is a function of the contributing graphs' states, the policy record and the cluster map, run after the write that changed one of them, never inside it ([03](03-storage-caches-and-search.md)).

Reconciliation happens at the level of Wikibase statements, not individual triples. Statements, references and values are already IRI nodes, so one graph can annotate a node that another graph asserts.

Default reconciliation rules:

| What | Rule |
|---|---|
| Statements, references, aliases | Union across graphs. A statement IRI that appears in two graphs is one statement. |
| Labels and descriptions (one per language), statement rank | The local graph wins wherever it says anything. Otherwise the tenant's derived graphs supply it, in `reconcile` order ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §7), and otherwise the mirror graph's value is used. |
| Sitelinks | Union by normalized URL: a sitelink is a URL, and its site is its host. Where two graphs give one host different URLs for an entity, the local graph wins, then the provider order. Badges are the union. A denied host is left out of the resolved view ([0026](../decisions/0026-sitelinks.md) §3–4). |
| Suppressions | An explicit local-graph assertion removes a mirrored statement or term from the resolved view. |
| Truthy (`wdt:`) and normalized (`wdtn:`) triples | Computed only in the resolved view, from the resolved ranks. Never stored in a source graph. |

The provider order among registry providers is the instance's `reconcile` record's, the policy record under which every shared row is computed; a tenant's record may hide a provider from its own view and orders only its own entity sources, after the registry providers ([04](04-entities-and-identifiers.md) §4.10, §6.6). The shared rows of the resolved view name the policy record they were computed under.

### 1.5 IRIs for foreign entities and the provider registry

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4.*

- **The canonical IRI is the provider's own.** `WDQ123` projects to `http://www.wikidata.org/entity/Q123`, and `OAW123` projects to `https://openalex.org/W123`. A provider that publishes no IRIs takes a template minted here: GDELT's entities project to `https://scatter.red/gdelt/{event|document|cameo|theme}/{id}` (§8.10).
  - Nodes that come from upstream keep upstream's IRIs. Examples are Wikidata's statement nodes and mirrored property predicates such as `http://www.wikidata.org/prop/direct/P31`.
  - Statements that the local graph makes about a foreign entity get statement IRIs under the instance's base.
  - An item that a key-mapped provider maps onto a keyed entity takes the key's IRI instead, such as `https://scatter.red/domain/{key}`, and the resolved view emits `<upstream item IRI> owl:sameAs` it (§4).
- **`{base}/entity/WDQ123` is an alias.** The instance can dereference it, but it is not used as a subject in RDF.
- **The prefixed ID is the working name** in the API and UI, as in a page called `Item:WDQ123`. It exists because an ID is easier to work with than a full IRI.
- **The provider registry** (`providers.toml`, [23](23-configuration-and-registry.md)) records the following for each type code:
  - the canonical IRI template;
  - the ID rewriting between upstream and prefixed forms, such as `Q123` ↔ `WDQ123`: strip or add the prefix, and canonicalize the rest by the type's ID grammar ([0017](../decisions/0017-entity-id-grammar.md) §2);
  and, once per provider, the adapter that imports it (§3.4, §8.1).

  This parallels Wikibase's own entity sources (see [wikibase-compat.md §5.1](../api/wikibase-compat.md)).
- **A tenant can be a provider.** Its IDs are rewritten when read, not when written: another tenant's projections read its partition and rewrite `Q6` to `LBQ6`, and `{base}/entity/LBQ6` is an alias of the provider tenant's canonical IRI ([0018](../decisions/0018-tenants.md) §5; [08](08-tenants-and-instances.md)).

### 1.6 How the four scenarios resolve

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §9.*

The four scenarios of upstream deletion, rescue, correction and extension are worked through in §2; their resolutions are:

| | Resolution |
|---|---|
| **A** | A `tombstone` in the mirror graph, followed by each tenant's policy, `orphan` or `cascade`. Compaction erases the mirrored data. |
| **B** | A bulk `retain`. IDs and canonical IRIs are kept, and data is materialized into the tenant's local graph only when each tombstone arrives. |
| **C** | A local-graph `override` or `add` on the existing foreign entity. A local entity is needed only when the upstream identity itself is wrong. |
| **D** | A local-graph `add` of local properties onto the foreign entity. |

## 2. Upstream deletion, retention, conversion and correction

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5–7; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9; [0012](../decisions/0012-api-requirements.md) §2.2, §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §4; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §3.*

### 2.1 Tombstones and retention policies (scenarios A and B)

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5.*

When upstream deletes an entity, its sync job appends a tombstone to the mirror graph. Compaction later removes the mirrored data from the log. A faithful mirror therefore really forgets deleted data, including data upstream suppressed or took down. This resolves the deletion question **for mirrored data only**. A provider may instead opt into the keyed-style clear of §2.2, a `put` of the empty state in place of a tombstone, by the registry flag `deletion = clear` (§8.1); OpenStreetMap sets it (§8.7).

**The tombstone carries nothing about retention.** It is one record in a shared partition, and retention is per tenant per entity. What happens next depends on the entity's **retention policy** on each tenant that reads the provider. Each tenant sets a default, the `site` setting `retention.default` ([23](23-configuration-and-registry.md)), whose shipped value is `orphan`; `cascade` is never a default, and a tenant that wants it sets it. Editors can override the policy for individual entities.

| Policy | Mirrored data | Local-graph assertions about the entity |
|---|---|---|
| `cascade` | Erased | Retracted |
| `orphan` | Erased | Kept, but hidden from the resolved view |
| `retain` | Its history is kept, and its last state is **materialized into the tenant's local graph** when the tombstone is applied | Kept. The entity survives. |

**Applying the policy is the entity projection's work, per overlay.** When the tombstone lands, the projection applies each reading tenant's policy for the entity as a record in that tenant's partition, attested by the instance under an authority record ([08](08-tenants-and-instances.md)), with a `retention/apply` log event ([16](16-logs-feeds-and-notifications.md)); it runs as a tier-3 job after the tombstone's composition, never inside the sync's transaction ([03](03-storage-caches-and-search.md)). Under `retain` that record carries the materialized state; under `cascade` it retracts the tenant's assertions; under `orphan` it writes the hiding. **The compaction exemption is any tenant's `retain` row**: compaction of a mirror entity waits while any tenant of the instance holds `retain` on it (§2.2).

**Scenario A** is `orphan`, which keeps local annotations recoverable, or `cascade` where a tenant has chosen it.

**Scenario B** is a bulk operation that sets `retain` on a set of entities. Nothing about their data changes when the operation runs. The mirror stays live and keeps syncing for as long as upstream holds each entity, and data moves into the local graph only when a tombstone arrives. Copying at rescue time is not done: the copy would go stale while upstream keeps editing the entity, and because local values win in reconciliation, stale labels would override current ones; and a lazy reference to the mirror's state at some log offset would break once that state is compacted away.

### 2.2 Retention preserves history and identity

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5.*

Setting `retain` on an entity does two more things:

1. **It exempts the entity from compaction,** so every state observed from then on is kept. The exemption is instance-wide and holds while any tenant's `retain` row names the entity, since the mirror partition is shared (§2.1).
2. **It starts a job that backfills the entity's upstream history** (§1.3). This is time-sensitive. Once Wikidata deletes an item, its revisions are visible only to Wikidata administrators, so the backfill has to finish before the deletion. The backfill brings every revision's content by default, from the provider's API or, for a large set, from a history dump the operator supplies ([0015](../decisions/0015-record-format-and-partition-registry.md) §4; [01](01-log-and-records.md)).

When the tombstone arrives, the local graph continues the entity's history: the first local revision's `prov:wasRevisionOf` points at the last upstream revision. This gives the same continuity as MediaWiki's transwiki import, where imported revisions keep their original authors.

**A retained entity keeps its ID and its canonical IRI.** `WDQ123` stays `WDQ123` and keeps projecting to `http://www.wikidata.org/entity/Q123`. If upstream later restores the entity, the mirror graph fills in again next to the local copy. Identical statement IRIs collapse into one statement under §1.4.

**Retention does not override upstream hiding** ([0011](../decisions/0011-logs.md) §5). `retain` keeps an entity that upstream deletes, not fields that upstream hides for privacy or legal reasons. Once a retained entity has been deleted upstream and materialized into the local graph, it is local data, and hiding within it is a local decision.

**Keyed entities do not stop existing** when upstream deletes the item mapped onto them (§4.2). The mirror's contribution is cleared with a `put` of an empty state, not a `tombstone`, and nothing else happens: local assertions are never retracted or hidden because of an upstream deletion, so `cascade` and `orphan` do not apply. `retain` still applies: it keeps the mirrored history and materializes the last mirrored state into the local graph when the deletion arrives. A `tombstone` is not used because it applies retention policies that assume the entity is gone.

### 2.3 Converting a foreign entity into a local one

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §3.*

Editors may convert a foreign entity into a local entity.

- **A local ID is minted.** For example, `WDQ123` becomes `Q456`. `convert` links the two with `same-as`, and the local member is the cluster's canonical member, so the rest of this list follows from the cluster rules ([04](04-entities-and-identifiers.md)).
- **The two IDs are members of one cluster.** A request for `WDQ123` or for `Q456` returns the same fused body, under the requested ID and with `Q456` as its `canonical`; the site UI and a consumer that prefers the local form see `Q456` wherever either is a value ([04](04-entities-and-identifiers.md) §4.12). In RDF, the fused body is emitted under both IRIs and the resolved view emits Wikibase's redirect form, `wd:Q123 owl:sameAs <{base}/entity/Q456>`, where `wd:` is Wikidata's namespace.
- **Foreign statements stay attached.** The sync job keeps writing to the mirror graph under the foreign ID, nothing stored is rewritten, and the fused body of the cluster carries those statements. The same applies to any local-graph assertions previously made about `WDQ123`.
- **The local entity survives upstream deletion.** Its mirrored statements follow its retention policy, and converted entities default to `retain`.

Retention keeps the foreign identity. Conversion gives the entity a local identity, for things that are now curated primarily on the instance.

### 2.4 Properties a tenant depends on are retained automatically

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6.*

The first local statement, qualifier or reference that uses a mirrored property (`WDP31`, say) sets that property's retention policy to `retain`, with a `retention/set` log event ([0011](../decisions/0011-logs.md) §6.1) whose performer is the editor and whose comment says why. An upstream deletion then materializes the property's last state into the local graph, as §2.1 provides, so its label, description and data type survive and every local statement using it keeps rendering, validating and exporting unchanged; the property page says "Deleted on Wikidata on {date}" from the provider log ([0011](../decisions/0011-logs.md) §9). If upstream later **changes the data type** of a retained property, the new mirrored state conflicts with the retained one: the retained type wins for the tenant's existing statements, new statements are validated against it, and the conflict is listed for maintenance beside the corrections of §2.5 so an editor can convert or retire the property with `equivalent-property` ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §6). Adapter mappings onto a retyped property stop and are reported, as 0004 §6 says. A tenant may turn the automatic `retain` off in `site` configuration (`retention.auto_properties`) and accept dangling statements, which is Wikidata's own behaviour after a property deletion.

### 2.5 Corrections and extensions (scenarios C and D)

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §4; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §3.*

Every local edit to a foreign entity is a **local-graph assertion whose subject is the foreign entity**. No local entity has to be created. The local graph's current state for the entity, the accumulation of those assertions, is held in `view.graph_state`, written by the appending transaction for every graph whose records are deltas (`local`, `derived/*`, `source/*`) and read by composition and by the update stream's delta as the old state ([03](03-storage-caches-and-search.md)); a mirror graph's state is its latest `put`, which `view.entity_source` names.

**Correcting a value (scenario C).** A correction is a rank override, an added statement, a suppression, or any combination of the three. It is never a changed value under the mirrored statement's own ID: a write that changes the main snak, a qualifier or a reference of a statement a mirror graph owns is refused with **`ts-foreign-statement`**, naming the statement's graph and the two things the writer can do instead, suppress it and add its own (which the statement UI offers as one action, [19](19-site-ui.md)) or propose the change upstream ([0067](../decisions/0067-proposals.md), in [14](14-discussions.md)). Silently suppressing and re-adding under a new GUID would leave the client holding a GUID that no longer exists ([04](04-entities-and-identifiers.md) §4.9). The forms a correction takes:

```trig
GRAPH <{base}/graph/mirror/openalex> {
  s:OAW123-a1 ps:P577 "2019-01-01T00:00:00Z"^^xsd:dateTime ; wikibase:rank wikibase:NormalRank .
}
GRAPH <{base}/graph/local> {
  s:OAW123-a1 wikibase:rank wikibase:DeprecatedRank .      # overrides the upstream rank
  s:OAW123-b2 ps:P577 "2018-01-01T00:00:00Z"^^xsd:dateTime ; wikibase:rank wikibase:PreferredRank .
}
```

(`s:` and `ps:` are used loosely here. Mirrored nodes carry their provider IRIs, per §1.5.)

Local corrections are tracked against upstream:

- If upstream comes to agree with a correction, the application detects that the correction is redundant and retires it: automatically, as an instance act, when the agreement follows a proposal and `upstream.retire_adopted` is on, which is the default; otherwise from `Special:Corrections` ([0067](../decisions/0067-proposals.md) §5).
- If upstream removes the statement a local override points at, the override is flagged as dangling.
- The set of local corrections doubles as a list of fixes to report upstream.
- The set is served as `Special:Corrections`, filterable by state, kind and upstream graph, with **Retire** for redundant corrections and an export per upstream graph ([0047](../decisions/0047-special-pages.md) §6; [21](21-special-pages.md)).

`upstream.retire_adopted` is a tenant `site` setting ([23](23-configuration-and-registry.md)).

**Correcting identity.** If upstream has conflated two things under one ID, the split requires new local entities, with the split recorded in the local graph. If upstream has two IDs for one thing, a local redirect or same-as assertion is enough.

**Extending (scenario D).** Local properties are asserted on foreign entities in the local graph. Their statement IDs take the form `WDQ42$<uuid>` and their IRIs sit under the instance's base. The UUID keeps them from colliding with upstream statement IDs.

### 2.6 Mirror metadata that survives compaction

*Sources: [0012](../decisions/0012-api-requirements.md) §2.2.*

Under the `latest` history policy, a mirror keeps only the newest `put` for each entity. The history view ([19](19-site-ui.md)) needs facts about earlier states that compaction would otherwise destroy. Projections must be rebuildable from the log, so these facts have to be in the surviving record.

A `put` therefore carries these fields:

| Field | Meaning |
|---|---|
| `prev_upstream` | The upstream version of the state this `put` replaces, such as Wikidata's `lastrevid`. It bounds the live fetch of upstream edits (§2.7). |
| `first_seen` | When the instance first mirrored the entity. It is copied forward from the previous `put`. |
| `size`, `prev_size` | The sizes of the new and previous states ([0012](../decisions/0012-api-requirements.md) §2.4) |
| `changes` | A summary of what changed. For each property, term kind (labels, descriptions, aliases, by language) and sitelink site: counts of values added, removed and changed. |
| `delta` | Optional. The statement-level difference. It is present only when the provider's registry entry sets `sync_deltas = "full"`. |

**The default is `sync_deltas = "summary"`.** `sync_deltas` is a field of the provider entry in `providers.toml` (§8.1), with the values `summary` and `full`. Without a stored delta, the diff of a sync shows the summary and links to the provider's own diff between `prev_upstream` and the new version. For Wikidata, that is `index.php?diff={new}&oldid={prev}`.

**Summaries and deltas are computed at ingest,** by comparing with the entity's current mirrored state. In bootstrap mode (§3.8) there is no previous state, and these fields are left empty.

### 2.7 Fetching upstream edits live

*Sources: [0012](../decisions/0012-api-requirements.md) §6.*

- **The server fetches, not the client.** It uses the API endpoint in the provider registry, with a proper User-Agent and `maxlag` ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8).
- **The interval** runs from `prev_upstream` to the upstream version of the sync (§2.6).
- **Rows** are `upstream-edit` activity rows ([16](16-logs-feeds-and-notifications.md)). Actors are identified by their upstream IRIs ([0007](../decisions/0007-actor-identity.md) §2), summaries are parsed as in [0001](../decisions/0001-revision-metadata-rdf.md), and an actor linked to a local account carries a `linked` flag.
- **Results are cached briefly and rate-limited,** per user and per instance, in the `upstream` rate-limit class ([0024](../decisions/0024-subsidiary-accounts.md) §5; [07](07-actors-and-accounts.md)). They are never written to the log.
- **Errors:**

| Error | When |
|---|---|
| `upstream-unavailable` | HTTP 503 with `Retry-After`, when the provider cannot be reached |
| `upstream-unsupported` | The provider publishes no history, as OpenAlex does not |
| `upstream-truncated` | The interval holds more edits than the cap. The first rows are returned, with a link to the provider's history. |

- **Retained and fully mirrored entities** are served from the log instead (§3.3), and their rows are marked as stored. They are read from the upstream revision records ([0015](../decisions/0015-record-format-and-partition-registry.md) §4), as `upstream-edit` rows with `record` present; the live fetch covers only the interval those records do not.

## 3. The change-set format and the ingest paths

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8, §8.1–8.7; [0013](../decisions/0013-postgres-storage.md) §9; [0030](../decisions/0030-edit-filters.md) §7; [0035](../decisions/0035-adopting-a-wikibase.md) §3; [0058](../decisions/0058-packed-record-storage.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §5; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §2, §3, §5.*

### 3.1 One change-set format, three entry points

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8, §8.1.*

All writes use a single internal change-set type. There are three ways to submit one:

1. **An in-process or CLI ingester that writes straight to the log.** This is the path for full loads of Wikidata or OpenAlex. Terabyte-scale dumps do not go over HTTP.
2. **An HTTP bulk-job endpoint.** It accepts streamed, compressed NDJSON, runs asynchronously, and reports status, rejected records and projection lag. The endpoint itself is in [18](18-api.md).
3. **The ordinary edit API,** which submits a batch of one.

### 3.2 Operations

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2; [0035](../decisions/0035-adopting-a-wikibase.md) §3; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §2, §3, §5.*

| Operation | Graph | Meaning |
|---|---|---|
| `put` | Mirror | Replaces the entity's mirrored state. It is skipped when the upstream version is not newer or the content hash is unchanged; a bulk job evaluates the skip for a whole block against `view.entity_source` before the block is written (§3.10). It carries `prev_upstream`, `first_seen`, the old and new sizes, a summary of what changed and, optionally, the statement-level delta, so that history survives compaction (§2.6). |
| `tombstone` | Mirror | Records an upstream deletion and applies the entity's retention policy (§2.1). |
| `redirect` | Mirror or local | In a mirror graph, records an upstream merge. In the local graph, records a merge within one namespace; for properties only, it may cross namespaces, retiring a local property in favour of a mirrored one ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). |
| `create` | Local | Creates a new local entity under a freshly minted ID. It takes an optional temporary `ref` (§3.5). It never writes to an entity that exists: a keyed entity, which exists by its key ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §4), is written with `add`. |
| `create-or-add` | Local | `create` with a `match` key (§3.5): if an entity with that identifier exists, the operation becomes an `add` to it; otherwise it creates one. With `overwrite` and an explicit base revision, it replaces the matched entity's state instead of merging. |
| `add` | Local | Merges statements, terms or references onto any subject, local or foreign. |
| `remove` | Local | Retracts local assertions. |
| `override` | Local | Overrides the rank, a term or a sitelink of an assertion from another graph, or suppresses it. Never its value: a value change on a statement another graph owns is refused with `ts-foreign-statement` (§2.5). An `override` naming a specific mirrored statement is checked by whether that statement still exists in the graph's current state, not by a revision number. |
| `retain` | Local (policy) | Sets the retention policy for a set of entities. |
| `convert` | Local | Mints a local entity and makes the foreign ID an alias of it (§2.3). |
| `same-as` | Local | Links two items from different namespaces ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). |
| `different-from` | Local | Blocks two IDs from ever sharing a cluster ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). |
| `equivalent-property` | Local | Links two properties from different namespaces ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). |
| `adopt` | Local | Writes one revision of a source wiki's entity as a local record: its whole state, or the redirect it became, under the ID the source minted and the source's revision ID. Accepted only from an adoption job (§7.2), in source-revision order for each entity. A revision already present is skipped when the content hash matches and rejected otherwise. |

Local entities are merged into by default. Replacing a local entity wholesale is allowed only with an explicit base revision, so a bulk job cannot overwrite editors' work unseen; `create-or-add` with `overwrite` and a base revision is that replacement. **The base check is per source graph.** A base revision that decodes to the tenant's local partition is checked against the local partition's newest record for the key; one that decodes to a mirror partition asserts only that the key still has no local record, and a mirror advancing is never a conflict, since nothing a local assertion depends on changed. A stale local base is accepted, with the warning `wikibase-conflict-patched`, when the change set touches no statement UUID, term, alias or sitelink that a later local record touched; otherwise, or when the base names a state compaction has replaced, the write fails with `editconflict` ([01](01-log-and-records.md); [18](18-api.md)). A Wikibase write submitted through the Action or REST API is diffed against the tenant's resolved view and reduced to these operations by whether what it touches is local-owned or mirror-owned: a new statement, term or sitelink is `add`; a changed rank, term or sitelink on mirrored content, or a removal of it, is `override`; a removal of local content is `remove`; `clear` removes every local assertion and touches no mirror graph; and a changed value on a mirror-owned statement is refused. The table from module to operation is in [18](18-api.md). `adopt` is accepted only from an adoption job on a tenant that is that wiki continuing on Triplespace, on a `local` partition that holds no other entity records. It follows only an earlier `adopt` of the same entity with a lower source revision ID, and re-running it skips a revision whose content hash matches and rejects one that differs. It is the one whole-entity write to the local graph that needs no base revision, and it is confined to adoption jobs (§7.3).

The operations that write identity links (`same-as`, `different-from`, `equivalent-property`, `convert`, the local `redirect`) are described with the clusters they form in [04](04-entities-and-identifiers.md).

### 3.3 Import jobs

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3.*

Every ingest run is recorded as a **job** in the log: a job record keyed by a job ID the instance mints, with further records under the same key for its finish, failure or revert ([0011](../decisions/0011-logs.md) §6.3; [16](16-logs-feeds-and-notifications.md)). The job record holds:

- the actor, which is always a subsidiary account, so the job's operator is known ([0024](../decisions/0024-subsidiary-accounts.md) §6; [07](07-actors-and-accounts.md)), except for an instance job such as a shallow-mirror fetch (§5.5), whose actor is the instance ([0040](../decisions/0040-instance-prerogatives.md) §2; [08](08-tenants-and-instances.md));
- the source, and the source version (a dump date or snapshot manifest);
- the adapter version;
- the job's parameters.

Every entity change in the run points to its job. A revision, for data that never passed through MediaWiki, is the entity's change within a job.

- **Observed states get thin revisions.** When a state arrives without upstream revision metadata, its revision node carries little more than `prov:wasGeneratedBy` pointing at the job, which is a `prov:Activity`, plus the upstream revision ID where there is one. The details are stored once, on the job. Full revision nodes for every entity in a Wikidata import would add on the order of a billion metadata triples.
- **Upstream revisions get full revision nodes.** When upstream history is backfilled (§1.3), each upstream revision becomes a revision node carrying the provider's own metadata, in the vocabulary [0001](../decisions/0001-revision-metadata-rdf.md) defines: timestamp, actor, parsed summary, tags and flags. The node describes the upstream revision itself. The job that brought it in is recorded separately, with `pav:retrievedFrom` and `pav:importedOn`. When a backfill covers a revision that was already observed, it enriches that revision's existing node instead of creating a second one. Each is recorded as an upstream revision record, identified by its upstream revision ID; content a backfill brings is a `put` carrying that ID ([0015](../decisions/0015-record-format-and-partition-registry.md) §4; [01](01-log-and-records.md)).
- **A whole job can be reverted.** For a local-graph job, reverting appends the inverse change sets. For a mirror job, reverting means syncing again.

### 3.4 Adapters and foreign imports

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4; [0091](../decisions/0091-one-wikibase-adapter.md) §1, §4.*

- **Adapters** are Rust trait implementations, one per kind of upstream ([22](22-crates-and-stack.md)): every MediaWiki+Wikibase provider is read by `scatter-adapter-wikibase`, parameterised by its registry entry, and a provider whose upstream is not a Wikibase has an adapter of its own ([0091](../decisions/0091-one-wikibase-adapter.md) §1). An adapter:
  - rewrites IDs (`Q`→`WDQ`, `P`→`WDP`) wherever an entity ID occurs, the `unit` of a quantity, the `globe` of a coordinate and the `calendarmodel` of a time included, so that the stored form carries prefixed IDs and the content hash is over that form ([04](04-entities-and-identifiers.md) §4.7);
  - maps non-Wikibase sources such as OpenAlex onto properties: registry-provider properties where one fits, otherwise the provider's own property type, never a tenant's ([04](04-entities-and-identifiers.md) §4.6);
  - supplies canonical IRIs.

  **Statement IDs from sources that have none, such as OpenAlex, must be deterministic.** They are a hash, as a name-based UUID, of the main snak and the qualifiers the adapter declares **identifying** for that property, an authorship's position, institution and award for example, and never of score-like qualifiers, so that they stay stable across re-imports and two authorships on one work do not collide. Without that, a local override would detach on every sync.
- **Reading a Wikibase dump** ([0091](../decisions/0091-one-wikibase-adapter.md) §4). `triplespace sync` chooses the adapter by the provider's `adapter` field, not by its slug. `scatter-adapter-wikibase` reads a JSON or an XML dump, and makes a first pass over the whole dump when the provider has key-map rules (§4.1), since the index needs every item before any value is rewritten to a key, or when the dump is XML, whose snaks carry no `datatype` and are typed from the dump's own property pages, as adoption types them (§7.3). A JSON dump of a provider without key maps is read once.
- **Version cursor.** An index, `view.entity_source`, records each entity's upstream version (Wikidata `lastrevid`, OpenAlex `updated_date`), content hash and `seen_job`, the last job that saw it. The ingester writes the cursor as its own state, not as a projection. A re-import appends only the entities that changed; a bulk job evaluates the skip for a whole block before the block is written (§3.10).
- **Import modes:**
  - **`upsert`** adds and updates entities. It never deletes anything.
  - **`snapshot`** declares that the input is complete for one provider and type. The job writes `seen_job` for every entity it saw, whether or not it appended a `put`, and the sweep afterwards tombstones the rows of that provider and type whose `seen_job` is older than the job, so unchanged and absent are told apart. A keyed entity, and every entity of a provider registered `deletion = clear`, is cleared with a `put` of the empty state instead (§2.1, §2.2), and one already cleared is left alone. The sweep runs only after the job completes successfully, is itself resumable, and aborts if the number of records it would write exceeds a configured threshold, so a truncated dump cannot wipe out a mirror.
- **Upstream redirects and deletions** are explicit operations. They are never inferred.
- **Subset imports** are supported alongside full imports. A subset can be:
  - a list of IDs;
  - the closure of everything the local graph references, which is the `linked` setting of shallow mirroring with a depth of zero and its closure properties (§5.2–5.3);
  - terms only, meaning labels, descriptions and aliases without statements.
- **Shallow mirroring** (§5) keeps a referenced subset current without a dump: each entity is fetched whole the first time it is read or referenced, written as an ordinary `put` with its upstream version, and followed by the provider's event stream. Its neighbours are not fetched, except along configured closure properties. A later dump sync continues from those records.
- **A provider that is itself a Triplespace instance** is read from its `local` graph only. The adapter rewrites references to the reader's own entities back to bare local IDs, and verifies each batch against the provider's checkpoint and key chain by default; a provider registered `trust = stream` is read like any other source ([0022](../decisions/0022-federation.md) §2; [08](08-tenants-and-instances.md); [17](17-federation-and-publication.md)).
- **Upstream hashes.** A `put` carries upstream's snak and reference hashes only where they differ from the instance's own recomputation; the ingester compares every one, keeps a differing hash in place and counts it on the job ([0006](../decisions/0006-log-integrity-and-erasure.md) §2; [01](01-log-and-records.md)).

### 3.5 Local bulk creation

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5.*

- **Temporary refs.** New entities in a batch are named with handles such as `$w1`, and claims in the same batch can refer to those handles. The server allocates the real IDs and returns the mapping.
- **Match keys.** A `create-or-add` carries a match key, for example `"match": {"P356": "10.1234/x"}`. If an entity with that identifier exists, the operation becomes an `add` to it; otherwise it creates one. This makes re-runs idempotent. It requires a uniqueness index on designated identifier properties. A match key may also be a foreign entity ID. `create` itself takes no match key and always mints: a job that must not touch existing entities says `create`, and one that may says `create-or-add`. The log records what happened, not only what was asked: a matched `create-or-add` is appended as an `add` keyed by the matched entity, carrying `match` and `via: "create-or-add"`; an unmatched one as a `create` with the same two fields.
- **Merging statements by default.** A statement is added only if no identical statement already exists. Two statements are identical when their main snak and qualifiers hash the same. If an identical statement exists, the incoming references are merged onto it.
- **ID blocks.** An ingester can reserve a block of IDs up front instead of making a round trip for each entity. Gaps in the ID sequence are acceptable. The blocks reserve ranges from the per-tenant entity ID sequences (`log.item_id`, `log.property_id`, …; §7.4).
- **Adoption keeps the source's IDs.** An adoption job writes local entities under the IDs the source wiki minted, with `adopt`, not `create` (§7.3).
- **Atomicity:**
  - An `atomic` batch is all-or-nothing, because temporary refs point across the batch.
  - A `stream` job applies each entity independently and writes rejected records to a rejects file, in one transaction per block rather than per entity (§3.10). An operation an edit filter disallows goes there too, with the filter named as the reason (§3.6).

### 3.6 Edit filters on bulk jobs

*Sources: [0030](../decisions/0030-edit-filters.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

Filters ([09](09-security-and-moderation.md)) are evaluated in the appending transaction, and **a filter's scope excludes job writes by default** (`jobs = false`). A filter that opts in applies to **each operation** of a bulk job in the `local` partition, in the job's stream, unless it exempts the job's group. `disallow` sends the operation to the job's **rejects file** (§3.5) with the filter named as the reason, and the job continues; the job's finish record ([0011](../decisions/0011-logs.md) §6.3) counts filter rejects separately. Hits from a job's writes aggregate into one hit record per (filter, job), carrying a count and the first N record coordinates, as the job's rejects already do, rather than one hit per operation. The `rate` variable is computed lazily, only for a filter that reads it. `warn` and `throttle` do not apply to jobs, since there is nobody to warn and the job's own rate is [0024](../decisions/0024-subsidiary-accounts.md) §5's business; `tag`, `unpatrol`, `notify` and `log` apply as usual, and a `block` or `degroup` on the job's actor stops the job. An `atomic` batch (§3.5) fails whole if any operation is disallowed, since it was asked to.

Cost: building a change-set context per operation is a walk over a change set already in memory, and a compiled CEL rule evaluates in microseconds. A job of a million operations under ten opted-in filters spends seconds on them. Ingest of mirrors, which is where the billions are, is never filtered.

### 3.7 Throughput and validation

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.6.*

- **Three modes.** An interactive write is one appending transaction under the partition lock, which covers the append alone: normalization, filters, the base check, ID allocation, the append, the graph state and the activity row ([03](03-storage-caches-and-search.md)); composition and everything derived from it run after commit. **Bootstrap mode** is for an empty partition: an initial load writes log segments and defers projections, which are then built from the log in a single pass, the way a database bulk load defers index builds; its Postgres form is §3.8. **Bulk append** is for a partition that is already live, block by block (§3.10), and **catch-up** is for a new provider's partition on a live instance (§3.10). Incremental projection only needs to keep up with steady-state syncs.
- **Validation in two tiers.** The shape of each change and property data types are checked eagerly; this needs only the property type map, which is small. References between entities are checked later, because dumps contain forward references.
- **Parsing is the expected bottleneck.** Decompression should be parallel (multistream bz2, for example) and JSON parsing should use SIMD.

### 3.8 Bootstrap mode in Postgres, and packed-storage bulk loading

*Sources: [0013](../decisions/0013-postgres-storage.md) §9; [0058](../decisions/0058-packed-record-storage.md) §7.*

An initial load writes records first and builds everything else afterwards. In Postgres ([03](03-storage-caches-and-search.md)):

1. The target child tables are created with the primary key only. The mirror partition's `(key, offset)` index and every `view` table are absent.
2. A coordinator hands each writer a contiguous block of offsets from `ops.bootstrap_block`. Writers `COPY` records in parallel. A block whose writer fails is handed out again, so no segment is sealed with a hole. Into a `packed` partition, writers pack bodies in-process with the domain's key and dictionaries and `COPY` new fragments into a staging table, merged into `log.fragment` after the load; a load and a sweep of the same domain exclude each other (below).
3. Leaf hashes are computed by the writers. Segment subtrees and the partition root are folded in one pass afterwards, as [0006](../decisions/0006-log-integrity-and-erasure.md) §5 describes, and written to `log.merkle_node`.
4. Indexes are built, then projections run in the order of [0013](../decisions/0013-postgres-storage.md) §7, each in one pass over the partition; composition is set-based, one pass over the loaded states, and the consumers of composition (constraints, scopes, search documents, the RDF delta and the rest, [03](03-storage-caches-and-search.md)) stay off until the operator turns them on.
5. Checkpoints are signed and the partition goes live.

Nothing about this is specific to Postgres except the use of `COPY` and the deferred index build, which is how any bulk load into it is done.

**Packed partitions.** Bootstrap writers pack in-process. Each writer holds the domain key and, if one exists yet, the dictionaries. A first block's sample trains the dictionaries before the load if none exist. Each writer computes refs locally, keeps the set of refs it has already written, and `COPY`s new fragments into an unlogged staging table beside its records. After the load, one statement merges the staging table into `log.fragment` with `ON CONFLICT DO NOTHING`, setting `shared` where a ref was written by more than one writer or already existed. Writers never wait on one another for a ref.

**A bulk load and a sweep of the same domain exclude each other** by an advisory lock on the domain, so the pinning cost of [0058](../decisions/0058-packed-record-storage.md) §3.1 never meets bootstrap volume. Incremental ingest jobs take the same lock in shared mode, and a sweep takes it exclusively.

### 3.9 Wire format

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.7; [0035](../decisions/0035-adopting-a-wikibase.md) §3.*

A mirror sync job:

```
{"job":{"source":"openalex","snapshot":"2026-09-01","mode":"upsert","graph":"mirror/openalex"}}
{"op":"put","id":"OAW123","upstream":"2026-08-30","entity":{…}}
{"op":"redirect","from":"OAW99","to":"OAW123"}
{"op":"tombstone","id":"OAW77"}
```

A local bulk job:

```
{"job":{"source":"citation-batch","graph":"local","atomic":true}}
{"op":"create-or-add","ref":"$w1","match":{"P356":"10.1234/x"},"entity":{…}}
{"op":"add","id":"Q5","claims":{"P50":[…"$w1"…]}}
{"op":"add","id":"WDQ42","claims":{"P7":[…]}}
{"op":"override","statement":"OAW123$…","rank":"deprecated"}
{"op":"retain","ids":["WDQ123","WDQ124"]}
```

An adoption job:

```
{"job":{"source":"https://librarybase.org/","version":"librarybase-20261019-history.xml.gz","mode":"adopt","graph":"local"}}
{"op":"adopt","id":"P12","entity":{…},"source_revid":903,"source_time":"2024-01-03T09:15:40Z","source_pageid":40}
{"op":"adopt","id":"Q6","entity":{…},"source_revid":41002,"source_time":"2026-09-19T10:41:07Z","source_pageid":12}
{"op":"adopt","id":"Q6","entity":{…},"source_revid":41877,"source_parentid":41002,"source_time":"2026-09-20T14:02:11Z","source_pageid":12,"minor":true}
{"op":"adopt","id":"Q9","redirect":"Q6","source_revid":41880,"source_parentid":41100,"source_time":"2026-09-20T14:05:30Z","source_pageid":15}
```

Each line of an adoption job is one source revision; the revision's edit summary is the record's comment part and its author the attestation's actor, so neither is a field of the operation (§7.3).

Entity payloads use the Wikibase canonical JSON described in [wikibase-compat.md §3](../api/wikibase-compat.md). The record forms are in [payloads.md](../api/payloads.md).

### 3.10 Bulk append into a live partition, and catch-up

*Sources: [0083](../decisions/0083-write-path-in-three-tiers.md) §5; [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.*

**Bulk append is the mode between interactive and bootstrap**, and it is the mode a weekly re-sync, a move from `linked` to `all` (§5.9) and a large local job walk. A job writing into a partition that already has its indexes and projections takes the partition lock **once per block** (`ops.bootstrap_block` has the shape, §3.8), `COPY`s the block's records, folds the block's leaves into the Merkle tree from the in-memory frontier ([01](01-log-and-records.md)), writes the block's graph states, and commits; composition then runs set-based over the block, and `ops.projection_state` advances per block ([03](03-storage-caches-and-search.md)). The consumers of composition are off for the job's duration, as in bootstrap, and catch up afterwards. **The `put` skip is evaluated in bulk**: the version cursor of §3.4 is compared for a whole block against `view.entity_source` before the block is written, so a re-sync's unchanged entities never reach the lock. A `stream` job (§3.5) is one transaction per block, not per entity.

**Per-partition catch-up.** A provider added to a live instance is loaded into its own partition in bootstrap mode (§3.8), then composed against the existing `view`: its tier-2 links into other providers' entities and the fusion of the clusters it joins ([04](04-entities-and-identifiers.md) §4.3, §4.8) are queued for the composition worker, which works through them at its own rate while the instance serves. Bootstrap mode is for an empty partition; catch-up is for a new partition on a live instance; bulk append is for a live partition.

## 4. Key-mapped providers

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9.*

### 4.1 Mapping upstream items onto keys

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9; [0091](../decisions/0091-one-wikibase-adapter.md) §2, §3.*

internetdomains.wiki is mirrored like any other Wikibase, with one difference: its domain items are **mapped by key**, not rewritten to prefixed IDs.

**Provider registry entry.** The provider gets a two-letter code, `XD`, for the parts of it that are not domains. Its **key map** is a list of rules in its adapter configuration, `[[provider.adapter_config.key_map]]`, one per class of mapped item ([0091](../decisions/0091-one-wikibase-adapter.md) §2). A rule names the **identity property**, the upstream property whose value is the key, and the keyed type the item is mapped onto, and may name a `class`: the rule then applies only to items that have that class as a direct value of the configuration's `class_property`, the wiki's "instance of". internetdomains.wiki's key map is one rule with no class, `identity_property = "P1"` and `key_type = "domain"` (§8.6); the OpenStreetMap wiki's is two rules with classes (§8.9). The identity and class properties are the provider's own and fixed by its entry, so they are named by their upstream IDs (`P1`), not by role; roles name the properties an adapter maps onto ([06](06-statements-and-properties.md) §1).

**What the adapter does:**

- **An upstream item with one valid identity value** is written to the mirror graph with its Domain ID, `domain:{key}`, as its subject ([0017](../decisions/0017-entity-id-grammar.md) §3). Its upstream item ID never appears as a subject.
- **Values that point at such items** are rewritten to `wikibase-domain` values with the same key.
- **Upstream items without an identity value,** such as registrars, organizations or classes, stay ordinary foreign items under the provider's prefix. Such an item is reported as a reject only when a rule that names its class is in scope: an item of a mapped class without its identity value is an upstream data problem.
- **An identity value that does not normalize to a valid key** is reported as a reject. The item stays an ordinary foreign item.
- **An item with more than one identity value** is reported and not mapped.
- **An item that two rules give identity values** is reported and not mapped.

**Other keyed types are mapped the same way.** A key map names the keyed type it maps onto: OpenAlex keywords map onto `keyword` ([0017](../decisions/0017-entity-id-grammar.md) §5), and the OpenStreetMap wiki's tag items onto notations in the `osm` scheme (§8.9; [0048](../decisions/0048-notation.md) §6). In a Wikibase provider's rules the keyed type supplies the normalizer, the mapped state's entity type and the data type of a value that points at a mapped item (`wikibase-domain`, `wikibase-keyword`, `wikibase-notation`), so the adapter hard-codes none of them, and a rule naming a type or scheme the build does not know is a configuration error when the adapter is built ([0091](../decisions/0091-one-wikibase-adapter.md) §3). The keyed types themselves are in [04](04-entities-and-identifiers.md).

### 4.2 Aliases, IRIs and upstream changes

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, A13.*

**The upstream ID becomes an alias.** A mapping index records upstream ID ↔ key, together with the version cursor (§3.4). Sync jobs use it, because upstream reports redirects and deletions by item ID. The API and the title resolver use it too, so the prefixed upstream ID resolves to the Domain page: the API answers with the Domain entity and Wikibase's `redirects: {from, to}`, and the site answers `Item:XDQ9` with a `302` to the Domain page ([0009](../decisions/0009-keyed-entity-types-and-domain.md) A14). Each key has one **owner**, the item mapped onto it, and may have further aliases, the items upstream **merged into** the owner. The `put` under the key carries both (`upstream_id`, `upstream_aliases`), and the newest `put` under a key decides its rows in the index ([payloads.md §3.1](../api/payloads.md); [03](03-storage-caches-and-search.md)).

**The canonical IRI is the key's, not the provider's.** For a mapped item, the resolved view uses `https://scatter.red/domain/{key}` and emits `<upstream item IRI> owl:sameAs <https://scatter.red/domain/{key}>`. Nodes that come from upstream, such as statement nodes, keep their upstream IRIs as before.

**Upstream changes:**

| Upstream event | Effect |
|---|---|
| An item's identity value changes, for example a typo is fixed | The mirror's state for the old key is cleared, and the item's state is written under the new key. Local assertions about the old key stay where they are, because the old name still exists |
| An item stops being mapped: its identity value is removed or becomes invalid, or it becomes one side of a duplicate | The mirror's state for its old key is cleared, and the item is written as an ordinary foreign item |
| An item becomes mapped | Its state is written under the key, and the foreign item it was (`XDQ9`) is tombstoned, since a mapped item's upstream ID never appears as a subject (§4.1) |
| An item is deleted | The mirror's state for that key is cleared (§2.2) |
| Two items are merged into a mapped item | The redirect updates the mapping index: the merged item becomes an alias of the target's key, and its foreign item, if it had one, is tombstoned. If both carried the same key, the duplicate is resolved |
| Two unmapped items are merged | An ordinary mirror `redirect` between the two foreign items, written once |
| Two items carry the same identity value | A conflict. Neither item's statements are attached to the key until the duplicate is resolved upstream. The conflict is listed with the others ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §10) |

A sync decides these from what it saw: a key is cleared for an item that **appeared in the job** and no longer owns it, so an `upsert` of part of a dump leaves alone the keys of items it did not read, and the sweep of a `snapshot` handles the rest.

Retention for keyed entities is §2.2: the mirror's contribution is cleared with a `put` of an empty state, `cascade` and `orphan` do not apply, and `retain` still does.

## 5. Shallow mirroring

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §1–8.*

### 5.1 Shallow means depth, not a slice

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §1.*

**A shallow mirror holds the whole current state of each entity it holds, and does not follow that entity's links.** "Enough of the entity" is read as *enough entities*, not *part of an entity*:

- Each mirrored entity is written as an ordinary mirror `put` (§3.2) of its full upstream JSON, with `lastrevid` as its version cursor, in the provider's mirror partition. Nothing distinguishes it from an entity a dump sync wrote.
- A partial entity is not written. A `put` of part of an entity would break two rules already in place: the `source` resolution kind needs a whole-state record ([0013](../decisions/0013-postgres-storage.md) A28), and a later dump sync would read every statement the slice left out as removed upstream.
- The entities an entity refers to are **not** fetched because it refers to them, except along closure properties (§5.3). Their labels are needed to display it; those are read through the label cache of §5.6, which is not the mirror.

### 5.2 The `entities.mirror` setting

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §2, §2.1.*

**`entities.mirror`** is a `site` setting, per provider and entity type, with four values ([0015](../decisions/0015-record-format-and-partition-registry.md) §3):

| Value | Mirrors |
|---|---|
| `off` | Nothing; foreign IDs are shown from the label cache only |
| `on-demand` | An entity the first time it is read (an entity page, `wbgetentities`, a table cell) on any tenant that lists the provider ([0018](../decisions/0018-tenants.md) §5) |
| `linked` *(default)* | Everything `on-demand` mirrors, and every entity used as a statement value, qualifier value, reference value or property in a graph of a listing tenant: `local`, `pages` and any derived graph ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1) |
| `all` | The provider's whole type, by dump sync; what §3.4 already does |

The setting is written as `entities.mirror = { WD = { item = "linked", property = "linked", lexeme = "on-demand" } }`. Properties are always at least `on-demand`, since a statement cannot be validated or rendered without its property's data type. [0065](../decisions/0065-mediainfo-captions-and-commons.md) §2's `mediainfo.mirror` is this setting for Wikidata's type `M`, under its own name for compatibility; it keeps its default of `on-demand`.

A tenant's entity source takes the setting under its name, `entities.mirror = { mhc = { item = "linked" } }`, as `entities.closure` does; its default is `linked` for a source with an `api` and `off` for one without, and its closure list is empty by default (§6.4).

### 5.3 When `linked` fetches, and closure properties

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §2.2, §2.3.*

**A write that mentions a foreign ID not yet mirrored enqueues it; it does not wait for it.** The `entity_ref` projection, a tier-2 step of composition ([03](03-storage-caches-and-search.md) §6), adds a row to `ops.entity_fetch` for each foreign entity ID it writes as a target that has no `view.entity_source` row for its provider. Validation at save time checks only the ID's grammar and its provider ([0017](../decisions/0017-entity-id-grammar.md) §2), as it does for an ID whose mirror has not caught up. The entity page shows the value from the label cache until the fetch lands, and the fetch re-renders the pages that use it through `view.entity_ref`.

A bulk job that will reference many foreign entities may prefetch them: the job's `start` names `prefetch = true`, and the ingester enqueues every foreign ID it validates before writing, so that the fetch runs alongside the job.

**`entities.closure`** (site setting, per provider) lists properties whose values are fetched transitively from every mirrored entity, up to `entities.closure_depth` (default **6**). The default for Wikidata is:

| Property | Why |
|---|---|
| `P31` instance of | The type of a value, for display and for constraints |
| `P279` subclass of | Class hierarchies, so a query for a class finds its subclasses |
| `P131` located in the administrative territorial entity | Place hierarchies, so a query for a state finds its counties |
| `P17` country | The top of a place hierarchy |

The depth cap bounds the walk up `P279`, which is deep. A tenant may add properties (`P361` part of, `P1365`/`P1366` replaces/replaced by for historical jurisdictions) or remove them. Closure stops at an entity already mirrored, so the walk is incremental.

### 5.4 Leaving the set

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §2.4.*

**A shallowly mirrored entity is never tombstoned for falling out of use.** A tombstone means "deleted upstream" and applies the entity's retention policy (§2.1), which under `cascade` retracts local assertions, so it cannot be reused for eviction. The set grows with use and is bounded by it.

Not yet: eviction from the set is an open question of [0070](../decisions/0070-shallow-entity-mirroring.md).

### 5.5 Fetching and records

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §3; [0078](../decisions/0078-entity-sources.md) §4; [0092](../decisions/0092-following-recent-changes.md) §1.*

**A fetch is an instance job per provider**, run by the instance ([0040](../decisions/0040-instance-prerogatives.md) §6; [08](08-tenants-and-instances.md)), like a page repository's sync job ([0053](../decisions/0053-mirrored-pages.md) §5; [13](13-mirrored-pages.md)):

- It drains `ops.entity_fetch` in batches through `wbgetentities` (50 IDs per request, the API's limit for clients without `apihighlimits`) or `Special:EntityData/{id}.json` for a single entity, through the upstream client and its `upstream` rate class (§2.7).
- Each entity becomes a `put` with the same fields a dump sync writes. An upstream redirect becomes a `redirect`. An entity already in the set (one with a `view.entity_source` row for the provider) that upstream reports missing becomes a `tombstone`, as §3.4 says. **A fetch miss for an entity with no `entity_source` row writes no record**: a typo'd or never-existing ID is not a deletion, nothing is tombstoned and no retention policy runs; the referring value renders unresolved, and the queue row is dropped.
- The job's records are one long-running job per provider, with a `job/start` when the instance starts it and periodic checkpoints, so `Special:Jobs` and `Special:Providers` show the set's size, the queue and the lag.

The job's actor is the instance ([0040](../decisions/0040-instance-prerogatives.md) §2), not a subsidiary: instance jobs are the exception to §3.3's rule.

**An entity source's fetch is the tenant's job, one per source**, not an instance job (§6.4). It writes to the tenant's `source/{name}` partition through the source's own `api` and `entity_data`, follows the source's `events` where it declares them, and is otherwise followed by its recent changes (§5.7; [0092](../decisions/0092-following-recent-changes.md) §1), with the sweep behind either.

### 5.6 Labels of entities not mirrored

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6.*

**Labels for display come from a cache, not the mirror.** Rendering a mirrored entity needs the labels of the entities its statements point at. The label cache is an L1/L2 entry per entity and language ([0014](../decisions/0014-caches-and-search.md) §2; [03](03-storage-caches-and-search.md)), filled by `wbgetentities&props=labels|descriptions` in batches, with a lifetime of `entities.label_ttl` (default 7 days). A label from the cache is never a term in `view.term`, never in search, and never in RDF. An entity that is mirrored reads its terms in the instance's term languages from `view.term` (§5.8); its labels in every other language are read from the record body at `entity_source.offset` through the same L1 label cache, so one cache serves unmirrored entities and the languages `view.term` does not hold.

### 5.7 Keeping mirrored entities current

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §5; [0092](../decisions/0092-following-recent-changes.md) §1, §2, §3, §10.*

**The set is followed by the provider's event stream where it has one, by its recent changes where it does not, and checked by a sweep.**

- **Events.** For Wikidata, the instance follows `mediawiki.recentchange` filtered to `wikidata.org`, as [0053](../decisions/0053-mirrored-pages.md) §6 follows `mediawiki.page-change.v1` for a page repository, with the same cursor table (`ops.repo_cursor`, [03](03-storage-caches-and-search.md) §4.12), debounce, and gap-closing by the recent-changes poll below. An event for an entity in the set (a `view.entity_source` row for the provider) enqueues a fetch. Events for entities outside the set are ignored.
- **Recent changes.** A provider or entity source with an `api` and no `events` is followed by polling its `list=recentchanges` every `follow.interval` (default one minute; 23), as the Wikidata Query Service's updater did before its event stream ([0092](../decisions/0092-following-recent-changes.md) §1):
  - **The query.** `rcdir=newer`, `rcprop=title|ids|timestamp`, `rctype=edit|new|log`, `rclimit=max`, and `rcnamespace` set to the wiki's entity namespaces, read from its `siteinfo` by their default content model, so a Miraheze wiki's 860 and 862 need no configuration. It runs from `rcstart` = the position while there is no continuation, and by `rccontinue` while there is one. Requests go through the upstream client in the `upstream` rate class, with `maxlag`, a User-Agent naming the instance and one request in flight. A follower may log in with a bot password for `apihighlimits` (5,000 rows, 500 IDs per `wbgetentities`); it is optional, and a failed login falls back to the anonymous limits ([0092](../decisions/0092-following-recent-changes.md) §10).
  - **A batch.** Every row moves the position forward. A row whose `rcid` was seen within `follow.tail` is skipped, and the rest are coalesced to one per title: the highest revision wins, and a log row (a deletion, restoration, merge or suppression) always stays.
  - **Settling.** Within `follow.settle` of now (default two minutes), a poll reads by time from ten seconds before the position, since rows can be committed late with earlier timestamps; a full batch with nothing new moves the position on one second.
  - **The tail pass.** Once caught up, each poll also re-reads the window `follow.tail` behind the head (default 15 minutes), deduped by `rcid`; what it finds that the head missed is counted on the job as `late`.
- **Which rows and events count.** Under `all`, every row or event in the entity namespaces; under `linked` and `on-demand`, those for entities already in the set (a `view.entity_source` row).
- **A row or an event says only that a title changed.** The job reads the entity's current state and writes what the state says, as a fetch does (§5.5): a `put` when its `lastrevid` is newer than the cursor, a `redirect` for an upstream redirect, a `tombstone` when an entity in the set is missing, nothing otherwise ([0092](../decisions/0092-following-recent-changes.md) §2). An edit, a creation, a deletion, a restoration and a merge take one path, and order does not matter. A state older than the row's revision comes from a lagging replica: the title is fetched again on the next poll and counted as `stale`.
- **Sweep.** Daily, the job asks `wbgetentities&props=info` for every entity in the set in batches of 50 and enqueues those whose `lastrevid` is newer than the cursor. The sweep catches anything the stream or the poll missed.

**The position is a hint; the version cursor is the guarantee** ([0092](../decisions/0092-following-recent-changes.md) §3). It is kept in `ops.repo_cursor`, written in the transaction that writes the records it covers. A replayed row finds the entity current and writes nothing, and a lost position is rebuilt from the newest upstream time in `view.entity_source`, less `follow.tail`.

**A gap the wiki cannot close.** A wiki keeps a limited window of recent changes (MediaWiki's `$wgRCMaxAge`, 90 days by default). Before resuming, the follower reads the oldest row the wiki still has, and a position older than it is a gap. Under `all` the job stops with `follow-gap` until a new load (§5.9). Under `linked` and `on-demand` it resumes from the oldest row and runs the sweep at once.

`triplespace follow --provider {slug}` or `--tenant {slug} --source {name}` runs a follower in the foreground, and `--once` catches up to the present and exits; a server runs its followers in its background loop.

### 5.8 Terms of mirrored entities

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6.*

**`view.term` holds rows for local entities, for entities in any tenant overlay, and for mirrored entities in the instance's term languages only.** `entities.term_languages` is instance configuration for the shared rows (default: the instance's content languages, `mul` and `en`), set in the instance policy record ([04](04-entities-and-identifiers.md) §4.10); the instance's value is the ceiling it sets for every tenant, the `tenancy:entities.term_languages` switch ([23](23-configuration-and-registry.md) §3.4), which replaces the default when present. **The instance's content languages are every tenant's `site.language` and the instance's default for it** (`template:site:site.language`), so each tenant's own language is always among the shared rows ([0082](../decisions/0082-source-form-and-the-shared-view.md) §6, A2). The rows are filtered as they are written, under the configuration in the store; a later change to the set reaches rows already written when the entity projection is rebuilt; a tenant's setting of the same name only narrows the languages its own display offers. A mirrored entity's other languages stay in the record at `entity_source.offset`: `wbgetentities` reads the record, and the label cache of §5.6 serves display, but they are not rows. A value shown under one member of a cluster may take its label from another member's rows, with precedence from the policy order.

**The same rule holds for a full mirror (`all`).** The budget is rows ≈ mirrored entities × |instance languages| × term kinds, which the Wikidata-scale profile ([03](03-storage-caches-and-search.md)) lists beside `entity_ref`; a shallow mirror of thousands or millions of entities is well inside it. `wbsearchentities` at Wikidata scale is the search index ([03](03-storage-caches-and-search.md)), and the `term_prefix` index is never created on an instance holding a full mirror; the small profile's index is partial over local entities.

### 5.9 Becoming a full mirror

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §7; [0092](../decisions/0092-following-recent-changes.md) §4, §5.*

**Moving a type from `linked` to `all` starts a dump sync, which continues from the shallow records.** The version cursor already holds each shallow entity's `lastrevid`; the sync skips every entity whose cursor is current, in bulk per block, and writes a `put` for the rest, as a bulk append into the live partition (§3.10). Nothing is replaced and nothing is deleted. Moving back from `all` to `linked` keeps every entity already mirrored (§5.4).

**A load is a dump sync or a crawl** ([0092](../decisions/0092-following-recent-changes.md) §4). A dump sync takes a JSON or XML dump the operator supplies (`triplespace sync --provider {slug}`, or `--tenant {slug} --source {name}` for an entity source, §6.4). Without a dump, a **crawl** lists the entity namespaces with `list=allpages` and fetches them through the fetch job in batches of 50, with the reason `crawl`; a crawl is a snapshot, whose sweep and threshold are a snapshot sync's (§3.4).

**A load hands over to the follower** ([0092](../decisions/0092-following-recent-changes.md) §5). It records where it ends: an XML dump's newest revision timestamp, a JSON dump's newest `modified`, or the time a crawl began. The follower starts there, less `follow.tail`, and the version cursor skips what the load already wrote. A load whose position is older than the wiki's oldest recent change cannot hand over: `sync` warns at its end, and the follower reports the gap (§5.7).

**A provider that is a tenant of this instance is never synced.** The instance refuses a sync job, shallow or full, for a provider whose registry `issuer` is a tenant of this instance: that tenant's `local` partition is the provider's graph, read directly ([08](08-tenants-and-instances.md); §8.3).

### 5.10 Local statements on mirrored entities

*Sources: [0070](../decisions/0070-shallow-entity-mirroring.md) §8.*

**Nothing new is needed.** A tenant's `add`, rank override, suppression and `retain` on a mirrored entity are local-graph assertions about a foreign subject (§2.5), resolved against the mirror by [0013](../decisions/0013-postgres-storage.md) A28's resolution, as the internetdomains tests already do for Domains. Proposals ([0067](../decisions/0067-proposals.md); [14](14-discussions.md)) can offer them upstream. A local statement on a foreign entity that is not yet mirrored enqueues it (§5.3), and until it lands the resolved view is the local contribution alone.

## 6. Entity sources a tenant declares

*Sources: [0078](../decisions/0078-entity-sources.md) §1–6, §8, §9.*

### 6.1 The `entity-source` record

*Sources: [0078](../decisions/0078-entity-sources.md) §1; [0080](../decisions/0080-tenants-as-entity-sources.md) §1.*

**An entity source is a graph of minted entities that one tenant reads, declared in that tenant's configuration instead of the registry.** It is a `config` record of kind **`entity-source`**, keyed `entity-source:{name}`, in the tenant `config` ([0015](../decisions/0015-record-format-and-partition-registry.md) §3; [23](23-configuration-and-registry.md)), written with `ts-config`. Its fields are a provider registry entry's (§1.5, `providers.toml`), cut to what a source of entities needs:

| Field | Meaning |
|---|---|
| `name` | The prefix of its IDs (§6.2): `[a-z][a-z0-9-]*`, at most 32 characters, the grammar of notation scheme names ([0048](../decisions/0048-notation.md) §2). Example: `mhc` |
| `label` | The name shown on source chips and origin lines: "Miraheze Communities" |
| `types` | One entry per entity type the source mints, with the fields of a `[[provider.type]]`: `upstream_prefix` (`Q`), `entity_type` (`item`), `id_grammar` ([0017](../decisions/0017-entity-id-grammar.md) §2; default `digits`), and `iri`, the concept IRI template over `{upstream_id}` ([02](02-graphs-rdf-and-query.md)) |
| `namespace` | The number of the source's namespace (§6.5), with its canonical name and aliases |
| `api`, `entity_data` | Optional: the source's Action API endpoint and `Special:EntityData` base, as [0065](../decisions/0065-mediainfo-captions-and-commons.md) §5's per-type source fields. Without `api`, the source can be named but not mirrored |
| `revision_ids` | Whether the source publishes revision IDs, as in `providers.toml` |
| `events` | Optional: an EventStreams endpoint and filter, as a page repository's ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1) |
| `role` | Optional: a role ([0003](../decisions/0003-statement-ui.md) §7) whose property holds the source's upstream ID as an `external-id`, for the lookup of §6.5 |
| `licence` | The licence of the source's data, as an SPDX identifier; required once anything is mirrored (§6.4) |
| `number` | Assigned by the server on the first record (§6.4); not writable |
| `promoted_to` | Empty, or the registry provider code that replaced the source (§6.8) |
| `tenant` | For a **tenant source**: the issuer code of the Triplespace tenant it reads ([08](08-tenants-and-instances.md) §1.2, §4). Fixed by the first record |
| `base` | For a tenant source: the tenant's base URI, writable, and followed from the provider's `alias` records on the same instance |

**A tenant source reads its description from the provider.** With `tenant` set, `types` (the provider's entity types, upstream prefixes and IRI templates over its current base), `licence` (its `content.licence`) and `api`, `entity_data` and `events` (at `base`) are not declared; `label` and `namespace` are the reader's. This is how one tenant reads another ([08](08-tenants-and-instances.md) §4; [0080](../decisions/0080-tenants-as-entity-sources.md) §1).

**What it is for.** A graph that one tenant, or a few, need. A graph that many tenants need belongs in the registry, and §6.8 moves a source there without losing anything.

### 6.2 Source IDs

*Sources: [0078](../decisions/0078-entity-sources.md) §2.*

**A source's entities have IDs of the form `{name}:{upstream ID}`:** `mhc:Q1`, `mhc:P31`. The rest is the source's own ID, verbatim, so the ID pastes into the source unchanged, as an MBID does ([0017](../decisions/0017-entity-id-grammar.md) §6).

**A source ID is a foreign ID in the colon shape, not a keyed ID.** A keyed ID names a thing whose key is minted by the thing itself; a source's entity was minted by a provider, the source ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §1). Everything §1 says of a foreign entity holds: the prefix records identity origin, the entity may have triples in the source's mirror partition (§6.4) and in the tenant's `local` graph, and the source is a namespace in [0004](../decisions/0004-identity-clusters-and-equivalence.md) §1's sense, so a cluster holds at most one of its entities. A source entity therefore needs no surrogate ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §7): its log header key is its ID, as `WDQ42`'s is.

- **Parsing.** A colon form is told apart by the kind of its prefix in the tenant's table of names (§6.3). For a source, the rest must be the `upstream_prefix` of one declared type followed by a string in that type's grammar, which also gives the entity type. A source that declares `lexeme` takes the part suffix of [0017](../decisions/0017-entity-id-grammar.md) §1 (`mhc:L3-F1`).
- **Canonical form.** The name in lowercase, and the rest by the type's grammar with its upstream prefix in uppercase: input `MHC:q1` is `mhc:Q1`. Input is case-insensitive, as for every ID ([0017](../decisions/0017-entity-id-grammar.md) §2).
- **Statement IDs and values** take the ID as any entity's do: `mhc:Q1$C7C2A847-…`, and `{"entity-type": "item", "id": "mhc:Q1"}`, which is the value form a federated Wikibase writes.
- **Pages.** A source entity's page is in the namespace of its entity type, `Item:mhc:Q1` or `Property:mhc:P31`, since entity namespaces are keyed by type, not by provider ([0008](../decisions/0008-namespaces-and-document-pages.md) §1). The source's own namespace (§6.5) is how a reader reaches it by a short title.
- **Relative.** `mhc:Q1` means the source's entity on the tenant where it is read, as `Q5` means the tenant's own ([0044](../decisions/0044-tenant-relative-ids.md) §1; [04](04-entities-and-identifiers.md)). It reaches another tenant only through the rewriting of §6.7.

### 6.3 One table of names per tenant

*Sources: [0078](../decisions/0078-entity-sources.md) §3.*

**Every colon prefix on a tenant is looked up in one table.** [0029](../decisions/0029-resolver-namespaces.md) §2 made keyed-type, resolver and notation scheme names one namespace in the registry. Two kinds join it:

- **Registry provider slugs.** `wikidata:Q42` is an **input form** of `WDQ42` everywhere, canonicalized as `QQQ5` is ([0044](../decisions/0044-tenant-relative-ids.md) §1): never stored, never output. This is what §7.3 already does to an adopted wiki's federated IDs, extended to all input. A slug with no entity types (a page provider, [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1) is reserved but forms no IDs.
- **The tenant's entity sources**, on that tenant only. Two tenants may each have an `mhc` that names different graphs.

**The rules are fixed, and none of them guesses:**

1. **A source may not take a name the registry has.** A declaration whose name is a keyed type, resolver, notation scheme or provider slug in the registry the instance runs is refused with `ts-source-name-taken`.
2. **A tenant's source shadows a registry name added later.** If a registry update adds a name a tenant already uses for a source, the source keeps it on that tenant. The registry's thing stays reachable there by its other forms: its IRI, its provider code, or, for a keyed type, its namespace title. A reference from elsewhere that uses the shadowed name is matched by IRI (§6.7) and never read as the source. `triplespace-cli registry check` lists every shadowed name after an update.
3. **Colon forms never collide with minted forms.** `mhc:Q1`, the provider-form ID `MHC1` (a provider `MH`, type `C`) and `Item:MHQ1` are three different strings, parsed by three rules ([0017](../decisions/0017-entity-id-grammar.md) §1). Whether two of them name the same entity is a question of identity, which §6.8 answers: on one tenant, they never both do.
4. **The tenant-relative form is untouched.** `QQQ5` has no colon.

`GET /resolve` ([0029](../decisions/0029-resolver-namespaces.md) §6) tries source IDs with the other entity IDs, first; the table says which kind a prefix is, so the order among colon forms decides nothing.

### 6.4 Numbers, the `source/{name}` partition and fetching

*Sources: [0078](../decisions/0078-entity-sources.md) §4; [0080](../decisions/0080-tenants-as-entity-sources.md) §2; [0092](../decisions/0092-following-recent-changes.md) §1, §4.*

**Provider numbers from 2^12 to 2^13 − 1 are reserved for entity sources.** The server assigns the lowest one not yet used on the tenant when the source's first record is written, and records it in the entry. A number is unique on its tenant and is never reused there, even after the source is retired. Uniqueness per tenant is enough: revision and page IDs are per-tenant sequences ([0015](../decisions/0015-record-format-and-partition-registry.md) §2), and no other tenant reads a source's records (§6.7). So `revid = number << 40 | n` and the derived page IDs work unchanged, every such ID stays below 2^53 for `n` below 2^40 (§8.1), and a move carries the number in the entry. The registry keeps the 4,095 numbers below the range.

**A source's records live in a tenant partition, `source/{name}`.** It is a source graph of tenant scope, with the policies of `mirror/{provider}`: history `latest` (a tenant may set `full`), integrity `hashed`, export public unless the tenant is private ([0056](../decisions/0056-security-model.md) §3), payloads `scatter:v0/changeset` and `scatter:v0/erase`. Only the source's own jobs write to it, its fetch job and its loads (§5.9; [0092](../decisions/0092-following-recent-changes.md) §4), and each attests its records, as an extraction job attests a derived graph's ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1, §3). Its records are mirror `put`, `redirect` and `tombstone` (§3.2). A tenant move carries it with the tenant's other partitions ([0018](../decisions/0018-tenants.md) §10). A tenant therefore has one `source/{name}` partition per entity source beside the partitions of [08](08-tenants-and-instances.md).

**Shallow mirroring applies as written, with the source's name in place of a provider code** (§5):

- `entities.mirror = { mhc = { item = "linked", property = "linked" } }`. The default is `linked` for a source with `api` and `off` for one without. `off` is the pointer state: the source's IDs are written and shown, from the label cache where there is an `api` and as bare IDs otherwise, and nothing is fetched.
- `entities.closure` takes the source's name too. Its default for a source is empty, since nothing is known about which of its properties form hierarchies.
- **The fetch job is the tenant's, one per source**, not an instance job: it drains `ops.entity_fetch` rows for `(tenant, source)` through the source's `api` in the `upstream` rate class ([0024](../decisions/0024-subsidiary-accounts.md) §5), follows `events` where the source declares them, and otherwise its recent changes, with the daily sweep behind either (§5.7; [0092](../decisions/0092-following-recent-changes.md) §1).
- The label cache keys a source entity's labels by tenant ([0018](../decisions/0018-tenants.md) §6).

**Views.** A source entity has tenant rows only. Nothing about it is computed from shared graphs, so [0018](../decisions/0018-tenants.md) §6 never gives it a shared row, and it is indexed in the tenant's search index, never in a provider index ([03](03-storage-caches-and-search.md)).

**A tenant source on the same instance has no partition.** The provider's `local` partition is read directly and rewritten to the source's name and number, so it has no `source/{name}` partition, no fetch job and no mirror setting. On another instance, `scatter-adapter-triplespace` writes its `source/{name}` partition ([08](08-tenants-and-instances.md) §4.4), and a move of the provider switches between the two.

**No issuer.** Shallow mirroring keeps no upstream history (§5.1), so no upstream actor is recorded and a source needs no issuer.

Not yet: mirroring a source's history is an open question of [0078](../decisions/0078-entity-sources.md).

### 6.5 The source's namespace

*Sources: [0078](../decisions/0078-entity-sources.md) §5.*

**A tenant's own namespaces take numbers from 3000 upward,** the numbers mediawiki.org's Extension default namespaces page leaves to system administrators, or from 100–199, MediaWiki's block for site-specific namespaces, except the numbers `namespaces.toml` lists (120–125, 146 and 147 among them). An adopted wiki (§7) also keeps the numbers it already gave its namespaces. A tenant `namespace` record with a number outside these ranges that `namespaces.toml` does not list is refused with `ts-namespace-number`. This is rule 4 of [0008](../decisions/0008-namespaces-and-document-pages.md) §2; the Triplespace blocks of rule 3 stay the registry's ([10](10-pages-and-content-models.md)).

**Each source has one namespace there, of kind `resolver`,** with its talk number `reserved`, as a resolver's is ([0029](../decisions/0029-resolver-namespaces.md) §2). The tenant chooses its canonical name, `MHC` or `Miraheze Communities`. **The source's name is always one of its aliases**, so `[[mhc:Q1]]` and `[[MHC:Q1]]` are both titles in it. A source whose namespace name or alias equals another namespace's name or alias on the tenant, or an interwiki prefix from the tenant's site aliases ([0026](../decisions/0026-sitelinks.md) §2), is refused with `ts-namespace-name-taken`. MediaWiki's precedence of namespace over interwiki therefore never decides anything. The title normalizer is the source's ID normalizer of §6.2.

**Viewing `MHC:Q1`, or resolving `mhc:Q1`, does the following:**

1. **The entity is known to the tenant.** If `mhc:Q1` has a row in the tenant's resolved view (it is mirrored, the tenant asserts something about it, or it is in a cluster), the response is a redirect (HTTP 303) to its canonical page. That is `Item:mhc:Q1`, or `Item:Q500` once the entity is in Q500's cluster (§6.6), by [0008](../decisions/0008-namespaces-and-document-pages.md) §3.
2. **An item carries its ID.** Otherwise, if the source names a `role`, the upstream ID is looked up as that property's value exactly as [0029](../decisions/0029-resolver-namespaces.md) §3 looks up a DOI, with the same responses: one entity redirects, several show a disambiguation page.
3. **Nothing is known.** Otherwise a page shows the ID, the external link (the concept IRI, or the source's entity page where the source has `entity_data`), and for editors **Create an item for this** (§6.6). If the source has `api` and its mirror setting is not `off`, the fetch is enqueued, and the page says so.

`MHC:Q1` is therefore the citable address of "the source's Q1, whatever it ends up as", as a resolver permalink is ([0029](../decisions/0029-resolver-namespaces.md) §4). A wiki link to it renders with the entity's label from the view or the label cache.

### 6.6 Minting a local item

*Sources: [0078](../decisions/0078-entity-sources.md) §6.*

**Create an item for this writes one change set:** a `create` of a local item, `Q500`, with the source entity's label where one is known; a tier-1 `same-as` between `Q500` and `mhc:Q1` ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §3); and, where the source names a `role`, a statement with that property and the upstream ID. A local member ranks above every provider's ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4), so `Q500` is canonical unless the cluster has a keyed member. Among providers, every entity source ranks after every registry provider, in the order the tenant's `reconcile` record gives its sources and otherwise by number; the order among registry providers is the instance's ([04](04-entities-and-identifiers.md) §4.4, §4.10).

After that, `mhc:Q1` is a cluster member. Its title redirects to `Q500`'s page, a request for either ID returns the fused body under the requested ID, and every reference to it reads as `Q500` to a consumer that prefers the local form ([04](04-entities-and-identifiers.md) §4.12), the tier-1 link being an exact match. In the mirror states its mirrored statements keep contributing to Q500's fused body, as a Wikidata item's do; in the pointer state it contributes only its name. The identifier is a member of a cluster, which the identity machinery already has, and not a new mechanism. Removing the `same-as` undoes it.

### 6.7 Across tenants and instances

*Sources: [0078](../decisions/0078-entity-sources.md) §8; [0080](../decisions/0080-tenants-as-entity-sources.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3.*

**A reference to a source entity crosses a tenant boundary by IRI.** The rewrite is the read-side rewrite of IDs between tenants, the same layer that rewrites a response to the consumer's preferred form ([04](04-entities-and-identifiers.md) §4.12); A's records are never changed. When tenant B reads provider tenant A's `local` graph ([0018](../decisions/0018-tenants.md) §5; [08](08-tenants-and-instances.md)), or another instance reads it through `scatter-adapter-triplespace` ([0022](../decisions/0022-federation.md) §2, which rewrites as 0018 §5 does), a reference to A's `mhc:Q1` is rewritten by the first rule that applies:

1. **A's source is unpublished.** The reference is not readable.
2. **The IRI is a registry provider's, or the source names a tenant that is one.** It becomes that provider's ID, `MHQ1`, once the registry has Miraheze Communities as `MH`.
3. **B has a source for the same graph:** one with the same `tenant` code for a tenant source, which survives a change of the provider's base, or otherwise the same IRI template. It becomes B's ID for the entity, `mhcom:Q1`, whatever B named its source.
4. **Otherwise it is unbound.** Every statement whose subject, value, qualifier value or reference value is the reference is **withheld** from B's resolved view. B's `Special:Providers` counts withheld statements per IRI base, so B's administrators can see what declaring a source would add. Nothing is guessed or partly shown.

B never reads A's `source/{name}` partition: the source's data comes from the source, and B fetches it itself once it declares one. The rewrite reads A's source table from A's `config` on the same instance, and from A's public `GET /entity-sources` on another ([18](18-api.md)). References to B's own entities still come back as bare IDs ([0022](../decisions/0022-federation.md) §2).

### 6.8 Binding is permanent; promotion

*Sources: [0078](../decisions/0078-entity-sources.md) §9; [0080](../decisions/0080-tenants-as-entity-sources.md) §1, §3.*

**A source's name, number and type IRI templates are fixed by its first record.** A later record may change its label, `api`, `entity_data`, `events`, `role`, licence, namespace names and aliases, and its mirror settings; one that changes the name, the number or a template is refused with `ts-source-binding`. A retired source, one whose record is null, keeps its name and number reserved on the tenant for good, and its IDs keep parsing and resolving read-only. Records in the log carry `mhc:Q1` in their headers and content, so its meaning cannot change under them. **A tenant source is bound by its `tenant` code instead of its templates,** which follow the provider's base; a record that changes `tenant` is refused with `ts-source-binding`.

**A source may not duplicate a registry provider.** A declaration with a type IRI template equal to a registry provider's, or a tenant source naming a tenant whose code a registry entry names as its `issuer`, is refused with `ts-source-is-provider`, and the tenant adds the provider to its `providers` list instead ([0018](../decisions/0018-tenants.md) §5). When the registry later gains such a provider, the tenant may not add it to its `providers` list while the source is unpromoted (`ts-source-is-provider` again), and `registry check` lists the source as promotable. So on one tenant, `mhc:Q1` and `MHQ1` never both name an entity.

**Promotion.** When the registry later allocates a provider whose IRI templates equal a source's, or, for a tenant source, whose `issuer` is the source's `tenant` code, the tenant writes the source's record with `promoted_to = "MH"`, which is refused if the templates differ. From then on:

- `mhc:` is an input form of `MH` on the tenant, as a provider slug is (§6.3), and `MHC:Q1` redirects to `Item:MHQ1`.
- Projections rewrite `mhc:Q1` to `MHQ1` when they read the tenant's records, as [0018](../decisions/0018-tenants.md) §5 rewrites a provider tenant's IDs on read. Statement IDs follow [0018](../decisions/0018-tenants.md) §7: the UUID is kept and the prefix becomes the provider form.
- `source/mhc` is frozen and kept. The provider joins the tenant's `providers` list, and mirroring continues in the instance's `mirror/{slug}`.
- Nothing in the log is rewritten.

## 7. Adopting an existing Wikibase

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §1–4, §7; [0078](../decisions/0078-entity-sources.md) §11; [0096](../decisions/0096-adopting-any-mediawiki.md) §1, §2, §5.*

### 7.1 Adoption

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §1; [0096](../decisions/0096-adopting-any-mediawiki.md) §1.*

A tenant **adopts** a MediaWiki Wikibase when the tenant *is* that wiki, continuing on Triplespace: the same identity, the same IDs, the same accounts, usually the same host. Adoption writes the source's entities into the tenant's `local` partition under the IDs the source minted, its accounts under their own numbers ([07](07-actors-and-accounts.md)), and sets every ID sequence past the source's (§7.4).

**A source without Wikibase is adopted the same way** ([0096](../decisions/0096-adopting-any-mediawiki.md) §1). It has no entities: the job writes nothing to `local`, takes no `--counters` and sets no entity floor, and sets the page, revision, log and user floors as for any source. Its pages, accounts and files come as every adopted wiki's do, and its namespaces are registered first (§7.6).

Adoption is neither of the two things it resembles:

- **Not mirroring.** The source is not a foreign provider to the tenant. Nothing is rewritten to a prefixed form, no `mirror/*` partition is created, and no version cursor is kept. If the tenant is a provider to others (`LB`), an adopted `Q6` is `LBQ6` to them from the moment it lands, by [0018](../decisions/0018-tenants.md) §5, with nothing further to do.
- **Not a move** under [0018](../decisions/0018-tenants.md) §10. There is no bundle to verify and no key chain to continue. The adopted records are new records of this instance, attested by this instance.

The tenant's `tenant` config record ([0018](../decisions/0018-tenants.md) §3) gains an **`adopted_from`** field: the source wiki's base URL and the date of adoption. If the host changes, an `alias` record ([0018](../decisions/0018-tenants.md) §9) is written as for any base-URI change; if Triplespace replaces MediaWiki at the same host, nothing is. The tenant side of adoption is in [08](08-tenants-and-instances.md).

### 7.2 Preconditions and the job

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §2; [0096](../decisions/0096-adopting-any-mediawiki.md) §2.*

An adoption is a job (§3.3; [0011](../decisions/0011-logs.md) §6.3) with mode `adopt`, run by a subsidiary of the tenant's operators ([0024](../decisions/0024-subsidiary-accounts.md) §6). Its input is the source's **full-history XML export** (`dumpBackup.php --full`), which holds every revision of every page the source kept. It is refused unless:

1. **the source is frozen.** The operator declares the source read-only and takes the final dump; adoption is a one-way door (§7.3), and anything edited on the source after the dump is lost;
2. **the tenant's slug is the issuer** whose numeric IDs the source's users become ([0018](../decisions/0018-tenants.md) §4), which is to say the tenant was created for this source;
3. **neither partition the job writes holds records** other than those written by earlier adoption jobs for the same source: `local` for entities, and `pages` for every other page (below). The test is per partition, so a job that adopted the entities can be resumed to carry the pages while no page has been edited. This is what makes a failed adoption resumable (§7.3) and everything else an ordinary edit;
4. **the job's operator holds `ts-runjob` and `ts-config`,** since the job seeds sequences (§7.4);
5. **no live mirror partition exists for the tenant's code.** Where the instance had been mirroring the source as a registry provider (`mirror/librarybase` for `LB`), that partition is frozen first: its `entity_source` rows are dropped and composition reads the tenant's `local` partition instead, so that readers see one graph for the code, not a mirror beside the tenant ([08](08-tenants-and-instances.md); §8.3). Adoption under a code with a live mirror partition is refused until then;
6. **the dump carries history,** unless the operator passes `--current-only` (§7.5). A dump in which every page has one revision while revisions name parent revisions is a current-state export, and the refusal says so. A page whose oldest revision names a parent the dump lacks had revisions deleted on the source; it is adopted from the revisions the dump has, and counted.

The job record carries the source's base URL, the dump's identity and date as the source version, the adapter version, `history` (`full`, or `current` under `--current-only`), the floors it set (§7.4), and the counts by outcome: revisions adopted, unchanged and rejected, the entities and pages they belong to, and pages left for later. It projects as `job/start` and `job/finish` like any job.

**Every page comes in the same job.** A page outside the entity namespaces is imported by the procedure of [10](10-pages-and-content-models.md) §6 within the adoption job, into `pages`, keeping its source page ID and, revision by revision, its source revision IDs, attributed as §7.3 attributes entity revisions; one revision sequence then serves both, as it did on the source. A page the job cannot import is counted as `deferred`, and a later adoption job against the same dump carries it under precondition 3. Adopting a wiki with files imports every file version from its file tables and upload directory, deleted versions included, onto file pages that keep their source page IDs ([0039](../decisions/0039-files-and-media.md) §14; [12](12-files-and-media.md)). Before the first page, the job registers the source's namespaces, renaming or archiving any whose name is taken (§7.6).

### 7.3 The `adopt` operation

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §3; [0078](../decisions/0078-entity-sources.md) §11.*

The `adopt` row of §3.2 and the wire form of §3.9 apply. In a record:

- **`id`** is in local form ([0017](../decisions/0017-entity-id-grammar.md) §1): the source's own ID, with no prefix. Any minted entity type the source has may be adopted (`Q`, `P` and `L`; lexemes are [0066](../decisions/0066-lexemes.md), [04](04-entities-and-identifiers.md) §5.1).
- **`entity`** is the source's canonical JSON of the revision ([wikibase-compat.md §3](../api/wikibase-compat.md)). **`redirect`**, in its place, names the target of a revision that made the page a redirect (`{"entity":"Q9","redirect":"Q6"}` in the dump, a merge), so that a merged entity redirects as it did on the source; a later revision that restores the entity carries `entity` again. IDs the source wrote with an entity-source prefix, such as `wikidata:Q42` from a federated install ([wikibase-compat.md §5.1](../api/wikibase-compat.md)), are rewritten to the provider form, `WDQ42`, by the rule of §1.5, using the source's own entity-source table. **An adopted wiki's entity-source prefix that names no registry provider becomes an entity source of the tenant under the same name** (§6), declared by the adoption job before the first `adopt` from that table, from the source wiki's own entity-source table (concept base URI, entity types). IDs in that prefix are then kept as written, `mhc:Q1`, instead of having no rule. If the name is taken (§6.3, rule 1), the adoption's preconditions (§7.2) fail and name it. Every other ID is kept as written: the source's `Q` numbers *are* the tenant's.
- **`source_revid`, `source_parentid`, `source_time`, `source_pageid`** are the source's revision ID, the revision it followed (absent for a page's first), its timestamp, and the page ID of the entity's page on the source; **`minor`** is the source's minor-edit flag. They are content, kept in the content part, and they give the record its provenance ([0035](../decisions/0035-adopting-a-wikibase.md) §6; [07](07-actors-and-accounts.md)), its revision ID and its page ID (§7.4).
- **Statement GUIDs** are kept as the source wrote them. They already carry the entity's own ID as their prefix, which is the invariant [0018](../decisions/0018-tenants.md) §7 preserves.

**The record is the source's revision.** Its revision ID, header field 7, is `source_revid` itself, below the revision floor (§7.4), not a number taken from the tenant's sequence ([0015](../decisions/0015-record-format-and-partition-registry.md) §2; [01](01-log-and-records.md) §2.5). Its comment part is the revision's edit summary. Its attestation names the revision's author as the actor, with the job beside it ([0015](../decisions/0015-record-format-and-partition-registry.md) §1; [01](01-log-and-records.md) §2.4): an adopted account `{code}:{n}` ([07](07-actors-and-accounts.md) §2.5), a surrogate `actor:{n}` for an IP or an `imported>Name` author ([07](07-actors-and-accounts.md) §1.4), or no actor and the `hidden` marker for a user the source hid. The newest revision of a page is the entity's current state, and no adoption revision is written after it: `lastrevid` is the source's, and every `oldid=` the source issued resolves to the same revision here. The record is appended at the time of the job; the time history and recent changes show is `source_time` ([16](16-logs-feeds-and-notifications.md) §2.2).

**Parts the source hid.** A revision the source revision-deleted reaches the export with its text, comment or contributor replaced by `deleted="deleted"`, and the export does not say whether it was deleted or suppressed. The record carries nothing for a withheld part, and **`withheld`** names the parts it lacks, as the parts of a `record` ACL are named ([09](09-security-and-moderation.md) §6.3): `content` when the text was hidden, so that the `adopt` has neither `entity` nor `redirect` and the revision has no state for `oldid` to show to anyone; `comment`, so that the comment part is empty; `attestation`, so that it names no actor. Beside each such record the job writes a `record` ACL with those parts restricted to `suppress` ([09](09-security-and-moderation.md) §6.5), and the history views show them as suppressed. MediaWiki does not let a page's current revision lose its text, so the current state is never withheld. Pages the source deleted are not in the export and are not adopted; the floors still cover their IDs (§7.4).

**`--current-only`** (§7.5) writes one `adopt` per entity, of its newest revision, and nothing older. That record takes a fresh revision ID from the tenant's sequence, its comment part is empty, and its attestation names the job's subsidiary and the job; the entity's `lastrevid` is that local ID, and `source_revid` is shown beside it as "adopted from revision N".

**Why a separate operation.** `create` allocates an ID, merges statements into an existing entity and takes match keys; `adopt` does none of these. `put` is a mirror operation with skip-if-not-newer semantics and the `prev_upstream` fields of §2.6; none of that applies to a first and only write. A separate name gives the permission and filter layers ([09](09-security-and-moderation.md)) something to name, and `adopt` is the one whole-entity write to the local graph that needs no base revision, confined to adoption jobs (§3.2).

**Order and re-running.** An entity's revisions are written in source-revision order: an `adopt` follows only an `adopt` of the same entity with a lower `source_revid`. Re-running an adoption job against the same dump skips every revision already present with the same content and rejects one present under the same revision ID with other content (`ts-adopt-conflict`), so it is how a partial adoption is completed. A newer dump is not a second adoption: once adopted, the wiki changes through edits.

### 7.4 Sequence floors

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §4.*

Every sequence the source consumed is set past what it consumed, so that no number the source ever allocated is allocated again here. The floors are recorded in the job record. Setting a floor never lowers a sequence. The sequences themselves are in [03](03-storage-caches-and-search.md) and [08](08-tenants-and-instances.md).

| Sequence | Floor | Why |
|---|---|---|
| **Entity IDs**, one per minted entity type per tenant (`log.item_id`, `log.property_id`, …), taken in the appending transaction as Wikibase's `wb_id_counters` are; the ID blocks of §3.5 reserve ranges from them | The source's own counters, supplied by the operator from its `wb_id_counters` table (`--counters`); failing that, the highest ID in the dump, with a warning | Numbers the source allocated and then deleted must not be reused, and a dump does not show them |
| **Page IDs** (`log.page_id`). The `adopt` record carries `source_pageid` in header field 9 in place of a freshly taken number; [0015](../decisions/0015-record-format-and-partition-registry.md) §2's rule that the ID is "taken from one sequence the first time a key is written" gains "or supplied by an adoption job" | The source's highest page ID | Clients cite `pageid`; document pages imported later from the same wiki under [0008](../decisions/0008-namespaces-and-document-pages.md) §9 keep theirs the same way |
| **Revision IDs** (`log.revision_id`). Adopted revisions keep the source's IDs, below the floor; every record after them takes a fresh ID above it, a `--current-only` adoption's own records included | The source's highest revision ID | No local revision ever shares a number with a source revision, so `oldid={source revid}` resolves to the source revision, and after a `--current-only` adoption to nothing until a backfill brings it (§7.5), never to an unrelated local revision |
| **Log IDs** (`log.log_id`) | The source's highest log ID | The same, for `logid` |
| **User IDs** ([0007](../decisions/0007-actor-identity.md) §3: "sequential, start at 1") | The source's highest user ID | Source accounts keep their numbers ([07](07-actors-and-accounts.md)) |

### 7.5 History

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §7.*

Adoption brings the source's **history** by default. Its shape is fixed by a constraint, not a preference: a later job must not displace the current state. `entity_source.offset` is the entity's newest record by offset ([0013](../decisions/0013-postgres-storage.md) §5.1) and the base-offset check reads the newest record for the key ([0006](../decisions/0006-log-integrity-and-erasure.md) §8), so past revisions cannot be appended into `local` behind the current one. Two shapes satisfy it:

- **(a) History first, in order: the default.** Each source revision is an `adopt` record under its own revision ID, with its original timestamp, author and summary (§7.3), as [0008](../decisions/0008-namespaces-and-document-pages.md) §9 steps 1–2 do for pages ([10](10-pages-and-content-models.md) §6), and the last revision *is* the current state. It needs the full-history export at adoption time, and it makes adoption one job.
- **(b) History after, as backfill.** Source revisions become `upstream-revision`-shaped records ([0015](../decisions/0015-record-format-and-partition-registry.md) §4) in the tenant's own `log` partition, keyed by entity, metadata first and content where wanted; the history view folds them as it folds upstream history, and `oldid=N` serves them. It works after a current-state adoption and never touches `local`.

**`--current-only`** adopts each entity's newest revision and nothing older (§7.3). It is for a source whose history cannot be had or is not wanted, and it forecloses (a) for that tenant: (b) is then its path.

### 7.6 The source's namespaces

*Sources: [0096](../decisions/0096-adopting-any-mediawiki.md) §2.*

**The job registers the source's namespaces.** It reads the namespaces in the dump's `<siteinfo>`, and treats each one that holds a page of the dump by its number:

1. **A number the registry lists for a `pages` namespace** keeps its pages there, under that namespace's own rules. This covers the main namespace, User, Project, File, Interface, Template, Help, Category and the rest of [10](10-pages-and-content-models.md) §1.4. A page of a model the namespace does not allow is `deferred`.
2. **A number the registry does not list** becomes a tenant `namespace` record ([10](10-pages-and-content-models.md) §1.3 rule 4). The job writes it before the namespace's first page:
   - the source's number;
   - the source's local name, as its canonical name;
   - the case rule the dump gives;
   - kind `pages`, with subpages;
   - the models `wikitext`, `markdown`, `json`, `yaml` and `text`, with `wikitext` the default.

   Where the source has a talk namespace for it, its number becomes a `pages` namespace of the model `triplespace-talk`, and its talk pages become threads as every adopted talk page does ([10](10-pages-and-content-models.md) §6). Semantic MediaWiki's Concept (108) and Page Forms' Form (106) come in this way. A page of a model the instance does not have, such as Semantic MediaWiki's `smw/schema`, is `deferred` ([0096](../decisions/0096-adopting-any-mediawiki.md) Q1).
3. **A number the registry lists for a namespace that holds no pages** (`virtual`, `resolver` or `reserved`) cannot take the source's pages. They are archived (below), or else deferred.

**A name that is taken is renamed or archived.** A `namespace` record whose name equals another namespace's name or alias on the tenant is refused with `ts-namespace-name-taken`. Semantic MediaWiki's Property (102) is the common case: its number is free, and its name is Wikibase's (122). The operator decides each case by number, with `--namespace`:

- **`--namespace 102="SMW property"`** registers the namespace under the new name. Its pages keep their number, their page IDs and their history. A link written `[[Property:Has author]]`, in any revision, still names namespace 122.
- **`--namespace 102=archive`**, or `archive:{prefix}`, registers nothing. It moves the namespace's pages into Project, each titled with the prefix (by default `Archive/`) and then its source title: `Property:Has author` becomes `Project:Archive/Property:Has author`. A page keeps its source page ID, every revision and every author; only its title changes. Its talk page comes as threads on the archived page's talk page. A link to the old title in another page does not follow it. Archiving is for a namespace of content the wiki is retiring.

`archive` applies as well to a namespace under rule 1 or 3 whose pages would otherwise be deferred.

**Conflicts are found before anything is written.** The survey pass that reads the dump for the floors and the accounts (§7.2) also lists every namespace whose name is taken, and every one under rule 3 that holds pages. **The job is refused until `--namespace` decides every taken name**, so that a one-way adoption never stops halfway on a name. Pages under rule 3 left undecided are deferred and counted. The job record carries the decisions with its other parameters, and the namespace records are written under the job's `ts-config` (§7.2, precondition 4).

### 7.7 Converting Semantic MediaWiki and Cargo data

*Sources: [0096](../decisions/0096-adopting-any-mediawiki.md) §5.*

**A wiki that kept data in Semantic MediaWiki (SMW) or Cargo is adopted first, and its data then comes in as one job:** change sets (§3) submitted through the bulk-job endpoint by a subsidiary of the tenant's operators.

**The conversion is attributed to that job.** Its actor is the subsidiary, and nothing it writes is attributed to the source's editors. Their authorship stays in the adopted page history, which holds every revision that wrote an annotation or a stored value.

**The conversion is one-way.** Triplespace keeps no coexistence mode, and holds no SMW or Cargo code: no annotation parser, no store and no query engine. The converter is a script in `tools/`, outside the build. It reads files and calls the instance's public API, and no crate depends on it. Continuous derivation from pages the tenant does not control is template mappings' ([13](13-mirrored-pages.md) §7).

**The converter reads the source's store, not the wikitext that fed it**, since the store holds the values the source wiki computed, including those made by template expansion, parser functions and Lua.

- **For SMW,** it reads the RDF export, by `dumpRDF.php` or its `ExportRDF` special page: the properties and their declared types, the subjects, the subobjects and the values.
- **For Cargo,** it reads each table's declaration from the `#cargo_declare` in its template, and each table's rows by its `CargoExport` special page, `action=cargoquery` or a database dump of its tables.

It also reads the full-history dump the adoption read, for each page's ID and newest revision ID, and for the text of the edits below. A source that offers only an XML dump has its store rebuilt first, outside Triplespace: the dump is imported into a MediaWiki with the same extensions, and the store rebuilt with `rebuildData.php` or `cargoRecreateData.php`.

**The converter drafts a mapping from the store's declarations, and a person reviews it before it is applied**, as a template mapping is drafted and never applied unreviewed (13 §7.5). The draft is TOML. It lists every SMW property or Cargo field with its declared type, its count and sample values. It suggests an existing property where one fits, such as one an SMW property's `Imported from` or `Equivalent URI` names, and otherwise a new local property of the data type its declared type suggests. Cargo fields of the same name in two tables are suggested as one property.

| Declared type | Data type |
|---|---|
| SMW `Page`; Cargo `Page` | `wikibase-item`: the subject of the page it names |
| SMW `Text`, `Code`, `Keyword`, `Telephone number`; Cargo `String`, `Text`, `Wikitext`, `Wikitext string`, `Searchtext` | `string`, or `monolingualtext` where the mapping gives a language |
| SMW `Monolingual text` | `monolingualtext` |
| SMW `Number`; Cargo `Integer`, `Float`, `Rating` | `quantity` without a unit |
| SMW `Quantity`, `Temperature` | `quantity`, its unit an item from a value map (13 §7.4) |
| SMW `Date`; Cargo `Date`, `Datetime`, `Start date`, `End date` | `time` |
| SMW `URL`, `Annotation URI`, `Email`; Cargo `URL`, `Email` | `url`, an email address as a `mailto:` URL |
| SMW `External identifier` | `external-id` |
| SMW `Geographic coordinates`; Cargo `Coordinates` | `globe-coordinate` |
| Cargo `File` | `localMedia` ([12](12-files-and-media.md) §7) |
| SMW `Boolean`; Cargo `Boolean` | `wikibase-item`: the mapping's items for true and false |
| SMW `Record` | The statement of its first field, with its other fields as qualifiers |
| SMW `Reference` | The statement of its value, with its other fields as the statement's reference |
| Cargo `List (…) of` a type | One statement per value, of that type's data type |

A declaration of allowed values, SMW's `Allows value` or Cargo's `allowed values`, becomes a one-of constraint on the property ([06](06-statements-and-properties.md) §2).

**The mapping says what each kind of subject becomes.** A kind is an SMW category, a Cargo table, or the subobjects of an SMW property. The draft leaves each choice to the reviewer ([0096](../decisions/0096-adopting-any-mediawiki.md) Q2):

- **`page`:** the page itself. Its statements are page statements ([06](06-statements-and-properties.md) §5), addressed by the source page ID the adoption kept.
- **`item`:** a local item labelled with the page's title, and paired with the page by a sitelink to the tenant's own host (06 §4.6), so that `wbgetentities` and every Wikibase client reach it.
- **For subobjects, and for a Cargo table with several rows per page:** `item`, an item of its own, linked from the page's subject by a property the mapping names; or `qualifiers`, one statement on the page's subject with the row's other fields as its qualifiers.

A `Page` value is the subject of the page it names. Where that page has no subject in the mapping, the property's mapping says what the value becomes: a new item labelled with the title, a string, or nothing. Every such value is reported. **Categories are not converted.** They stay categories, and a category mapping ([10](10-pages-and-content-models.md) §5.4) projects statements from them where the tenant wants it to.

**Properties are written first, then subjects, as `create-or-add`** (§3.5). The match key is a local `external-id` property the mapping names. Its value is the source's identifier of the subject: the page ID for a page's subject, `{page ID}#{subobject name}` for an SMW subobject, and `{table}/{_ID}` for a Cargo row. A failed or partial job can therefore be re-run, and it adds nothing twice.

**Every statement cites the revision that stated it.** Its reference carries the properties the mapping's `reference` names, as an extraction source's does (13 §6.2): the URL `index.php?oldid={revision ID}` of the page's newest revision in the dump, which resolves on the tenant because adoption kept the source's revision IDs (§7.3), and the date of the dump. **The values are the store's, as of the dump.** No statement history is reconstructed. Earlier values are in the adopted page history only.

**After the data job, the converter makes one edit to each page that carried storage or query syntax.** The edits are made by the same subsidiary, with a summary naming the job. Every revision before that edit is the source's, untouched.

- `#set`, `#subobject`, `#declare`, `#cargo_store`, `#cargo_attach` and `#cargo_declare` calls are removed. Their data is in statements, and the declarations are in the mapping.
- Inline annotations are unwrapped. `[[P::V]]` becomes `[[V]]`, and `[[P::V|text]]` becomes `[[V|text]]`, where `P` is a `Page` property. For any other type, the annotation becomes `V` or `text` as plain text. An annotation written to be hidden (`[[P::V| ]]`) is removed.
- An `#ask`, `#show` or `#cargo_query` whose query the converter can express in SPARQL becomes a `Query:` page, with a `{{#query:}}` block in its place ([15](15-structured-pages.md) §5). Any other query stays, as a chip ([11](11-rendering-templates-and-modules.md) §2.1), and is reported (0096 Q3).
- Templates are edited the same way. A template whose output depended on queried data is reported, to be ported by hand.
- Form pages, and Page Forms calls in other pages, are left as they are, as chips.

**The converter reports:** per property or field, the statements written and the values it could not read, with examples; the `Page` values that named a page with no subject; the pages it rewrote; the queries it translated, and those it left; and the templates to port by hand.

## 8. The provider catalogue

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9; [0036](../decisions/0036-openstreetmap-providers.md) §1, §2, §4, §5; [0037](../decisions/0037-gdelt-provider.md) §1–7.*

### 8.1 The registered providers

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4; [0036](../decisions/0036-openstreetmap-providers.md) §1; [0037](../decisions/0037-gdelt-provider.md) §1; [0091](../decisions/0091-one-wikibase-adapter.md) §1.*

`providers.toml` ([23](23-configuration-and-registry.md)) is the registry of record for providers. A provider's code, number, slug and per-type IRI templates are never changed once allocated, and codes and numbers are never reused; number 0 is the instance itself and is never assigned; registry provider numbers are below 2^12, and numbers 4096–8191 (2^12 to 2^13 − 1) are never assigned in the registry, since each tenant assigns them to its entity sources (§6.4), so that `number << 40 | n` stays below 2^53 for `n` below 2^40 and **every ID a client sees is below 2^53**, exact in a JavaScript number; and the twenty-six doubled letters `AA`…`ZZ` are reserved codes, because a type letter written three times is the tenant-relative form of a local ID ([04](04-entities-and-identifiers.md)). Each provider has a two-letter `code`, a `slug` used in graph names (`mirror/{slug}`) and the API, a `number` for the revision-ID range `revid = number << 40 | n`, an `issuer` that its revisions attribute actors to (or the provider itself for providers without individual actors), an `actor_model` (`individual` when the provider's changes carry upstream actors, `provider-only` when they are attributed to the provider as a whole; `issuer == slug` is not the test, since OpenStreetMap's issuer is its slug and its changesets have authors), `revision_ids` (true when the provider publishes revision IDs, so that `n` is the upstream revision ID; otherwise `n` is the mirror record's offset), `sync_deltas` (`summary` or `full`, §2.6), `deletion` (`tombstone`, or `clear` for a provider whose upstream deletions clear the mirror's contribution with a `put` of the empty state, §2.1), one `adapter` crate per provider, shared by every provider with the same kind of upstream, so that every MediaWiki+Wikibase names `scatter-adapter-wikibase` ([22](22-crates-and-stack.md); [0091](../decisions/0091-one-wikibase-adapter.md) §1), an optional `trust` with `keys` for a provider that publishes a tlog checkpoint (`verified`, the default for `scatter-adapter-triplespace`, checks every mirrored batch against the provider's checkpoint and key chain; `stream` mirrors without proofs), and chip colours ([19](19-site-ui.md)). Each `[[provider.type]]` gives one entity type the provider mints: a one-letter `code`, the `entity_type`, the `upstream_prefix`, the `iri` template over `{upstream_id}` and the `namespace` whose pages host the type; `id_grammar` may be set on a provider or, overriding it, on a single type ([04](04-entities-and-identifiers.md)); `key_mapped` on a type means its upstream entities are written under a keyed type's keys (§4) and the type code stays reserved; a type with `entity_type = "property"` on a provider that publishes no properties of its own (OpenAlex, OpenStreetMap, GDELT) is the provider's **property type**, whose properties the adapter mints under deterministic IDs with its mapping version, so that an adapter never maps onto a tenant's properties ([04](04-entities-and-identifiers.md) §4.6). A provider marked `pages` is one whose pages a page repository serves ([13](13-mirrored-pages.md)); it may mint no entities, and English Wikipedia's entry (slug `enwiki`, number 9, issuer `enwiki`) is a pending allocation rather than a row, since `scatter-providers` does not yet accept a codeless entry.

| Code | Slug | Number | Types (code: upstream prefix → entity type) | ID grammar | Issuer | `actor_model` | `revision_ids` | `deletion` | Adapter | Trust |
|---|---|---|---|---|---|---|---|---|---|---|
| `WD` | `wikidata` | 1 | `Q`: `Q` → item; `P`: `P` → property; `L`: `L` → lexeme; `M`: `M` → mediainfo (Commons, §8.2); `E`: `E` → entityschema | `digits` | `wikidatawiki`; agent `http://www.wikidata.org/entity/Q2013` | `individual` | true | `tombstone` | `scatter-adapter-wikibase` | — |
| `LB` | `librarybase` | 2 | `Q`: `Q` → item; `P`: `P` → property | `digits` | `librarybase` | `individual` | true | `tombstone` | `scatter-adapter-wikibase` | — |
| `OA` | `openalex` | 3 | `W` Work, `A` Author, `S` Source, `I` Institution, `T` Topic, `P` Publisher, `F` Funder, `G` Award: each `{code}` → item; `K`: `keywords/` → keyword, `key_mapped` | `digits` | `openalex` | `provider-only` | false | `tombstone` | `scatter-adapter-openalex` | — |
| `MB` | `musicbrainz` | 4 | `A` artist, `G` release group, `R` release, `C` recording, `W` work, `L` label, `D` area, `P` place, `E` event, `S` series, `I` instrument, `N` genre: each `""` → item | `uuid` | `musicbrainz` | — | false | `tombstone` | `scatter-adapter-musicbrainz` | — |
| `XD` | `internetdomains` | 5 | `Q`: `Q` → item (domain items key-mapped onto `domain`); `P`: `P` → property | `digits` | `internetdomains` | `individual` | true | `tombstone` (keyed items: clear, §4.2) | `scatter-adapter-wikibase` | — |
| `OS` | `openstreetmap` | 6 | `N` node, `W` way, `R` relation: each `""` → item | `digits` | `openstreetmap` (the changeset author's OSM user ID) | `individual` | false | `clear` | `scatter-adapter-openstreetmap` | — |
| `OW` | `osmwiki` | 7 | `Q`: `Q` → item (key and tag items key-mapped onto `notation`, scheme `osm`); `P`: `P` → property | `digits` | `osmwiki` | `individual` | true | `tombstone` (keyed items: clear, §4.2) | `scatter-adapter-wikibase` | — |
| `GD` | `gdelt` | 8 | `E` event, `C` CAMEO event code: `""` → item, `digits`; `D` document: `""` → item, `gdelt-record`; `T` GKG theme: `""` → item, `token` | per type | `gdelt` | `provider-only` | false | `tombstone` | `scatter-adapter-gdelt` | — |

No registered provider sets `trust`; it applies to a provider that is a Triplespace instance or Scatterbase ([0022](../decisions/0022-federation.md) §2). A provider without individual actors has one actor record of kind `provider`, agent type `prov:Organization`, written when the provider is registered ([07](07-actors-and-accounts.md)).

No registered provider sets `sync_deltas`, so every row takes the default `summary`. The `deletion` column is `clear` only for OpenStreetMap (§8.7); the key-mapped items of `XD` and `OW` are cleared by the keyed-entity rule of §4.2 regardless of the flag.

### 8.2 Wikidata, and Commons as the source of type `M`

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2, §4; [0012](../decisions/0012-api-requirements.md) §2.2; [0070](../decisions/0070-shallow-entity-mirroring.md) §2.1, §2.3, §5.*

Wikidata is the provider the mirror model is written against: every observed state carries its upstream revision ID (`lastrevid`, the version cursor of §3.4), its upstream history can be backfilled from the API or history dumps (§1.3), and without a stored delta the diff of a sync links to `index.php?diff={new}&oldid={prev}` (§2.6). Its registry entry carries `article_path`, `api` and `entity_data`; its entities project to `http://www.wikidata.org/entity/{upstream_id}`, type `M` to `https://commons.wikimedia.org/entity/{upstream_id}`. Shallow mirroring follows `mediawiki.recentchange` filtered to `wikidata.org` (§5.7), and the default closure properties of §5.3 are Wikidata's. Type `E` is read from namespace 640 pages, not the entity dumps ([0064](../decisions/0064-entityschema-and-validation.md) §1; [15](15-structured-pages.md)); type `L` is [0066](../decisions/0066-lexemes.md) ([04](04-entities-and-identifiers.md)).

**Commons.** A MediaInfo entity is the `mediainfo` slot of a File page, not a namespace of its own, and Commons is the source of type `M` under Wikidata's provider ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §2); `mediainfo.mirror` is `entities.mirror` for that type under its own name, default `on-demand` (§5.2). The per-type source fields `api`, `entity_data` and `dumps` that 0065 §5 adds are a pending allocation in `providers.toml` until `scatter-providers` accepts them. The rest of Commons as a file and MediaInfo source is in [12](12-files-and-media.md).

### 8.3 Librarybase

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4; [0035](../decisions/0035-adopting-a-wikibase.md) §1, §7; [0091](../decisions/0091-one-wikibase-adapter.md) §1.*

Librarybase (`LB`, number 2) is any MediaWiki+Wikibase mirrored through `scatter-adapter-wikibase`, which is parameterised by the registry entry; its entities project to `https://librarybase.org/entity/{upstream_id}`. It is also the worked example of a tenant as a provider, whose `Q6` is `LBQ6` to readers by rewriting on read ([08](08-tenants-and-instances.md)), and of adoption: its first milestone is API-only, and it adopts with its history (§7.5). **Whether `LB` is a remote wiki to sync or a tenant here is instance configuration, read from the registry's `issuer`:** an instance one of whose tenants is the provider's issuer refuses every sync job for that provider and reads the tenant's `local` partition as the provider's graph (§5.9); an instance that had been mirroring Librarybase before adopting it freezes `mirror/librarybase` as a precondition of the adoption (§7.2).

### 8.4 OpenAlex

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2, §4, §8.4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9.*

OpenAlex (`OA`, number 3) publishes no revision history, so only observed history exists for it (§1.3), its version cursor is `updated_date` (§3.4), the live fetch of upstream edits answers `upstream-unsupported` (§2.7), and its changes are attributed to the provider as a whole and to the import job ([07](07-actors-and-accounts.md)). Its adapter maps a non-Wikibase source onto properties: a Wikidata property where one fits, and otherwise a property of OpenAlex's own property type, minted by the adapter under deterministic IDs and never a tenant's property ([04](04-entities-and-identifiers.md) §4.6). Its statement IDs are deterministic, a hash of the main snak and the adapter-declared identifying qualifiers, an authorship's position, institution and award, never its scores (§3.4). Its entities project to `https://openalex.org/{upstream_id}`. Keywords (`K`, upstream prefix `keywords/`) are mapped by key onto the `keyword` keyed type (§4.1): no `OAK` entity is ever minted, and the code stays reserved.

### 8.5 MusicBrainz

*Sources: [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4.*

MusicBrainz (`MB`, number 4) has `id_grammar = "uuid"`: an entity ID is the three-letter prefix followed by the MBID verbatim, lowercase, such as `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d`, and each type projects to `https://musicbrainz.org/{artist|release-group|release|recording|work|label|area|place|event|series|instrument|genre}/{upstream_id}`. MusicBrainz `url` entities are not mirrored as items; they become URL-valued statements on the entity that links them. The grammar is in [04](04-entities-and-identifiers.md) §2.2.

### 8.6 internetdomains.wiki

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §9; [0091](../decisions/0091-one-wikibase-adapter.md) §2.*

The Internet Domains Wikibase (`XD`, number 5) is the key-mapped provider of §4. Its key map is one rule with no class ([0091](../decisions/0091-one-wikibase-adapter.md) §2), `identity_property = "P1"`, a string property holding the domain name, and `key_type = "domain"`, and its adapter configuration sets `identity_property_on_mapped = "drop"`: on a mapped item `P1` is dropped from the mirrored state, since the key is the value, and kept on unmapped or rejected items, where it is ordinary data. The identity property is adapter configuration, not part of the ID scheme. The wiki was domains.wikibase.cloud before it moved to internetdomains.wiki; the slug and every `internetdomains:{id}` actor key are stable across the move ([0018](../decisions/0018-tenants.md) §4).

### 8.7 OpenStreetMap: two providers, and `OS` map objects

*Sources: [0036](../decisions/0036-openstreetmap-providers.md) §1, §2; [0091](../decisions/0091-one-wikibase-adapter.md) §1.*

| | Map data | Vocabulary |
|---|---|---|
| Code, slug, number | `OS`, `openstreetmap`, 6 | `OW`, `osmwiki`, 7 |
| Types | `N` node, `W` way, `R` relation | `Q` item, `P` property |
| IDs | `OSN123`, `OSW123`, `OSR123` | `OWQ7`, `OWP16` |
| Source | planet, replication diffs, API | `wiki.openstreetmap.org` Wikibase |
| Adapter | `scatter-adapter-openstreetmap` | `scatter-adapter-wikibase`, parameterised, with key-mapping (§4) |
| `revision_ids` | false | true |
| Issuer | `openstreetmap` (OSM user IDs) | `osmwiki` (wiki user IDs) |

Both are in `providers.toml` and `issuers.toml`. They are two providers rather than five types under `OS` because `revision_ids` is one flag per provider, the revision-ID range `provider_number << 40 | n` ([0015](../decisions/0015-record-format-and-partition-registry.md) §2) needs one scheme for `n`, the issuers' user IDs are unrelated, and one mirror graph would mix two licences and two sync cursors.

**`OS`: map objects.**

- Each node, way and relation is a foreign item under its type code. The IDs are bare digits (`upstream_prefix = ""`, `id_grammar` `digits`), and the type code is what separates node 1 from way 1. Deleted IDs are never reused upstream; the provider sets `deletion = clear` (§8.1), so a deletion clears the mirror's contribution with a `put` of the empty state, as §2.1 allows, rather than a tombstone.
- `revision_ids = false`: OSM versions are per object and changesets are not a global revision sequence, so `n` is the mirror record's offset. The upstream version and changeset ID are kept as source metadata on the record, and the changeset author's numeric user ID is the actor (`openstreetmap:{uid}`).
- Canonical IRIs are `https://www.openstreetmap.org/{node|way|relation}/{id}`.
- Way and relation membership become references to `OSN`, `OSW` and `OSR` values, by role. A node's coordinates become a coordinate value. Geometry beyond that is not derived.

### 8.8 Tags on map objects

*Sources: [0036](../decisions/0036-openstreetmap-providers.md) §4.*

A tag `k=v` on an `OS` object becomes one of three things. Nothing is dropped.

1. **A mapped key.** If `k` is in the adapter's key map, it becomes the mapped property, a term, or a link, with the native value type. `name` and `name:{lang}` become labels and `alt_name` aliases. `website` becomes a URL value. The key map names properties **by role** ([0003](../decisions/0003-statement-ui.md) §7), like the identity property in §4.1, so it never depends on a property's number; the bindings it reads are the instance-scope `role` record's ([06](06-statements-and-properties.md) §1), never a tenant's, and they name registry-provider properties (`WDP…`, `OWP…`) or properties of OpenStreetMap's own property type ([04](04-entities-and-identifiers.md) §4.6). It is adapter configuration with shipped defaults, and mapping another key later reprojects from the log.
2. **A documented tag.** If a notation for exactly `osm:k=v` exists in the `OW` mirror, or the wiki says the key takes well-known values, the object gets a statement with role `osm-tag` and value `notation:osm:k=v`.
3. **Everything else.** The object gets a statement with role `osm-tag-text`, string value `v`, and a qualifier with role `osm-key` and value `notation:osm:k`.

Free-text keys such as `name`, `phone` and `opening_hours` should have their own properties, by role, through case 1. Case 3 is the fallback until they do, and it means no property is minted per key and no per-instance property numbers exist: whatever a mapped key becomes is the same property on every instance. The roles `osm-tag`, `osm-tag-text` and `osm-key` are registered with the other roles of [0003](../decisions/0003-statement-ui.md) §7 ([06](06-statements-and-properties.md)), and the properties bound to `osm-tag` and `osm-key` carry the `notation-scheme` statement `osm` ([0048](../decisions/0048-notation.md) §4). Without a mirrored `OW`, case 2 does not occur and every unmapped tag takes case 3.

### 8.9 `OW`: the vocabulary

*Sources: [0036](../decisions/0036-openstreetmap-providers.md) §5; [0091](../decisions/0091-one-wikibase-adapter.md) §2.*

The wiki's Wikibase models keys and tags as items and describes them with about fifty properties. The adapter maps two classes to notations in the `osm` scheme by two key-map rules (§4.1; [0091](../decisions/0091-one-wikibase-adapter.md) §2), each naming its `class`, its identity property, `key_type = "notation"` and `key_scheme = "osm"` ([0048](../decisions/0048-notation.md) §6; [04](04-entities-and-identifiers.md)). The configuration's `class_property` is `P2`, the wiki's "instance of" ([0036](../decisions/0036-openstreetmap-providers.md) A8), and a rule matches an item's direct class only:

| Class | Identity property | Becomes |
|---|---|---|
| `Q7` OpenStreetMap key | `P16` permanent key ID, stated never to change | `notation:osm:{key}` |
| `Q2` OpenStreetMap tag | `P19` permanent tag ID, `key=value` | `notation:osm:{key}={value}` |

The rules of §4 apply. An item with one valid identity value is written under its key, and the upstream ID becomes an alias. Values pointing at such items become `wikibase-notation` values. An item with no valid identity value, or with several, is reported and stays `OWQ{n}`. `P16` and `P19` are dropped from a mapped item's mirrored state, since the key is the value (`identity_property_on_mapped = "drop"`).

**What the wiki adds to the tag entity**, all as mirrored statements:

- **Concept links.** `P7` (Wikidata equivalent) and `P12` (Wikidata concept) link a tag or key to a Wikidata item. They become ordinary statements, and never `same-as` (`concept_link_properties`; notation has `clusters = false`, [04](04-entities-and-identifiers.md)).
- **Relations between tags.** `P3` (subclass of), `P45` (implies), `P44` (incompatible with), `P51` (identical to), `P18` (different from) and `P20` (property different from), with targets rewritten to notation values where the target item is mapped. These are exactly the rule-shaped facts constraints ([0031](../decisions/0031-property-constraints.md); [06](06-statements-and-properties.md)) could use, and OSM has no other place for them.
- **Usage.** Applicability to nodes, ways, relations and areas (`P33`, `P34` and siblings), regional use (`P29`, `P30`), status, key type (`P9`) and documentation pages (`P31`).

Properties marked DEPRECATED upstream are skipped (`skip_deprecated_properties = true`). The wiki documents only the keys and tags someone has written up; every other tag still exists as a keyed entity, without these statements.

### 8.10 GDELT: one provider, its types and IDs

*Sources: [0037](../decisions/0037-gdelt-provider.md) §1, §2.*

`GD` (number 8, slug `gdelt`, issuer `gdelt`) is registered in `providers.toml` and `issuers.toml`. It has no individual actors (`actor_model = "provider-only"`, as OpenAlex), and `revision_ids = false`, so `n` in the revision-ID range is the mirror record's offset ([0015](../decisions/0015-record-format-and-partition-registry.md) §2). The adapter is `scatter-adapter-gdelt` ([22](22-crates-and-stack.md)). The GDELT 1.0 files (events only, a different column set) are out of scope; the provider covers the 2.0 files that begin in 2015.

| Code | Type | ID | Notes |
|---|---|---|---|
| `E` | Event | `GDE1234567890` | `GlobalEventID`; grammar `digits` |
| `D` | Document | `GDD20150218230000-12`, `GDD20150218230000-T12` | `GKGRECORDID`; grammar `gdelt-record` |
| `C` | CAMEO event code | `GDC0311` | the code as GDELT prints it, leading zeros kept; grammar `digits`, canonical as given |
| `T` | GKG theme | `GDTTAX_FNCACT_MAYOR` | grammar `token` |

**Two grammars, and a grammar may be set on a type.** `gdelt-record` is 14 digits, `-`, an optional uppercase `T`, digits; `token` is uppercase letters, digits and underscores, starting with a letter, at most 128 characters; and `id_grammar` may be set on a single type, overriding the provider's ([04](04-entities-and-identifiers.md) §2.2). Disjointness from keyed types is unaffected: a keyed ID carries its `type:` prefix ([0017](../decisions/0017-entity-id-grammar.md) §3).

**Canonical IRIs are Triplespace's.** GDELT publishes none, so `https://scatter.red/gdelt/{event|document|cameo|theme}/{id}` is minted here (§1.5). What `scatter.red` serves at those addresses is as open as it is for domains ([0009](../decisions/0009-keyed-entity-types-and-domain.md)).

**Themes and CAMEO codes are provider items, not keyed types.** They are finite vocabularies that GDELT defines and lists (the Category List and the CAMEO tables). Keyed types are for open-ended natural keys that several providers share (Domain, Keyword, and the `osm` scheme of Notation, [0048](../decisions/0048-notation.md) §1, §6). Each theme and CAMEO code carries its label and, for CAMEO, its position in the hierarchy (root, base, code) as `subclass of` statements to its parent code. No namespace pair is used.

### 8.11 What a GDELT document becomes

*Sources: [0037](../decisions/0037-gdelt-provider.md) §3.*

A document (`D`) is an item. It carries, by role, the roles being bound in the instance-scope `role` record ([06](06-statements-and-properties.md) §1) to Wikidata properties or to properties of GDELT's own property type ([04](04-entities-and-identifiers.md) §4.6):

| Role | Value | From |
|---|---|---|
| `document-url` | a URL, normalized by the sitelink normalizer ([0026](../decisions/0026-sitelinks.md) §1) | `DocumentIdentifier` |
| `published` | a time | the GKG date |
| `source-domain` | a `wikibase-domain` value ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §5) | `SourceCommonName`, when it is a valid Domain key |
| `theme` | a `GDT` item | the enhanced themes, without offsets |
| `location` | a coordinate, with qualifiers for the name, FIPS country, ADM1 code and `FeatureID` | the enhanced locations, without offsets |
| `tone`, `positive-score`, `negative-score`, `polarity`, `word-count` | quantities | the V1.5 tone field |
| `source-language` | an ISO 639-2 code | the translation info, when present |

Documents are items, not resolver keys. The `url` resolver ([0029](../decisions/0029-resolver-namespaces.md) §5) is bound to sitelinks, so it finds an item only when the URL is one of its sitelinks.

**Version 1 mirrors only web sources** (`SourceCollectionIdentifier` 1). The other collections use other kinds of identifier (citations, DOIs), and they are left for later.

**People and organizations are off by default.** GKG lists the names it extracts from an article, and a private individual named in a news story is not a public figure. The adapter's `include_persons` and `include_organizations` options default to false. When on, a name is a string-valued statement on the document with no character offsets, never an entity, and never reconciled to Wikidata automatically. Erasure of a document ([01](01-log-and-records.md)) removes them with it.

Not yet: version 1 does not carry quotations (verbatim text the publisher owns), amounts, GCAM, images and embeds, `AllNames`, extracted dates or the extras XML; whether a `document-url` role should also be bound to the `url` resolver is open in [0037](../decisions/0037-gdelt-provider.md).

### 8.12 What a GDELT event becomes

*Sources: [0037](../decisions/0037-gdelt-provider.md) §4.*

An event (`E`) is an item with statements, by role, bound as a document's are (§8.11):

- **`event-type`**: a `GDC` item for the `EventCode`. Base and root codes follow from its `subclass of` chain, so they are not stored again.
- **`event-date`**: the `Day`.
- **`goldstein-scale`**, **`avg-tone`**, **`quad-class`**: quantities.
- **`event-actor`**, once per actor: the actor's name as a string (or its CAMEO code when the name is empty), with qualifiers for its ordinal (1 or 2), CAMEO actor code, country code, known-group code, ethnic and religion codes and type codes. Actors are strings, not entities: GDELT gives them no identity beyond a code and a name.
- **`location`**: the action geography as a coordinate, with the same qualifiers as a document's location.
- **`source-url`**: the row's `SOURCEURL`.
- **`mentioned-in`**, once per kept mention: the mention's document, with qualifiers for the confidence, mention time, sentence number, character offsets, and the document's tone at mention.

`NumMentions`, `NumSources` and `NumArticles` are not stored. They change as mentions arrive and the projection can count `mentioned-in` statements.

**Resolved references.** A `location` or `mentioned-in` value is written as a reference to another entity only when a lookup by identifier finds exactly one: a place by its `FeatureID` against the GNS or GNIS identifier properties that Wikidata mirrors carry (P2326 and P590), and a document by its normalized `document-url`. The rule is the shape of a tier-3 link ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §3) but produces a value, not a cluster. Otherwise the coordinate or URL stands alone. Nothing about an event ever joins an identity cluster: an event is not the place it happened or the article that reported it.

**The FIPS problem.** Geography fields use FIPS 10-4 country codes and actor countries use CAMEO codes, not ISO codes. The adapter keeps the codes as given and adds no mapping table. Wikidata's FIPS 10-4 property (P901) is the bridge, reached by ordinary identifier lookup when a tenant wants it.

### 8.13 GDELT scope, feed, attribution and licence

*Sources: [0037](../decisions/0037-gdelt-provider.md) §5, §6, §7.*

**Scope: a configured subset.** Nothing is mirrored until an instance configures a filter. The adapter's configuration selects, independently for events and for documents:

- a date range;
- event root codes (`EventRootCode`) and `QuadClass`;
- countries and ADM1 regions of the action geography;
- themes;
- source domains.

A mention is kept only when its event is kept and its confidence reaches a configured threshold. A document is kept when it passes the document filter or is the target of a kept mention. Filtering is by row and is applied before anything is written, so filtered-out data never reaches the log. A tenant whose purpose is the citation graph would filter documents by source domain and mirror few events. A tenant studying an event class would filter events by root code and country.

**The feed.** The adapter follows the GDELT 2.0 file lists: `lastupdate.txt` for the current slot, `masterfilelist.txt` for the backlog, and the translation list for translated records (`-T` IDs). Each 15-minute slot has an events export, a mentions file and a GKG file. The version cursor of §3.4 is the last slot fully ingested, so a slot is the unit of resumption, and re-running a slot is idempotent: the same IDs produce the same records. If GDELT ever re-emits an ID with different content, the adapter treats it as an upsert. The codebook does not say whether events are revised or deleted after publication. The adapter assumes none are and reports a surprise, so nothing here depends on the answer. Bootstrap of a large backlog uses the mode of §3.8. An instance that filters tightly will not need it.

**Attribution and licence.** GDELT's terms allow use, redistribution and mirroring in any form, and require a citation to the GDELT Project with a link to its website. The `mirror/gdelt` graph's metadata carries that citation, and every dump and page that shows GDELT data shows it. This settles GDELT's licence for a mirror ([0005](../decisions/0005-crate-organization.md) §6; [22](22-crates-and-stack.md)). It does not extend to article text, which is why the adapter stores URLs, names, codes and scores and never text or quotations.
