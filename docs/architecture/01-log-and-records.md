# 01. Log and records

This chapter describes the append-only log that is Triplespace's source of truth: its partitions and their policies, what a record is (a header the Merkle tree commits to and a body of erasable parts), how records are encoded and hashed, how the tree, segments and signed checkpoints make the log verifiable, and what erasure, compaction, edit conflicts, verification and export mean at the level of the log. It assumes the per-tenant partition layout and the instance partitions from [08](08-tenants-and-instances.md), the Postgres tables and the physical packing of rows from [03](03-storage-caches-and-search.md), the change-set payload and the jobs that write mirror records from [05](05-providers-and-ingest.md), actors and their keys from [07](07-actors-and-accounts.md), permissions and visibility from [09](09-security-and-moderation.md), log events and job records from [16](16-logs-feeds-and-notifications.md), and the API surfaces that expose revision IDs from [18](18-api.md).

## 1. The log and its partitions

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0006](../decisions/0006-log-integrity-and-erasure.md) §1, §4; [0046](../decisions/0046-primary-tenant.md) §4.*

### 1.1 Scope: the log, not the claim

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §1.*

Integrity is specified at the level of the log, not of the claim.

| Concern | Level | Covered here |
|---|---|---|
| Canonical encoding, record hashes, Merkle log, signed checkpoints | Log | Yes |
| Erasure with a verifiable gap | Log | Yes |
| Edit conflicts per entity | Log | Yes (§7) |
| Client signatures, attribution keys, key registration by clients | Claim | No. The attestation part (§2.4) holds them: [0015](../decisions/0015-record-format-and-partition-registry.md) §1 defines the `signature` field and the `key` record, and [0024](../decisions/0024-subsidiary-accounts.md) §4 lets subsidiaries use them |
| Prior-use acknowledgments for each term | Claim | No. The attestation slot has room for them |
| Claim IDs and their preimages | Claim | No. Scatterbase specifies them |

Triplespace's instance key is the only signer of checkpoints and manifests (a client signature in the attestation, §2.4, is in addition). A signed checkpoint means "this instance attests that these records were appended in this order". It does not prove which person wrote a record.

### 1.2 Partitions carry their own policy

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0006](../decisions/0006-log-integrity-and-erasure.md) §2, §3, §4.*

The log is split into partitions by source graph ([0002](../decisions/0002-source-graphs-and-mass-ingest.md), Consequences). A partition is identified by 64 random bits, drawn when the partition is created and never changed ([0018](../decisions/0018-tenants.md) §2). Each partition has:

- a **history policy** (`full` or `latest`, [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2): keep every record, or compact to the latest record for each key;
- an **integrity policy:** `logged`, a Merkle log over the whole partition with signed checkpoints, or `hashed`, commitments for each segment only;
- a **segment size** 2^k and the **hash function**, fixed at creation. Changing the hash function means starting a new partition.

**A partition has no genesis record.** Its name, hash function, `k`, history policy, integrity policy and export policy are its `graph:` record in the `config` partition ([0015](../decisions/0015-record-format-and-partition-registry.md) §3; [23](23-configuration-and-registry.md)), which the export bundle carries (§8); the partition's first record is an ordinary record at offset 0. In `config` itself that record is the founding record, the `key:` record that registers the instance key or, for a tenant, copies it in (§2.4, §4.3).

Scatterbase's claim partition keeps everything and is `logged`. Triplespace's mirror partitions compact. `scatter-log` supports both from its first release. Adding either one later would change the on-disk format.

The two integrity policies are fixed when the partition is created:

| Policy | What is committed | Compaction | Used for |
|---|---|---|---|
| **`logged`** | Every header, in one Merkle tree for the whole partition, with signed checkpoints (§4) | Never | Triplespace's local and configuration partitions; Scatterbase's claim, meta and server partitions |
| **`hashed`** | Every header, in one Merkle tree for each sealed segment, with a signed manifest for each sealed segment and nothing else | Allowed | Triplespace's mirror partitions; Scatterbase's foreign partition |

**`logged` partitions prove that history is complete and in order.** A verifier can show that no record was added, removed, altered or reordered between any two checkpoints. `log.checkpoint` rows exist for `logged` partitions only ([03](03-storage-caches-and-search.md) §2.2).

**`hashed` partitions prove less.** A verifier can show that each record in a sealed segment is unaltered since the segment was sealed. A `hashed` partition has segment manifests and no checkpoints: the "checkpoint" a job writes at its end (§4.2) is the manifest of the last sealed segment plus an unsigned head offset, and the records in the open segment are **uncommitted until the segment is sealed**. A compacted offset keeps its leaf, so a segment's tree and its manifest are unchanged by compaction, and the holes are visible. A verifier cannot show that compaction kept the right records. That limit is deliberate: a mirror's source of truth is upstream, and [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2 lets a mirror forget.

**Retained mirror entities stay in `hashed` partitions.** They are exempt from compaction ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5), but they get no extra commitment. Their durable copy is the record that materializes them into the local graph when the tombstone arrives, and that record is in a `logged` partition.

A mirror with the `full` history policy is still `hashed`. It simply never compacts.

**A partition may be backed by files or by Postgres.** The `segments` backend writes one file per segment. The Postgres backend of [0013](../decisions/0013-postgres-storage.md) §1–2 stores records as rows, and a segment is then the offset range `[n·2^k, (n+1)·2^k)`. Both implement `LogStore`, and export bundles use the file format whichever backend is live.

### 1.3 The `LogStore` trait

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

**`LogStore` is asynchronous and transaction-shaped.** Its methods (`create_partition`, `partitions`, `head`, `append`, `read`, `scan`, `erase_parts`, `compact`) return `Send` futures, because an interactive write in Postgres appends and does the rest of the appending transaction, the base check, ID allocation, the written graph's state and the activity row, in one transaction on the async driver ([0083](../decisions/0083-write-path-in-three-tiers.md) §1; [03](03-storage-caches-and-search.md) §6): the Postgres backend implements the trait on a handle that borrows the caller's transaction, so the append composes with everything else in it, and the conformance suite ([0005](../decisions/0005-crate-organization.md) rule 8) exercises the same append the write path uses. The trait needs no runtime (rule 2). The store fills in only the header fields it alone can know, the partition, the offset and the commitment; the caller's `Draft` carries the time, the payload type, the key and the global IDs, which the write path allocates from its own per-tenant sequences in the same transaction ([0013](../decisions/0013-postgres-storage.md) §6, [0015](../decisions/0015-record-format-and-partition-registry.md) §2).

### 1.4 Instance records live in instance partitions

*Sources: [0046](../decisions/0046-primary-tenant.md) §4.*

Every record in an instance partition is attested by one of:

- an upstream actor, for the records a sync job writes into `mirror/*`, `actors/{provider}`, `log/{provider}` and `files/{repo}` (the job's ID in the attestation names the primary-tenant subsidiary that ran it);
- a farm account, for its own records in `actors/{farm}`, `accounts/{farm}` and `log/{farm}`, and for what members of global groups write ([0028](../decisions/0028-tenancy-policy.md) §2–3, [0040](../decisions/0040-instance-prerogatives.md) §4);
- an actor of the primary tenant, for everything else.

It is the instance counterpart of [0040](../decisions/0040-instance-prerogatives.md) §1's rule for a tenant's partitions. Nothing the instance holds on its own behalf is written into the primary tenant's partitions, so a transfer of the primary tenant moves no records and splits no history.

Two kinds of record go to the instance `log` accordingly:

- **Mirror sync job records** ([0011](../decisions/0011-logs.md) §6.3) are appended to the **instance `log`**, keyed by job ID, as the records of jobs that write instance acts are ([0040](../decisions/0040-instance-prerogatives.md) §4). A mirror sync is an instance action. Tenant bulk jobs stay in the tenant's `log`.
- **OAuth consumer events** (`oauth/propose`, `oauth/update`, `oauth/approve`, `oauth/reject`, `oauth/disable`; [0025](../decisions/0025-oauth-server.md) §5) are appended to the **instance `log`**, not the primary tenant's `log` or `log/{farm}`.

The instance `log` stays `internal` ([0039](../decisions/0039-files-and-media.md) §10). Its public records (job records, consumer events, the public reasons of authority records) are served at the farm base by the same projections as a tenant's log: `Special:Log`, `list=logevents` and the job pages ([0010](../decisions/0010-site-ui.md) §9). The primary tenant itself, the farm slug and the transfer are in [08](08-tenants-and-instances.md).

## 2. Records

*Sources: [0005](../decisions/0005-crate-organization.md) §4.3; [0006](../decisions/0006-log-integrity-and-erasure.md) §3; [0012](../decisions/0012-api-requirements.md) §2.1; [0015](../decisions/0015-record-format-and-partition-registry.md) §1, §2, §4; [0058](../decisions/0058-packed-record-storage.md) §1.*

### 2.1 A record is a header and a body

*Sources: [0005](../decisions/0005-crate-organization.md) §4.3; [0006](../decisions/0006-log-integrity-and-erasure.md) §3.*

`scatter-log` stores records as a **header** and a **body**.

- **The header** is small, is never erased, and is what the Merkle tree commits to (§2.2). It must not contain personal data.
- **The body** holds everything that might have to be erased: a list of salted **parts**, each erasable on its own; the commitment is the root over their leaves (§2.3).

Who attests a change is kept apart from what the change is. Everything above `scatter-log` reads payloads and never depends on the attestation format.

### 2.2 The header

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §3; [0015](../decisions/0015-record-format-and-partition-registry.md) §2; [0097](../decisions/0097-versioning-and-skins.md) §4.*

The header is a CBOR array, in this order:

| # | Field | Type | Meaning |
|---|---|---|---|
| 0 | Format version | uint | Version of this layout |
| 1 | Partition | uint | 64 random bits, drawn when the partition is created and never changed ([0018](../decisions/0018-tenants.md) §2) |
| 2 | Offset | uint | Position in the partition, starting at 0, with no gaps |
| 3 | Appended at | uint | Microseconds since the Unix epoch, UTC |
| 4 | Payload type | text | For example, `scatter:v0/changeset` |
| 5 | Key | text or null | The entity or subject ID used for compaction and indexing. It must be an identifier, never content. It is an actor key only in the `actors` partitions, where the actor is the record's subject; the actor responsible for a record is in the attestation part (§2.3), never in the header |
| 6 | Body commitment | bstr (32) | `H(0x02 ‖ leaf_0 ‖ … ‖ leaf_{n−1})`, the root over the body's parts |
| 7 | Revision ID | uint or null | The global revision ID of [0012](../decisions/0012-api-requirements.md) §2.1 (§2.5) |
| 8 | Log ID | uint or null | The global log ID of [0012](../decisions/0012-api-requirements.md) §2.1 (§2.5) |
| 9 | Page ID | uint or null | The page ID of the record's key ([0008](../decisions/0008-namespaces-and-document-pages.md) §4, [0013](../decisions/0013-postgres-storage.md) §6) |

Fields 7–9 are assigned in the appending transaction, before the leaf hash is computed, so they are inside the Merkle tree (§2.5).

**Versions** ([0097](../decisions/0097-versioning-and-skins.md) §4). Every reader reads every record ever written: no record is rewritten, so reading what older writers wrote is a requirement on every release, not a property a change may give up. A record format's version describes forward compatibility, whether an older binary can read what a newer one writes. **The format version (field 0) is the header layout's major**: every field is positional and committed, so any change to the header is a new format version. **A payload type's tag carries its major**, `scatter:v0/changeset` being major 0: a change older readers can safely ignore keeps the tag, and one they cannot interpret takes a new one, `scatter:v1/changeset`, both read from then on. **A new major ships first behind a switch, off by default**, in a minor release that reads it and can write it, so that the minor can still be rolled back; the release that writes it by default is a product major ([22](22-crates-and-stack.md) §3.5). Every payload type is at major 0 until the product reaches 1.0. The stored form (§2.7) is never a version.

### 2.3 The body is a tree of erasable parts

*Sources: [0005](../decisions/0005-crate-organization.md) §4.3; [0006](../decisions/0006-log-integrity-and-erasure.md) §3; [0015](../decisions/0015-record-format-and-partition-registry.md) §1.*

**A body is a fixed list of parts.** Each payload type declares how many parts it has and what each holds. Every Triplespace payload type has at least these three, in this order, and a type may declare more after them:

| # | Part | Holds | MediaWiki's RevisionDelete bit |
|---|---|---|---|
| 0 | **Content** | The payload: a change set, a page operation and its text, an actor record, a log event, an upstream revision (§2.6), a configuration record ([0015](../decisions/0015-record-format-and-partition-registry.md) §3); a claim for Scatterbase | text |
| 1 | **Comment** | The edit summary or comment, as one string. Its parsed form ([0001](../decisions/0001-revision-metadata-rdf.md) §6) is a projection | comment |
| 2 | **Attestation** | Who is responsible, and how: the actor key, the job, the change tags ([0030](../decisions/0030-edit-filters.md) §5), an optional `evidence` field holding a signed remote activity ([0022](../decisions/0022-federation.md) §8), an optional client `signature` (§2.4), and the values the server fills in (§2.4) | user |

**The actor key lives here and nowhere else in the record.** The header never carries it: field 5 is an actor key only for records of the `actors` partitions, whose subject is the actor. `view.activity`'s issuer index therefore filters on the tenant's issuer code read from this part ([03](03-storage-caches-and-search.md) §4.5). The spelling of actor keys is [0079](../decisions/0079-derived-issuer-codes.md) ([07](07-actors-and-accounts.md) §1.1).

The content is also, by payload type, a membership, a block, an ACL ([0023](../decisions/0023-moderation.md) §3), a filter or a filter hit ([0030](../decisions/0030-edit-filters.md) §5). The comment is not in the change-set payload. A payload type with no comment, such as an actor record, carries a null in that part; the slot still exists, so every record has the same shape.

Payload types that declare more parts or fix their keys:

- the thread type adds a fourth part, the post text ([0019](../decisions/0019-discussions.md) §4);
- `scatter:v0/upload`, in `pages`, has the three standard parts; its content part holds the SHA-256 of bytes kept outside the log, so erasing that part is what allows the bytes to be destroyed ([0039](../decisions/0039-files-and-media.md) §2);
- `scatter:v0/task`, in `pages`, has the three standard parts and is keyed to a sprint's page ID; its content part names the rule and subject claimed ([0061](../decisions/0061-sprints-and-tasks.md) §5);
- `scatter:v0/derivation`, in `derived/{source}`, has four parts, the fourth being **evidence**, the page text the statements came from, so that it can be erased on its own; it is keyed by page ID and extractor ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §3);
- `scatter:v0/upstream-revision` has an optional fourth text part (§2.6).

**Each part is salted and hashed on its own.** Part *i* is stored as the CBOR array `[salt, bytes]`, where the salt is 16 random bytes. Its leaf is `H(0x04 ‖ i ‖ salt ‖ bytes)`, with *i* as one byte. The body commitment in the header is the root over the leaves in order:

```
commitment = H(0x02 ‖ leaf_0 ‖ leaf_1 ‖ … ‖ leaf_{n-1})
```

Tag `0x02` is the body commitment; `0x04` never appears in the header tree (§3.3).

**The salt keeps erased content from being confirmed.** Without it, anyone holding a header could test a guess at a short erased part, such as a username or a date of birth, by hashing the guess and comparing it with the commitment. Once a part's salt is erased with it, that test is no longer possible; 128 random bits are enough, and there are three salts per record.

**Deduplication uses the content hash** (`0x03`), which is computed over the content part's bytes alone, without its salt. It lives in indexes such as the version cursor, which are projections, never in the header. Erasing a record removes its content hash from those indexes too (§5).

**Scatterbase** declares its own part list for its claim payload type: the claim, the attribution, the client signature, the server signature and, later, prior-use acknowledgments. The root rule is the same, so the same `verify` checks both products.

### 2.4 The attestation

*Sources: [0005](../decisions/0005-crate-organization.md) §4.3; [0006](../decisions/0006-log-integrity-and-erasure.md) §3; [0015](../decisions/0015-record-format-and-partition-registry.md) §1; [0079](../decisions/0079-derived-issuer-codes.md) §4; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

**The attestation is in the body because it can identify people.** In Triplespace it is a CBOR map holding the actor and the job ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3), the change tags ([0030](../decisions/0030-edit-filters.md) §5), a client `signature`, and, for a post that arrived from the fediverse, the signed remote activity as `evidence` ([0022](../decisions/0022-federation.md) §8); the instance key is the only signer of checkpoints and manifests, and an actor may additionally sign its own record. In Scatterbase it holds the attribution and the client and server signatures, and it can later hold prior-use acknowledgments. Nothing below the claim level reads it.

**Server-filled values never enter a client-signed part.** A value the server mints into a record while appending it, such as a thread's talk-page ID and suffixed title ([0019](../decisions/0019-discussions.md) §2; [14](14-discussions.md)) or a sync job's `n` ([0053](../decisions/0053-mirrored-pages.md) §5; [13](13-mirrored-pages.md)), is carried in the attestation map, so that the content and comment parts stay the bytes the client submitted and a client signature over them verifies. The same holds for a decision the write path makes from the state of another partition at append time, autopatrol from the actor's memberships ([09](09-security-and-moderation.md) §6.4) or the operator of a transfer ([07](07-actors-and-accounts.md) §4.4): the decision is written into the attestation map, and the projection reads it there rather than reconstructing "as of" across partitions, which a vanish could make unreconstructible ([0083](../decisions/0083-write-path-in-three-tiers.md) §6).

**Client signatures.** The attestation map may carry a **`signature`** field: `{key: <key ID>, alg: "ed25519", sig: <64 bytes>}`, a signature by the *actor's own key* over `H(0x05 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment))`, where `0x03` is the content hash and `0x05` is a domain tag that never appears in the header tree (§3.3). The preimage is the canonical bytes of the two parts before the server salts them, so a client that produces canonical CBOR can sign what it submits and a verifier holding the record can check it; erasing either part makes the signature unverifiable. **Keys are actor records:** a `scatter:v0/key` record in the tenant `actors` partition, keyed by the actor, whose content holds the key ID (the Base32z hash of the public key), algorithm, public key and validity; a later record rotates or revokes it. The server verifies a submitted signature against the actor's current key before appending and refuses a mismatch with `ts-bad-signature`; `verify` (§8, level 2) checks every present signature against the key records in the bundle. Which actors may hold keys, and how they sign, is [0024](../decisions/0024-subsidiary-accounts.md) §4: subsidiaries. The instance key (§4.3) still signs every checkpoint; a client signature is in addition, never instead. Scatterbase's client and server signatures are the same field shape in its own part layout.

**The instance attestation.** The attestation part has a second form, the **instance attestation**, for records the instance writes into a tenant on its own authority: `actor` (`instance:{farm code}`), `authority` (an instance record's partition, offset and leaf hash), `job`, `binding`, and a `signature` by the instance key. Only the server writes it; a submitted record carrying one is refused with `ts-prerogative`. Its signature preimage is tag `0x06` (§3.3); the authority records are in [08](08-tenants-and-instances.md).

**The founding attestation.** The attestation part has a third form, carried by exactly one record per `config` partition: the record at offset 0, the `key:` record that opens the partition's key chain (§4.3). It names **no actor**; its `signature` is by the key the record itself registers, so the record is self-certifying; and for a tenant's `config` it carries the `authority` `(instance config, 0)`, the instance's own founding record. The tenant's issuer code, or the instance's farm code, is derived from the record's leaf afterwards ([08](08-tenants-and-instances.md) §1.2), so nothing that depends on the code can exist before it. Offsets 1 and 2 of the instance `config`, the primary tenant's `tenant:` record and the first `primary` record, are attested by `instance:{farm code}` with the founding record as authority ([08](08-tenants-and-instances.md) §7.2). A single-tenant instance therefore needs no authority record of its own and no `ts-prerogative` path to exist before its first entity is written.

**Right to vanish** ([0007](../decisions/0007-actor-identity.md) §4) and **unlinking** ([0007](../decisions/0007-actor-identity.md) §7) erase whole records. A demand to cut the tie between a record and an account erases the attestation part only.

### 2.5 Global IDs live in the header

*Sources: [0012](../decisions/0012-api-requirements.md) §2.1; [0015](../decisions/0015-record-format-and-partition-registry.md) §2; [0083](../decisions/0083-write-path-in-three-tiers.md) §4; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5.*

MediaWiki clients depend on numeric IDs from one sequence per wiki. These include `revid`, `parentid`, `oldid`, `baserevid`, `lastrevid`, `logid`, and `fromrev` and `torev` in `action=compare`. Diff URLs, `prop=revisions`, `list=recentchanges`, `list=logevents`, edit conflicts and the core REST history routes depend on them.

The IDs are Postgres sequences, one set per tenant, taken in the appending transaction and written into the record's hashed header, fields 7–9 ([0013](../decisions/0013-postgres-storage.md) §6, [0018](../decisions/0018-tenants.md) §2). They meet these requirements:

- **One sequence for revisions and one for log events, per tenant,** since MediaWiki clients expect one sequence per wiki. Each spans every partition that holds the tenant's revisions or events: `local`, `pages`, `log` and `actors`. A revision ID is assigned to every record in `local` and `pages`; a log ID to **every** record in `local`, `pages`, the tenant `log` and the tenant `actors` partitions at append, whether or not the log-event catalogue ([0011](../decisions/0011-logs.md) §6.1; [16](16-logs-feeds-and-notifications.md)) projects that record as an event, so that the rule is a function of the partition and never of what a later projection chooses to show.
- **Every ID a client sees is below 2^53,** so that it survives a JavaScript `Number` and every JSON parser; the provider ranges below are sized to keep it so.
- **IDs are assigned when a record is appended, and stored in its header.** An ID that came from replay order would change whenever projections were rebuilt in a different interleaving. In the header, an inclusion proof covers the ID, and an export bundle needs no sidecar.
- **The sequence increases with append order.** Gaps are acceptable. The one exception is an adoption's history: an adopted revision carries the source wiki's own revision ID in field 7, below the floor the job set, in place of a number from the sequence, and the records come in the source's order within each page ([0035](../decisions/0035-adopting-a-wikibase.md) §3, §4; [05](05-providers-and-ingest.md) §7.3, §7.4).
- **Mirror records get a provider-ranged revision ID,** computed by the writer with no allocation, so that `lastrevid`, `baserevid` and `schema:version` are defined for an entity with no local revision. They are still not revisions of this wiki: `prop=revisions` and `list=recentchanges` list local revisions only ([0012](../decisions/0012-api-requirements.md) §4).
- **Erasure does not free an ID.** An erased record keeps its ID, as its header survives (§5.3).

**Provider-ranged revision IDs.** `lastrevid`, `baserevid` and `schema:version` are integers in the contracts (`baserevid` in the Action API; `schema:version "26"^^xsd:integer` in [wikibase-compat.md §5.2](../api/wikibase-compat.md)), so the provider cannot be a letter prefix as it is in entity IDs. It is a number prefix instead. The revision-ID space is partitioned by **provider number**:

```
revid = provider_number << 40  |  n
```

Provider number 0 is the instance itself, so local revision IDs are the plain sequence. Each registry provider has a number **below 2^12** in the registry ([0015](../decisions/0015-record-format-and-partition-registry.md) §5). Numbers from **2^12 to 2^13 − 1** are never allocated in the registry: each tenant assigns them to its entity sources, and they are unique on their tenant only, which suffices because revision and page IDs are per tenant and no other tenant reads a source's records ([0078](../decisions/0078-entity-sources.md) §4). With the number below 2^13 and *n* below 2^40, every ranged ID is below 2^53. For a provider that publishes revision IDs, *n* is the upstream revision ID; for one that does not, such as OpenAlex, *n* is the record's offset in the mirror partition. Forty bits hold a thousand billion upstream revisions, and the split is the same on every instance, so the ID is computed by the writer, needs no allocation, and never changes on rebuild. It works across tenants without a new rule: Librarybase's revision 900 is `900` at home and `LB_number << 40 | 900` when another tenant reads it ([0018](../decisions/0018-tenants.md) §2, §5). A `put` therefore carries its revision ID in field 7 like any other record.

What this gives the compatibility surfaces:

- **`lastrevid`** of an entity is the revision ID of its newest **local** record when the key has one, and otherwise the ranged ID of its newest mirror record. It is the `local_revid` column of `view.entity` ([03](03-storage-caches-and-search.md) §4.2), monotonic per entity within the local sequence, and jumps to a ranged value only for an entity with no local history. `prop=revisions` and the history view still show mirror records in their place; `lastrevid` is what a client bases a write on, and a client bases a write on local state ([0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5).
- **`baserevid`** is decoded to a partition and offset, and the base check of §7 is per source graph: a base in the local partition is checked against the local partition's newest record for the key; a base in a mirror partition asserts only that the key still has no local record. A base that names a state compaction has since replaced is an `editconflict`.
- **`schema:version`** on the document node emits the same integer. **`oldid=N`** and `Special:Diff/N` with a mirror revision ID resolve to that observed state and its sync diff ([0012](../decisions/0012-api-requirements.md) §7); with a local revision ID they read that revision's state from `view.entity_revision`, one row per local revision, never a replay of the key's records ([0083](../decisions/0083-write-path-in-three-tiers.md) §4; [03](03-storage-caches-and-search.md)). `prop=revisions` and `list=recentchanges` still list local revisions only, as [0012](../decisions/0012-api-requirements.md) §4 requires.
- The UI and the REST `revid` field show the decoded form as well: "Wikidata revision 2148573921".

**Log IDs.** Upstream log events keep their upstream log ID in their content and get no local one, since `list=logevents` selects a provider explicitly ([0012](../decisions/0012-api-requirements.md) §4, `leprovider`).

**Page IDs are carried forward.** A local page ID is taken from the tenant's own sequence the first time a key is written in a tenant partition — or supplied by an adoption job, which carries the source wiki's page ID and has set the sequence past it ([0035](../decisions/0035-adopting-a-wikibase.md) §4) — and every later record for that key, in every partition, repeats it in field 9. It is never derived from replay order, and an entity's first record is not relied on to recover it, since compaction may have removed that record. Bootstrap writers ([0013](../decisions/0013-postgres-storage.md) §9) of a tenant partition take page IDs in blocks from the coordinator, as they take offsets.

**Mirrored entities and repository pages have provider-ranged page IDs.** A mirrored entity's page ID is `provider_number << 40 | upstream page ID` where the provider publishes one, and `provider_number << 40 | mirror offset` otherwise, computed by the writer with no allocation; `log."instance.page_id"` is not used for entities, and a local assertion about a mirrored entity repeats the ranged ID in its own field 9. A page a page repository serves has `provider_number << 40 | upstream page ID` the same way, derived and never minted, so that a foreign page has a stable `pageid` on every tenant without any write; in `mirror` mode the `pages/{repo}` records carry it in field 9 and the ranged upstream revision ID in field 7 ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6, [0053](../decisions/0053-mirrored-pages.md) §5). The range below 2^40 is the tenant's own sequence, so a ranged ID never collides with a local one, and `view.entity`'s unique key on the page ID is `(tenant, page_id)` ([03](03-storage-caches-and-search.md) §4.2).

**Why in the header.** An inclusion proof is about the leaf: with the ID inside it, a permalink or `Special:Diff/N` is something a third party can verify. In Postgres, `revid`, `logid` and `page_id` are denormalized from `header`, like the other header columns, and the segment file format has no sidecar; the tenant's page-ID sequence is `log.page_id` ([03](03-storage-caches-and-search.md) §2.3). An erased record keeps all three IDs, as its header survives.

### 2.6 Upstream revision records

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §4.*

An upstream revision that the instance learns about from a backfill ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5), from a history dump, or from the live stream is recorded as an **upstream revision record**.

| | |
|---|---|
| Partition | The provider log, `log/{provider}` ([0011](../decisions/0011-logs.md) §2). It is `full`, `hashed` and internal, keyed by target with upstream IDs, and is where upstream redaction runs (§5.4) |
| Payload type | `scatter:v0/upstream-revision`, one of the types [0011](../decisions/0011-logs.md) §3 admits in a log graph |
| Header key | The entity's prefixed ID, or its surrogate for a keyed type, as [0011](../decisions/0011-logs.md) §3 |
| Content part | Upstream revision ID and parent ID; upstream timestamp; size and SHA-1; content model; change tags; minor and bot flags; and, where the revision was also observed as a state, the `(partition, offset)` of that `put` |
| Comment part | The upstream edit summary |
| Attestation part | The upstream actor key ([0007](../decisions/0007-actor-identity.md) §1), or a hidden marker ([0007](../decisions/0007-actor-identity.md) §5), and the job that brought the record in |
| Text part (optional, fourth) | For a **page** revision seeded by a fork, the revision's wikitext, kept apart so that hiding or erasing the content touches nothing else; such records live in the tenant's `log` partition keyed by the fork's page ID ([0054](../decisions/0054-forking-a-mirrored-page.md) §3) |

A revision is identified by its upstream revision ID. Re-reading one the instance already holds changes nothing, as for log events ([0011](../decisions/0011-logs.md) §4). Where the same revision was already observed as a state, enriching that revision's existing node ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3) is a join on (provider, upstream revision ID).

**Content is a `put`.** When a backfill brings the content of an upstream revision, not only its metadata, the content is written as a `put` in the mirror partition carrying the upstream revision ID ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2), the same shape a sync writes. No new content record exists. Such `put`s are written only for entities exempt from compaction: retained entities and `full` mirrors.

**Content by default, from the API or a dump.** A `retain` backfill fetches **every upstream revision's content** as well as its metadata, so a rescued entity's whole history is diffable and exportable here and survives upstream deletion whole; `retain --history metadata` narrows it to `upstream-revision` records alone. The backfill job takes a **source**: `api`, the provider's revision API ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8, under the `upstream` rate class), which is the default and suits a modest set; or `dump:{path}`, a provider history dump the operator has on hand, which is what a mass rescue of Wikidata or another very large dataset uses. A bulk `retain` whose set exceeds `retention.api_max_entities` (`site`, default 10,000) is refused for `api` unless forced, so a million-entity rescue is not attempted one request at a time. Either source writes the same records; a `dump` job records the dump's identity as its source version ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3), and revisions newer than the dump are fetched from the API to close the gap. The automatic retention of properties ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6) uses the API, since a property's history is small.

**Serving.** One table, kept apart from `view.activity` because it grows with backfilled history rather than local activity:

```sql
CREATE TABLE view.upstream_revision (
  provider text NOT NULL, upstream_revid bigint NOT NULL,
  entity_id text NOT NULL, parent_revid bigint, time timestamptz NOT NULL,
  actor_key text, comment text, tags text[] NOT NULL DEFAULT '{}',
  minor boolean, bot boolean, size integer, sha1 text, visibility smallint NOT NULL DEFAULT 0,
  observed_partition bigint, observed_offset bigint,
  partition bigint NOT NULL, "offset" bigint NOT NULL,          -- bigint since 0018 §2
  PRIMARY KEY (provider, upstream_revid)
);
CREATE INDEX upstream_revision_entity ON view.upstream_revision (entity_id, time DESC);
```

[0012](../decisions/0012-api-requirements.md) §6's "served from the log instead" reads from it; those rows are `upstream-edit` activity rows with `record` present. The live fetch runs only for the interval the table does not cover.

**IRIs.** The revision node of any state that carries an upstream revision ID, observed or backfilled, is the upstream revision:

| Revision | IRI |
|---|---|
| Upstream revision, observed or backfilled | `{article path}Special:Redirect/revision/{revid}` on the provider's wiki, for example `https://www.wikidata.org/wiki/Special:Redirect/revision/123`. This parallels `Special:Redirect/user/{id}` ([0007](../decisions/0007-actor-identity.md) §2) and `Special:Redirect/logid/{id}` ([0011](../decisions/0011-logs.md) §8) |
| Local revision | `{base}/revision/{revid}` ([0001](../decisions/0001-revision-metadata-rdf.md) §1), which §2.5 makes well-defined |
| Observed state from a provider with no revision IDs, such as OpenAlex | `{base}/record/{partition}/{offset}`, the record IRI of [0011](../decisions/0011-logs.md) §8 |

An upstream revision node is `prov:specializationOf` the **upstream** document node, `https://www.wikidata.org/wiki/Special:EntityData/Q123`.

### 2.7 Logical form and stored form

*Sources: [0058](../decisions/0058-packed-record-storage.md) §1.*

A record's **logical form** is what [0015](../decisions/0015-record-format-and-partition-registry.md) §1 and [payloads.md](../api/payloads.md) define: the header and the body of salted parts, in canonical CBOR. Everything that hashes, signs, proves, exports or crosses a wire uses the logical form: leaves, commitments, content hashes, checkpoints, inclusion proofs, bundles, the `segments` backend, `LogStore::read` and `scan`, and every API.

A record's **stored form** is how `scatter-log-postgres` keeps the body in `log.record`. It is physical: the body is rebuilt from it byte for byte, and nothing outside the store can tell which form a row uses. Changing the stored form is never a format version. A partition may hold rows in several stored forms at once, and `repack` converts rows between them. The stored forms themselves, fragments and dictionaries, are in [03](03-storage-caches-and-search.md).

**Every read checks the commitment.** When a stored body is rebuilt, the store recomputes each present part's leaf and the commitment, and compares the result with the row's `commitment` column (header field 6). This costs one SHA-256 pass over a few kilobytes, a few microseconds. A mismatch is a `Corrupt` error, which is logged and never served. A packing bug, a damaged dictionary or a wrongly deleted fragment therefore fails loudly on the first read, never silently. A `plain` row is checked the same way.

## 3. Encoding and hashing

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §2.*

### 3.1 Canonical CBOR

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §2.*

Record headers, record bodies and payloads are stored in the **core deterministic encoding of CBOR** ([RFC 8949](https://www.rfc-editor.org/rfc/rfc8949) §4.2.1). The encoding rules are:

- the shortest form for every integer and length;
- definite lengths only;
- map keys sorted by their encoded bytes, with no duplicates;
- only the CBOR tags listed in the payload type's schema.

The stored bytes are the preimage, so no separate canonicalization step exists. The decoder is strict: it re-encodes every item it reads and rejects the item if the bytes differ. A payload that decodes but is not canonical is therefore a verification failure, not a variant.

The NDJSON wire format of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.7 is unchanged. Ingest converts it to CBOR before appending. Values that JSON carries as strings, such as Wikibase quantity amounts, stay strings.

### 3.2 The JSON → CBOR mapping and the hash guard

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §2.*

**The JSON → CBOR mapping is structural.** The CBOR of a change set, entity state or page operation is the JSON's structure and nothing else: every JSON string is a CBOR text string (quantity amounts and bounds, time values, coordinate decimals all stay the strings Wikibase gives them), JSON numbers are CBOR integers or floats exactly as JSON typed them, objects are maps with text keys, arrays are arrays, `true`/`false`/`null` are the CBOR simple values. No CBOR tags are used for Wikibase data. Two derivable fields are **dropped** at ingest and recomputed on output: the `hash` on snaks and references, and `numeric-id` beside `id` on entity values. `scatter-wikibase-model` implements Wikibase's own hash computation so that a recomputed reference hash equals Wikidata's for mirrored data, which is what [0003](../decisions/0003-statement-ui.md) §5 and [0004](../decisions/0004-identity-clusters-and-equivalence.md) §8 compare by. The test is a round trip: canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §3) → CBOR → canonical JSON reproduces the bytes for every entity file in `docs/samples/wikibase-1.43` and `docs/samples/wikidata`. Because the content hash `0x03` and a client signature (§2.4) are computed over these bytes, this mapping is part of the record format.

**The hash guard.** Dropping `hash` at ingest is safe only while the recomputation matches Wikibase's own, and nothing in a dump says whether it does. The hashes are not decoration: the RDF names reference nodes by the reference hash (`ref:{hash}`, [wikibase-compat.md](../api/wikibase-compat.md) §5.1) and value nodes by an `md5` of the serialized value (`v:{hash}`) that the JSON never carries, and the Action API takes snak and reference hashes as handles (`wbsetreference`, `wbremovereferences`, `wbremovequalifiers`). A faithful implementation is therefore required for local data and for RDF whatever happens to the JSON field; the guard is a check that it *is* faithful, made on every mirrored entity rather than only on the snapshots. **At ingest, every snak and reference hash is recomputed and compared with the one upstream sent.** When they are equal, the field is dropped as above. When they differ, **upstream's hash is kept in place**: the `hash` key stays in the CBOR map of that snak or reference, so the mapping is still structural, the content hash `0x03` covers it, and no schema changes. On output a present `hash` is emitted as stored and an absent one is recomputed, so what the instance serves equals what upstream served whether or not the implementation is right. Every mismatch is counted by value type in the job's finish record ([0011](../decisions/0011-logs.md) §6.3), shown on the job page and in `GET /jobs/{id}` ([0012](../decisions/0012-api-requirements.md) §5), and summed instance-wide as `hash_mismatches` in `siprop=triplespace` ([0012](../decisions/0012-api-requirements.md) §4); a nonzero count means the emulation has drifted from Wikibase's `serialize()` or upstream has changed its algorithm, and the breakdown says which value types. The `site` setting `ingest.hash_mismatch` is `keep`, the default, or `fail`, which aborts the job at the first mismatch for an instance that would rather stop than store. A kept hash is never used to decide equality: statement fusion ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §8) and "identical references share a node" compare recomputed hashes, so a drift shows up as a count, never as a silently split node. Adopted entities ([0035](../decisions/0035-adopting-a-wikibase.md) §3) pass through the same guard, since the source wiki's hashes come from the same code.

### 3.3 Hashing and domain tags

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §2; [0081](../decisions/0081-recovery-keys-and-continuations.md) §2.*

Every hash is **SHA-256**. This matches Scatterbase's claim IDs and content-addressed blobs, and it makes the Merkle tree exactly the RFC 6962 tree, so existing transparency-log tooling can check it. Each use prefixes its input with a one-byte domain tag, so no hash can be mistaken for another kind:

| Tag | Use |
|---|---|
| `0x00` | Merkle leaf: the hash of a record header (§2.2) |
| `0x01` | Merkle interior node: the hash of two child hashes |
| `0x02` | Body commitment (§2.3) |
| `0x03` | Content hash of a part alone, for deduplication and version cursors ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4) |
| `0x04` | Leaf of one body part (§2.3) |
| `0x05` | Preimage of a client signature over the content and comment parts (§2.4) |
| `0x06` | Preimage of an instance attestation's signature: `H(0x06 ‖ H(0x03 ‖ content) ‖ H(0x03 ‖ comment) ‖ authority)`, signed by the instance key ([0040](../decisions/0040-instance-prerogatives.md) §3) |
| `0x07` | Preimage of a recovery signature: `H(0x07 ‖ code ‖ entry)`, the tenant's issuer code and the canonical CBOR of a `recovery-key`, `continuation` or `continuation-cancel` entry without its `signatures` field, signed by a recovery key ([0081](../decisions/0081-recovery-keys-and-continuations.md) §2; [08](08-tenants-and-instances.md) §6.8) |

Tags `0x00` and `0x01` are the leaf and node prefixes of RFC 6962, so the Merkle tree (§4.1) is the RFC 6962 tree unchanged. Tags `0x02` to `0x07` are Triplespace's own and never appear inside the tree.

The hash function is fixed for a partition when the partition is created, and is named in its `graph:` record in `config` (§1.2). Changing it means starting a new partition.

**Text forms.** Checkpoints carry hashes in base64, as C2SP requires (§4.2). IRIs and identifiers carry them in Base32z, matching Scatterbase.

## 4. The Merkle tree, segments, checkpoints and keys

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0006](../decisions/0006-log-integrity-and-erasure.md) §5, §6.*

### 4.1 The Merkle tree and segments

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0006](../decisions/0006-log-integrity-and-erasure.md) §5.*

- **The tree is the RFC 6962 Merkle tree** (as updated by [RFC 9162](https://www.rfc-editor.org/rfc/rfc9162) §2.1), with SHA-256 as the hash. Leaves are `H(0x00 ‖ header)`. Inclusion proofs and consistency proofs follow the RFC.
- **Segments hold 2^k records,** except the last one. The exponent k is fixed for each partition when it is created, in its `graph:` record (§1.2). A full segment is then a complete subtree of the partition's tree. In Postgres a segment is the offset range `[n·2^k, (n+1)·2^k)`, sealed when every offset in it has been appended and its manifest written; the file backend writes one file per segment ([0013](../decisions/0013-postgres-storage.md) §1).
- **Bulk ingest hashes in parallel.** In bootstrap mode ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.6), each segment's leaves and subtree root are computed independently. The partition root is then folded from the segment roots. The only sequential step runs once per segment.
- **Several processes append.** An appending transaction reads the tree's right edge under the partition's append lock ([03](03-storage-caches-and-search.md) §2.1) and persists only the complete subtrees its append closes, so two API replicas and a bulk job writing one partition never disagree about a node; the right edge itself is recomputed from the persisted nodes by whoever appends next. Each append costs one leaf hash plus O(log n) node hashes in the worst case.
- **In a `hashed` partition,** each segment's tree stands alone. Its root is what the segment manifest signs; the partition has no checkpoint (§1.2).
- **A compacted offset keeps its leaf.** Compaction (§6) removes the record but leaves its 32-byte leaf hash in its place, so the tree over every offset still folds, offsets are never reused, and a reader can tell a hole from a gap. In Postgres the leaf is already in `log.merkle_node`; the file backend stores it as the slot ([payloads.md](../api/payloads.md) §10).

### 4.2 Checkpoints

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §6; [0081](../decisions/0081-recovery-keys-and-continuations.md) §5, §6.*

**Format.** Checkpoints use the [C2SP tlog-checkpoint](https://c2sp.org/tlog-checkpoint) format, signed as a [C2SP signed note](https://c2sp.org/signed-note):

- **Origin line,** without a scheme: `{tenant host}/log/{partition name}` for a tenant's partition ([0018](../decisions/0018-tenants.md) §2), where the host is the tenant's host when the partition was created or when the tenant arrived on this instance by a move or a continuation, and an `alias` does not change it, so that a witness sees one log per origin ([0081](../decisions/0081-recovery-keys-and-continuations.md) §5), and `{farm host}/instance/log/{partition name}` for an instance partition, so that the two cannot collide when the farm base is a tenant's base ([0046](../decisions/0046-primary-tenant.md) §7). For segment manifests in a `hashed` partition, the origin followed by `/segment/{n}`.
- **Tree size**, in decimal.
- **Root hash**, in base64.
- **No extension lines.** C2SP recommends against them because monitors cannot audit them.
- **Signature:** the instance's Ed25519 key.

**When checkpoints are written**, for a `logged` partition:

- at the end of every job ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3);
- after every `atomic` batch;
- in steady state, at least every N records or T seconds, whichever comes first. N and T are instance configuration.

Records appended after the latest checkpoint are durable but not yet signed. A `hashed` partition has no checkpoints: a job that ends there records the manifest of the last sealed segment and an unsigned head offset, and the records of the open segment are uncommitted until the segment is sealed and its manifest signed (§1.2).

**Who signs.** Checkpoints and segment manifests are signed by the projection worker, under an advisory lock per partition, and it is the only process that holds the instance key; an API replica or a bulk job appends and never signs. The signing worker reads the persisted tree (§4.1), so a checkpoint covers only complete subtrees that every appender has closed.

Checkpoints are stored beside their partition. They are not log records, since a checkpoint cannot be a leaf of the tree it signs. Every checkpoint is kept, so consistency between any two of them can be proved. A segment manifest is keyed `(partition, segment)`, one per sealed segment and never re-signed: it carries no list of manifests it replaces and no sealing time in its key, since compaction leaves a sealed segment's tree unchanged (§6).

**Checkpoints are served** at `{base}/.well-known/tlog/{partition}/checkpoint`, with any witness cosignatures as additional signature lines (§4.3), with historical checkpoints and manifests beside it and the key chain at `/.well-known/tlog/keys` ([0022](../decisions/0022-federation.md) §1).

### 4.3 Keys and witnesses

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §6; [0081](../decisions/0081-recovery-keys-and-continuations.md) §1, §3, §6.*

- The instance's first public key is registered in the first record of the configuration partition, the `config` partition of [0015](../decisions/0015-record-format-and-partition-registry.md) §3, under the founding attestation (§2.4): no actor, signed by the key it registers. This is the same shape as Scatterbase's server-key registration claim. Each tenant's `config` partition carries its own copy of the key chain ([0018](../decisions/0018-tenants.md) §2), opened by a founding record of the same form whose authority is the instance's.
- **Rotation** is a configuration record naming the new key, signed by the old key inside its attestation. Checkpoints identify their key by the signed-note key ID. Moving a tenant to another instance is one such rotation: the record names the new instance's key and the tenant's final checkpoint ([0018](../decisions/0018-tenants.md) §10). A move the old instance does not sign is a **continuation** instead, authorized by the tenant's **recovery keys**, keys its community holds and no host sees ([08](08-tenants-and-instances.md) §6.8; [0081](../decisions/0081-recovery-keys-and-continuations.md) §1, §3).
- Recovering from a compromised instance key is out of scope. It belongs to the identity work that Scatterbase and Triplespace share. A tenant with recovery keys can, however, continue to a new key on the same instance ([0081](../decisions/0081-recovery-keys-and-continuations.md) §3).

**Witnesses are optional, and the key chain is what they are for** ([0081](../decisions/0081-recovery-keys-and-continuations.md) §6). The tenant `site` setting `integrity.witnesses` lists witnesses that implement [C2SP tlog-witness](https://c2sp.org/tlog-witness), each with its endpoint and verifier key. The instance submits every checkpoint of the tenant's `config` partition to each, with the consistency proof from the witness's last-seen size, stores each cosignature beside the checkpoint and serves it as an additional signature line. Other partitions are submitted only if `integrity.witness_partitions` lists them; the default is `config` alone, which holds the founding record, the key chain, the recovery-key records and every continuation. Because the tree is RFC 6962 with SHA-256, any such witness works unchanged. A witness refuses a checkpoint inconsistent with one it has cosigned, so a host cannot show two key chains for one origin, and a continuation's delay runs on witnessed time.

**An instance can be a witness.** With the instance setting `witness.enabled`, `triplespace-server` serves the tlog-witness API at the farm base for the origins and keys its operator lists, keeping its last cosigned size and root per origin in durable state that is never rolled back. Federation partners may witness each other's key chains ([0022](../decisions/0022-federation.md) §1).

## 5. Erasure

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7; [0012](../decisions/0012-api-requirements.md) §2.1; [0013](../decisions/0013-postgres-storage.md) §3; [0015](../decisions/0015-record-format-and-partition-registry.md) §1, §2, §4; [0058](../decisions/0058-packed-record-storage.md) §6.*

### 5.1 The `erase` record

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7; [0015](../decisions/0015-record-format-and-partition-registry.md) §1.*

**Erasure is a record.** An `erase` record is appended to the same partition as its targets. It names its targets in one of two ways:

- a list of offsets;
- a key, meaning every record in the partition with that key, for erasing a whole entity.

It also names the **parts** it erases in a `parts` field, any subset of the payload type's parts (`content`, `comment` and `attestation`, plus any the type adds) and all of them by default, a reason class and a reference to the authority for the erasure. The reason classes are `legal`, `privacy`, `upstream` (§5.4) and `operational`, extensible in `site` configuration ([0016](../decisions/0016-permissions-and-access-control.md) §6). Its own attestation records who erased. **Who** may erase is the `ts-erase` permission ([0016](../decisions/0016-permissions-and-access-control.md) §2; [09](09-security-and-moderation.md)).

### 5.2 Effect on storage

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7; [0013](../decisions/0013-postgres-storage.md) §3; [0058](../decisions/0058-packed-record-storage.md) §6.*

- Each erased part's `[salt, bytes]` is replaced by `[null, leaf]`, so the root, the header and every checkpoint still verify. In Postgres the target's row is rewritten in the same transaction that appends the `erase` record, and the row is marked:

```sql
UPDATE log.record SET body = $rewritten_body, erased = erased | $parts_mask, erased_by = $erase_offset
 WHERE partition = $p AND "offset" = $target;
```

  one row at a time, since each body is rewritten from its own bytes; erasing by key selects the targets with the `(key, "offset")` index first. The header, the commitment and the leaf survive.

- In a `packed` partition, where repeated subtrees are stored once per dedup domain, the remaining parts stay packed and the erased parts' fragment references become candidates in `ops.fragment_candidate`. There are no reference counts: a fragment becomes garbage only when the rows that used it are compacted away or erased. The erasure worker runs a candidate sweep before the `VACUUM` it already runs, deleting the fragments only the erased parts used, so bytes used only by the erased record are gone within the same job. A fragment the erased record **shared** with other rows is still held by them, legitimately, unless they too are gone; for reason class `legal` the worker therefore runs a full sweep of the dedup domain as well, and vacuums the domain's fragment table afterwards, so no fragment survives that only erased parts used. How candidate and full sweeps run is in [03](03-storage-caches-and-search.md).
- Headers are kept, so the Merkle tree and every existing checkpoint still verify.

**Erased bytes linger until vacuumed.** MVCC keeps the old tuple until `VACUUM` reclaims it, and the WAL archive keeps it for the archive's retention. The erasure worker runs `VACUUM` on the affected child table after an `erase`, and the instance's runbook sets a WAL retention window that bounds how long erased bytes can survive in archives; an instance under a legal deadline runs `VACUUM FULL` or `pg_repack` on that child table, which rewrites it.

### 5.3 Effect on projections, and what remains

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7; [0012](../decisions/0012-api-requirements.md) §2.1; [0015](../decisions/0015-record-format-and-partition-registry.md) §1, §2.*

Projections replay the `erase` record and remove everything derived from the erased parts: resolved and source-graph triples, metadata-graph revision nodes, content hashes in the version cursor, and search entries. Rebuilding from the log excludes erased bodies automatically. What each erasure removes:

| Part erased | Effect | Action API flags ([0012](../decisions/0012-api-requirements.md) §4) |
|---|---|---|
| Content | The change, page text, event parameters or record body are gone | `texthidden`, `erased` |
| Comment | The summary disappears from history, recent changes, diffs and the metadata graph | `commenthidden`, `erased` |
| Attestation | The actor and job are removed. The record stays in the entity's history and keeps its IDs (§2.5), attributed to no one | `userhidden`, `erased` |

Hiding ([0001](../decisions/0001-revision-metadata-rdf.md) §4) is a projection-level flag with the same three bits, and the content stays in the log. Erasure of a part is the irreversible form of hiding that part.

**What remains after erasure:** each target's header. That is its partition, offset, time appended, payload type, key, body commitment and its revision, log and page IDs; erasure does not free an ID. The commitment is salted (§2.3), so the erased content cannot be confirmed from it. That something was recorded about key K at time t remains visible.

**This follows MediaWiki's precedent.** Revision deletion and suppression keep the revision's row (its ID, page and timestamp) and hide its content, summary or username. Erasure keeps the same kind of trace, and goes further in one respect. MediaWiki keeps an unsalted SHA-1 of deleted text in its database, which could confirm a guess at that text. An erased record's commitment is salted, so it cannot.

**Later records.** Change sets are deltas, so erasing a record can leave later records that refer to what it created. Replay skips erased records, and projections apply the remaining ones as best they can. To remove an entity entirely, erase by key.

### 5.4 Mirror partitions and upstream redaction

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7; [0015](../decisions/0015-record-format-and-partition-registry.md) §4.*

Upstream tombstones followed by compaction still erase mirrored data ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5). `erase` is also available in a mirror partition, for a takedown that cannot wait for the next compaction. Compaction later drops the header too.

**Redaction is partial erasure.** When upstream hides part of a revision or a log event, the sync job erases that part with reason class `upstream`: a hidden comment erases the comment part, a hidden user erases the attestation part, and hidden content erases the content part of the observed `put`. No redacted copy is written before erasing. The visibility bits a projection reports are derived from which parts are erased with reason `upstream`. Outright suppression, where an event or revision vanishes from upstream's public record, still erases all three parts.

### 5.5 Copies outside the instance, files and Scatterbase

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §7.*

**Copies outside the instance.** The `erase` record travels on the feed like any other record. Mirrors and exports taken before the erasure can comply only if they process it. Backups must be handled operationally in the same way. Neither can be enforced from the log. Inside the instance, erased bytes survive until vacuumed, and in WAL archives for their retention window (§5.2).

**Files.** An upload record holds the SHA-256 of its file's bytes, which live in a blob store outside the log. Erasing its content part removes that reference, and the bytes are destroyed when no unerased record in their storage scope references them. An operator's **expunge** erases every reference to a hash in every tenant and destroys the bytes at once ([0039](../decisions/0039-files-and-media.md) §8–10; [12](12-files-and-media.md)).

**Scatterbase.** Erasing a signed claim's body makes its signatures unverifiable, while its leaf remains. Whether a claim may be erased is Scatterbase policy. Its decision record routes destructible data to the `mutable` graph.

## 6. Compaction

*Sources: [0005](../decisions/0005-crate-organization.md) §4.2; [0006](../decisions/0006-log-integrity-and-erasure.md) §4, §5, §7; [0013](../decisions/0013-postgres-storage.md) §3; [0058](../decisions/0058-packed-record-storage.md) §6.*

Compaction applies to a partition with the `latest` history policy (§1.2) and removes every record for a key except the newest. In Postgres it is a batched delete:

```sql
DELETE FROM log.record_mirror_wikidata r
 USING (SELECT key, max("offset") AS keep FROM log.record_mirror_wikidata
        WHERE key = ANY($keys) GROUP BY key) k
 WHERE r.key = k.key AND r."offset" < k.keep;
```

Offsets are never reused, so a compacted partition has holes. That is the model [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2 borrowed from Kafka. A compacted offset keeps its leaf (§4.1); the header is dropped with the record. A tombstoned entity that is not retained loses every record for its key, including the tombstone once its retention policy has been applied. In a `packed` partition the deleted rows' fragment references become candidates for the next candidate sweep, and fragments they shared with other rows wait for a full sweep ([03](03-storage-caches-and-search.md)).

Because a compacted offset keeps its leaf, a sealed segment's tree and its manifest are unchanged by compaction; no manifest is re-signed, and a manifest is one row per `(partition, segment)` with nothing to supersede (§4.2).

Retained mirror entities are exempt from compaction (§1.2).

## 7. Edit conflicts per entity

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §8; [0015](../decisions/0015-record-format-and-partition-registry.md) §2; [0083](../decisions/0083-write-path-in-three-tiers.md) §1; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5.*

A change set in the local graph may carry a **base**: the offset of the latest record for that key that the client saw, which `baserevid` decodes to (§2.5). The check is a step of the appending transaction, made against the local graph alone ([0083](../decisions/0083-write-path-in-three-tiers.md) §1).

- **The check is per source graph.** A base in the local partition is checked against the local partition's newest record for the key. A base in a mirror partition asserts only that the key still has **no local record**; a mirror advancing is never a conflict, because nothing a local assertion depends on changed. The one operation whose meaning depends on mirrored state, an `override` naming a mirrored statement, is checked by whether that statement still exists in the graph's current state, not by a revision number.
- **A stale local base is patched where it can be.** When a newer local record exists but the change set touches no statement UUID, term, alias or sitelink that a later local record touched, the write is accepted and the response carries the warning `wikibase-conflict-patched`, as Wikibase does; otherwise the Action API reports `editconflict`, naming the current `lastrevid`, and the REST API HTTP 409. Statement modules on disjoint GUIDs therefore never conflict. A base that names a state compaction has since replaced is an `editconflict`.
- A base is **required** for replacing a local entity wholesale ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2).
- A base is **optional** for everything else, as `baserevid` is in Wikibase. Bulk jobs that merge by default do not send one.

The check needs an index from each key to its latest local offset, which is the version cursor, and the local graph's current state, which is `view.graph_state` ([0083](../decisions/0083-write-path-in-three-tiers.md) §4; [03](03-storage-caches-and-search.md)). The diff of a `wb*` write against the resolved view, and what each module becomes, are in [18](18-api.md).

Scatterbase's prior-use acknowledgments are the same check applied to each term instead of each entity. They are not implemented here. They can be carried in the attestation and checked by Scatterbase's own acceptance code.

## 8. Verification and export

*Sources: [0006](../decisions/0006-log-integrity-and-erasure.md) §9; [0015](../decisions/0015-record-format-and-partition-registry.md) §1; [0058](../decisions/0058-packed-record-storage.md) §1; [0081](../decisions/0081-recovery-keys-and-continuations.md) §8.*

**An export bundle** holds, for each exported partition:

- its segments, with headers and bodies;
- every checkpoint and segment manifest;
- the configuration records that register and rotate keys, and the `graph:` record that names the partition, its hash function, `k` and its policies (§1.2);
- for each exported partition that holds client-signed records, the `scatter:v0/key` records of the `actors` partition for every actor whose signatures appear (§2.4).

It needs nothing else to verify. Bundles are written in the logical form (§2.7), whichever backend is live. Default exports follow each graph's export policy ([0005](../decisions/0005-crate-organization.md) §4.1). Bundles also take `--blobs include|list|omit` ([0039](../decisions/0039-files-and-media.md) §14). A bundle that moves a tenant carries its extracts as well ([0018](../decisions/0018-tenants.md) §10).

**`verify` checks three levels:**

1. **Structure.** Strict CBOR decoding, leaf hashes, the root at every checkpoint, checkpoint signatures, consistency proofs between successive checkpoints, and gapless offsets. For a tenant, level 1 also reports its **chain of custody** ([08](08-tenants-and-instances.md) §6.8): the tenant's origins in order, each with its key and the record linking it to the next (a signed rotation, a `recovered` continuation with whether its delay has passed and whether it was cancelled, or an `unauthorized` one); each partition's tail ranges attested only by the host that appended them; the witness cosignatures on each `config` checkpoint; and any fork. A `recovery-key` record without valid recovery signatures is reported and ignored.
2. **Bodies.** Every present part against its leaf and the commitment: a present part hashes to a leaf that reproduces the commitment; a missing part carries its leaf and is named by an `erase` record, and the verifier reports it as erased, naming the erasing offset; a missing part with no `erase` record, or anything else, is a failure. Every blob a present upload record names, at depth `presence` or `full`; a missing object with no `erase` accounting for it is a failure ([0039](../decisions/0039-files-and-media.md) §14). Every present client signature against the key records in the bundle (§2.4). Every instance attestation's signature against the key chain, with the key current when the record was appended ([0040](../decisions/0040-instance-prerogatives.md) §8).
3. **Projections** (optional and expensive). Rebuild projections from the log and compare them with the stored ones.

**Inclusion proofs** are available for any record against any later checkpoint. With one, a third party can check that a particular revision is in the log without holding the whole log.

**Bundles carry an authority extract:** the authority records the tenant's instance attestations cite, with inclusion proofs and with the operator's attestation part withheld ([0040](../decisions/0040-instance-prerogatives.md) §8).
