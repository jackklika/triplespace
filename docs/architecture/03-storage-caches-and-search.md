# 03. Storage, caches and search

This chapter describes where Triplespace's bytes live and how they are served: Postgres as the store of the log and of every projection the API and UI read, the four schemas and the roles that may read them, the `log` schema and its packed stored form, the `view` schema with one catalogue of every table later ADRs added to it, projections and their synchrony with the write path, the shared views and overlays that tenancy adds, the blob store, the cache layers and their keys, HTTP and response caching, and search on OpenSearch with its Postgres fallback. It assumes the log, records, partitions, erasure and integrity of [01](01-log-and-records.md), the graphs and RDF output of [02](02-graphs-rdf-and-query.md), the tenants of [08](08-tenants-and-instances.md) and the security model of [09](09-security-and-moderation.md); the crate map and the deployment profiles are in [22](22-crates-and-stack.md), bootstrap and bulk loading in [05](05-providers-and-ingest.md), and the web tier that sits in front of the API in [20](20-web-tier.md).

## 1. Postgres as the store

*Sources: [0013](../decisions/0013-postgres-storage.md) §1, §4; [0005](../decisions/0005-crate-organization.md) §4.4; [0033](../decisions/0033-backend-stack.md) §4.*

### 1.1 Postgres holds the log

*Sources: [0013](../decisions/0013-postgres-storage.md) §1.*

The log records of [0006](../decisions/0006-log-integrity-and-erasure.md) are rows in Postgres. The alternative, segment files on disk with Postgres holding only `(partition, offset)` pointers, is MediaWiki's external-storage split; it is not used, because every interactive write would then span two systems:

- appending the record;
- allocating its global revision ID ([0012](../decisions/0012-api-requirements.md) §2.1);
- checking the base offset ([0006](../decisions/0006-log-integrity-and-erasure.md) §8);
- writing the graph state the next composition reads, the activity row and the projections the write path itself reads (§6).

In one database, these are one transaction. Erasure becomes an `UPDATE`, compaction a `DELETE`, and backup, replication and point-in-time recovery are the database's.

**The segment file format stays.** `scatter-log` keeps a file format for two purposes:

- **export bundles** ([0006](../decisions/0006-log-integrity-and-erasure.md) §9), which `verify` reads without a database;
- **Scatterbase**, which may store its claim log in files.

`scatter-log` therefore defines a `LogStore` trait with two backends ([0005](../decisions/0005-crate-organization.md) §2). The record and header formats, the hash tags, the Merkle tree and the checkpoint format of 0006 are unchanged. What changes is where the bytes live.

**Segments are offset ranges.** A segment is the range `[n·2^k, (n+1)·2^k)` of a partition. It is sealed when every offset in the range has been appended and its manifest written. Nothing physical corresponds to it in Postgres. The file backend writes one file per segment, as before.

### 1.2 Four schemas and the roles that read them

*Sources: [0013](../decisions/0013-postgres-storage.md) §4.*

| Schema | Holds | Lifetime | Role that may read it |
|---|---|---|---|
| `log` | §2: partitions, records, Merkle nodes, checkpoints, manifests | Permanent. The source of truth. | The server, the ingester, `verify` |
| `view` | §4: every projection the API and UI read | Rebuildable from `log` at any time | The server |
| `private` | State that belongs to one account and has no public history: the accounts graph's binding index, password hashes for the built-in `password` issuer ([0007](../decisions/0007-actor-identity.md) A3), sessions' secrets, IP addresses held for abuse handling and IP blocks, when each binding was last used ([0007](../decisions/0007-actor-identity.md) §3, [0012](../decisions/0012-api-requirements.md) §5, [0016](../decisions/0016-permissions-and-access-control.md) §3); the watch set and its Atom token ([0020](../decisions/0020-change-feeds.md) §3); inboxes and read state ([0021](../decisions/0021-notifications.md) §3); email addresses, fediverse handles, and the ActivityPub keys and followers of the notifier and of every opted-in actor ([0021](../decisions/0021-notifications.md) §5, §8, [0022](../decisions/0022-federation.md) §6); subsidiaries' API keys, OAuth tokens and confidential consumers' secrets ([0024](../decisions/0024-subsidiary-accounts.md) §4, [0025](../decisions/0025-oauth-server.md) §8); every preference, including the notification matrix and the auto-watch flags ([0027](../decisions/0027-preferences-and-portability.md) §1); the IP row behind each edit-filter hit ([0030](../decisions/0030-edit-filters.md) §11). §5 lists the tables | Permanent; never exported. The portable part travels with an account as a user data bundle, and with a cooperative tenant move as a sealed private extract applied on reclaim ([0027](../decisions/0027-preferences-and-portability.md) §2–4); nothing else leaves ([0018](../decisions/0018-tenants.md) §10) | `triplespace-accounts`, `triplespace-notify` and, for `ap_key` and `ap_follower` only, `triplespace-federation` |
| `ops` | Projection positions and queued work (§6), the stream and cache epochs (§4.11, §9.4), job runtime state, the bootstrap coordinator's block table ([0013](../decisions/0013-postgres-storage.md) §9), the email and fediverse delivery queue ([0021](../decisions/0021-notifications.md) §8, [0022](../decisions/0022-federation.md) §7), and the asynchronous jobs that are not log jobs: large user-data exports ([0027](../decisions/0027-preferences-and-portability.md) §3), filter tests ([0030](../decisions/0030-edit-filters.md) §6) and property-wide constraint re-checks ([0031](../decisions/0031-property-constraints.md) §2) | Operational | The server, the ingester, the notifier |

**Privacy is enforced by grants.** The database role the public API connects with has no privilege on `private`. Only the accounts service ([0007](../decisions/0007-actor-identity.md) §10), the notifier ([0021](../decisions/0021-notifications.md) §10) and the federation service ([0022](../decisions/0022-federation.md) §13), for actor keys and followers, connect with a role that does; all three are holder-only services, and none serves a route that returns another account's rows. The OAuth server ([0025](../decisions/0025-oauth-server.md) §8) reaches `private` only through the accounts service. The privacy test of [0012](../decisions/0012-api-requirements.md) §8, which checks that no route returns private data, is backed by a check that no query outside those three crates can. Where a private set has to meet public data, as the watch set meets `view.activity`, the private side resolves to a list of identifiers and hands it to a query under the public role ([0020](../decisions/0020-change-feeds.md) §3); there is no cross-schema join. `pg_dump` for exports and verification bundles excludes `private` and `ops`.

**The roles are named** `ts_server` (the server and the ingester), `ts_accounts`, `ts_notify`, `ts_federation` and `ts_verify` (read-only on `log`), created by `triplespace-db`'s migrations as `NOLOGIN` group roles that a deployment grants to its login roles; `ts_server` holds no privilege on `private`.

**The accounts partition is in `log`.** Bindings are log records ([0007](../decisions/0007-actor-identity.md) §8), and they keep their `logged` integrity. Their child table is in the `log` schema but owned by the accounts role, and the public role's `SELECT` on `log.record` is granted per child table, excluding it.

### 1.3 Storage contracts

*Sources: [0005](../decisions/0005-crate-organization.md) §4.4.*

`scatter-quadstore` is the storage contract from Scatterbase's plan: `lookup(s, p, o, g)` with wildcards, and since [0032](../decisions/0032-sparql-update-stream.md) §9 `apply(deleted, inserted)`. Scatterbase's drivers implement it, and it is Scatterbase's serving store. In Triplespace it is a consumer of `view.rdf_delta` ([0013](../decisions/0013-postgres-storage.md) §8): an instance that wants a local SPARQL endpoint applies the same deltas the SPARQL Update stream serializes, and nothing reads from it but the query service of [0059](../decisions/0059-query-service.md) §1.

Triplespace's serving store is the `view` schema (§4). It is not a shared seam. Scatterbase has no need of it, and its tables are Wikibase-shaped by design.

`LogStore` in `scatter-log` is the third contract, and the one both products share fully ([0005](../decisions/0005-crate-organization.md) §4.2).

Extra access paths, such as claim retrieval or chain traversal, are separate traits in the crates that need them. They are not added to `scatter-quadstore` or `LogStore`.

### 1.4 Version, driver, queries, migrations and queues

*Sources: [0033](../decisions/0033-backend-stack.md) §4.*

- **Version:** 17 is the minimum; 18 is the target. The schema uses features from 15 and later (`UNIQUE NULLS NOT DISTINCT`), 14 (lz4 TOAST) and none from extensions. **No Postgres extension is required**, so the small profile runs on any managed Postgres.
- **Driver:** `tokio-postgres` with `deadpool-postgres` for pooling. It gives binary `COPY` for bootstrap ([0013](../decisions/0013-postgres-storage.md) §9), pipelining, and exact control of the append transaction and its row lock (§2.1). `LogStore` is itself asynchronous, because the append shares the write path's transaction (§6), and a blocking caller drives it with an executor instead.
- **Queries:** hand-written SQL in each owning crate. No ORM. Query shapes are checked by integration tests against a real database ([0033](../decisions/0033-backend-stack.md) §15), not by compile-time macros, because several crates share one schema and migrations run in a fixed cross-crate order.
- **Migrations:** each owning crate embeds its SQL files ([0005](../decisions/0005-crate-organization.md) rule 9). A small runner in `triplespace-db` applies them in 0005's build order, each in its own transaction, and records each with a checksum in `ops.migration`; a recorded migration whose SQL has changed is refused, since a schema change is a new migration, never an edit. `instance create` runs it on a new database and `triplespace migrate` on an existing one; `triplespace status` lists pending migrations, and `triplespace-server` refuses to start on a database behind the migrations it carries, naming them, rather than answer reads with internal errors ([0033](../decisions/0033-backend-stack.md) A20). `refinery` is an acceptable substitute if the in-house runner stops being small.
- **Queues and background work:** `ops` tables claimed with `FOR UPDATE SKIP LOCKED`, woken with `LISTEN/NOTIFY`. This covers the ActivityPub delivery queue ([0021](../decisions/0021-notifications.md)), exports ([0027](../decisions/0027-preferences-and-portability.md)), filter tests ([0030](../decisions/0030-edit-filters.md)), constraint re-checks ([0031](../decisions/0031-property-constraints.md)) and jobs ([0012](../decisions/0012-api-requirements.md)). No broker.

## 2. The `log` schema

*Sources: [0013](../decisions/0013-postgres-storage.md) §2, §6.*

### 2.1 Partitions and records

*Sources: [0013](../decisions/0013-postgres-storage.md) §2; [0058](../decisions/0058-packed-record-storage.md) §2; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §5.*

```sql
CREATE TABLE log.partition (
  partition     bigint PRIMARY KEY,            -- 64 random bits, fixed at creation (0018 §2); 0 is the instance config
  tenant        text,                          -- tenant slug, or NULL for an instance partition (0018 §2)
  name          text NOT NULL,                 -- "local", "mirror/wikidata", "pages", …
  graph_iri     text NOT NULL,                 -- {base}/graph/{name} (0015 §5)
  history       text NOT NULL,                 -- full | latest            (0002 §2)
  integrity     text NOT NULL,                 -- logged | hashed          (0006 §4)
  export        text NOT NULL,                 -- public | internal | private (0005 §4.1, 0007 §8)
  segment_k     smallint NOT NULL,             -- segments hold 2^k records (0006 §5)
  hash          text NOT NULL,                 -- named in the partition's graph: record in config (0006 §2, 0015 §3)
  next_offset   bigint NOT NULL DEFAULT 0,
  UNIQUE (tenant, name)                        -- the registry key of 0018 §2
);

CREATE TABLE log.record (
  partition     bigint   NOT NULL REFERENCES log.partition,
  "offset"      bigint   NOT NULL,
  appended_at   bigint   NOT NULL,             -- µs since the epoch, UTC   (header 3)
  payload_type  text     NOT NULL,             --                          (header 4)
  key           text,                          -- identifier, never content (header 5)
  commitment    bytea    NOT NULL,             -- root over the part leaves, 32 bytes (header 6, 0015 §1)
  revid         bigint,                        -- global or provider-ranged revision ID (header 7, 0015 §2)
  logid         bigint,                        -- global log ID (header 8, 0015 §2)
  page_id       bigint,                        -- page ID of the key, carried forward (header 9, 0015 §2)
  header        bytea    NOT NULL,             -- the canonical CBOR header, the Merkle preimage
  leaf          bytea    NOT NULL,             -- H(0x00 ‖ header)
  body          bytea    NOT NULL,             -- canonical CBOR; an erased part is [null, leaf] (0015 §1)
  erased        smallint NOT NULL DEFAULT 0,   -- bitmask of erased parts (0015 §7)
  erased_by     bigint,                        -- offset of the latest erase record touching this row
  PRIMARY KEY (partition, "offset")
) PARTITION BY LIST (partition);
```

Packed storage (§3) adds `storage` to `log.partition` and `codec` and `dict` to `log.record`; the `ALTER TABLE` statements are in §3.2.

- **One child table per log partition**, which since [0018](../decisions/0018-tenants.md) §2 means one per tenant partition as well as one per shared partition. Compaction, vacuum, bootstrap loading and index choice are then per partition. A mirror partition under `latest` carries an index on `(key, "offset" DESC)` for compaction and the version cursor. The local partition carries the same index for base-offset checks. Whether small tenants should share physical tables is open in 0018.
- **The header columns are denormalized from `header`.** The CBOR bytes are the preimage that `leaf` and every proof are computed over. The columns exist so that the header can be queried without decoding. A test checks that re-encoding the columns reproduces the bytes. `revid`, `logid` and `page_id` are header fields 7–9 ([0015](../decisions/0015-record-format-and-partition-registry.md) §2), so they are inside the Merkle tree.
- **Bodies use TOAST with `lz4` compression in a `plain` partition.** Canonical CBOR compresses well, and `lz4` decompresses fast enough for the read path. Bodies are never queried inside the database; they are decoded by the server. A `packed` partition (§3) stores each body instead as zstd, against a dictionary trained per dedup domain, over a packed form in which repeated subtrees (references, qualifier sets and, under `full` history, whole statements) are fragment references into `log.fragment`; `log.partition` gains `storage`, `log.record` gains `codec` and `dict`, and `log.fragment_domain`, `log.fragment` and `log.dict` hold the domains, fragments and dictionaries. The packed form is physical: the body is rebuilt byte for byte, and every rebuilt body, plain or packed, is checked against `commitment` before it is used.
- **A body is never `NULL`.** Under [0015](../decisions/0015-record-format-and-partition-registry.md) §1 erasure is per part, and an erased part is rewritten as `[null, leaf]` so the commitment still verifies. `erased` records which parts are gone; a fully erased record has every bit set. Which `erase` record removed which part is recoverable from the `erase` records themselves, which name their targets and parts; `erased_by` is a convenience for the gap row of [0010](../decisions/0010-site-ui.md) §5.2 and holds the latest one.

**Offsets are allocated in the appending transaction.** The transaction locks the partition's row in `log.partition`, takes `next_offset`, inserts, and advances it. A rollback releases the offsets, so the sequence stays gapless as [0006](../decisions/0006-log-integrity-and-erasure.md) §3 requires. Appends to one partition are serialized by that lock. **The lock covers the append and nothing more**: what tier 1 of §6.2 does (normalize, evaluate filters, check the base against the local graph, allocate IDs, append, write the written graph's state, write the activity row) is a function of the written record alone, and nothing composes while the lock is held, so the interactive ceiling of a partition is its append rate, not its projection rate. That is the right behaviour for the local partition, where edits must be ordered. A bulk job writing into a live partition takes the lock **once per block**, and a bootstrap into an empty partition takes it once per block as well ([05](05-providers-and-ingest.md); [0013](../decisions/0013-postgres-storage.md) §9); the three write modes are set out in §6.2.

**No partition but `config` has a genesis record.** A partition's name, hash function, `k` and policies are its `graph:` record in `config`, which an export bundle carries; the `hash` column above is denormalized from it.

### 2.2 Merkle nodes, checkpoints and manifests

*Sources: [0013](../decisions/0013-postgres-storage.md) §2.*

**Merkle nodes are persisted.**

```sql
CREATE TABLE log.merkle_node (
  partition bigint   NOT NULL,
  level     smallint NOT NULL,                 -- 0 is the leaf level
  index     bigint   NOT NULL,
  hash      bytea    NOT NULL,
  PRIMARY KEY (partition, level, index)
);
```

An inclusion proof for a record in a partition of 10^8 records needs 27 sibling hashes. Recomputing them from leaves would read millions of rows; reading them from this table reads 27. In a `hashed` partition, each segment's tree is stored the same way and stands alone.

**Several processes append.** The tree's right edge is not cached across appends: the appending transaction reads it from this table under the append lock of §2.1, and only complete subtrees are persisted, so the interior nodes on the right edge are rewritten by each append (O(log n) row updates per edit) and no process relies on an in-memory copy.

**Checkpoints and manifests** are rows, not files. [0006](../decisions/0006-log-integrity-and-erasure.md) §6 says checkpoints are "stored beside their partition"; a table is beside the partition.

```sql
CREATE TABLE log.checkpoint (                   -- logged partitions only
  partition   bigint   NOT NULL,
  tree_size   bigint   NOT NULL,
  root        bytea    NOT NULL,
  key_id      text     NOT NULL,               -- the signed-note key hash of the signing key, 8 hex digits (0006 §6)
  signed_note text     NOT NULL,               -- the C2SP note, verbatim
  written_at  timestamptz NOT NULL,
  PRIMARY KEY (partition, tree_size)
);

CREATE TABLE log.segment_manifest (
  partition   bigint   NOT NULL,
  segment     bigint   NOT NULL,
  root        bytea    NOT NULL,
  signed_note text     NOT NULL,
  sealed_at   timestamptz NOT NULL,
  PRIMARY KEY (partition, segment)
);
```

A manifest is signed once and is never re-signed, and no manifest lists a manifest it replaces. **A `hashed` partition has segment manifests only.** Its "checkpoint" at the end of a job is the manifest of the last sealed segment together with an unsigned head offset; records in the open segment are uncommitted until the segment seals; `log.checkpoint` rows exist for `logged` partitions only. **Checkpoints are signed by the projection worker**, under an advisory lock per partition, and that worker is the only process that holds the instance key; the API processes append and never sign.

`GET /record/{partition}/{offset}/checkpoint` ([0012](../decisions/0012-api-requirements.md) §5) is `SELECT min(tree_size) WHERE tree_size > offset` on a `logged` partition, and the last sealed segment's manifest on a `hashed` one.

### 2.3 Global revision, log and page IDs

*Sources: [0013](../decisions/0013-postgres-storage.md) §6.*

[0012](../decisions/0012-api-requirements.md) §2.1 asks for one sequence for revisions and one for log events, assigned at append and stored in the record. Postgres sequences do this, one set per tenant ([0018](../decisions/0018-tenants.md) §2), since MediaWiki clients expect one sequence per wiki:

- `log.revision_id` is taken for every record appended to a partition whose records are this tenant's revisions: `local` and `pages`, except an adopted revision, which carries the source wiki's own (below). It is written to header field 7 and to `log.record.revid` in the appending transaction.
- `log.log_id` is taken for **every** record in the tenant's `local`, `pages`, `log` and `actors` partitions, whether or not the catalogue of [16](16-logs-feeds-and-notifications.md) projects it as a log event ([0011](../decisions/0011-logs.md) §6.1), written to header field 8 and `log.record.logid` at append.
- `log.page_id` is taken the first time a key is written in one of the tenant's own partitions, and every later record for that key repeats it in header field 9 ([0015](../decisions/0015-record-format-and-partition-registry.md) §2). It is never derived from replay order, and it stays below 2^40. Talk pages take theirs from the same sequence through the thread record that first attaches to them ([0019](../decisions/0019-discussions.md) §2).
- Mirror records take a **provider-ranged** revision ID, `provider_number << 40 | n` ([0015](../decisions/0015-record-format-and-partition-registry.md) §2), computed by the writer with no allocation. **A mirrored entity's page ID is provider-ranged the same way**: `provider_number << 40 | upstream page ID` where the provider publishes one, as Wikidata does, and `provider_number << 40 | mirror offset` otherwise, computed by the writer from the record in hand, with no allocation and no read of `view` to mint a hashed header field. A page served by a page repository takes `provider_number << 40 | upstream page ID` in the same way ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6). The sub-2^40 range of the page ID space is therefore the tenant's own sequence, and one `pageid` space covers local pages, local entities and mirrored entities on a tenant without collision (§4.5). Provider numbers and entity-source numbers are small enough that every ranged ID stays below 2^53 ([01](01-log-and-records.md) §2.5).
- Local entity IDs come from one more set of per-tenant sequences, one per minted entity type (`log.item_id`, `log.property_id`, …), taken in the appending transaction as Wikibase's `wb_id_counters` are; the ID blocks of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5 reserve ranges from them. An adoption sets every sequence here, entity, page, revision, log and user IDs, past what the source wiki consumed, records the floors in its job record, and supplies `page_id` for adopted records, and `revid` for adopted revisions, rather than taking them ([0035](../decisions/0035-adopting-a-wikibase.md) §4).

**The instance's own sequences** live under the reserved tenant name `instance`, which no tenant may take as its slug: `log."instance.job_id"` for job IDs ([0011](../decisions/0011-logs.md) §6.3), `log."instance.log_id"` and `log."instance.page_id"` for records in the instance partitions, and one `log."instance.{type}_surrogate"` per keyed type ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §7). `log."instance.page_id"` is never used for entities: a mirrored entity's page ID is ranged, as above. Per-tenant sequences also include `log."{tenant}.user_id"` ([0007](../decisions/0007-actor-identity.md) §3) and `log."{tenant}.actor_surrogate"`, from which the tenant mints the surrogates `actor:{n}` ([0007](../decisions/0007-actor-identity.md) §5; [07](07-actors-and-accounts.md) §1.4).

**Surrogates** are allocated in the appending transaction under an advisory lock on `(keyed_type, key)`, and the mapping record that binds the surrogate to its key is appended in the same transaction as the first record that uses it, so two writers racing on one key cannot mint two surrogates and no record is ever keyed to a surrogate the log does not map. Bulk jobs reserve surrogate blocks from the sequence as they reserve entity ID blocks. A mapping is erased only when no live record in any partition is keyed to the surrogate.

The IDs are inside the hashed header, so an inclusion proof covers them, an export bundle needs no sidecar, and an erased record keeps them.

## 3. Packed record storage

*Sources: [0058](../decisions/0058-packed-record-storage.md) §1, §2, §3, §3.1, §4, §5, §6, §9, §10.*

### 3.1 Stored form and logical form

*Sources: [0058](../decisions/0058-packed-record-storage.md) §1 (shared with [01](01-log-and-records.md)).*

A record's **logical form** is what [0015](../decisions/0015-record-format-and-partition-registry.md) §1 and payloads.md define: the header and the body of salted parts, in canonical CBOR. Everything that hashes, signs, proves, exports or crosses a wire uses the logical form: leaves, commitments, content hashes, checkpoints, inclusion proofs, bundles, the `segments` backend, `LogStore::read` and `scan`, and every API. The logical form itself is the subject of [01](01-log-and-records.md).

A record's **stored form** is how `scatter-log-postgres` keeps the body in `log.record`. It is physical: the body is rebuilt from it byte for byte, and nothing outside the store can tell which form a row uses. Changing the stored form is never a format version. A partition may hold rows in several stored forms at once, and `repack` (§3.8) converts rows between them.

**Every read checks the commitment.** When a stored body is rebuilt, the store recomputes each present part's leaf and the commitment, and compares the result with the row's `commitment` column (header field 6). This costs one SHA-256 pass over a few kilobytes, a few microseconds. A mismatch is a `Corrupt` error, which is logged and never served. A packing bug, a damaged dictionary or a wrongly deleted fragment therefore fails loudly on the first read, never silently. A `plain` row is checked the same way.

### 3.2 Schema additions

*Sources: [0058](../decisions/0058-packed-record-storage.md) §2.*

```sql
ALTER TABLE log.partition
  ADD COLUMN storage text NOT NULL DEFAULT 'plain';   -- plain | packed (§9)

ALTER TABLE log.record
  ADD COLUMN codec smallint NOT NULL DEFAULT 0,       -- 0 plain: body is the logical body
                                                      -- 1 packed: body is zstd(packed body), §3–4
  ADD COLUMN dict  integer;                           -- the log.dict a packed body was compressed with, or NULL

CREATE TABLE log.fragment_domain (                    -- §8: one per dedup scope
  domain    integer PRIMARY KEY,
  scope     text    NOT NULL UNIQUE,                  -- '' for the instance's shared partitions, else a tenant slug
  key       bytea   NOT NULL,                         -- 32 random bytes that key fragment references (§3)
  body_dict integer,                                  -- current dictionary for bodies, or NULL
  frag_dict integer,                                  -- current dictionary for fragments, or NULL
  epoch     integer NOT NULL DEFAULT 0,               -- bumped when a sweep starts (§6)
  sweeping  boolean NOT NULL DEFAULT false
);

CREATE TABLE log.fragment (                           -- §3
  domain    integer NOT NULL,
  ref       bytea   NOT NULL,                         -- 16 bytes
  bytes     bytea   NOT NULL,                         -- zstd of the subtree's packed CBOR
  dict      integer,                                  -- the log.dict it was compressed with, or NULL
  shared    boolean NOT NULL DEFAULT false,           -- more than one append has used it (§6)
  epoch     integer NOT NULL,                         -- the domain epoch when it was last written or pinned
  PRIMARY KEY (domain, ref)
) PARTITION BY LIST (domain);

CREATE TABLE log.dict (                               -- §4; immutable
  dict       integer PRIMARY KEY,
  domain     integer NOT NULL,
  purpose    text    NOT NULL,                        -- body | fragment
  bytes      bytea   NOT NULL,
  sha256     bytea   NOT NULL,
  trained_at timestamptz NOT NULL,
  samples    integer NOT NULL
);

CREATE TABLE ops.fragment_candidate (                 -- §6: possibly unreferenced fragments
  domain integer NOT NULL,
  ref    bytea   NOT NULL,
  PRIMARY KEY (domain, ref)
);
```

The section references in the comments are to [0058](../decisions/0058-packed-record-storage.md); its §8, domains and tenants, is in [08](08-tenants-and-instances.md).

- **The child table of a packed partition sets `body` to `STORAGE EXTERNAL`,** so TOAST never tries to recompress zstd output. `log.fragment`'s children do the same for `bytes`. A plain partition keeps `lz4` as before.
- **Fragments, dictionaries and domains belong in the `log` schema.** Bodies cannot be rebuilt without them, so they share the log's backup and recovery point and the read grants of `ts_server` and `ts_verify` (§1.2). They are never exported: bundles carry logical bytes.
- **Fragments have one child table per domain,** so a tenant's fragments are dropped, vacuumed and sized on their own.

### 3.3 Fragments

*Sources: [0058](../decisions/0058-packed-record-storage.md) §3.*

A **packed body** is the logical body with zero or more subtrees inside present parts replaced by **fragment references**. Erased parts (`[null, leaf]`) are left as they are.

- **A reference** is CBOR tag `0x54530001` wrapping a 16-byte string. Canonical CBOR has no tags (payloads.md §1), so a tag in stored form is unambiguous. The tag is never registered, and it never appears outside `log.record` and `log.fragment`.
- **`ref`** is the first 16 bytes of HMAC-SHA-256 under the domain's `key`, computed over the subtree's **logical** bytes (with nested references expanded). A ref therefore does not depend on how the subtree's own children were packed. Being keyed, refs cannot be computed or probed without the key, and colliding inputs cannot be crafted. At 128 bits, accidental collisions are negligible: about 10⁻¹⁵ expected at 10¹² distinct fragments. Any collision, accidental or not, is still caught by §3.1's commitment check.
- **A fragment's `bytes`** are the subtree's canonical CBOR, packed in turn: a statement fragment can refer to its reference fragments. Rebuilding splices bytes recursively. Because canonical CBOR is compositional (a map's encoding is its header followed by its encoded pairs), splicing reproduces the logical bytes exactly, with no re-encoding. The depth limit is 4.
- **The packer is generic.** `scatter-log` walks CBOR by path pattern and knows nothing about Wikibase. The Wikibase knowledge lives in the registry file, so that file can change at any time, and `repack` (§3.8) applies the change to old rows.

**Which subtrees become fragments** is registry data. `docs/registry/fragments.toml` is authoritative and gives path patterns per payload type, a `history` condition and `min_bytes`. It lists:

| Payload type | Path | History | What |
|---|---|---|---|
| `scatter:v0/changeset` | `/**/claims/*/*/references/*` | any | One reference: `snaks`, `snaks-order`, `hash` where kept |
| `scatter:v0/changeset` | `/references/*/*` | any | One reference added by an `add` op |
| `scatter:v0/changeset` | `/**/claims/*/*/qualifiers` | any | A statement's qualifier map |
| `scatter:v0/changeset` | `/**/claims/*/*` | `full` | A whole statement, GUID included, which repeats across one entity's revisions |

A subtree is packed only if it encodes to at least `min_bytes` (64 initially). A reference costs 22 bytes (a 5-byte tag head, a 1-byte string head and 16 bytes of ref), so anything smaller is cheaper inline. Repeats inside one record are packed once. Statement fragments are limited to `full` partitions: in a `latest` partition every statement GUID is unique to its entity and every edit replaces the statement, so a fragment for it would save nothing and become garbage on the next compaction.

### 3.4 Writing a packed record

*Sources: [0058](../decisions/0058-packed-record-storage.md) §3.1.*

Inside the appending transaction (§6), after the logical record is sealed (header, leaf and commitment unchanged), a packed partition:

1. reads its domain's row (`epoch`, `sweeping`, `body_dict`, `frag_dict`), one cached heap page;
2. packs the body bottom-up, computing each ref locally;
3. writes the fragments:
   - **No sweep running:** a ref the process holds in its *known-shared* cache (§9.5) for the current epoch is skipped. Every other ref goes through `INSERT … ON CONFLICT (domain, ref) DO NOTHING RETURNING ref`. Refs that conflicted are marked with `UPDATE log.fragment SET shared = true WHERE domain = $d AND ref = ANY($conflicted) AND NOT shared`, which locks only rows that are not yet shared. That is at most once in each fragment's life, so a popular reference is never a hot row.
   - **A sweep running:** every ref, cached or not, goes through `INSERT … ON CONFLICT (domain, ref) DO UPDATE SET epoch = $epoch, shared = true`. This pins it against the sweep (§3.7). Pinning costs row locks for the sweep's duration, which is why bulk loads and sweeps exclude each other ([0058](../decisions/0058-packed-record-storage.md) §7).
4. compresses the packed body with the domain's body dictionary and inserts the row with `codec = 1`.

### 3.5 Dictionaries

*Sources: [0058](../decisions/0058-packed-record-storage.md) §4.*

Small bodies and fragments compress badly alone, and well against a dictionary trained on their siblings. The keys, data-type names, common property IDs and common item IDs that every Wikidata entity repeats are exactly what a dictionary captures.

- **Dictionaries are per domain and purpose** (`body`, `fragment`), trained with zstd's trainer (`ZDICT_trainFromBuffer`, through the `zstd` crate) on a sample of that domain's packed bodies or fragments, about 110 kB each.
- **A domain starts without a dictionary** and compresses with plain zstd. `triplespace-cli storage retrain` trains one once a domain holds enough samples (10,000 bodies by default). New writes use the new dictionary, and old rows keep the one they name.
- **Dictionaries are immutable and never deleted while a row names them,** checked by `retrain --prune`. They are small enough to keep forever, and every process holds the dictionaries of the domains it serves.
- **Level:** zstd 3 for interactive appends and 19 for `repack` and bulk loads. Decompression speed is about the same at either level.

### 3.6 Reading

*Sources: [0058](../decisions/0058-packed-record-storage.md) §5.*

`LogStore::read` and `scan` return logical records. For a packed row the store decompresses the body, collects its refs, fetches the missing ones with one `SELECT … WHERE domain = $d AND ref = ANY($refs)` per batch (a page of records in a `scan`), splices them, recurses for nested refs, and checks the commitment (§3.1). Fragments are immutable, so the fragment cache (§9.5) holds them without invalidation except on erasure.

The serving read of a `source` entity (§4.2) is therefore one row, at most one fragment query on a cold cache, and a decompression. Above the store, the L1 entity JSON cache (§9.4) hides even that for hot entities.

### 3.7 Garbage: candidates, sweeps and erasure

*Sources: [0058](../decisions/0058-packed-record-storage.md) §6 (shared with [01](01-log-and-records.md)).*

There are no reference counts. A fragment becomes garbage only when the rows that used it are compacted away or erased, and two mechanisms reclaim it.

**Candidates.** Compaction and erasure write the refs of what they remove (the deleted rows' bodies, the erased parts) into `ops.fragment_candidate`, in their own transaction. A **candidate sweep** deletes every candidate that is **not `shared`**. `shared = false` means exactly one append ever used the fragment, and that append's row is the one just removed. Deleting a fragment makes its nested refs candidates in turn. A shared candidate is left for a full sweep, and the domain's count of such leftovers decides when one is worth running.

**Full sweeps.** A **full sweep** marks every ref reachable from the domain's live rows (bodies, then nested fragments) into a temporary table, then deletes every fragment that is not marked and whose `epoch` is older than the sweep's.

**The fence.** Both kinds of sweep start the same way:

1. set `sweeping = true` and increment `epoch` on the domain's row;
2. wait until every transaction that began before that commit has ended, as `CREATE INDEX CONCURRENTLY` does.

After the fence, every append into the domain sees `sweeping` and pins each ref it uses (§3.4 step 3). An append that began before the fence has committed by the time marking starts, so the mark sees its row. If a deletion and a pin race on the same fragment, Postgres's row lock decides. If the pin commits first, the deletion's recheck finds the new `epoch` and skips the row. If the deletion commits first, the pin's `INSERT` finds no row and writes the fragment again, from the bytes the writer is holding. When the sweep ends, `sweeping` is cleared and a `NOTIFY ts_fragment` names the domain and epoch.

**Erasure.** The erase transaction rewrites the target's body as [0013](../decisions/0013-postgres-storage.md) §3 says (in [01](01-log-and-records.md)). In a packed row the remaining parts stay packed, and the erased parts' refs become candidates. The erasure worker then runs a candidate sweep before the `VACUUM` it already runs, so bytes used only by the erased record are gone within the same job. A fragment the erased record **shared** with other rows is still held by them, legitimately, unless they too are gone. For reason class `legal`, the worker therefore runs a full sweep of the domain as well, and vacuums the domain's fragment table afterwards. The vacuum policy covers the fragment tables as it covers `log.record`.

### 3.8 Which partitions are packed; measurement and repacking

*Sources: [0058](../decisions/0058-packed-record-storage.md) §9, §10.*

`log.partition.storage` is an operator setting, not a log record: `[storage] packed = ["mirror/*", "local"]` in the server's configuration, applied when a partition is created and changed later with `storage pack` and `repack`. The default is `plain` for every partition until the measurements of 0058's Q1 are recorded there. Then `mirror/*` is expected to become `packed`, since mirror partitions hold nearly all of the volume.

Interactive appends to a packed `local` partition pay for one HMAC per fragment-sized subtree, one `INSERT` per new fragment and a zstd pass, inside the tier-1 transaction of §6.2, which already writes the graph state and the activity row. That is small next to composition, which runs after commit. Packing `local` pays mainly for full-history tenants that adopt or backfill ([0035](../decisions/0035-adopting-a-wikibase.md) §7).

`triplespace-cli storage measure --partition {name} [--sample N]` packs a sample of a partition's rows without writing anything, and reports bytes per record under each candidate: plain canonical CBOR, `lz4` alone (what TOAST would do), zstd, zstd with a dictionary, and packed with a dictionary. It also reports the number of distinct fragments, the distribution of how many times each is used, and fragment bytes per body byte. Run over a sample of the Wikidata JSON dump through `scatter-adapter-wikidata`, and over Librarybase's adoption, it answers 0058's Q1 and [0013](../decisions/0013-postgres-storage.md) Q1.

`triplespace-cli storage repack --partition {name} [--dict new]` rewrites rows in batches to the partition's current storage setting, dictionary and `fragments.toml`. It checks every row's commitment before and after, and runs as a writer under §3.4, so it is safe beside sweeps. `storage sweep --domain {scope} [--full]` runs §3.7 on demand. `storage pack` changes `log.partition.storage`.

## 4. The `view` schema

*Sources: [0013](../decisions/0013-postgres-storage.md) §5, §5.1, §5.2, §5.3, §5.4, §5.5.*

### 4.1 The serving rule and the tenant column

*Sources: [0013](../decisions/0013-postgres-storage.md) §5; [0018](../decisions/0018-tenants.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5, §5.1.*

The rule for the serving model is: **the blob serves the page; tables serve queries.** A table exists only where a route needs a lookup or an order that the entity's own JSON cannot give. Nothing is normalized for its own sake.

**The stored graph is in source form.** Every `view` table keeps the entity IDs its source wrote: `WDQ5` where Wikidata wrote `Q5`, `Q9` where a local editor wrote `Q9`, in statement subjects, in values, in `entity_ref`, in `identifier` and in the search documents. The canonical ID of a cluster is an attribute of `view.cluster`, never applied to the data, so nothing in this schema is rewritten because a cluster formed, changed or dissolved; the form a consumer sees is decided on the way out ([04](04-entities-and-identifiers.md); [18](18-api.md)), after the cache (§9.3).

**Tenants.** Under [0018](../decisions/0018-tenants.md) §6, `view.entity` and the tables that hang off it (`entity_source`, `graph_state`, `term`, `sitelink`, `identifier`, `entity_ref`, `statement_assertion`, `correction`, `constraint_violation`, `value_key`, and the cluster tables `cluster`, `cluster_member`, `link` and `different_from`) carry a `tenant text NOT NULL DEFAULT ''` column that leads every primary key: the **empty string is the instance**, and the row with `tenant = ''` is the shared row computed from the shared source graphs under the instance's policy record (§7); a tenant that has local assertions, a cluster link, a correction or a derivation touching the entity has its own `(tenant, id)` row; a tenant with none reads the shared one. A read by primary key is therefore one lookup with the viewer's tenant, falling back to one with `''`; a read over shared rows by any other key excludes the shared rows of entities the viewer's tenant has overlaid, the rule §7 states once. Every other table in this section is per tenant outright and carries `tenant` in its key, except a third class, **instance-scope** tables with no `tenant` column: `view.foreign_title`, `view.foreign_page` and `view.foreign_thread` (§4.8, §4.12); and `view.rdf_delta`'s shared rows carry `tenant = ''` (§4.11). The sentinel is chosen over a `NULL` tenant because a primary key cannot hold `NULL` and a plain unique constraint would not reject a duplicate shared row; no slug can be empty ([0018](../decisions/0018-tenants.md) §1), so nothing collides. Where an earlier ADR says "a `NULL` tenant", read `''`. The SQL below is written for one tenant; the column is implied throughout except where a key or an index names it. `partition` columns are `bigint`, as in §2.

### 4.2 Entities

*Sources: [0013](../decisions/0013-postgres-storage.md) §5.1; [0022](../decisions/0022-federation.md) §10; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §5; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5, §5.1; [0083](../decisions/0083-write-path-in-three-tiers.md) §2, §4; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5.*

```sql
CREATE TABLE view.entity (
  tenant            text NOT NULL DEFAULT '',  -- §4.1: '' is the shared row
  id                text NOT NULL,             -- local, foreign or keyed ID in the form of 0017 §1, as the source wrote it
  type              text NOT NULL,             -- item, property, domain, …
  page_id           bigint NOT NULL,           -- §2.3: the tenant's sequence, or provider-ranged for a mirrored entity
  surrogate         bigint,                    -- keyed types only (0009 §7)
  cluster_id        bigint,                    -- NULL when the entity is in no cluster
  canonical_id      text NOT NULL,             -- the cluster's canonical member, itself when in no cluster (0004 §4)
  policy            text NOT NULL DEFAULT 'default',  -- the policy record the row was composed under (§7)
  retention         text,                      -- cascade | orphan | retain (0002 §5)
  first_seen        timestamptz NOT NULL,
  local_offset      bigint,                    -- latest local-graph record for this key
  local_revid       bigint,                    -- lastrevid: the newest local revision, else the newest mirror record's ranged ID
  resolved_version  bigint NOT NULL,           -- from one instance-wide sequence; new on every composition (0014 §3)
  composed_from     bytea NOT NULL,            -- the inputs of the composition: graph-state offsets, policy offset, cluster map version
  generation        integer NOT NULL DEFAULT 0,-- bumped on erasure and hiding (0014 §5)
  resolved_kind     text NOT NULL,             -- source | materialized
  resolved          bytea,                     -- canonical JSON, compressed; NULL when source
  resolved_size     integer NOT NULL,
  ref_count         bigint NOT NULL DEFAULT 0, -- distinct referrers in entity_ref; a counter, updated in batches
  revision_counts   jsonb,                     -- revisions by origin (imported, bot, local); a counter, updated in batches
  deleted           boolean NOT NULL DEFAULT false,  -- derived from a read ACL on the entity (0023 §4, §10)
  PRIMARY KEY (tenant, id),
  UNIQUE (tenant, page_id)
);

CREATE TABLE view.entity_source (               -- the version cursor of 0002 §8.4, per graph
  tenant            text NOT NULL DEFAULT '',  -- '' for a registry provider's graph; the tenant for local and source/{name}
  entity_id         text NOT NULL,
  graph             text NOT NULL,             -- local, mirror/wikidata, source/{name}, …
  "offset"          bigint NOT NULL,           -- the graph's current record for the entity
  upstream_version  text,                      -- lastrevid, updated_date, …
  prev_upstream     text,                      -- 0012 §2.2
  content_hash      bytea NOT NULL,            -- H(0x03 ‖ payload)
  size              integer NOT NULL,          -- 0012 §2.4
  synced_at         timestamptz NOT NULL,
  job_id            bigint,
  seen_job          bigint,                    -- written by the ingester as cursor state (0002 §8.4)
  provider_revid    text,                      -- 0022 §2: the provider's own revision ID, for a verified Triplespace provider
  verified_at_size  bigint,                    -- 0022 §2: the provider checkpoint tree size this record was verified against; written by the sync job as cursor state, with synced_at and job_id
  PRIMARY KEY (tenant, entity_id, graph)
);

CREATE TABLE view.graph_state (                 -- the current state of every graph whose records are deltas
  tenant            text NOT NULL DEFAULT '',
  entity_id         text NOT NULL,
  graph             text NOT NULL,             -- local, derived/{source}, source/{name}
  state             bytea NOT NULL,            -- the graph's assertions for the key, canonical JSON, compressed
  "offset"          bigint NOT NULL,           -- the record that produced it
  version           bigint NOT NULL,
  PRIMARY KEY (tenant, entity_id, graph)
);

CREATE TABLE view.entity_revision (             -- the local-graph state after every local revision
  tenant            text NOT NULL,
  revid             bigint NOT NULL,            -- a revision of the local or pages partition
  entity_id         text NOT NULL,
  partition         bigint NOT NULL, "offset" bigint NOT NULL,   -- the record, UNIQUE
  state             bytea NOT NULL,
  PRIMARY KEY (tenant, revid)
);
CREATE INDEX ON view.entity_revision (tenant, entity_id, "offset");

CREATE TABLE view.entity_history (              -- the one backing of a mirrored entity's history
  tenant            text NOT NULL DEFAULT '',
  canonical_id      text NOT NULL,
  time              timestamptz NOT NULL,
  partition         bigint NOT NULL, "offset" bigint NOT NULL,
  kind              text NOT NULL,              -- put | tombstone | upstream event kinds (delete/*, protect/*, create/create)
  member            text NOT NULL,              -- the cluster member the row belongs to
  PRIMARY KEY (tenant, canonical_id, time, partition, "offset")
);
```

**`view.entity` is a cache of composition over the graph states, which are the truth.** An entity's fused body, the resolved view of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, is a deterministic function of the current state of each contributing graph (`view.graph_state` for the graphs whose records are deltas, `view.entity_source` for mirror graphs, whose state is the latest `put`), of the policy record it is computed under (§7) and of the cluster map; `composed_from` records exactly those inputs, so staleness is a comparison and the same function, run anywhere, produces the same bytes. Composition is tier 2 of §6, and it runs after commit, never inside the appending transaction. A composed row is replaced only when its inputs are newer than `composed_from`, so two composers meeting on one key cannot overwrite a newer result with an older one, and `resolved_version` is drawn from one instance-wide sequence, so a rolled-back version is never reused.

**`graph_state`** is written in tier 1 by the appending transaction, one row per written key, for every graph whose records are deltas (`local`, `derived/*`, `source/*`); it is the old state that composition reads, that the diff of a Wikibase write reads ([18](18-api.md)), and that the delta consumer of §4.11 reads. **`entity_revision`** is written beside it for every revision of the `local` and `pages` partitions, so that `oldid=`, `action=compare`, `GET /entity/{id}/compare`, `?as_of=`, `Special:PermanentLink` and export read one row: MediaWiki's `text` table at local scale, since local edits are the small part of a mirror-dominated instance and the rows pack well (§3). The row names its record and subject, so a diff resolves a `{partition, offset}` without a join and finds the state before a revision at the previous offset the stored state lists ([0083](../decisions/0083-write-path-in-three-tiers.md) A2). The `revision_state` projection (migration `0010_revision_state`) writes both for the `local` partition today; `pages`, `derived/*` and `source/*` join with their writers. Where a row is missing — a database migrated but not rebuilt — the readers fold the subject's local records instead, and a key with no `graph_state` row is seeded from its earlier records on its next write ([0083](../decisions/0083-write-path-in-three-tiers.md) A1, A2). Mirror states are not reconstructable under `latest`; only the current `put` exists.

**`entity_history`** is written by composition, which has the record in hand, with one row per mirror record and per relevant upstream log event of every member of the cluster, so that a mirrored entity's history page is one keyset query over one index, bounded by the entity's own record count. Upstream log events are rows here and never of `view.activity` (§4.6); the default mirrored provider log is `delete/*` and `protect/*`, the backfill covers held entities only, and `create/create` is derived from the first upstream revision ([16](16-logs-feeds-and-notifications.md)).

**The resolved view is materialized only where it differs from its source.** Reconciliation across graphs ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3) and statement fusion ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §8) are too costly to do per request, so composition stores the fused canonical JSON. But for most entities in a full mirror exactly one graph contributes, and the resolved JSON *is* the mirror record's payload, which the adapter already wrote in prefixed-ID form ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4). Storing it twice would double the largest table in the database. So **`materialized` means two or more contributing graphs, or local deltas, and nothing else**:

- Where one whole-state record (a `put`, a `create`, or an `adopt` that is the key's only local record) is the only contributor, `resolved_kind = 'source'` and `resolved` is `NULL`; a reader takes the body at `entity_source.offset`.
- Where two or more graphs contribute, or the only graph is a delta graph (a local entity edited with `add` or `remove`), `resolved_kind = 'materialized'` and `resolved` holds the fused JSON. `resolved` is compressed by the column's TOAST method (`lz4`, as §2.1 compresses bodies); the composer writes the plain canonical JSON.
- A mirrored entity that merely *references* a clustered entity is `source`, since nothing in it is rewritten (§4.1). The set of materialized rows is bounded by what tenants have asserted about an entity, never by what references it, and a cluster change recomposes the cluster's members and nothing else.

Either way, a reader gets the entity with one or two primary-key lookups, and `resolved_version` is what the cache and `ETag` key on (§9.3).

**Every member of a cluster keeps its row.** `cluster_id` and `canonical_id` name the cluster and its canonical member; the fused body is composed once per cluster from the members' graph states and stored on the canonical member's row, and a read of a non-canonical member serves that body under the requested ID ([18](18-api.md)). Each member keeps its own `entity_source`, `graph_state` and `page_id`, and its terms in the per-graph views.

**`lastrevid`** is `local_revid`: the revision ID of the key's newest local record when it has one, and otherwise the provider-ranged ID of its newest mirror record. The base check of a write is per source graph, against the local partition's newest record for the key ([18](18-api.md)); a mirror advancing is never a conflict.

**The counters.** `ref_count` and `revision_counts` are counter columns, maintained by the `entity_ref` and activity projections in batches, never by one `UPDATE` per referrer row, and computed by one aggregate at the end of a bootstrap; the About panel renders a count above a threshold as "10,000+" and has its own response and `max-age` (§10.3). They are not part of the entity's version: a referrer's edit changes a counter and no `resolved_version`.

**Verified providers.** The mirror record content for a `verified` provider carries the provider header and checkpoint size ([0022](../decisions/0022-federation.md) §2); `view.entity_source` holds them in `provider_revid` and `verified_at_size`, and nowhere else. `verified_at_size` is cursor state written by the sync job, as `synced_at`, `job_id` and `seen_job` are, and not a projection of any record: a rebuild of the entity projection from the log leaves it as the job last wrote it.

**Other entity types.** `view.entity` gains rows for `WDM` as for any foreign entity ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; the API and RDF halves of that section are in [18](18-api.md) and [02](02-graphs-rdf-and-query.md)), and rows for `L` with the resolved JSON including parts ([0066](../decisions/0066-lexemes.md) §5).

### 4.3 Terms, sitelinks, identifiers and references

*Sources: [0013](../decisions/0013-postgres-storage.md) §5.2; [0026](../decisions/0026-sitelinks.md) §6; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §5; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1, §6.*

```sql
CREATE TABLE view.term (                        -- the resolved fingerprint (wikibase-compat §2)
  entity_id text NOT NULL, kind smallint NOT NULL,   -- 1 label, 2 description, 3 alias
  lang text NOT NULL, ordinal smallint NOT NULL DEFAULT 0,
  text text NOT NULL,
  PRIMARY KEY (tenant, entity_id, kind, lang, ordinal)
);
-- Prefix search without OpenSearch (0014 §8). Partial over tenant rows (local entities and overlays),
-- so it is small on every profile; read by the small profile's Postgres fallback, never by the large one.
CREATE INDEX term_prefix ON view.term (lang, kind, lower(text) text_pattern_ops) WHERE tenant <> '';

CREATE TABLE view.sitelink (                    -- sitelinks are URLs; the host is the site (0026 §1–2, §6)
  url_key text NOT NULL,                        -- the normalized URL; one item per URL
  entity_id text NOT NULL, host text NOT NULL,  -- host is a Domain key
  site_id text, title text,                     -- alias site ID (enwiki) where one applies; display title
  badges text[] NOT NULL DEFAULT '{}',
  PRIMARY KEY (tenant, url_key),
  UNIQUE (tenant, entity_id, host)              -- one link per host per item
);
CREATE INDEX sitelink_host ON view.sitelink (host);
CREATE INDEX sitelink_entity ON view.sitelink (entity_id);

CREATE TABLE view.identifier (                  -- normalized external-id values (0004 §7), in source form
  property text NOT NULL, value_key text NOT NULL,
  entity_id text NOT NULL, graph text NOT NULL, statement_id text NOT NULL,
  PRIMARY KEY (tenant, property, value_key, entity_id, graph)
);

CREATE TABLE view.match_key (                   -- uniqueness for designated properties (0002 §8.5)
  property text NOT NULL, value_key text NOT NULL, entity_id text NOT NULL,
  PRIMARY KEY (tenant, property, value_key)
);

CREATE TABLE view.entity_ref (                  -- the reverse index: "Links here" and the backlink count
  target_page_id bigint NOT NULL, source_page_id bigint NOT NULL,
  roles smallint NOT NULL,                      -- bitmask: 1 main snak, 2 qualifier, 4 reference
  PRIMARY KEY (tenant, target_page_id, source_page_id, roles)
);
```

- **`term` serves label lookups**, which every entity page does in bulk for the entities it links to. It is the equivalent of Wikibase's term store. **Which rows it holds** is the budget that makes it affordable beside a full mirror: rows for local entities, for entities in any tenant overlay, and for mirrored entities in the **instance's** term languages only (`entities.term_languages` is instance configuration for the shared rows; a tenant's setting of the same name narrows only its own display). Every other label lookup reads the record body at `entity_source.offset` through the L1 label cache (§12.1), as [05](05-providers-and-ingest.md) §5.6 describes for unmirrored entities; a value shown under one member of a cluster may take its label from another member's rows, with precedence from the policy order of §7. The table is therefore sized at mirrored entities × |instance languages| × term kinds, and the Wikidata-scale profile ([22](22-crates-and-stack.md)) lists it beside `entity_ref`. Prefix search on it is a fallback for the small profile; the search index (§11) is the primary path, and at Wikidata scale the only one, since `term_prefix` is partial over local entities and **no prefix index over mirrored terms is ever created**. `term` gains three term kinds for lexemes: **4 lemma**, **5 representation**, **6 gloss**, with `entity_id` the lexeme's ID for lemmas and the part's ID for representations and glosses, and `lang` the language code or variant; the term projection writes them with the other terms, so label lookups, prefix search without OpenSearch and the term-based APIs see them with no new table ([0066](../decisions/0066-lexemes.md) §5). Captions are `term` rows with `entity_id` of the form `M{page ID}` and `WDM{page ID}`; the page projection writes the local ones from `add`, `remove` and `override` of terms on the subject `M{page ID}`, the mirror projection the foreign ones ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §4). `M{page ID}` is the one key form for a page subject across `view` (§4.4).
- **`sitelink`** is the form of [0026](../decisions/0026-sitelinks.md) §6, whose SQL the block above is, with the tenant column of §4.1. `view.registry` holds the policy records and aliases like every config kind. The denied-host filter is applied by composition, so `view.sitelink` holds only permitted links; a policy change re-runs it for the affected hosts, which the `host` index finds.
- **`identifier` holds every normalized external-id value in the resolved view**, keyed so that "which entity has DOI *x*" is one lookup; `entity_id` is the ID as its source wrote it. It serves match keys, tier-3 inferred links and identifier resolution in the title resolver. `match_key` is the subset under a uniqueness constraint; which properties populate it is registry configuration, and an existing violation is reported when the constraint is turned on, not silently dropped.
- **`entity_ref` is keyed on integers.** `page_id` is already a unique `bigint` per entity on a tenant (§4.2), so the row is `(target_page_id, source_page_id, roles)` with `roles` a bitmask of main snak, qualifier and reference, and no statement ID: the resolved JSON answers "which statement" when a panel asks. Its one job is "Links here" and the backlink counter of §4.2; it is not needed for correctness, since a cluster change recomposes the cluster's members and never its referrers (§4.2). It is still the largest table after `record` at Wikidata scale, on the order of 10^9 rows, and the Wikidata-scale profile may serve "Links here" and the count from the query service and omit the table. Both IDs are in source form; a lookup by any member of a cluster is a lookup over the member set ([04](04-entities-and-identifiers.md)). Statements on lexeme parts produce `entity_ref` rows as any statement does ([0066](../decisions/0066-lexemes.md) §5).
- **Lookups by value exclude overlaid entities.** `identifier` by `(property, value_key)`, `sitelink` by `url_key`, `entity_ref` by target and `term` by prefix are the reads over shared rows by a key other than the entity; each carries an index that makes the anti-join of §7 one probe.

### 4.4 Statements, clusters, corrections and constraints

*Sources: [0013](../decisions/0013-postgres-storage.md) §5.3; [0031](../decisions/0031-property-constraints.md) §5; [0038](../decisions/0038-page-metadata-and-categories.md) §10; [0066](../decisions/0066-lexemes.md) §5; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

There is no row per statement. A statement's provenance ([0003](../decisions/0003-statement-ui.md) §6) is derivable for the common case: absent a row, the statement is asserted only by the mirror graph of the entity's provider. Rows exist for the exceptions:

```sql
CREATE TABLE view.statement_assertion (         -- local statements, and statements asserted by 2+ graphs
  statement_id text NOT NULL, graph text NOT NULL, entity_id text NOT NULL, property text NOT NULL,
  member_statement_id text NOT NULL,            -- the member's own ID when fused (0004 §8)
  rank smallint, "offset" bigint NOT NULL,
  PRIMARY KEY (statement_id, graph, member_statement_id)
);

CREATE TABLE view.correction (                  -- local overrides on mirrored assertions (0002 §7)
  statement_id text NOT NULL, entity_id text NOT NULL, kind text NOT NULL,   -- rank | suppress | term
  "offset" bigint NOT NULL, upstream_graph text NOT NULL,
  state text NOT NULL,                          -- active | redundant | dangling (0003 §4)
  PRIMARY KEY (statement_id, kind)
);

CREATE TABLE view.record_statement (            -- "History of this value" (0010 §5.7); local records only
  statement_id text NOT NULL, partition bigint NOT NULL, "offset" bigint NOT NULL,
  PRIMARY KEY (statement_id, partition, "offset")
);

CREATE TABLE view.cluster (                     -- the canonical ID is an attribute of the cluster, never applied to data
  tenant text NOT NULL DEFAULT '', cluster_id bigint NOT NULL, canonical_id text NOT NULL,
  version bigint NOT NULL,                      -- the cluster map version composition records in composed_from
  PRIMARY KEY (tenant, cluster_id)
);
CREATE TABLE view.cluster_member (              -- the map; L0 caches hot entries, nothing assumes it fits in a process
  tenant text NOT NULL DEFAULT '', cluster_id bigint NOT NULL, namespace text NOT NULL, entity_id text NOT NULL,
  PRIMARY KEY (tenant, cluster_id, namespace),  -- one member per namespace (0004 §2)
  UNIQUE (tenant, entity_id)
);
CREATE TABLE view.link (                        -- every link, applied or held (0004 §3, §10)
  tenant text NOT NULL DEFAULT '',
  a text NOT NULL, b text NOT NULL, tier smallint NOT NULL,
  source text NOT NULL,                         -- statement ID, or the local record's offset
  status text NOT NULL,                         -- applied | held
  conflict_id bigint,
  PRIMARY KEY (tenant, a, b, tier, source)
);
CREATE TABLE view.different_from (tenant text NOT NULL DEFAULT '', a text NOT NULL, b text NOT NULL, "offset" bigint NOT NULL, PRIMARY KEY (tenant, a, b));
CREATE TABLE view.property_link (a text NOT NULL, b text NOT NULL, "offset" bigint NOT NULL, PRIMARY KEY (a, b));
```

`cluster_member`'s primary key is the one-to-one rule of [0004](../decisions/0004-identity-clusters-and-equivalence.md) §2 as a constraint. A link that would violate it is held, with `status = 'held'`, and the conflict list ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §10) is a query on `link`. **The cluster tables carry `tenant`** with the `''`-then-tenant lookup of §4.1: the shared rows hold the clusters the shared graphs' tier-2 and tier-3 links form under the instance policy, and a tenant's tier-1 link on `WDQ42`, or its `different-from`, sits beside the shared cluster in the tenant's own rows, so `(tenant, entity_id)` is the unique key of membership. The member-set expansion that every entry point performs ([04](04-entities-and-identifiers.md)) is one indexed lookup here per constant.

**Subjects that are not entities.** `view.statement_assertion.entity_id`, `view.constraint_violation.entity_id` and `view.term.entity_id` hold a page subject in one form across `view`: a File page's subject is its MediaInfo entity, `M{page ID}`, under which its captions and local annotations are stored ([12](12-files-and-media.md)); any other page's subject is its page ID in decimal ([0038](../decisions/0038-page-metadata-and-categories.md) §1). `view.entity_ref` keys both by page ID as it keys every subject (§4.3). In `statement_assertion`, `graph` is `pages` for an asserted statement and `derived` for a projected one. So "Links here" on the item for *cleanup* lists the pages whose statements use it, and constraint reports include pages. Page statements are **not** written to `view.identifier`, `view.match_key` or `view.value_key`, so a resolver ([0029](../decisions/0029-resolver-namespaces.md)) or a match key never lands on a page. `view.statement_assertion` also takes lexeme part IDs as subjects, so a `statement` scope over `P5137` (item for this sense) selects senses; [0060](../decisions/0060-scopes.md) §4 gains `form` and `sense` as subject types alongside `entity` ([0066](../decisions/0066-lexemes.md) §5).

**Constraints.**

```sql
CREATE TABLE view.constraint_violation (
  entity_id text NOT NULL, statement_id text NOT NULL,
  property text NOT NULL, constraint_type text NOT NULL, constraint_statement text NOT NULL,   -- the P2302 statement's GUID
  constraint_source text NOT NULL,             -- the graph the constraint came from
  severity text NOT NULL,                      -- mandatory | normal | suggestion
  snak_role smallint NOT NULL,                 -- 1 main, 2 qualifier, 3 reference
  detail jsonb,                                -- the failing parameter and value, for the popover
  checked_at timestamptz NOT NULL,
  PRIMARY KEY (statement_id, snak_role, constraint_statement)
);
CREATE INDEX cv_entity   ON view.constraint_violation (entity_id);
CREATE INDEX cv_property ON view.constraint_violation (property, constraint_type, severity);
CREATE TABLE view.constraint_count (property text NOT NULL, constraint_type text NOT NULL, severity text NOT NULL, violations bigint NOT NULL, PRIMARY KEY (property, constraint_type, severity));
```

The tenant column rule of §4.1 applies. **The constraint checker is a tier-3 consumer** (§6.1): it consumes composition events, runs in no one's transaction, and is off during bootstrap and bulk modes. Its scope is the `site` setting `constraints.scope`: `local` by default (local-graph statements and entities in the tenant's overlay), `overlay`, or `all`, and `all` is a batch job with progress, never a step; a change to a constraint statement on a mirrored property enqueues a batch rather than a scan. Shared-row violations are computed under the instance's role bindings (§7). Parsed constraints per property are cached in L0 and invalidated by the property's `resolved_version` (§9.3); `type` and `value type` are checked with the L0 class cache, and a `P279` change does not invalidate stored results until the subject's next composition. *Distinct values* on non-identifier data types needs a value index; `view.identifier` already serves `external-id`, and the projection maintains a small `view.value_key (property, value_key, entity_id, statement_id, normalizer_version)` for the other data types a *distinct values* constraint names, populated only for those properties. `value_key` is also populated for the media data types (`commonsMedia`, `localMedia`) when a file repository is configured, so that a file's usage by statement is served from it ([12](12-files-and-media.md)); where no repository is configured, no rows are written for them.

### 4.5 Keyed types, pages, actors and permissions

*Sources: [0013](../decisions/0013-postgres-storage.md) §5.4; [0023](../decisions/0023-moderation.md) §10; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0022](../decisions/0022-federation.md) §10; [0028](../decisions/0028-tenancy-policy.md) §12; [0047](../decisions/0047-special-pages.md) §13; [0051](../decisions/0051-page-redirects.md) §5; [0056](../decisions/0056-security-model.md) §14; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

```sql
CREATE TABLE view.keyed_surrogate (             -- 0009 §7; key is NULLed on erasure
  keyed_type text NOT NULL, surrogate bigint NOT NULL, key text,
  PRIMARY KEY (keyed_type, surrogate), UNIQUE (keyed_type, key)
);
CREATE TABLE view.keyed_map (                   -- 0009 §9: upstream item ↔ key
  provider text NOT NULL, upstream_id text NOT NULL, keyed_type text NOT NULL, key text NOT NULL,
  alias boolean NOT NULL DEFAULT false,         -- an item upstream merged into the owner (0009 A13)
  PRIMARY KEY (provider, upstream_id)
);
CREATE UNIQUE INDEX ON view.keyed_map (keyed_type, key, provider) WHERE NOT alias;  -- one owner per key

CREATE TABLE view.page (                        -- 0008 §4; rows for threads and talk pages too (0019 §11)
  page_id bigint PRIMARY KEY, ns integer NOT NULL, title text NOT NULL,
  content_model text NOT NULL, owner_actor text, -- a content model registry ID (0041 §3, §10)
  latest_offset bigint, latest_revid bigint,    -- NULL while the page ID is only reserved (0023 §2)
  reserved boolean NOT NULL DEFAULT false,      -- create-protection holds the ID (0023 §2, §10)
  deleted boolean NOT NULL DEFAULT false,       -- derived from a read ACL on the page (0023 §4)
  is_redirect boolean NOT NULL DEFAULT false,   -- the latest text begins with a redirect line (0051 §5)
  generation integer NOT NULL DEFAULT 0
);
CREATE UNIQUE INDEX page_title ON view.page (ns, title) WHERE NOT deleted;   -- the title index; reserved rows included
CREATE TABLE view.page_link (                   -- 0008 §10, both directions served by two indexes
  from_page bigint NOT NULL, target_kind smallint NOT NULL, target_id text NOT NULL,   -- 1 entity: its ID; 6 title: '{ns}:{title}', existing or not (0008 A42)
  from_render boolean NOT NULL DEFAULT false,   -- one writer per row class (§6.1): the page projection or the refresh job
  PRIMARY KEY (from_page, target_kind, target_id, from_render)
);
CREATE INDEX page_link_target ON view.page_link (target_kind, target_id);

CREATE TABLE view.actor (                       -- 0007 §4; name and raw are NULLed on erasure
  actor_key text PRIMARY KEY, issuer text NOT NULL, subject text NOT NULL,
  kind text NOT NULL,                           -- registered | temporary | bot | anonymous | imported | provider | federated (0022 §8)
  name text,
  status text NOT NULL,                         -- active | renamed | vanished | hidden | retired (0024 §2) | pending (0025 §3)
  raw text,                                     -- a surrogate's raw value: IP, imported name, remote actor IRI
  operator text,                                -- the operator's actor key for a subsidiary (0024 §1); NULL otherwise
  groups text[] NOT NULL DEFAULT '{}',          -- 0016 §9
  editcount bigint NOT NULL DEFAULT 0,          -- local edit rows, erased included, job rows excluded (0024 §2)
  created_at timestamptz,
  iri text NOT NULL
);
-- The tenant's own accounts: `issuer` is the tenant's derived issuer code (0079 §1), which `local` names.
CREATE UNIQUE INDEX actor_local_name ON view.actor (lower(name)) WHERE issuer = {the tenant's issuer code} AND name IS NOT NULL;
-- Name tombstones (0010 A6) are not here: a vanished account's names are erased
-- from the log, so they live as keyed hashes in private.name_tombstone (§4.16), which survives a rebuild.
CREATE TABLE view.account_link (                -- public links only (0007 §7); a row is deleted on unlink
  local_actor text NOT NULL, foreign_actor text NOT NULL UNIQUE, "offset" bigint NOT NULL,
  PRIMARY KEY (local_actor, foreign_actor)
);

-- The permission tables of 0016 §9. `layer` is `tenant` for a tenant's own partition and
-- `instance` for an instance partition, which is a floor the tenant layer cannot lower (0040 §5).
-- Expiry is stored and evaluated on read; `actor.groups` lists every current membership row.
CREATE TABLE view."group" (                     -- a group's current definition: the registry's group: entry
  name text NOT NULL, permissions text[] NOT NULL DEFAULT '{}', scope text, "offset" bigint NOT NULL,
  PRIMARY KEY (name)                             -- a global group has tenant '' and a scope (0028 §8)
);
CREATE TABLE view.membership (
  actor_key text NOT NULL, "group" text NOT NULL, layer text NOT NULL, expires bigint, "offset" bigint NOT NULL,
  PRIMARY KEY (actor_key, "group", layer)
);
CREATE TABLE view.block (                       -- the latest block per actor and layer; an unblock deletes it
  actor_key text NOT NULL, layer text NOT NULL, removes text[], expires bigint, "offset" bigint NOT NULL,
  PRIMARY KEY (actor_key, layer)                 -- removes NULL is all-but-read
);
CREATE TABLE view.acl (                         -- the current ACL per target; a null record retires it
  target text NOT NULL, restrictions jsonb NOT NULL, read_kind text, extra jsonb, "offset" bigint NOT NULL,
  PRIMARY KEY (target)                           -- extra: parts, reserved, talk, name, members
);
CREATE INDEX acl_confidential ON view.acl (target) WHERE read_kind = 'confidential' OR extra ? 'confidential';   -- the hidden set (§4.14; 0056 A19, A20)
```

**`view.page` gains columns.** [0047](../decisions/0047-special-pages.md) §13 adds four, maintained by the page projection, each with an index with `ns`:

- `len integer`: bytes of the latest text, as MediaWiki's `page_len`;
- `latest_at timestamptz`;
- `revisions integer`;
- `random double precision`, as `page_random`: a function of the page, not a draw, so that a rebuild projects the value the write path did (§6.1). It is the first 52 bits of the MD5 of `{tenant}:{page_id}` as a fraction in [0, 1), uniform as MediaWiki's draw is ([0047](../decisions/0047-special-pages.md) A29).

`is_redirect boolean NOT NULL DEFAULT false` is from [0051](../decisions/0051-page-redirects.md) §5, so `prop=info` and `list=allpages&apfilterredir` need no join. **Reserved page IDs** are rows in `view.page` with `latest_offset` null and a `reserved boolean`; the title index includes them so that `create` finds the ID ([0023](../decisions/0023-moderation.md) §10).

**`view.actor` gains columns and status values.** `operator text` (a local actor key, null for primary accounts) and the status value `retired` ([0024](../decisions/0024-subsidiary-accounts.md) §10); the status value `pending`, which an OAuth-created subsidiary alone takes ([0025](../decisions/0025-oauth-server.md) §8); rows of kind `federated` with `raw` holding the remote IRI ([0022](../decisions/0022-federation.md) §10); `last_active timestamptz` ([0047](../decisions/0047-special-pages.md) §13); `editcount` and `created_at`, maintained by the activity projection from local `edit` rows, erased rows included and job rows excluded, from which the implicit `autoconfirmed` group is computed ([07](07-actors-and-accounts.md)). **The actor key is in the attestation part of a record, never in its header**, except as the key of an actor record itself; the actor tables and `view.activity.actor_key` are projected from the attestation, which is why an attestation cut (erasing that part) leaves the row with no actor. The three instance partitions `actors/{farm}`, `accounts/{farm}` and `log/{farm}` are child tables like any partition; farm accounts are rows in `view.actor` with the farm issuer, and their memberships and blocks in the tables above, with the empty-string tenant that marks the instance; nothing is new in `private`, since farm bindings are bindings and the aggregated bell reads existing inboxes ([0028](../decisions/0028-tenancy-policy.md) §12). The **name registry** is a unique index over the names of farm accounts and of every tenant's accounts, maintained by the actor projection when `identity.shared_names` is on; it is the farm-wide form of `actor_local_name`. Evaluation joins `view.account_link` to reach a tenant account's farm account ([0028](../decisions/0028-tenancy-policy.md) §3–4); L0 caches the result per session.

**An erased actor record still projects a row.** Erasure NULLs `name` and `raw` and sets `status = 'vanished'`; on a rebuild the erased record may be the first for its key, so the row is then created with the default kind, the erased content having held the real one.

Local entity pages get their `page_id` from the same per-tenant sequence as document pages, and mirrored entities take provider-ranged page IDs above 2^40 (§2.3), so `view.entity.page_id` and `view.page.page_id` never collide on a tenant and one `pageid` space covers both kinds of page; the unique key on `view.entity` is `(tenant, page_id)`, since a shared row and a tenant's overlay row for one entity carry the same page ID.

**ACLs and the denormalized flags.** `view.acl` holds moderation ACLs beside graph ACLs, indexed by target; the evaluation path reads it for the target and its enclosures, cached in L0, and the common case, no row, is one indexed miss ([0023](../decisions/0023-moderation.md) §10). Its columns are those of the SQL above, `(target, restrictions jsonb, read_kind, extra jsonb, "offset")` with `PRIMARY KEY (target)`; `target` takes the `tenant` and `set` target kinds of [0056](../decisions/0056-security-model.md) §3 as well, and `read_kind` (`confidential` or `moderation`) is derived from the right that set the record. Denormalized flags keep lists from joining: `view.page.deleted` and `view.entity.deleted boolean NOT NULL DEFAULT false` are derived from `read` ACLs on the target; `view.activity.visibility` and `view.post.hidden` are derived from `record` ACLs together with the erased-parts bitmask of §2.1; `view.actor.status = 'hidden'` from an `actor` ACL.

### 4.6 Activity, jobs and configuration

*Sources: [0013](../decisions/0013-postgres-storage.md) §5.5; [0020](../decisions/0020-change-feeds.md) §6; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0022](../decisions/0022-federation.md) §10; [0028](../decisions/0028-tenancy-policy.md) §12; [0056](../decisions/0056-security-model.md) §14; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §3, §6, §7.*

```sql
CREATE TABLE view.activity (                    -- 0010 §13, 0012 §3: one row per local record, job or event
  time timestamptz NOT NULL, partition bigint NOT NULL, "offset" bigint NOT NULL,
  kind text NOT NULL,                           -- edit | sync | log | job | erased
  source text NOT NULL,
  revid bigint, logid bigint,
  target_kind smallint, target_id text, target_member text,
  actor_key text, operator text, requested_by text, job_id bigint,
  summary_op text, summary_args text, comment text,
  tags text[] NOT NULL DEFAULT '{}',            -- from the attestation part (0030 §5)
  minor boolean, bot boolean, new boolean,
  size integer, delta integer,
  visibility smallint NOT NULL DEFAULT 0,       -- record ACLs and the erased bitmask (0023 §5)
  patrolled boolean NOT NULL DEFAULT false,     -- the patrol projection and the autopatrol rule (0023 §6)
  log_type text, log_action text, params jsonb,
  changes jsonb,                                -- sync summaries (0012 §2.2)
  PRIMARY KEY (partition, "offset")
);
CREATE INDEX activity_time   ON view.activity (time DESC, partition, "offset");   -- recent changes, 0012 §2.3
CREATE INDEX activity_actor  ON view.activity (actor_key, time DESC);             -- contributions
CREATE INDEX activity_target ON view.activity (target_kind, target_id, time DESC);-- history
CREATE INDEX activity_job    ON view.activity (job_id) WHERE job_id IS NOT NULL;
CREATE INDEX activity_unpatrolled ON view.activity (time DESC) WHERE NOT patrolled;  -- the review queue (0023 §10)
CREATE INDEX activity_tags   ON view.activity USING GIN (tags);                     -- tag filters (0047 §4)
CREATE UNIQUE INDEX activity_revid ON view.activity (revid) WHERE revid IS NOT NULL;
CREATE UNIQUE INDEX activity_logid ON view.activity (logid) WHERE logid IS NOT NULL;

CREATE TABLE view.job (                         -- projected from the local log (0011 §6.3)
  job_id bigint PRIMARY KEY, actor_key text NOT NULL, requested_by text,
  source text NOT NULL, source_version text, adapter_version text, mode text, graph text NOT NULL,
  params jsonb NOT NULL, status text NOT NULL, started timestamptz NOT NULL, finished timestamptz,
  counts jsonb, checkpoint_size bigint, sweep_count integer, sweep_threshold integer,
  start_offset bigint NOT NULL, finish_offset bigint
);
CREATE TABLE view.job_reject (job_id bigint NOT NULL, line bigint NOT NULL, match_key text, reason text NOT NULL, PRIMARY KEY (job_id, line));

CREATE TABLE view.registry (                    -- current configuration (0004 §9): providers, issuers,
  tenant text NOT NULL DEFAULT '',              -- '' for instance-level kinds (0018 §3); the sentinel rule above
  kind text NOT NULL, code text NOT NULL,       -- namespaces, keyed types, roles, graph registry
  config jsonb NOT NULL, "offset" bigint NOT NULL,
  PRIMARY KEY (tenant, kind, code)
);
```

`view.activity` is MediaWiki's `recentchanges`, `revision` and `logging` tables folded into one, because [0012](../decisions/0012-api-requirements.md) §3 already folds their rows into one shape. Its row is written in tier 1, in the appending transaction (§6.2). It grows with local activity and the number of jobs, never with the mirrors: a sync of a million entities is one `job` row and one activity row, and a mirrored entity's own history, its mirror records and the upstream log events about it, is `view.entity_history` (§4.2), never rows here. `view.activity.patrolled boolean` with its partial index `WHERE NOT patrolled` serves the recent-changes filter, set by the patrol projection and the autopatrol rule of [0023](../decisions/0023-moderation.md) §6; the autopatrol decision, like every decision the write path takes from another partition's state (the operator of a transfer is the other), is written into the record's attestation part, and the projection reads it rather than reconstructing "as of" across partitions. `view.activity` gains `read_enclosures text[]`, the enclosures (namespace, sets, tenant) whose `read` ACLs govern the row, evaluated against L0 at serve time so that feed and log rows are filtered by principal without a stored group list going stale ([0056](../decisions/0056-security-model.md) §5, §14; §4.14).

**Feeds over `activity`.** The everything, one-target and watched sets are served by `view.activity`'s `activity_time` and `activity_target` indexes, with cluster and thread expansion done before the query. A watch set of tens of thousands of targets, which experienced MediaWiki editors have, is passed as an array parameter and bounded by the period window; this is the join MediaWiki runs on every watchlist load. Sync rows for a watched mirrored entity are read per target from `view.entity_history`, as a history reads them. Related sets are joins against `page_link_target` or `entity_ref` under the same window and the limit of [0020](../decisions/0020-change-feeds.md) §2. `private.watch` is small per account and never scanned across accounts ([0020](../decisions/0020-change-feeds.md) §6). `(time, partition, offset)` is the display order of a paged list; a stream consumer's resume token is a vector of per-partition high-water marks, which offsets are under the append lock, since source time is not commit order ([16](16-logs-feeds-and-notifications.md)).

`job_reject` holds the first N rejects for the job page ([0010](../decisions/0010-site-ui.md) §9); the full rejects file lives on disk or in object storage, and the download route streams it.

`view.registry`'s key is `(tenant, kind, code)` since [0018](../decisions/0018-tenants.md) §3 split the config kinds by scope; instance-level kinds (`key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, and since then `tenancy`, `template`, `consumer`, the instance `sitelink-policy` and `federation-policy` lists, global `group`s, and `primary`, whose single code is `primary` ([0046](../decisions/0046-primary-tenant.md) §2)) have the empty-string tenant. Later ADRs project more kinds into it like every other setting: the rate-limit `site` config records ([0024](../decisions/0024-subsidiary-accounts.md) §10); OAuth consumers and `consumer-policy` ([0025](../decisions/0025-oauth-server.md) §8); `federation-policy` records, with `providers.toml` gaining `trust` (`verified` or `stream`) and the provider's key-chain URL ([0022](../decisions/0022-federation.md) §10); sitelink policy records and aliases ([0026](../decisions/0026-sitelinks.md) §6); the `tenancy`, `template`, `provider-readers` and global `group` kinds ([0028](../decisions/0028-tenancy-policy.md) §12); `view-pin` ([0027](../decisions/0027-preferences-and-portability.md) §5); `resolver` ([0029](../decisions/0029-resolver-namespaces.md) §3); `file-repo` ([0039](../decisions/0039-files-and-media.md) §20).

`provider-readers` ([0028](../decisions/0028-tenancy-policy.md) §5) is tenant scope, a record in the provider tenant's own `config`, not an instance-level kind.

```sql
CREATE TABLE view.tenant (                      -- the cache purge epoch of §9.6 (0056 §7, §14)
  tenant text PRIMARY KEY,
  visibility_epoch integer NOT NULL DEFAULT 0,  -- bumped by a namespace or tenant read ACL only
  live_statement_acls integer NOT NULL DEFAULT 0  -- live statement and property read ACLs; composition skips the lookup at zero
);
```

`view.tenant` is a projection of the tenant's `read` ACL records: one on a **namespace or tenant** target increments the epoch (a `set` change bumps the generation of the members it touches instead, §9.6), and the count of live `statement` and `property` `read` ACLs is kept so that composition skips the per-statement ACL lookup while it is zero, which on every tenant but a private one it is; shared-row composition never looks. A tenant's configuration stays in `view.registry`.

The `view.filter`, `view.filter_hit`, `view.constraint_violation`, `view.constraint_count` and `view.value_key` tables of [0030](../decisions/0030-edit-filters.md) §11 and [0031](../decisions/0031-property-constraints.md) §5 sit beside these (§4.4, §4.13).

### 4.7 Page statements, categories, redirects and tables

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §10; [0051](../decisions/0051-page-redirects.md) §5; [0045](../decisions/0045-table-content-model.md) §12; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

```sql
CREATE TABLE view.page_statements (             -- the resolved statements of a page (0038 §1–2)
  page_id bigint PRIMARY KEY,
  version bigint NOT NULL,                      -- bumped on every change; the cache and ETag key
  statements bytea NOT NULL,                    -- canonical JSON statements, asserted ∪ projected, fused; compressed
  generation integer NOT NULL DEFAULT 0         -- bumped on erasure and hiding (0014 §5)
);

CREATE TABLE view.page_category (               -- categorylinks (0038 §3)
  page_id bigint NOT NULL, category text NOT NULL,   -- the normalized name, without the prefix
  sortkey text NOT NULL,
  from_render boolean NOT NULL DEFAULT false,   -- one writer per row class (§6.1)
  PRIMARY KEY (page_id, category, from_render)
);
CREATE INDEX page_category_members ON view.page_category (category, sortkey, page_id);

CREATE TABLE view.category (                    -- categoryinfo (0038 §4)
  title text PRIMARY KEY, page_id bigint,       -- NULL when the category has no page
  pages integer NOT NULL, subcats integer NOT NULL,
  hidden boolean NOT NULL DEFAULT false
);

CREATE TABLE view.redirect (                   -- MediaWiki's redirect table (0051 §1)
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_ns integer, target_title text NOT NULL,   -- target_ns NULL for an external redirect
  fragment text, interwiki text,                   -- interwiki set for an external redirect
  target_page_id bigint                            -- resolved at projection; NULL when broken or external
);
CREATE INDEX redirect_target ON view.redirect (target_ns, target_title);
```

**Projection order.** `page_category` and `category` run in step 2 of the replay order of §6.1, with `page`, since they read the latest text. The mapping and status projections, and the resolution of `page_statements`, run in step 4. Search and RDF consume pages in tier 3. A text edit's own categories and mapped statements are composed for the written page after commit (§6.2), so an editor who adds a category sees the mapped statement on reload ([0038](../decisions/0038-page-metadata-and-categories.md) §10). **`page_category`, `page_link` and `page_prop` have one writer per row class**, marked by `from_render`: the page projection writes the source-derived rows (what the stored text says) and the refresh job of [11](11-rendering-templates-and-modules.md) writes the expansion-derived rows, each owning its class exclusively; readers union them, and the refresh job never deletes a row it did not write.

**Redirects** are written in step 2 with `page`, because the projection reads only the page's latest text, and for the written page after commit, so a saved redirect works on the next request. `target_page_id` is refreshed when the target is created, deleted, undeleted or moved, by the title-index maintenance; `page_link` needs none, since it records a link's target by title ([0008](../decisions/0008-namespaces-and-document-pages.md) A42). The links projection ([0008](../decisions/0008-namespaces-and-document-pages.md) §10) records a `page_link` row from the redirect to its target, so "What links here" on the target lists its redirects, `list=backlinks&blredirect` expands through them, and `prop=redirects` is a query on `view.redirect`. The title resolver's lookup of `view.redirect` is one indexed read ([0051](../decisions/0051-page-redirects.md) §5).

**Tables** add no table. The definition is a page record's text ([0008](../decisions/0008-namespaces-and-document-pages.md) §4); the table has a `view.page` row with `content_model` `triplespace-table`; its links are `page_link` rows ([0045](../decisions/0045-table-content-model.md) §8). The grid is assembled from the entities' cached resolved views and is not cached as a whole page, since any row's edit would invalidate it ([0045](../decisions/0045-table-content-model.md) §12).

### 4.8 Threads, talk pages, boards and foreign threads

*Sources: [0019](../decisions/0019-discussions.md) §11; [0049](../decisions/0049-boards.md) §12; [0069](../decisions/0069-synchronized-talk-pages.md) §10; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §9.*

Threads and talk pages get rows in `view.page` (namespace, title, content model `triplespace-thread` or `triplespace-talk` ([0041](../decisions/0041-content-models.md) §10), latest offset and revision ID, deleted, generation), so the title index and `page_link` cover them without change. Beside it:

```sql
CREATE TABLE view.thread (
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_kind smallint NOT NULL, target_id text NOT NULL, talk_page_id bigint NOT NULL,   -- the home
  subject text NOT NULL, status text NOT NULL DEFAULT 'open',
  created timestamptz NOT NULL, last_post timestamptz NOT NULL, post_count integer NOT NULL
);
CREATE TABLE view.thread_attachment (
  thread_id bigint NOT NULL REFERENCES view.thread,
  talk_page_id bigint NOT NULL,
  home boolean NOT NULL,
  attached timestamptz NOT NULL,
  attached_revid bigint NOT NULL,          -- the record that attached it here
  pinned timestamptz,                      -- when it was pinned here, if it is (0069 §4)
  PRIMARY KEY (thread_id, talk_page_id)
);
CREATE INDEX thread_attachment_talk ON view.thread_attachment (talk_page_id, attached);
CREATE UNIQUE INDEX thread_attachment_home ON view.thread_attachment (thread_id) WHERE home;
CREATE TABLE view.post (
  revid bigint PRIMARY KEY, thread_id bigint NOT NULL REFERENCES view.thread,
  parent_revid bigint, actor text, published timestamptz NOT NULL, updated timestamptz,
  latest_offset bigint NOT NULL,           -- the record holding the current text
  hidden smallint NOT NULL DEFAULT 0, erased boolean NOT NULL DEFAULT false,
  dt_name text, dt_id text,                -- an imported post's DiscussionTools name and ID (0090 §9)
  original timestamptz                     -- an imported post's original time (0090 §6)
);
CREATE INDEX post_thread ON view.post (thread_id, published);
CREATE INDEX post_dt_name ON view.post (dt_name) WHERE dt_name IS NOT NULL;
CREATE INDEX post_dt_id ON view.post (dt_id) WHERE dt_id IS NOT NULL;
CREATE TABLE view.talk_page (
  page_id bigint PRIMARY KEY REFERENCES view.page,
  target_kind smallint NOT NULL, target_id text NOT NULL, UNIQUE (target_kind, target_id)
);
```

`view.thread_attachment` is the form of [0049](../decisions/0049-boards.md) §12 with the `pinned` column of [0069](../decisions/0069-synchronized-talk-pages.md) §10; [0019](../decisions/0019-discussions.md) §11 gives it with only `(thread_id, talk_page_id, attached)`. `view.thread.talk_page_id` stays, and is the home. The listing query reads `view.thread_attachment`, which replaces the `thread_talk` index. **A board has a `view.talk_page` row** from its creation, with `target_kind` `page` and `target_id` its own page ID, so every query that starts from a talk page works on boards. These are log-replayed projections in the sense of §6, written for the thread or talk page an interactive write touched in the after-commit slot of §6.2, so a posted thread is listed on the next request. An imported post's `dt_name`, `dt_id` and `original` come from its `imported_from`; `Special:GoToComment` finds a post by either index ([0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §9; [13](13-mirrored-pages.md) §5.17).

**Foreign threads** are the projection of the followed talk pages' puts ([0069](../decisions/0069-synchronized-talk-pages.md) §2):

```sql
CREATE TABLE view.foreign_thread (               -- instance scope; projection of the followed talk pages' puts (0069 §2)
  repo text NOT NULL, n bigint NOT NULL,           -- page ID = provider number << 40 | 1 << 39 | n
  name_hash bytea NOT NULL,                        -- 16 bytes of SHA-256 of DiscussionTools' heading name
  talk_upstream_page_id bigint NOT NULL,           -- the talk page it belongs to
  at_upstream_page_id bigint NOT NULL,             -- where it is now: the talk page or an archive subpage
  section integer NOT NULL, pinned boolean NOT NULL DEFAULT false,   -- section 0 is pinned
  state text NOT NULL,                             -- open | archived | removed
  first_comment timestamptz, last_comment timestamptz, comments integer NOT NULL DEFAULT 0,
  partition bigint NOT NULL, "offset" bigint NOT NULL,              -- the put that last changed it
  PRIMARY KEY (repo, n)
);
CREATE UNIQUE INDEX foreign_thread_name ON view.foreign_thread (repo, name_hash);
CREATE INDEX foreign_thread_talk ON view.foreign_thread (repo, talk_upstream_page_id, first_comment);

CREATE TABLE ops.upstream_post (                 -- tenant scope; the send of 0069 §5–6
  id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
  actor_key text NOT NULL, repo text NOT NULL, upstream_user_id bigint NOT NULL,
  talk_upstream_page_id bigint NOT NULL, thread_n bigint, parent_anchor text,  -- NULL thread_n: a new section
  subject text, wikitext text,                     -- dropped once synced, or seven days after failing
  state text NOT NULL,                             -- queued | sent | synced | failed
  upstream_revid bigint, error jsonb,
  created timestamptz NOT NULL, updated timestamptz NOT NULL
);
```

The talk listing reads `view.thread_attachment` and `view.foreign_thread` and merges them by time.

### 4.9 Files

*Sources: [0039](../decisions/0039-files-and-media.md) §20; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4.*

| Schema | Table | Holds |
|---|---|---|
| `view` | `file` | `(tenant, page_id, current_partition, current_offset, sha256, mime, media_type, size, width, height)`: the current version of each file page; gains `mediainfo_id` for the foreign file's `WDM` ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §4) |
| `view` | `file_version` | `(tenant, page_id, partition, offset, revid, action, sha256, sha1, size, mime, media_type, width, height, pages, duration, metadata jsonb, visibility)`, indexed by `sha256` and by `sha1` |
| `view` | `blob` | `(scope, sha256, size, refs, readable_refs, unreadable_since, state)`: the reference count of [0039](../decisions/0039-files-and-media.md) §9, `unreadable_since` for reclamation, `state` `present` or `gone`. The projection is a pure count with no side effect: byte destruction is an `ops` job that takes a hash whose count reached zero, re-checks the count at the live head under a lock, and deletes, so a replay destroys nothing (§6.1) |
| `view` | `file_link` | `(tenant, from_page_id, file_name)`: file usage ([0039](../decisions/0039-files-and-media.md) §13) |
| `view` | `file_block` | `(alg, hash, takedown_id, expunged boolean)`: from `blob` ACLs and `expunge` records ([0039](../decisions/0039-files-and-media.md) §10), read on every upload and every serve |
| `view` | `file-repo` in `registry` | Repository configuration ([0039](../decisions/0039-files-and-media.md) §11) |
| `ops` | `upload_stash` | `(key, tenant, actor_key, sha256, sha1, size, mime, warnings, created, expires)` ([0039](../decisions/0039-files-and-media.md) §5) |
| `ops` | `blob_delete` | Objects queued for deletion by erasure, expunge and the orphan sweep, with attempts, so that a store outage delays deletion and never forgets it |

**Projection order.** The file projection runs in step 2 of §6.1 with pages, after the ACL projection, since `visibility` and `readable_refs` read ACLs; for an interactive write it is applied for the written file page in the after-commit slot of §6.2, so an upload, deletion or takedown is effective on the next request. Metadata extraction ([0039](../decisions/0039-files-and-media.md) §6) runs behind, as the search indexer does, and a version is served before its metadata is ready.

### 4.10 Reports and site statistics

*Sources: [0047](../decisions/0047-special-pages.md) §13; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §6.*

```sql
CREATE TABLE view.report_entry (                -- 0047 §4.3: projection and batch reports
  tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL DEFAULT 'local',  -- graph set (0047 §4.2)
  key text NOT NULL, sort_value bigint, detail jsonb,
  PRIMARY KEY (tenant, report, scope, key)
);
CREATE INDEX report_entry_order ON view.report_entry (tenant, report, scope, sort_value DESC, key);
CREATE TABLE view.report_state (
  tenant bigint NOT NULL, report text NOT NULL, scope text NOT NULL,
  mode text NOT NULL,                           -- projection | batch
  computed_at timestamptz, row_count bigint,
  PRIMARY KEY (tenant, report, scope)
);
CREATE TABLE view.site_stats (                  -- 0047 §9: MediaWiki's site_stats
  tenant text PRIMARY KEY,                      -- text, not bigint: 0013 §5's tenant column
  pages bigint, articles bigint, edits bigint, files bigint, users bigint, active_users bigint, entities bigint
);
```

The `tenant` column of `report_entry`, `report_state` and `site_stats` is `text NOT NULL DEFAULT ''` like every other (§4.1); the SQL above shows it so.

`articles` counts main-namespace pages with at least one link, MediaWiki's default `$wgArticleCountMethod`.

**Projection class.** `report` and `site_stats` are **view-derived** projections (§6.1): entity reports consume composition events, triggering only when a canonical's row changes and the cluster has a local member, with freshness bounded by composition lag and shown beside "computed at"; page and user reports follow `activity` and `page_link`; and a rebuild of any of them is a scan of the `view` tables it reads, never a log replay. The refresh job applies their deltas for the tables it writes ([0047](../decisions/0047-special-pages.md) §4.3). Batch runs and export jobs are `ops` work, beside the export jobs of §4.16.

### 4.11 RDF deltas

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §7.*

```sql
CREATE TABLE view.rdf_delta (
  seq          bigint PRIMARY KEY,         -- the cursor; assigned by the one delta consumer, so insert order is commit order
  tenant       text NOT NULL DEFAULT '',  -- '': a shared-row delta, visible to every tenant that reads its provider (0018 §6, 0013 §5)
  graph        text NOT NULL,             -- resolved | local | mirror/{provider} | metadata
  provider     text,                      -- for a shared delta, the provider whose graph produced it (0032 §4)
  entity_id    text,                      -- the subject entity, in source form; NULL for a metadata-only event
  partition    bigint, "offset" bigint,   -- the record that caused it, where one did
  resolved_version bigint,                -- 0013 §5.1, for resolved deltas
  deleted      bytea NOT NULL,            -- N-Triples (N-Quads under full), canonical order, lz4
  inserted     bytea NOT NULL,
  txn          bigint NOT NULL,            -- the appending transaction; one event per txn (0032 §6)
  time         timestamptz NOT NULL
);
CREATE INDEX rdf_delta_txn ON view.rdf_delta (txn, seq);
CREATE INDEX rdf_delta_tenant ON view.rdf_delta (tenant, seq);
CREATE INDEX rdf_delta_entity ON view.rdf_delta (entity_id, seq);   -- the erasure purge (0032 §5)
CREATE INDEX rdf_delta_record ON view.rdf_delta (partition, "offset");   -- the erasure purge of metadata-only rows

CREATE TABLE ops.tenant_epoch (            -- the stream epoch per tenant
  tenant text PRIMARY KEY,                 -- '' for the shared stream
  epoch bigint NOT NULL DEFAULT 0, rendering_version integer NOT NULL
);
```

- **The delta is a tier-3 consumer** (§6.1), behind the `site` setting `updates.enabled` (default off). It consumes composition events, reads the old state from `view.graph_state` and the previous composed row and the new state from the composed row, renders both to triples and stores the difference. It is computed in no write and in no composition; an instance that does not serve the stream pays nothing for it, and bootstrap and bulk modes produce no deltas.
- **`seq` is the cursor, assigned by a single sequencer.** The one delta consumer assigns `seq` as it writes, so insert order is commit order and a consumer reading `seq > cursor` never skips a row that committed late; it is one sequence for the instance, so every tenant's stream is a filter over one total order, and it can be served from a replica. Consumers apply events in `seq` order and never see two events for one entity out of order.
- **A transaction is one event.** Every row carries the `txn` of the appending transaction whose records caused it (`txn bigint NOT NULL`, indexed with `seq`); the consumer delivers the deltas of one transaction, an edit or one block of a bulk job, as one event whose cursor is the transaction's last `seq`, so a consumer applies an edit atomically and never sees half of it.
- **Retention is a window.** Rows older than `updates.retention` (`site` configuration, default 30 days) are deleted by a daily sweep. A consumer whose cursor has fallen out of the window is told to reload ([0032](../decisions/0032-sparql-update-stream.md) §6). This is how a Kafka-fed updater behaves, with the window as the topic's retention.
- **The table is stored, not recomputed.** A rebuild of `view` **truncates** `rdf_delta` rather than replaying it, since replaying a `latest` mirror would emit all of Wikidata's RDF into a thirty-day window to no purpose; delta production restarts at the rebuild's head, and consumers reload from the post-rebuild dump as they do after bootstrap ([02](02-graphs-rdf-and-query.md)). Every cursor carries an **epoch**: `ops.tenant_epoch` holds one per tenant, and one for the shared stream, bumped by a rebuild, by a change to the tenant's provider list or isolation, and by a base alias, so a tenant that opts in to a provider tells only its own consumers to reload. The epoch carries a **rendering version** that a change to `scatter-wikibase-rdf`, `triplespace-rdf` or a normalizer increments, because `DELETE DATA` matches only byte-identical triples; the lexical forms are pinned as a fixture in `wikibase-compat.md`. A cursor from an earlier epoch is refused with the reload response, as an expired one is.
- **Erasure purge** finds rows by `entity_id` and, for metadata-only rows, by `(partition, "offset")` (§9.6).
- **A delta is never a record.** Nothing here enters the log; the stream is derived, unverifiable and rebuildable, which is what distinguishes it from the verified sync of [0022](../decisions/0022-federation.md) Part A.

### 4.12 Page repositories, mirrored pages, forks, shallow mirrors and derivations

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8; [0053](../decisions/0053-mirrored-pages.md) §8; [0054](../decisions/0054-forking-a-mirrored-page.md) §10; [0070](../decisions/0070-shallow-entity-mirroring.md) §10; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12; [0092](../decisions/0092-following-recent-changes.md) §3.*

```sql
CREATE TABLE view.foreign_title (            -- the title index of a repository (0052 §2–3; filled by 0053 §3)
  repo text NOT NULL,                          -- the repository name; instance scope, shared by every tenant that lists it
  ns integer NOT NULL, title text NOT NULL,    -- the repository's own namespace number and normalized title
  upstream_page_id bigint NOT NULL,
  is_redirect boolean NOT NULL DEFAULT false,
  redirect_ns integer, redirect_title text, redirect_fragment text,
  lastrevid bigint, touched timestamptz,
  PRIMARY KEY (repo, ns, title)
);
CREATE INDEX foreign_title_page ON view.foreign_title (repo, upstream_page_id);
```

The table is instance scope, like `files/{repo}` and the shared entity views (§7): a repository's titles are the same for every tenant, and English Wikipedia's are tens of millions of rows kept once. Which tenants read which repository is `pages.repos`. `view.page` is unchanged: foreign pages have no row in it, and the resolver joins the two. A stack is not cached; its two reads are indexed. A rendered foreign page is cached as [0053](../decisions/0053-mirrored-pages.md) §4 says. The `p:` key of a local page gains nothing, since a local primary renders as before ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8).

```sql
CREATE TABLE view.foreign_page (                 -- the bundles the instance holds (0053 §1), both modes
  repo text NOT NULL, upstream_page_id bigint NOT NULL,
  ns integer NOT NULL, title text NOT NULL,
  revid bigint NOT NULL, revision_time timestamptz NOT NULL,
  content_model text NOT NULL,
  is_redirect boolean NOT NULL DEFAULT false, redirect_ns integer, redirect_title text, redirect_fragment text,
  metadata bytea NOT NULL,                        -- categories, templates, images, langlinks, page properties, sections; compressed
  retrieved timestamptz NOT NULL, expires_at timestamptz NOT NULL,
  partition bigint, "offset" bigint,              -- the mirrored-page record in mirror mode; NULL in proxy mode
  last_read timestamptz,                          -- for mirror.set = on-demand and the proxy cache's grace
  PRIMARY KEY (repo, upstream_page_id)
);
CREATE UNIQUE INDEX foreign_page_title ON view.foreign_page (repo, ns, title);

CREATE TABLE ops.repo_cursor (                   -- a follower's position: an event stream's or a recent-changes poll's (0053 §6; 0092 §3)
  tenant text NOT NULL DEFAULT '',                -- '' for the instance's followers
  kind text NOT NULL,                             -- repo | provider | source
  name text NOT NULL,                             -- the repository, the provider slug or the source name
  last_event_id text, last_event_time timestamptz,           -- an event stream's
  rc_time timestamptz, rc_continue jsonb, rc_seen jsonb,     -- a poll's: newest time seen, continuation, [[rcid, time], …] within follow.tail
  rc_retry jsonb,                                 -- [[entity ID, revid], …] read stale, to read again (0092 §2)
  loaded_to timestamptz, handed_over boolean NOT NULL DEFAULT false,  -- where the last load ended; whether the follower took over (0092 §5)
  late bigint NOT NULL DEFAULT 0, stale bigint NOT NULL DEFAULT 0,    -- the poll's counters (0092 §7)
  lag_seconds integer, updated timestamptz NOT NULL DEFAULT now(),
  PRIMARY KEY (tenant, kind, name)
);
CREATE TABLE ops.page_fetch (                    -- fetch and put queue, deduplicated (0053 §4–6)
  repo text NOT NULL, ns integer NOT NULL, title text NOT NULL, reason text NOT NULL, queued timestamptz NOT NULL,
  PRIMARY KEY (repo, ns, title)
);
```

`view.foreign_page` is instance scope. In `proxy` mode its rows are the pages whose bundles are in the L1 cache (the `fp:` keys of §12.1), and a row is dropped when its bundle is evicted; it is the one `view` table that is not a projection of any log, which the eviction rule and the `proxy` mode's definition ([0053](../decisions/0053-mirrored-pages.md) §4) make deliberate, as [0042](../decisions/0042-template-expansion-and-parsoid.md) §10 made the render tables. In `mirror` mode it is the projection of `pages/{repo}` and is rebuilt from it.

```sql
CREATE TABLE view.fork (                        -- one row per fork, article or dependency (0054 §1, §4)
  page_id bigint PRIMARY KEY REFERENCES view.page,
  repo text NOT NULL, upstream_page_id bigint NOT NULL, upstream_revid bigint NOT NULL,
  upstream_time timestamptz NOT NULL, forked timestamptz NOT NULL, forked_by text NOT NULL,
  kind text NOT NULL,                             -- article | dependency | talk | file-page
  parent_fork bigint,                             -- the article fork a dependency came with
  job_id bigint,
  history_state text NOT NULL,                    -- pending | seeding | complete | partial | metadata-only
  revisions_seeded integer NOT NULL DEFAULT 0, revisions_total integer,
  files_copied timestamptz
);
CREATE INDEX fork_repo_page ON view.fork (repo, upstream_page_id);
```

`view.upstream_revision` takes page subjects as [0054](../decisions/0054-forking-a-mirrored-page.md) §3 says. `ops.fork` is the job queue. `view.page` is unchanged: a fork is a page.

**Shallow mirroring** ([0070](../decisions/0070-shallow-entity-mirroring.md) §10):

- `ops.entity_fetch (tenant, provider, entity_id, reason, enqueued, attempts)` with reasons `read`, `linked`, `closure`, `event`, `sweep`, `manual`; a unique key on `(tenant, provider, entity_id)` so a busy entity is fetched once. `tenant` is `''` for a registry provider; for a tenant's entity source it is the tenant, and `provider` is the source's name ([0078](../decisions/0078-entity-sources.md) §4).
- `ops.repo_cursor` holds a row of kind `provider` per provider's follower and, under the tenant, of kind `source` per entity source's, whether it follows an event stream or polls recent changes ([0092](../decisions/0092-following-recent-changes.md) §3).
- The set itself is `view.entity_source` rows for the provider (`tenant = ''`); no new table.

**Derived statements** ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12):

- `view.derivation (tenant, source, page_id, extractor, upstream_revid, config_rev, offset, subjects)`, one row per key, the projection of `derived/{source}`.
- `view.derivation_subject (tenant, subject, source, page_id, extractor)`, primary key on all five, indexed by `(tenant, subject)`: the reverse index composition reads ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §7) and that `GET /entity/{id}/derivations` serves. **A derivation naming a foreign subject is an overlay trigger** (§7): the tenant's resolved view of `WDQ123` differs from the shared row once a derivation names it, so composition finds the subject's derivations here and writes the tenant's overlay row. The derived statement ID hashes the match key, never the resolved subject, so a later `same-as` that resolves the key to another member of a cluster changes no statement ID and orphans no correction.
- `ops.extraction (tenant, source, page_id, reason, enqueued, attempts)`, unique on `(tenant, source, page_id)`.

The derivation projection runs in step 2 of §6.1, beside `entity_source`, and writes the `derived/{source}` graph's state to `view.graph_state` (§4.2); composition of the subjects it names follows as tier 2.

### 4.13 Edit filters

*Sources: [0030](../decisions/0030-edit-filters.md) §11; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

- `view.filter (id, tenant, version_offset, name, enabled, private, scope, contexts, rule, actions jsonb, exempt_groups, hits bigint, last_hit)`: the projection of the latest record per filter key; the compiled rule is held in L0 per process and rebuilt on change.
- `view.filter_hit (partition, offset, filter_id, filter_version, actor_key, actions, target_kind, target_id, record_partition, record_offset, refused boolean, time)`, indexed by filter and by actor, for the log page and `list=abuselog`; the context variables and attempted content are read from the hit record itself.
- Filters are evaluated in tier 1, the appending transaction (§6.2, after the ACL checks), and hits are appended in the same transaction as the write they concern, or alone for a refusal. A filter's scope excludes job writes by default (`jobs = false`); a filter that opts in sees them, and hits from job writes aggregate into one hit record per (filter, job) carrying a count and the first N record coordinates, as a job's rejects do; the `rate` variable is computed lazily. The `filter` projection runs in step 1 of §6.1, since the write path reads it.
- Throttle counters are `rl:filter:{id}:{key}` in Valkey with the filter's window as TTL.
- The abuse-handling store of [0007](../decisions/0007-actor-identity.md) §3 gains a row per hit `(hit partition, hit offset, ip, expires)`, in `private`, under the store's retention.

### 4.14 Sets and visibility

*Sources: [0056](../decisions/0056-security-model.md) §14; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

| Schema | Table | Serves |
|---|---|---|
| `view` | `set_member (tenant, set_id, kind, id)`, indexed by `(tenant, kind, id)` | The sets a target is in, one lookup per read; written by the ACL projection |
| `view` | `visibility_epoch integer` and `live_statement_acls integer` on `tenant`; the partial index `acl_confidential` on `acl` | The purge epoch of §9.6; the counter that lets composition skip the statement-ACL lookup (§4.6); the per-request hidden set that filters feed and log rows by principal ([0056](../decisions/0056-security-model.md) §5) |
| `view` | `acl` (existing) gains the `tenant` and `set` target kinds and a `kind = confidential \| moderation` column, derived from the right that set the record | The two kinds of [0056](../decisions/0056-security-model.md) §2 |

The visibility set of a target is computed at read time from the enclosure chain: the tenant row, the namespace, the sets `set_member` names, the page or entity, and for a statement its property and entity, which is the one indexed query [0023](../decisions/0023-moderation.md) §2 already describes with two more terms. It is cached in L0 per target version and tenant epoch. **A stored row never holds a group list.** A feed or log row stores nothing about visibility. A list is filtered at serve time by the principal's **hidden set**: the targets of the tenant's confidential `read` restrictions in force that the principal does not satisfy (the tenant, the `pages` graph, namespaces, sets and pages, and the threads whose home talk page those hide), read once per request through the partial index `acl_confidential` and applied to the list query as array parameters, as it is to every lookup by title or ID ([0056](../decisions/0056-security-model.md) A19). Its cost grows with the number of confidential restrictions, not with rows, and a restriction written, changed or expired changes what every row means at once. Where a tenant holds many individually restricted targets, the set is cached in L0 per tenant, ACL offset and the principal's group set; many targets are better restricted by a namespace or a set, one row each, and a set filters by `set_member` rather than by expanding its members. Search documents store a *reference* to their target's enclosures at projection time (`read_enclosures`: the namespace, the sets and the tenant the target is under), and the groups those enclosures require are evaluated at serve time from the L0 ACL cache, so a namespace or set restriction, or one that expires, changes what a stored document means without a rewrite of every document under it (§11.5). The ACL and visibility re-projection of the rows a restriction touches (the `deleted` and `hidden` flags, the search documents) is a tier-3 consumer (§6.1). Nothing is added to `private`; a watch, inbox row or preference is already the holder's alone, and filtering by what the holder may read happens on the public side of the join ([0020](../decisions/0020-change-feeds.md) §3).

### 4.15 Scopes, sprints, schemas, publications and dumps

*Sources: [0060](../decisions/0060-scopes.md) §10; [0061](../decisions/0061-sprints-and-tasks.md) §11; [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8; [0076](../decisions/0076-dataset-publication.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

- `view.scope` and `view.scope_member` as in [0060](../decisions/0060-scopes.md) §5; the scope projection is a tier-3 consumer (§6.1) and the refresh job is in `ops` ([0060](../decisions/0060-scopes.md) §10). **`view.scope_trigger (tenant, trigger_kind, key, scope_page_id)`** is compiled from each scope definition at save, with keys such as `property:P31`, `category:{page}`, `table-row:{page}` and `sitelink`, and held in L0, so a composition event that matches no trigger costs one hash probe. A scope's initial computation is always a job; a truncated scope is "the first N found, refilled by a job when it falls below N", and an `intersection` or `difference` whose non-probeable operand is truncated is refused at save.
- `view.sprint` and `view.task` ([0061](../decisions/0061-sprints-and-tasks.md) §6), a tier-3 consumer following the scope projection; the initial computation as an `ops` job ([0061](../decisions/0061-sprints-and-tasks.md) §11).
- `view.schema_report`: a subject's conformance to an entity schema bound to a scope it is in ([0064](../decisions/0064-entityschema-and-validation.md) §5), written only by the tier-3 validation consumer, never inline; binding a schema to a scope is refused at scope save when bound-schema × scope size exceeds a per-tenant cap an operator may raise. See the catalogue in §5.
- `ops.publication_page (tenant, publication, title, group_key, last_revid, text_hash, written_at, state)` with states `current`, `dirty`, `conflict`; `private.publication_credential (tenant, publication, username, secret)` ([0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8). The routes and the CLI are in [18](18-api.md).
- `ops.dump (tenant, scope, format, generated_at, blob, sha256, members, manifest)`; dumps run as jobs and appear on `Special:Jobs` ([0076](../decisions/0076-dataset-publication.md) §6). The routes and the CLI are in [18](18-api.md).

### 4.16 The `private` and `ops` tables

*Sources: [0013](../decisions/0013-postgres-storage.md) §4; [0021](../decisions/0021-notifications.md) §8; [0022](../decisions/0022-federation.md) §10; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0027](../decisions/0027-preferences-and-portability.md) §8; [0030](../decisions/0030-edit-filters.md) §11; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2, §5, §9; [0092](../decisions/0092-following-recent-changes.md) §10.*

Beside the `private` tables of [0021](../decisions/0021-notifications.md) §3: `private.fediverse_handle` (actor key, handle, actor IRI, inbox IRI, verified, broken, added), `private.email` (actor key, address, verified, added), `private.ap_key` (tenant, key ID, private key, public key, created) and `private.ap_follower` (the notifier's followers: actor IRI, accepted). The last two are keyed by actor, the notifier being one actor among the opted-in ones, and are read by `triplespace-federation` ([0022](../decisions/0022-federation.md) §10, §13); under [0022](../decisions/0022-federation.md) §10 they become per-actor (`actor_key` beside the notifier's tenant row). The delivery queue for email and fediverse messages is one `ops` table, `ops.delivery`, with attempts, next attempt and last error per item. Outbound requests go through the same egress path as live upstream fetches ([0012](../decisions/0012-api-requirements.md) §6); requests an actor initiates, verification messages and handle registrations, are rate-limited in the `notify` class ([0024](../decisions/0024-subsidiary-accounts.md) §5). The `notifications.fediverse_domains` allow and deny lists are site configuration. [0022](../decisions/0022-federation.md) §10 uses the `ops` delivery queue of [0021](../decisions/0021-notifications.md) §8, unchanged.

**`private.api_key`** as [0024](../decisions/0024-subsidiary-accounts.md) §4 describes: `(actor_key, key_id, label, hash, grants text[], ip_ranges cidr[], created, expires, last_used, revoked_at)`, unique on `(actor_key, label)`. Read by `triplespace-accounts` only; sessions in Valkey carry `key_id` so revocation can find them. `docs/registry/grants.toml` lists the grants of 0024 §4 and the permissions each covers; `scatter-actors` embeds it ([0024](../decisions/0024-subsidiary-accounts.md) §10).

**OAuth** ([0025](../decisions/0025-oauth-server.md) §8). Consumers are `config` records and project into `view.registry` like every other setting. `private.oauth_consumer` `(slug, secret_hash, created, rotated_at)` holds confidential clients' secrets. `private.oauth_token` `(actor_key, token_id, consumer_slug, grants text[], refresh_hash, access_hash, issued, access_expires, refresh_expires, last_used, revoked_at)`, unique on `(actor_key, consumer_slug)`, is read and written by `triplespace-accounts`, which stores tokens as it stores keys; `triplespace-oauth` ([0025](../decisions/0025-oauth-server.md) §11) speaks the protocol and calls it, so the set of crates that read `private` is unchanged. `last_used` is written at most once a minute per token, as for keys. Authorization codes and device codes are Valkey keys, `oauth:code:{hash}` and `oauth:device:{hash}`, with the ten-minute TTL as their expiry; they are never rows. An instance without a shared cache holds them, with the request handles of [0025](../decisions/0025-oauth-server.md) §9, in `private` through `triplespace-accounts`, as it holds sessions, so that any replica of an API pool can finish a flow another began ([0057](../decisions/0057-web-tier.md) §13). Sessions opened with a token carry `token_id` beside 0024's `key_id`. The `oauth:{slug}` tag is in the attestation part of each record it applies to ([0030](../decisions/0030-edit-filters.md) §11); no table is added for it. Nothing here enters a header ([0006](../decisions/0006-log-integrity-and-erasure.md) §3): a token, a consumer and a grant are operational state, and the attestation names the subsidiary only.

**Preferences** ([0027](../decisions/0027-preferences-and-portability.md) §8). `private.preference (actor_key, key, value jsonb, updated, PRIMARY KEY (actor_key, key))`. `private.notification_pref` is dropped ([0021](../decisions/0021-notifications.md) §3). `ops` gains the export-job rows for large bundles. Every `private` table carries its portability class ([0027](../decisions/0027-preferences-and-portability.md) §2) in the schema definition. The private extract is not a table; it is produced at move time and destroyed on completion.

**Edit-filter hits.** The abuse-handling store gains a row per hit `(hit partition, hit offset, ip, expires)`, in `private`, under the store's retention ([0030](../decisions/0030-edit-filters.md) §11).

**External identity** ([0088](../decisions/0088-external-identity-providers.md) §3, §5, §7; [07](07-actors-and-accounts.md) §2.7). `private.oauth_client (issuer, client_id, secret, callback, created, updated)`, primary key `issuer`, one consumer per issuer for the whole instance ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §10), the secret sealed under the instance sealing key, "never leaves" under [0027](../decisions/0027-preferences-and-portability.md) §2. `private.oauth_flow (state_hash, tenant, session, issuer, purpose, verifier, name, returnto, door, outcome jsonb, expires)`, a login flow's ten-minute handle where there is no shared cache (`oauthc:{state hash}` in Valkey otherwise), swept as sessions are. `private.binding (tenant, issuer, subject, actor_key, partition, offset, added, last_used)`, unique on `(tenant, issuer, subject)`: the index of bindings, whose truth is the binding records, of a login's external identities (empty tenant, the login as holder, records in `logins`) and of each tenant or farm account's binding to its login (issuer `login`, records in `accounts`); a password binding's index is `private.password`. All three are read and written by `triplespace-accounts` only.

**Upstream logins** ([0092](../decisions/0092-following-recent-changes.md) §10). `private.upstream_login (tenant, kind, name, username, secret, created, updated)`, keyed as `ops.repo_cursor` is, one optional bot password per follower; the secret sealed under the instance sealing key, "never leaves" under [0027](../decisions/0027-preferences-and-portability.md) §2.

**Instance logins** ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2, §5; [07](07-actors-and-accounts.md) §2.8). `private.login (id, name, name_key, created, last_used, partition, offset)`, unique on `name_key`, the index of the `logins` partition's login records, with an instance sequence for `id`; "never leaves" under [0027](../decisions/0027-preferences-and-portability.md) §2. `private.password` is keyed by login. `private.auth_request (handle_hash, tenant, state, challenge, purpose, expires)` holds a tenant's request at the auth host's `/auth/authorize` while the person authenticates, and `private.auth_code (code_hash, login, method, subject, username, tenant, challenge, central, authenticated, expires)` a login code for 60 seconds, each where there is no shared cache (`authr:` and `authc:` in Valkey otherwise), swept as sessions are. `private.session` gains `login` and `central`, the login a session holds and, for a tenant session, the central session it came from.

**Rate-limit configuration** is the `site` config records of [0024](../decisions/0024-subsidiary-accounts.md) §5, projected into `view.registry` like every setting; counters are Valkey keys, never rows ([0024](../decisions/0024-subsidiary-accounts.md) §10).

## 5. The table catalogue

*Sources: [0013](../decisions/0013-postgres-storage.md) §2, §5.1, §5.2, §5.3, §5.4, §5.5, §5.6, §7; [0014](../decisions/0014-caches-and-search.md) §8; [0019](../decisions/0019-discussions.md) §11; [0021](../decisions/0021-notifications.md) §8; [0022](../decisions/0022-federation.md) §10; [0023](../decisions/0023-moderation.md) §10; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0026](../decisions/0026-sitelinks.md) §6; [0027](../decisions/0027-preferences-and-portability.md) §8; [0028](../decisions/0028-tenancy-policy.md) §12; [0030](../decisions/0030-edit-filters.md) §11; [0031](../decisions/0031-property-constraints.md) §5; [0032](../decisions/0032-sparql-update-stream.md) §3; [0033](../decisions/0033-backend-stack.md) §4; [0038](../decisions/0038-page-metadata-and-categories.md) §10; [0039](../decisions/0039-files-and-media.md) §20; [0047](../decisions/0047-special-pages.md) §13; [0049](../decisions/0049-boards.md) §12; [0051](../decisions/0051-page-redirects.md) §5; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8; [0053](../decisions/0053-mirrored-pages.md) §8; [0054](../decisions/0054-forking-a-mirrored-page.md) §10; [0056](../decisions/0056-security-model.md) §14; [0058](../decisions/0058-packed-record-storage.md) §2; [0060](../decisions/0060-scopes.md) §10; [0061](../decisions/0061-sprints-and-tasks.md) §11; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §5; [0069](../decisions/0069-synchronized-talk-pages.md) §10; [0070](../decisions/0070-shallow-entity-mirroring.md) §10; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12; [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8; [0076](../decisions/0076-dataset-publication.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5, §5.1, §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §2, §3, §4, §6, §7; [0087](../decisions/0087-account-registration-and-challenges.md) §3, §6.*

Each table is specified where it is listed; this table is the index, so that the schema has one map. The first rows are the tables [0013](../decisions/0013-postgres-storage.md) itself defines (§2 and §4 of this chapter); the rest are the rows of [0013](../decisions/0013-postgres-storage.md) §5.6, in its order, with the rows that the "Storage" sections of later ADRs add and that 0013 §5.6 does not list marked in the last column.

| Schema | Table | Serves | Specified in | In 0013 §5.6 |
|---|---|---|---|---|
| `log` | `partition`, `record`, `merkle_node`, `checkpoint`, `segment_manifest` | Partitions, records, Merkle nodes, checkpoints, manifests | [0013](../decisions/0013-postgres-storage.md) §2 | 0013's own |
| `view` | `entity`, `entity_source` | The composed view, in source form, and the version cursor per graph; `tenant` on both; `policy`, `composed_from`, `(tenant, page_id)` unique, and the `ref_count` and `revision_counts` counters on `entity`; `seen_job` on `entity_source` | [0013](../decisions/0013-postgres-storage.md) §5.1; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §2 | 0013's own |
| `view` | `graph_state`, `entity_revision` | The current state of every delta graph per key, written in tier 1; the local-graph state after every local revision | [0083](../decisions/0083-write-path-in-three-tiers.md) §4 | **No** |
| `view` | `entity_history` | A mirrored entity's history: one row per mirror record and per upstream log event of every cluster member, written by composition | [0013](../decisions/0013-postgres-storage.md) §5; the SQL is in §4.2 | **No** |
| `view` | `term`, `sitelink`, `identifier`, `match_key`, `entity_ref` | Terms (local, overlaid, and mirrored in the instance's languages), sitelinks, normalized identifiers, match keys, the integer-keyed reverse index | [0013](../decisions/0013-postgres-storage.md) §5.2; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6 | 0013's own |
| `view` | `statement_assertion`, `correction`, `record_statement`, `cluster`, `cluster_member`, `link`, `different_from`, `property_link` | Statement provenance, corrections, value history, clusters and links; `tenant` on the cluster tables, `(tenant, entity_id)` unique on `cluster_member` | [0013](../decisions/0013-postgres-storage.md) §5.3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1 | 0013's own |
| `view` | `keyed_surrogate`, `keyed_map`, `page`, `page_link`, `actor`, `account_link`, `group`, `membership`, `block`, `acl` | Keyed types, pages and links, actors, permissions; `from_render` on `page_link` | [0013](../decisions/0013-postgres-storage.md) §5.4; [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | 0013's own |
| `view` | `activity`, `job`, `job_reject`, `registry` | Activity, jobs, configuration | [0013](../decisions/0013-postgres-storage.md) §5.5 | 0013's own |
| `ops` | `projection_state`, `projection_work` | Projection positions (`applied_offset` the contiguous prefix); the queued work that is the unit of truth for lag | [0013](../decisions/0013-postgres-storage.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | 0013's own |
| `view` | `upstream_revision`, with `tenant`; admits page subjects | Backfilled and observed upstream revisions; the stored half of the upstream-edits fold; seeded revisions of forked pages | [0015](../decisions/0015-record-format-and-partition-registry.md) §4; [0054](../decisions/0054-forking-a-mirrored-page.md) §10 | Yes |
| `view` | `group`, `membership`, `acl`, `block`; `groups text[]` on `actor` | Permissions, ACLs by target and enclosure, blocks | [0016](../decisions/0016-permissions-and-access-control.md) §9; the SQL is in §4.5 | Yes |
| `view` | `thread`, `post`, `talk_page`; rows in `page` for threads and talk pages | Threads, posts and composite talk pages | [0019](../decisions/0019-discussions.md) §11 | Yes |
| `view` | `thread_attachment` | Every attachment of a thread, home or listing; `pinned` when it is pinned there | [0019](../decisions/0019-discussions.md) §11, [0049](../decisions/0049-boards.md) §12, [0069](../decisions/0069-synchronized-talk-pages.md) §10 | **No** |
| `private` | `watch`, `watch_token` | The watch set, its expiry and `seen`, the Atom token; the four auto-watch preferences | [0020](../decisions/0020-change-feeds.md) §3 | Yes |
| `private` | `inbox`, `inbox_state` | Notifications, seen and read state (the reason × channel matrix was `notification_pref`, now preferences) | [0021](../decisions/0021-notifications.md) §3, [0027](../decisions/0027-preferences-and-portability.md) §1 | Yes |
| `private` | `preference` | Every registered preference key, including the notification matrix, auto-watch flags and shape pins; each `private` table carries a portability class | [0027](../decisions/0027-preferences-and-portability.md) §1–2, §8 | Yes |
| `log`, `view` | Instance partitions `actors/{farm}`, `accounts/{farm}`, `log/{farm}`; farm accounts in `actor` with the instance (empty-string) tenant; the farm-wide name-registry index; `tenancy`, `template`, `provider-readers` in `registry` | Farm identity, global groups and blocks, templates | [0028](../decisions/0028-tenancy-policy.md) §12 | Yes |
| `view`, `private` | `filter`, `filter_hit`; a per-hit IP row in the abuse-handling store | Edit filters and their log | [0030](../decisions/0030-edit-filters.md) §11 | Yes |
| `view` | `constraint_violation`, `constraint_count`, `value_key` | Property-constraint violations, derived from the resolved view | [0031](../decisions/0031-property-constraints.md) §5 | Yes |
| `private` | `email`, `fediverse_handle`, `ap_key`, `ap_follower` | Contact details and their verification; the notifier's key and followers and, since [0022](../decisions/0022-federation.md) §6, each opted-in actor's, keyed by actor | [0021](../decisions/0021-notifications.md) §5, §8, [0022](../decisions/0022-federation.md) §10 | Yes |
| `private` | IP blocks | Blocks on addresses before a temporary account exists | [0016](../decisions/0016-permissions-and-access-control.md) §3 | Yes |
| `private` | `password` | `(login, hash, updated)` for the built-in `password` issuer, keyed by instance login ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §3); a portable-class-"re-established" table under 0027 §2 | [0007](../decisions/0007-actor-identity.md) A3 | Yes |
| `private` | `name_tombstone` | `(name_hmac PRIMARY KEY, released)`: a keyed hash of every name a local account has released by rename or vanish, so that no name is ever reused; holds no actor key and no plaintext, survives erasure by design, and is "never leaves" class under 0027 §2 | [0010](../decisions/0010-site-ui.md) A6 | Yes |
| `view` | Moderation ACLs in `acl`; `entity.deleted`, `activity.patrolled`, `page.reserved`; `visibility`, `hidden` and `actor.status` derived from `record` and `actor` ACLs | Protection, deletion, hiding, suppression and patrolling | [0023](../decisions/0023-moderation.md) §10 | Yes |
| `private` | `api_key` | Subsidiary accounts' keys: label, hash, grants, IP ranges, expiry, last used | [0024](../decisions/0024-subsidiary-accounts.md) §4, §10 | Yes |
| `view` | `actor.operator`; status `retired`; `actor.editcount`, `actor.created_at` | The operator of a subsidiary account; the counts behind the implicit `autoconfirmed` group | [0024](../decisions/0024-subsidiary-accounts.md) §1, §2, §10 | Yes |
| `view` | `entity_source.provider_revid`, `entity_source.verified_at_size`; `actor` rows of kind `federated`; `federation-policy` in `registry` | Verified sync from another instance; fediverse surrogates and domain lists | [0022](../decisions/0022-federation.md) §10 | Yes |
| `private` | `oauth_token`, `oauth_consumer` | OAuth tokens as credentials of a subsidiary; confidential consumers' secrets. Authorization and device codes are Valkey keys, not rows | [0025](../decisions/0025-oauth-server.md) §8 | Yes |
| `view` | `consumer` and `consumer-policy` in `registry`; status `pending` on `actor` | Registered OAuth consumers; a subsidiary awaiting approval | [0025](../decisions/0025-oauth-server.md) §2–3, §8 | Yes |
| `ops` | The delivery queue | Email and fediverse deliveries with attempts and backoff; ActivityPub fan-out to followers | [0021](../decisions/0021-notifications.md) §8, [0022](../decisions/0022-federation.md) §7 | Yes |
| `ops` | Export jobs, filter-test jobs, constraint re-check jobs | Asynchronous work that is not a log job: a large user data bundle, a draft filter run over a period, a property-wide constraint re-check | [0027](../decisions/0027-preferences-and-portability.md) §3, [0030](../decisions/0030-edit-filters.md) §6, [0031](../decisions/0031-property-constraints.md) §2 | Yes |
| `view` | `sitelink`, rewritten | Sitelinks as normalized URLs keyed by host, with site aliases; only permitted links are rows | [0026](../decisions/0026-sitelinks.md) §6 (the SQL in §4.3 is the current form) | Yes |
| `view` | `view-pin` in `registry` | Tenant-wide shape pins | [0027](../decisions/0027-preferences-and-portability.md) §5 | Yes |
| `view` | `resolver` in `registry`; no table | Resolver namespaces read `identifier` and `sitelink` | [0029](../decisions/0029-resolver-namespaces.md) §3 | Yes |
| `view`, `ops` | `rdf_delta`; `ops.tenant_epoch` | The deleted and inserted triples of every change to a public graph, in one sequencer-assigned sequence, kept for a retention window and truncated by a rebuild; the SPARQL Update stream and the local quad store read it; the per-tenant epoch and rendering version | [0032](../decisions/0032-sparql-update-stream.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §7 | Yes |
| `view` | `schema_report` | A subject's conformance to an entity schema bound to a scope it is in: `conforms`, the reasons, `depth_limited`, the revisions checked; written after the scope projection, by the subject's write, a scope change, a schema revision or the recheck job | [0064](../decisions/0064-entityschema-and-validation.md) §5 | Yes |
| `view` | `term` rows with `entity_id` of the forms `M{page ID}` and `WDM{page ID}` | Captions: a File page's labels and descriptions, local and mirrored | [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4 | Yes |
| `view` | `entity` rows for `WDM`; `file.mediainfo_id` | The foreign file's MediaInfo entity | [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4 | **No** |
| `view` | `term` kinds 4 lemma, 5 representation, 6 gloss | Lexeme terms, with the part's ID as `entity_id` for representations and glosses | [0066](../decisions/0066-lexemes.md) §5 | Yes |
| `ops` | The cache epoch | An instance-wide counter carried in every L1 key, bumped by every projection rebuild (§9.4) | [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | **No** |
| `ops` | `ap_inbox_seen` | Accepted inbound activity IDs for the replay window | [0022](../decisions/0022-federation.md) §8 | Yes |
| `ops` | `migration` | Each applied migration with a checksum | [0033](../decisions/0033-backend-stack.md) §4 | **No** |
| `view` | `page_statements`, `page_category`, `category`; page subjects (`M{page ID}` for File pages, the decimal page ID otherwise: the one key form of §4.4) in `statement_assertion`, `entity_ref` and `constraint_violation`; `from_render` on `page_category`. `page_category` and `category` run in step 2 of §6.1, page-statement resolution in step 4 | Page statements, asserted and projected; category membership and category info | [0038](../decisions/0038-page-metadata-and-categories.md) §10; [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | Yes |
| `view`, `ops` | `file`, `file_version`, `blob`, `file_link`, `file_block`; `file-repo` in `registry`; `ops.upload_stash`, `ops.blob_delete` | Files and their versions, blob reference counts, file usage, takedown blocks, the upload stash and queued object deletions; the file projection runs in step 2 of §6.1. Blob destruction is an `ops` job that re-checks the live head under a lock with an idempotency key, never a projection step; adoption retirement is the same kind of job ([05](05-providers-and-ingest.md)) | [0039](../decisions/0039-files-and-media.md) §20; [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | Yes |
| `view`, `ops` | `transclusion`, `render_state`; `ops.render_refresh` | What each render read, its epoch and expiry, and the refresh queue; written by the refresh job, not by replay | [0042](../decisions/0042-template-expansion-and-parsoid.md) §10 | Yes |
| `view` | `entity_usage` | Wikibase usage aspects per page and entity | [0043](../decisions/0043-lua-modules.md) §10 | Yes |
| `view` | `report_entry`, `report_state`, `site_stats`; `len`, `latest_at`, `revisions` and `random` on `page`; `last_active` on `actor` | Report pages, live or batch; site statistics; page and user reports | [0047](../decisions/0047-special-pages.md) §4, §13 | Yes |
| `view` | `redirect`; `is_redirect` on `page` | Page redirects and their targets, written in step 2 with `page` | [0051](../decisions/0051-page-redirects.md) §5 | Yes |
| `view` | `foreign_title` (instance scope) | The title index of a page repository, over which the title stack is computed | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8, [0053](../decisions/0053-mirrored-pages.md) §3 | Yes |
| `view`, `ops` | `foreign_page` (instance scope); `ops.repo_cursor`, `ops.page_fetch` | The bundles the instance holds for repository pages, in `proxy` mode the L1 cache's index and in `mirror` mode the projection of `pages/{repo}`; the event cursor and fetch queue | [0053](../decisions/0053-mirrored-pages.md) §8 | Yes |
| `view`, `ops` | `fork`; page subjects in `upstream_revision`; `ops.fork` | Forks and their seeding state; seeded upstream revisions of forked pages; the fork job queue | [0054](../decisions/0054-forking-a-mirrored-page.md) §3, §10 | Yes |
| `view` | `page_prop`, with `from_render` | MediaWiki's page properties: `templatedata` and `defaultsort` in step 2, the rest by the refresh job, each class with one writer | [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §6 | Yes |
| `view` | `set_member`; the partial index `acl_confidential` on `acl`; `visibility_epoch` and `live_statement_acls` on `tenant`; the `tenant` and `set` target kinds and a `kind` (confidential or moderation) on `acl` | The sets a target is in; the hidden set that filters feed and log rows by principal at serve time; the cache purge epoch; the statement-ACL counter; the two kinds of `read` restriction | [0056](../decisions/0056-security-model.md) §14; [0083](../decisions/0083-write-path-in-three-tiers.md) §3 | Yes |
| `log`, `ops` | `storage` on `partition`; `codec` and `dict` on `record`; `fragment_domain`, `fragment`, `dict`; `ops.fragment_candidate` | Packed record storage: domains, fragments, dictionaries and sweep candidates | [0058](../decisions/0058-packed-record-storage.md) §2 | **No** |
| `view`, `ops` | `scope`, `scope_member`, `scope_trigger`; the refresh job in `ops` | Scopes and their membership; the compiled triggers a composition event is probed against | [0060](../decisions/0060-scopes.md) §5, §10; [0083](../decisions/0083-write-path-in-three-tiers.md) §3 | **No** |
| `view`, `ops` | `sprint`, `task`; the initial computation as an `ops` job | Sprints and tasks | [0061](../decisions/0061-sprints-and-tasks.md) §6, §11 | **No** |
| `view`, `ops` | `foreign_thread` (instance scope); `ops.upstream_post`; `pinned` on `thread_attachment` | Synchronized talk pages: foreign threads, the send queue, pinned threads | [0069](../decisions/0069-synchronized-talk-pages.md) §10 | **No** |
| `view`, `ops` | `term` rows of mirrored entities in the instance's `entities.term_languages` only, shallow or full; `ops.entity_fetch`; provider rows in `ops.repo_cursor` | Mirroring: the term budget, the fetch queue and the entity stream's cursor | [0070](../decisions/0070-shallow-entity-mirroring.md) §6, §10; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6 | Yes |
| `view`, `ops` | `derivation`, `derivation_subject`; `ops.extraction`. `derivation` runs in step 2 of §6.1 | Derived statements: one row per page and extractor, the subject index resolution reads, and the extraction queue | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12 | Yes |
| `ops`, `private` | `ops.publication_page`; `private.publication_credential` | Publications: the data pages written to another wiki and their state; the bot password a publication writes with | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §8 | Yes |
| `ops` | `dump` | Scope dumps and their manifests | [0076](../decisions/0076-dataset-publication.md) §6 | Yes |
| `view` | `page_text` | A `tsvector` column with a GIN index, for `list=search` without OpenSearch | [0014](../decisions/0014-caches-and-search.md) §8 | **No** |
| `private` | `account_request`, `challenge_spent`, `rate_event` | Account requests awaiting review: the chosen name and its case-folded key, the credential, the statement, the address key and, once decided, the decision, reviewer, time and reason, deleted 7 days after a rejection or when the request expires, "never leaves" class under 0027 §2; spent challenges `(mac, tenant, expires)` until they expire, and the `createaccount` and `badlogin` counts `(tenant, class, key, at)` until they leave their window, where there is no shared cache ([07](07-actors-and-accounts.md) §2.6, §6.3) | [0087](../decisions/0087-account-registration-and-challenges.md) §3, §4, §6 | **No** |
| `private` | `oauth_client`, `oauth_flow`, `binding` | Login through external issuers: client credentials per issuer, the secret sealed; flow handles where there is no shared cache; the index of external bindings with when each was added and last used ([07](07-actors-and-accounts.md) §2.7) | [0088](../decisions/0088-external-identity-providers.md) §3, §5, §7 | **No** |
| `private` | `upstream_login` | An optional bot password per follower, for `apihighlimits` upstream; the secret sealed | [0092](../decisions/0092-following-recent-changes.md) §10 | **No** |
| `private` | `login`, `auth_request`, `auth_code`; `session` columns | Instance logins: the index of the `logins` partition; a tenant's pending request at the auth host and its login code where there is no shared cache; the login and central session a session records ([07](07-actors-and-accounts.md) §2.8) | [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2, §5 | **No** |

The rows marked **No** are the tables and columns that 0013 §5.6's map of the schema does not yet list.

**Two rules follow from the table.** Every `view` table belongs to a projection of a declared class under §6.1: **log-replayed** (rebuilt by replaying the log) or **view-derived** (rebuilt by a scan of the `view` tables it reads). Three things are pure functions of neither. On a tenant with `wikitext` pages, `transclusion`, `render_state`, `entity_usage`, and the render-owned rows (`from_render`) of the links, categories, page properties and file usage are written by the refresh job and rebuilt by re-rendering, not by replay; where a render read a foreign template repository or the clock, a rebuild can differ ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10, [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6). `view.foreign_page` in `proxy` mode indexes a cache rather than a log, and is the one `view` table that is a projection of nothing ([0053](../decisions/0053-mirrored-pages.md) §8). `view.rdf_delta` is stored, not recomputed: a rebuild truncates it (§4.11). A report configured as `batch` keeps a dated snapshot in `view.report_entry`: a rebuild empties it, and the next scheduled run fills it; projection-backed report rows are rebuilt with the tables they read ([0047](../decisions/0047-special-pages.md) §4.3). The `ops` queues are not projection targets and are never truncated by a rebuild. Nothing in `private` is: the watch set, inboxes and contact details survive on the strength of that schema's backups, and two things in it are not recoverable at all, `seen` on a watch and `read_at` on a notification ([0020](../decisions/0020-change-feeds.md) §3, [0021](../decisions/0021-notifications.md) §3). Each `private` table also carries a portability class in `triplespace-db`'s schema definition, from which the user data bundle and the private extract are generated ([0027](../decisions/0027-preferences-and-portability.md) §2).

## 6. Projections, synchrony and read-your-writes

*Sources: [0013](../decisions/0013-postgres-storage.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §2, §3, §5, §6.*

The write path has three tiers, and everything in `view` is written by one of them. **Tier 1** is the appending transaction: it records, and does only what is a function of the written record and the local graph. **Tier 2** is composition: the pure function that turns the graph states into an entity's fused body, run after commit. **Tier 3** is the consumers of composition: everything that is a function of a composed entity, each on its own queue, never in anyone's transaction. §6.1 names the projections of each tier and the order they replay in; §6.2 is the write path, tier by tier, and what read-your-writes means under it; §6.3 is replica routing.

**The record being applied is not decoded again.** The write path hands the projections the operation it has just encoded into a record's content (`IngestStore::encoded`), and a projection takes that rather than decoding the content it reads, and the record itself rather than reading it back by offset ([0013](../decisions/0013-postgres-storage.md) §7, A75). An operation encodes to one canonical form and decodes back to itself, which the ingest tests check on the records a sync appends.

**A sync writes its units as blocks.** A sync takes its items a unit of `batch` at a time, each unit its own transaction ([0013](../decisions/0013-postgres-storage.md) §7, A76). A unit whose subjects are their own keys, each named once, with no mapped state, no clearing deletion and a provider that owns no keys, is read once for the unit (cursors, the records at them, the revisions their keys claimed, page IDs), decided item by item in memory as the item-by-item path decides, appended together and projected over the block by the set-based path a bootstrap load uses (§6.2; [05](05-providers-and-ingest.md) §3.8). Any other unit is written item by item. Both write the same records, offsets, page IDs and revision IDs. What needs no database runs on every core (A77): a JSON dump of a provider with no key map is parsed and adapted on worker threads and handed over in dump order, and within a block each state's reconciliation, the decoding of its previous record, its decision and the encoding and hashing of its record are spread over the threads, while the unit's reads, append and projections stay on its connection.

### 6.1 Projections, their tiers and their order

*Sources: [0013](../decisions/0013-postgres-storage.md) §7; [0019](../decisions/0019-discussions.md) §11; [0023](../decisions/0023-moderation.md) §10; [0030](../decisions/0030-edit-filters.md) §11; [0031](../decisions/0031-property-constraints.md) §5; [0038](../decisions/0038-page-metadata-and-categories.md) §10; [0039](../decisions/0039-files-and-media.md) §20; [0047](../decisions/0047-special-pages.md) §13; [0051](../decisions/0051-page-redirects.md) §5; [0060](../decisions/0060-scopes.md) §10; [0066](../decisions/0066-lexemes.md) §5; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12; [0083](../decisions/0083-write-path-in-three-tiers.md) §2, §3, §6.*

Every table in `view` belongs to a named projection, and **every projection declares its class.** A *log-replayed* projection is a function of the log and is rebuilt by replaying it from offset 0. A *view-derived* projection (reports, constraints, `site_stats`, scopes, sprints and tasks, schema reports, entity usage) is populated by a scan of the named `view` tables it reads and never by a log replay; "rebuild projection P for tenant T from `view`" is a named operation. A projection added after a mirror exists is view-derived or it does not exist, since bringing a new projection up by replaying a mirror partition would read the whole mirror to produce a few rows about local entities. The projection order is a partial order over the projections a build registers; `ops.projection_state (projection, partition, applied_offset)` has rows for those only, and `applied_offset` records the contiguous prefix each has applied, with the records tier 1 applied inline entered as done. **`ops.projection_work` is the unit of truth for lag**: one row per projection, tenant, record and key of work not yet applied, and lag is what job pages and the entity page's identity line report ([0010](../decisions/0010-site-ui.md) §9).

**One worker applies every queue** ([0083](../decisions/0083-write-path-in-three-tiers.md) §6, A4). The fan-out a projection queues past the synchronous budget and the work of every tier-3 consumer that follows other projections' tables go through that one table and one generic worker (`scatter_projection::Worker`), which drives each consumer the same way, one tenant of one consumer per unit of work: it takes the tenant's lock for that consumer (a transaction-scoped advisory lock; another worker on the same tenant and consumer moves on rather than waits), leases the tenant's oldest items with `FOR UPDATE SKIP LOCKED`, hands the consumer the distinct keys in queueing order, and removes the items in the unit of work that applied them, so a worker that dies mid-batch leaves them queued and the next applies them again, which every consumer tolerates because its work is a recompute of the key, never an increment. A failed batch is rolled back and the attempt recorded on its items (`attempts`, `last_error`, `failed_at`); an item that has failed the attempt limit (five) is left out of every lease and counted as *failed* in the lag report, so one poisoned key stops neither its tenant's queue nor anyone else's. Several workers run as tasks of the serving process (`--workers`, two by default; `0` for a process that only serves) and share the tenants through the locks; a worker needs nothing the serving process has, so it can run as its own process. A read never applies queued work: a page shows the lag and the worker catches up behind it. The lag is reported per consumer and tenant — waiting, failed, and how long the oldest has waited — by `GET /version` (`queues`; [18](18-api.md) §3.2) and on the consumers' own pages, a report's state among them ([21](21-special-pages.md) §2.3).

**The three tiers.** Tier 1's projections are the ones the write path itself reads, and the two state tables: they are functions of the record alone and are written in the appending transaction. Tier 2 is composition and the tables that are functions of the composed body, written by the composer after commit. Tier 3 is every consumer of "entity X, tenant T, composed to version N", each with its own queue, rate and lag, reporting lag on its own pages, running in no one's transaction, and **off during bootstrap and bulk modes** until the operator turns it on. The log-replayed projections replay in this dependency order, and the tier each belongs to is marked:

1. **Tier 1.** `registry`, `keyed_surrogate`, `actor`, and `group`, `membership`, `acl`, `block` ([0016](../decisions/0016-permissions-and-access-control.md) §9, [0023](../decisions/0023-moderation.md) §10), and `filter` ([0030](../decisions/0030-edit-filters.md) §11), since the write path reads all of these; then `graph_state` and `entity_revision` (§4.2) and `activity` with `patrolled` ([0023](../decisions/0023-moderation.md) §6) and `filter_hit` ([0030](../decisions/0030-edit-filters.md) §11), one row each per written record;
2. **Tier 1 or 2 by kind.** `entity_source`, `keyed_map` ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9), `page`, `redirect` ([0051](../decisions/0051-page-redirects.md) §5), `page_prop`, `job`, `upstream_revision` ([0015](../decisions/0015-record-format-and-partition-registry.md) §4), `page_category` and `category` ([0038](../decisions/0038-page-metadata-and-categories.md) §10), `derivation` ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12), the thread tables ([0019](../decisions/0019-discussions.md) §11) and the file projection, after the ACL projection ([0039](../decisions/0039-files-and-media.md) §20): each is a function of one record's content (its latest text, its upload, its put) and is written for the written key in the after-commit slot of §6.2, for everything else by the worker;
3. **Tier 2.** `cluster` and `link`, the map composition reads;
4. **Tier 2.** `entity` (composition), and from the same pass `term` (lexeme terms included, [0066](../decisions/0066-lexemes.md) §5), `sitelink` with the denied-host filter ([0026](../decisions/0026-sitelinks.md) §6), `identifier`, `value_key`, `entity_ref`, `statement_assertion`, `correction`, `entity_history` and page-statement resolution ([0038](../decisions/0038-page-metadata-and-categories.md) §10), each a function of the composed body and written by the composer;
5. **Tier 3.** `page_link`, `record_statement`, then `report` and `site_stats`, after `activity` and `page_link` ([0047](../decisions/0047-special-pages.md) §4.3, §13);
6. **Tier 3.** the addressing projection that fills inboxes ([0021](../decisions/0021-notifications.md) §1), which reads `activity` and writes to `private`; it is the one projection whose target is not `view`, and it runs under the notifier's role;
7. **Tier 3.** the remaining consumers: constraints ([0031](../decisions/0031-property-constraints.md) §2; §4.4), scopes through `view.scope_trigger`, then sprints and tasks, then schema validation ([0060](../decisions/0060-scopes.md) §5, [0061](../decisions/0061-sprints-and-tasks.md) §6, [0064](../decisions/0064-entityschema-and-validation.md) §5; §4.15), entity usage ([0043](../decisions/0043-lua-modules.md) §10), which checks `view.entity_usage` for the canonical ID and every cluster member by index before computing an aspect diff and does nothing while no page uses an entity, the ACL and visibility re-projection (§4.14), the RDF delta ([0032](../decisions/0032-sparql-update-stream.md) §2; §4.11, behind `updates.enabled`) and the search indexer (§11.3).

**One projection may own several tables.** The list above names tables; `term`, `identifier` and the rest of step 4 are written by the composer from the one composition pass, since each is a function of the composed state and splitting them would compose every subject several times. `ops.projection_state` has one row per projection, so their position is `entity`'s. The ACL projection runs in step 1 with the permission tables, since composition, terms, search and activity all read it, and it is applied in tier 1 so that a deletion is invisible the moment it commits ([0023](../decisions/0023-moderation.md) §10).

**No projection has side effects during replay.** `view.blob` is a pure count and byte destruction is an `ops` job (§4.9); proposal adoption writes `view.proposal.state` and enqueues the retirement, and a job appends the `remove` with the proposal thread and upstream revision as an idempotency key in its content part after checking the current composed state ([14](14-discussions.md)). A rebuild re-derives the tables and re-runs no job; the `ops` queues are not projection targets and are not truncated.

**Rebuild by swap, with a cache epoch.** A projection whose tables are large rebuilds into `view_next` and swaps, so the instance serves throughout. Every rebuild bumps an **instance-wide cache epoch**, carried in every L1 key (§9.4), so a key from before the rebuild never matches a body after it. `rdf_delta` is truncated rather than rebuilt (§4.11).

**Replay order.** A rebuild orders partitions by dependency: the `config` partition, then the instance `log` (surrogates, instance jobs), then mirrors, then tenant partitions; composition fails loudly on a keyed subject whose surrogate it cannot find ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §7). Within a tenant, its partitions replay in `(appended_at, partition, offset)` order, with `appended_at` set once per appending transaction, so records one transaction wrote to two partitions (a move over a redirect, [0051](../decisions/0051-page-redirects.md) §2) replay together. The title index is rebuilt with `deleted` applied in a pre-pass over the ACL partition, so no replay order can hit the partial unique index of §4.5. Where the write path decided something from the state of another partition at the time (autopatrol from memberships, the operator of a transfer), the decision is in the record's attestation part and the projection reads it there (§4.6).

### 6.2 The write path: tier 1 records, tier 2 composes, tier 3 consumes

*Sources: [0013](../decisions/0013-postgres-storage.md) §7; [0030](../decisions/0030-edit-filters.md) §11; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §2, §3, §5.*

**Tier 1: the appending transaction.** It does only what is a function of the written record and the local graph, in this order: authenticate the credential and resolve the actor and its effective permissions, including a key's or token's grants ([0024](../decisions/0024-subsidiary-accounts.md) §4, [0025](../decisions/0025-oauth-server.md) §3); rate limit; ACLs on the target and its enclosures; edit filters ([0030](../decisions/0030-edit-filters.md) §1, §11; a refusal appends only the hit record); normalization of every ID to the internal form and, for a Wikibase write, the diff against the tenant's resolved view that drops unchanged content ([18](18-api.md)); the base check against the local graph ([0006](../decisions/0006-log-integrity-and-erasure.md) §8; a mirror advancing is never a conflict); ID allocation; the append; the written graph's new state into `view.graph_state` and, for a local revision, `view.entity_revision`; the activity row; the step-1 projections the write path reads; commit. **The partition lock of §2.1 covers exactly this**, and nothing composes inside it. Read-your-writes for the thing edited is then a matter of composing it.

**Tier 2: composition, after commit.** Composition is the pure function of §4.2, and it runs in four places. *After commit, for the written key*: the write response composes that one entity inline, with no fan-out and no lock held, and returns the body, so an editor who saves and reloads sees their edit, as on MediaWiki; the same slot writes the written page's own rows of step 2. *In the composition worker, for everything else*: the other members of a changed cluster, the overlays of tenants that hold one, the subjects a derivation names, the entities a mirror block touched; work is keyed by entity and **coalesced**, so a mirror `put` and a local edit on one key in the same second compose once. *On miss, on the read path*: a read that finds `composed_from` behind the key's newest graph state composes now and stores the result, which is why a cold read immediately after a sync may pay one entity's composition rather than find a row; the worker racing ahead makes this rare. *Set-based in bootstrap and bulk modes*: one pass over the loaded states rather than one call per entity. A composed row is replaced only when its inputs are newer than `composed_from`, so the inline path and the worker cannot overwrite a newer result with an older one. **L1 is written by the composer after commit**, never by the write path before it (§9.3). The budgets `projections.sync_budget` and `projections.sync_time` bound the after-commit slot: the written key's own composition and rows always run there, and anything beyond them is the worker's. Inbound `maxlag` reports the larger of replica lag and the tenant's local-partition composition lag, so a bot throttles itself during a bulk load ([18](18-api.md)).

**Tier 3: consumers.** After each composed row commits, the composer emits "entity X, tenant T, composed to version N", and the consumers of §6.1 step 5–7 take it from their own queues. Constraint markers, scope membership, report entries, search documents and the RDF delta therefore follow composition by milliseconds for one entity and honestly, as reported lag, for a sync; none of them is ever "on reload". Each consumer reports its lag on its own pages and in `GET /version` ([18](18-api.md)).

**Three write modes.** *Interactive* is the path above, one record per transaction under the lock. *Bulk append into a live partition*, the mode a weekly re-sync, a `linked` → `all` widening or a stream job uses, takes the partition lock once per block (`ops.bootstrap_block` has the shape), `COPY`s the block's records, folds the block's leaves into the tree from the in-memory frontier, writes the block's graph states and commits; tier 2 then composes the block set-based and `ops.projection_state` advances per block; the `put` skip of the version cursor is evaluated in bulk against `view.entity_source` before a block is written, so a re-sync's unchanged entities never reach the lock, and `stream` jobs are one transaction per block, not per entity. *Bootstrap* is for an empty partition ([05](05-providers-and-ingest.md) §3.8), and a provider added to a live instance is loaded that way and then **caught up**: composed against the existing `view`, with its tier-2 links into other providers' entities and the fusion of the clusters it joins queued for the composer. Bulk append and bootstrap are both "tier 1 in blocks, tier 2 set-based, tier 3 off until the operator turns it on".

**Tenant overlays.** Under §7 the shared rows are composed once, under the instance policy, and a tenant's rows only where its own graphs change the result. A record in a tenant partition therefore composes that tenant's overlay of the affected keys; a record in a shared partition composes the shared row and queues the overlays of every tenant that holds one for the key.

### 6.3 Replicas

*Sources: [0013](../decisions/0013-postgres-storage.md) §7.*

Reads may go to streaming replicas. After a write, the session records the commit's WAL position; a read is routed to a replica only when `pg_last_wal_replay_lsn()` on that replica has passed it, and to the primary otherwise; a server for which `pg_is_in_recovery()` is false is the primary whatever its replay position says, since a crash-recovered or promoted primary keeps a stale one. This is MediaWiki's ChronologyProtector, with the LSN in place of the binlog position.

## 7. Shared views and tenant overlays

*Sources: [0018](../decisions/0018-tenants.md) §6; [0013](../decisions/0013-postgres-storage.md) §5; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5, §5.1, §5.2, §6; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §12.*

Everything derived from shared source graphs alone is computed once, for the instance. Everything a tenant's own graphs change is an **overlay** keyed by tenant.

**Shared rows are computed under a named instance policy.** The shared row (`tenant = ''`) of every `view` table is composed under one policy record, the instance's `reconcile` record (`reconcile:default` in the instance `config`), which holds for shared rows the provider order among registry providers, the tier-2 link properties, the normalizer overrides, the role bindings the constraint checker and the adapters read (the `role` kind's instance-scope record), and the languages that populate shared `view.term` (§4.3). **A tenant narrows; it does not reorder.** A tenant's own `reconcile` record may hide a provider in its view and API (a filter on read), restrict its own display languages, and add link properties and ordering for the entity sources it declares ([05](05-providers-and-ingest.md) §6), whose graphs are read per tenant anyway; it may not reorder shared providers, change a shared normalizer or rebind a shared role, and a tenant-scope write that tries is refused with `ts-instance-policy` ([23](23-configuration-and-registry.md)). A tenant whose role bindings differed from the instance's would need an overlay row for everything the difference touches, which on a Wikidata mirror is everything; in practice a tenant on a farm does not rebind the constraint roles. **The shared row names the policy it was computed under**: `view.entity.policy` and the shared rows of its dependents carry the policy record's code. Only `default` exists; *resolution profiles*, a shared view computed once per distinct policy record with tenants bound to a profile and the operator deciding how many the instance affords, can be added later by adding records and rebuilding, with no change to the log.

- **Entities.** `view.entity` and its dependents carry the `tenant` column of §4.1. A row with the empty-string tenant is the shared row for a mirrored entity, composed from the shared graphs under the instance policy. A tenant that has local assertions about the entity, a cluster link touching it, a correction on it, or a derivation naming it as subject (§4.12) gets its own `(tenant, id)` row, composed from the shared graphs plus its own; a tenant with none reads the shared one. This is §4.2's "materialize only where it differs", applied per tenant, and because the stored graph is in source form the overlay holds only what the tenant asserted something about, never what references it. `view.term`, `view.identifier`, `view.entity_ref`, `view.statement_assertion`, `view.correction`, `view.graph_state` and `view.entity_source` follow the same rule; `view.page`, `view.activity`, `view.job` and the actor tables are per tenant outright.
- **The overlay-exclusion rule.** A read by primary key is one lookup with the viewer's tenant and one with `''`. **A read over shared rows by any key other than the entity excludes shared rows whose entity the viewer's tenant has overlaid**: `identifier` by `(property, value_key)`, `sitelink` by `url_key`, `entity_ref` by target, `term` by prefix. The overlay may have suppressed the statement the shared row came from, so the shared row cannot stand for the tenant; each of those tables carries an index that makes the anti-join one probe (§4.3).
- **Clusters.** Tier-2 and tier-3 links come from shared graphs under the instance policy and form the shared clusters (`tenant = ''`); tier-1 links are the tenant's own, and its `different-from` blocks are too, and they sit in the tenant's rows of the cluster tables beside the shared cluster (§4.4). A tenant's clusters are the shared clusters with its overlay applied, recomputed for the members its links touch ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §2) and never for their referrers. A conflict is a conflict for the tenant that holds the links that cause it.
- **Search.** One shared `entities` index per provider, holding that provider's entities as the shared view has them, and one index per tenant holding the tenant's local entities and its overlay documents (§11.4). The overlay projection sets a **`tenant_overlaid`** term on the shared document naming the tenants that have overlaid the entity, and a tenant's query excludes shared documents that name it, so the tenant's document *replaces* the shared one in the index itself, with no post-query dedup to break paging and scoring, and a tenant that suppresses an entity from search is representable. Duplicating a provider's index per tenant is not an option at Wikidata scale, so the overlay is the only shape that works.
- **The quad store** holds one shared resolved graph, the instance's under `default`, plus small per-tenant overlay graphs holding the entities each tenant has overlaid; a tenant's dataset is the shared graph with its overlay shadowing ([02](02-graphs-rdf-and-query.md)). Forty tenants mirroring Wikidata share one copy of its resolved triples.
- **Caching.** Cache keys gain the tenant where the value is tenant-specific and omit it where it is shared, so a mirrored entity's JSON is cached once for every tenant that reads it unchanged (§9.4).

## 8. The blob store

*Sources: [0039](../decisions/0039-files-and-media.md) §3; [0033](../decisions/0033-backend-stack.md) §5.*

**Bytes live in a blob store, addressed by SHA-256.** A substrate crate, `scatter-blob`, defines the store over the [`object_store`](https://crates.io/crates/object_store) crate (Apache Arrow), which gives one interface to a local directory, S3 and S3-compatible services (MinIO, Ceph RGW, Garage, Cloudflare R2), Google Cloud Storage and Azure. Two backends are supported and tested:

| Backend | What it is | When |
|---|---|---|
| `fs` *(default)* | A directory: local disk, a volume mounted into the container, or a mounted network filesystem | Every small instance. It keeps [0033](../decisions/0033-backend-stack.md) §1's deployment: the binary and Postgres, with no new service |
| `s3` | Any S3-compatible API, with endpoint, region, bucket, prefix and credentials | Larger instances, and any instance whose storage is object storage already |

The backend is **deployment configuration**, read from the server's configuration file or environment, not a `config` record ([0015](../decisions/0015-record-format-and-partition-registry.md) §3), because it holds credentials and can differ between a primary and its replicas. Everything else about files is in `config`. The client crates are in the table of §9.2.

**Layout.** Within a **scope** ([0039](../decisions/0039-files-and-media.md) §4), objects are keyed:

| Key | Holds |
|---|---|
| `{scope}/sha256/{h[0..2]}/{h[2..4]}/{h}` | Original bytes, `{h}` the lowercase hex SHA-256 |
| `{scope}/thumb/{h}/{transform}` | Thumbnails and other derived renderings ([0039](../decisions/0039-files-and-media.md) §6) |
| `{scope}/stash/{key}` | Uploads in progress and stashed uploads ([0039](../decisions/0039-files-and-media.md) §5) |
| `{scope}/cache/{repo}/{sha1}/…` | Bytes fetched from a proxied foreign repository ([0039](../decisions/0039-files-and-media.md) §11) |

Objects under `sha256/` are **immutable**: a key's bytes never change, and identical bytes have one key. So the store needs no locks, a move or rename touches no object, deduplication is free, and two servers writing the same file at once write the same bytes to the same key, which is harmless.

**Bytes first, then the record.** An upload streams into `stash/` while the server computes SHA-256 and SHA-1 and enforces the size limit, is validated ([0039](../decisions/0039-files-and-media.md) §5), and is copied to its `sha256/` key unless an object is already there. Only then is the `upload` record appended. A crash leaves at worst an unreferenced object, which the **orphan sweep** deletes after a grace period (`files.orphan_grace`, default 24 hours); it never leaves a record pointing at bytes that were never stored.

**What a network filesystem must provide.** The `fs` backend writes to a temporary name and renames it into place. A mounted filesystem must make that rename atomic within a directory, which NFS, SMB and local volumes do. A FUSE mount of an object store (s3fs, goofys, rclone) generally does not, and an instance on object storage should use the `s3` backend directly.

**The same store serves Scatterbase**, whose blobs are already content-addressed by SHA-256 ([0006](../decisions/0006-log-integrity-and-erasure.md) §2). `scatter-blob` is a substrate crate for that reason ([0039](../decisions/0039-files-and-media.md) §22).

## 9. Cache layers

*Sources: [0014](../decisions/0014-caches-and-search.md) §1, §2, §3, §4, §5; [0056](../decisions/0056-security-model.md) §7.*

### 9.1 Principles

*Sources: [0014](../decisions/0014-caches-and-search.md) §1; [0056](../decisions/0056-security-model.md) §7.*

1. **Keys name versions, not things.** A cache entry is keyed by the entity, page or record *and* the version of the data it was computed from. A write creates a new version and a new key. Old entries become unreachable and expire; nothing is purged on an ordinary edit.
2. **Mutable state lives only in Postgres.** The one mutable question, "what is the current version of X?", is answered by `view.entity.resolved_version` and `view.page.latest_offset` (§4), cached briefly in-process and nowhere else.
3. **Shared caches hold the form computed for a visibility set, keyed by that set.** The visibility set of a target is the `read` restrictions enclosing it ([0056](../decisions/0056-security-model.md) §2); an entry computed under set *V* is served to any principal that satisfies *V*, so a private wiki of fifty members caches as well as a public one, and the **public form**, the response as an anonymous reader sees it with hidden fields removed and erased bodies absent, is the entry for the empty set. Only the empty set's entries reach L2 and the proxy. Administrators' moderation views, which include hidden fields and erased gaps' reasons ([0012](../decisions/0012-api-requirements.md) §8), bypass every shared layer, as do `/account` and `/auth`. Nothing from a `private` graph is ever cached outside the holder's session.
4. **Erasure purges; everything else expires.** Erasure, upstream hiding ([0011](../decisions/0011-logs.md) §5), and deletion and hiding by `read` ACL ([0023](../decisions/0023-moderation.md) §5) are the only events that reach into caches. Every entry also has a TTL ceiling, so a purge that is missed is bounded in time.
5. **A small instance runs with no shared cache and no search service.** The layers below are optional above the in-process cache; the code paths are the same, with the shared layer absent and the Postgres fallback (§11.6) in place.

### 9.2 Layers

*Sources: [0014](../decisions/0014-caches-and-search.md) §2; [0033](../decisions/0033-backend-stack.md) §5.*

| Layer | Technology | Holds | Bounded by |
|---|---|---|---|
| **L0, in-process** | An in-memory LRU in each server process (`moka`) | Registries and role maps; property data types; the canonical ID of each cluster member; labels in the instance's hot languages; current versions of recently read entities and pages; the additions of §12.2 | Size, and a TTL of seconds for current versions |
| **L1, shared** | Valkey (Redis protocol) | §12.1: resolved entity JSON, term batches, rendered pages, provenance responses, structured diffs, suggestions, upstream fetches. Also rate-limit counters and sessions. | Per-key TTL ceilings (§12.1) |
| **L2, HTTP** | `ETag` and `Cache-Control` on anonymous responses, honoured by the reverse proxy and CDN in front of the instance | Whole responses | Short `s-maxage`, `stale-while-revalidate`, purge by tag on erasure |
| **L2, web tier** | The response cache of `triplespace-web`, in each process (`moka`) or in a Valkey its configuration names (§10.2) | Public API responses to the requests it sends without credentials | Revalidated with `If-None-Match` on every use, except responses whose `max-age` the API sets because no purge can change them, kept for up to 60 seconds; never purged, so it never holds what a purge or an erasure has changed beyond the next request |
| **Replicas** | Postgres streaming replicas | Everything in `view` | LSN routing (§6.3) |

L1 is the layer MediaWiki fills with memcached. Valkey is chosen over memcached for one reason: keys can be enumerated by prefix, so an erasure can delete every entry for an entity rather than rely on generation keys alone (§9.6). Session state lives in Valkey (`s:{session}`) when a shared cache is configured and in `private` otherwise, through `triplespace-accounts` ([0025](../decisions/0025-oauth-server.md) §8); that fallback is what 0013 §4's "sessions' secrets" in `private` refers to. OAuth tokens and bindings always stay in Postgres `private` (§1.2).

**Clients** ([0033](../decisions/0033-backend-stack.md) §5):

| Service | Client | Notes |
|---|---|---|
| In-process cache (L0) | `moka` | As 0014. |
| Valkey (L1) | `redis` (redis-rs) with its tokio connection manager | Works against Valkey unchanged. Prefix deletion for erasure (§9.6) uses `SCAN` + `UNLINK`. |
| OpenSearch | `reqwest` with typed request and response structs | The surface used (index, bulk, msearch, aliases, `version_type: external`) is small; a typed client module in `triplespace-search` is easier to keep current than the official client crate. |
| Blob storage | `object_store` (Apache Arrow), local-filesystem and S3 backends, in `scatter-blob` | §8 |
| Thumbnails | `image` for raster formats, `resvg` for SVG, in `scatter-files` | [0039](../decisions/0039-files-and-media.md) §6 |

### 9.3 Versions and `ETag`

*Sources: [0014](../decisions/0014-caches-and-search.md) §3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §2.*

Every cacheable thing has one version number, and the key carries it.

| Thing | Version | Where it comes from |
|---|---|---|
| Resolved entity | `resolved_version` | Drawn from one instance-wide sequence by the composer on every composition, including one caused by a change to another member of the entity's cluster (§4.2); never by a referrer's change |
| Document page | `latest_offset` | The page's newest record (§4.5) |
| Record-derived data: a diff, a change set, a proof | `(partition, offset)` | Immutable by construction |
| Label batch | The max `resolved_version` of the entities in the batch | Computed at lookup |
| Rendered page HTML | `latest_offset`, the render epoch, the renderer version ([0012](../decisions/0012-api-requirements.md) §4, `action=parse`), and the interface language | The epoch is bumped when anything the render read changes, and the entry's lifetime is capped by the render's `expires_at` ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10) |

**`ETag` is the version.** An entity response carries `ETag: "Q42:<resolved_version>:<generation>"`. `If-None-Match` is answered from L0's current-version cache without touching Postgres when the version is unchanged. The version also appears in the JSON response body, so a client that has just written can wait for the version its write returned.

**Read-your-writes** follows from §6.2. The write transaction commits; the composer composes the written entity after commit, takes its new version, writes the new resolved JSON into L1 and returns the body, so the response reports the version and the editor's next read hits. **L1 is written by the composer after commit, never by the write path before it**, so a rolled-back transaction leaves nothing in the cache. Replica routing by LSN covers the Postgres side.

**L1 holds the internal form, and the rewrite runs after the cache.** A cached entity body is in source form, once per entity version. The layer that rewrites entity IDs to the form the consumer prefers (`prefer`, [18](18-api.md); [04](04-entities-and-identifiers.md)) is applied to a hit on the way out, so there is one cache entry per version and not one per preference; `ETag` is the version of the stored form, and two consumers with different preferences share it.

### 9.4 Key structure: generation, version, tenant and visibility

*Sources: [0014](../decisions/0014-caches-and-search.md) §4, §10; [0018](../decisions/0018-tenants.md) §6; [0056](../decisions/0056-security-model.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

The keys themselves are catalogued in §12.1. `{gen}` is the entity's or page's `generation` (§4.2, §4.5), which changes only on erasure or hiding (§9.6). `{ver}` is the version of §9.3.

**The cache epoch.** Every L1 key begins with the instance-wide **cache epoch**, `c{epoch}:`, a counter in `ops` that every projection rebuild bumps (§6.1), so a key written before a rebuild never matches a body after it, whatever its version says; §12.1 writes the keys without it. It is distinct from a tenant's visibility epoch below and from the stream epochs of §4.11.

**Tenant.** Keys gain a tenant component where the value is tenant-specific, and omit it where the value is shared, so a mirrored entity's JSON is cached once for every tenant that reads it unchanged ([0018](../decisions/0018-tenants.md) §6).

**Visibility.** Every key whose value depends on what the viewer may read also carries a **`{vis}`** segment, written after `{gen}`: for a non-empty visibility set, a short hash of the sorted group names of the set together with the tenant's visibility epoch; for the empty set, the public form, **`-{epoch}`** on a tenant key, so that a public entry cached before a namespace restriction lapses with it, and plain `-` on a shared key, which carries no tenant epoch because a shared row is under no tenant's namespace. That is `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:`, `post:`, `th:`, `tp:`, `fi:`, `rf:`, `ld:`, `sc:` and `sp:`; `s:`, `rl:`, `up:` and `fp:` (a repository bundle is public upstream content) do not carry it. §12.1 marks each key. **Only a `namespace` or `tenant` `read` ACL bumps the tenant epoch**, and every entry under the tenant lapses at once (§9.6); a `set` change bumps the generation of the members it touched instead, so adding one page to a confidential set does not cold the tenant's cache. The required privacy test is: an entry cached under the empty set before a namespace restriction is not served after it. A page's render carries the set of its own visibility, which by Rule 2 of [0056](../decisions/0056-security-model.md) §6 bounds every include.

**Stampedes** are handled per process: concurrent requests for one missing key share one computation (single flight), and a short lock key in L1 keeps two servers from resolving the same large entity at once.

### 9.5 L0 additions and the fragment cache

*Sources: [0014](../decisions/0014-caches-and-search.md) §10; [0058](../decisions/0058-packed-record-storage.md) §11.*

Later ADRs add to L0 without adding a layer; the entries are all small and all invalidated by the record that changes them, and §12.2 lists them. The one with its own mechanics is the **fragment cache** of packed storage. Each process keeps an L0 LRU of fragments by `(domain, ref)`, holding their decompressed bytes, together with a set of refs known to be `shared` in the current epoch, which §3.4 uses to skip writes.

- **The known-shared set is dropped whenever the domain's `epoch` differs from the one it was built under.** Every append reads the epoch (§3.4 step 1), so a writer never relies on a ref that a sweep has since deleted.
- **The LRU drops a domain's entries on `NOTIFY ts_fragment`,** and on erasure that is the seventh erasure step of §9.6. Entries also expire after the L0 ceiling, which bounds a missed notification, as the TTL ceilings bound a missed purge.

### 9.6 Erasure, hiding and visibility changes reach every layer

*Sources: [0014](../decisions/0014-caches-and-search.md) §5, §10; [0056](../decisions/0056-security-model.md) §7; [0039](../decisions/0039-files-and-media.md) §20; [0058](../decisions/0058-packed-record-storage.md) §11.*

When an `erase` record is appended ([0006](../decisions/0006-log-integrity-and-erasure.md) §7), an upstream hiding is followed ([0011](../decisions/0011-logs.md) §5), or a `read` ACL deletes or hides something ([0023](../decisions/0023-moderation.md) §5), the projection that removes the derived data also:

1. **bumps `generation`** on every affected entity and page, so every existing key for them is unreachable at once;
2. **deletes the old keys** in L1 by prefix (`e:{id}:*`, `t:*:{id}:*`, `prov:{id}:*`, `p:{pageid}:*`, `d:{partition}:{offset}:*`), so the bytes leave memory rather than wait for the ceiling;
3. **purges L2 by tag.** Every anonymous response carries a cache tag for each entity and page it is about (`Cache-Tag: entity:Q42, page:17`; label sources are not tagged, §10.3); the purge names the tags. A proxy or CDN that cannot purge by tag gets a shorter `s-maxage` in configuration instead;
4. **deletes the search documents** (§11.3);
5. **deletes the inbox rows** whose activity was hidden or erased, in every inbox ([0021](../decisions/0021-notifications.md) §2);
6. **deletes or rewrites the `rdf_delta` rows** within the retention window whose triples include the erased ones, found by entity and, for metadata-only rows, by the record's `(partition, offset)` (§4.11), and emits the deleting delta ([0032](../decisions/0032-sparql-update-stream.md) §5);
7. in a packed partition, **sweeps the erased parts' fragments** and drops the domain's fragment cache in every process (§3.7, §9.5).

The end-to-end erasure test of 0014's Consequences covers all of these. A rename or vanish ([0007](../decisions/0007-actor-identity.md) §4) bumps the generation of the actor's user pages and of every activity row's actor display, which is served uncached in any case. The TTL ceilings in §12.1 are the bound on a missed purge. An instance under a legal deadline lowers them for the duration.

**Deletion and hiding** ([0023](../decisions/0023-moderation.md) §4–5) take the same path: a `read` ACL on a page, entity, record, actor, statement or property bumps the target's generation, purges its keys and tags, deletes or re-sends its search document and clears its inbox rows, because the shared layers hold only the public form (§9.1) and the public form has changed; a `property` read ACL bumps every entity the property appears on, found through `view.statement_assertion` and the resolved JSON, which is the one moderation action with a fan-out. Protection purges nothing. A **sitelink or federation policy change** ([0026](../decisions/0026-sitelinks.md) §3, [0022](../decisions/0022-federation.md) §8) recomposes the affected entities through the `host` index and gives them new versions, not their generations, since nothing was hidden for privacy.

**Visibility changes** ([0056](../decisions/0056-security-model.md) §7). A `read` ACL written, changed or retired on target *T*, of either kind, bumps *T*'s **generation**, as hiding does above, since both kinds of read restriction hide for privacy and no entry at any version may remain reachable; a version bump is for changes that hide nothing. On a **namespace or tenant**, it bumps the tenant's **visibility epoch**, a counter in `view.tenant` that is part of every `{vis}` segment for that tenant, the public `-{epoch}` included (§9.4), so that every entry under the enclosing target lapses at once; on a **set**, adding or removing a member bumps the generation of the members touched and nothing else. The trade is coarse purges for the two targets whose ACLs change rarely, and exact ones for the one that changes often. Both run through the same path as erasure, including the inbox purge; the re-projection of stored flags and search documents is a tier-3 consumer (§4.14).

**Files** ([0039](../decisions/0039-files-and-media.md) §20). Deletion, hiding, takedown and erasure take the purge path above and also purge the Cache-Tags `blob:{scope}:{h}` and `file:{tenant}:{page id}` on every media response. **Rendering** ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10, [0043](../decisions/0043-lua-modules.md) §6): every rendered page carries a `Cache-Tag` for each page and entity in its manifest, and erasing, deleting or hiding a dependency bumps the epoch of each page that read it and purges their tags.

### 9.7 What is not cached

*Sources: [0014](../decisions/0014-caches-and-search.md) §4, §10; [0045](../decisions/0045-table-content-model.md) §12; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8.*

Activity lists, histories and contributions are served from `view.activity` with continuation, and are not cached above L2: their pages change with every edit and their cost is one index scan. Anything under `/account` and `/auth` is `no-store` ([0012](../decisions/0012-api-requirements.md) §8) and never enters L1 or L2.

**Never cached above L2,** in addition: the watchlist and every response under `/watchlist` ([0020](../decisions/0020-change-feeds.md) §5), the inbox and everything under `/inbox` and `/notifications` ([0021](../decisions/0021-notifications.md) §7), everything under `/account`, including preferences, subsidiaries, keys, authorizations and the data export ([0024](../decisions/0024-subsidiary-accounts.md) §8, [0025](../decisions/0025-oauth-server.md) §9, [0027](../decisions/0027-preferences-and-portability.md) §6), all of which are `Cache-Control: private` or `no-store`. The everything feed's page and Atom forms are public and cache briefly at L2 ([0020](../decisions/0020-change-feeds.md) §5). The stream ([0020](../decisions/0020-change-feeds.md) §4) is not cached; it is a fan-out from the activity projection. Constraint reports and the filter log are feeds or lists over `view` and follow the activity-list rule. The SPARQL Update stream and `/updates` ([0032](../decisions/0032-sparql-update-stream.md) §6) are range reads over `view.rdf_delta` and are not cached either; its batch files under `/dumps/updates/` are immutable once written and cache like dumps. Responses to signed media URLs are `private, no-store` ([0039](../decisions/0039-files-and-media.md) §20).

A table page's grid is assembled from the entities' cached resolved views and is not cached as a whole page, since any row's edit would invalidate it ([0045](../decisions/0045-table-content-model.md) §12). A title stack is not cached; its two reads are indexed ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8).

## 10. HTTP and response caching

*Sources: [0014](../decisions/0014-caches-and-search.md) §6; [0057](../decisions/0057-web-tier.md) §5, §6.*

### 10.1 HTTP caching of API responses

*Sources: [0014](../decisions/0014-caches-and-search.md) §6; [0056](../decisions/0056-security-model.md) §7.*

For anonymous requests:

- `Cache-Control: public, max-age=0, s-maxage=60, stale-while-revalidate=300`, and `ETag` as in §9.3. Readers see at most a minute of staleness through a proxy, and none when they revalidate, because `If-None-Match` is cheap.
- `Vary: Accept, Accept-Language`. Cookies are absent on anonymous requests, so `Vary: Cookie` is not needed; an authenticated request is never served from L2.
- `Cache-Tag` as in §9.6.

MediaWiki purges its CDN on every edit because its `s-maxage` is long. Triplespace keeps `s-maxage` short and purges only on erasure. A proxy that can purge by tag may raise `s-maxage`; the default is chosen so that a proxy that cannot is still correct.

For authenticated requests: `Cache-Control: private, no-cache`, with `ETag` still present so the client revalidates. Only a response with empty visibility carries public `Cache-Control`; every other response is `private, no-cache` with an `ETag`, as authenticated responses already are ([0012](../decisions/0012-api-requirements.md) §8). The proxy and the CDN therefore never hold a restricted form; L1 may, because it is inside the boundary ([0056](../decisions/0056-security-model.md) §10).

Media bytes are the exception to the short `s-maxage`: a public file version is served with a long one, because any change that makes it unreadable purges its tags ([0039](../decisions/0039-files-and-media.md) §7; §9.6).

### 10.2 The web tier's response cache

*Sources: [0057](../decisions/0057-web-tier.md) §5.*

The web tier may cache API responses that are the same for everyone, in its own memory (`moka`) or in a Valkey named by `web.cache.valkey_url`. The CDN in front of it is purged by tag (§9.6) and the web tier's cache is not, so the cache never serves a response the API has since changed: a cache that cannot be purged, beneath one that can, would refill the CDN with what a purge or an erasure had just removed.

- **Only credential-free requests consult it.** A request the web tier sends without `Cookie` or `Authorization` may use the cache; any other request goes to the API.
- **Only public responses enter it.** A response with `Cache-Control: public`, an `ETag` and no `Set-Cookie`, keyed `web:{host}:{hash of method, path, query and Accept-Language}`.
- **Every use revalidates.** The web tier sends the cached `ETag` in `If-None-Match`; the API answers `304` while the response is current, and a new version, a purge ([0057](../decisions/0057-web-tier.md) §14) or an erasure changes the `ETag` (§9.3). The cache saves the API building and sending the body and the web tier parsing it, not the round trip.
- **Except what no purge can change.** A response whose `max-age` is greater than zero is used without revalidation for that long, never longer than `web.cache.max_ttl`, whose default and ceiling is 60 seconds, the default `s-maxage` of §10.1, whatever `s-maxage` a deployment configures. The API sends such a `max-age` only on responses that purging, erasure and moderation never change: `meta=siteinfo`, the registries and message bundles. The web tier never honours `s-maxage`, which is set for proxies that can be purged.
- **The Valkey is held to [0056](../decisions/0056-security-model.md) §10 line 3,** `AUTH` or a private address, whether it is the instance's L1 Valkey or a separate one, and `instance check` covers it.
- **The embedded site uses no response cache;** the API's own layers (§9.2) already serve it.
- **Losing it costs only latency.** The replicas stay interchangeable.

A notice after a redirect ("Your edit was saved") travels as a short-lived cookie holding a message key from a fixed set, as MediaWiki's `postedit` does; it needs no signing and is not state.

### 10.3 A page's own cacheability

*Sources: [0057](../decisions/0057-web-tier.md) §6; [0014](../decisions/0014-caches-and-search.md) §6; [0085](../decisions/0085-settings-and-preferences.md) §2.2; [0097](../decisions/0097-versioning-and-skins.md) §15.*

A page or fragment is composed from several API responses, and its HTTP caching follows from theirs:

- **It is public only if every response it used was public;** otherwise `private, no-cache`. This is [0056](../decisions/0056-security-model.md) §6 rule 1 applied to HTTP.
- **Its `ETag`** hashes the web tier's build ID, the interface language and the `ETag`s of the responses it used. The build ID names the binary's build and the skin that drew the page, with its version ([0097](../decisions/0097-versioning-and-skins.md) §15).
- **Its `Cache-Tag`s are exact and bounded.** A composed page carries a tag for each of its subjects (the entity or page it is about) and for each page in its render manifest, and nothing else: label dependencies are not tagged, since a large item draws labels from thousands of entities and a tag per label source would exceed every proxy's header limit and let a mirror sync purge the whole site. Label staleness is accepted up to `s-maxage`, as MediaWiki accepts it in its label cache; the purges of §9.6 reach the page through its subject and manifest tags.
- **It varies on `Cookie` and `Accept-Language`**, and the web tier's public cache keys it by the negotiated interface language rather than the raw header, so content negotiation does not split the cache by every browser's `Accept-Language` ([0085](../decisions/0085-settings-and-preferences.md) §2.2).
- **A token never appears in public HTML.** Anonymous pages fetch `meta=tokens` from their components when they need one; a logged-in page is `private` and may carry its tokens in its data blocks.

A region fetched with `action=render` is cached the same way, with its page's tags. The About panel of an entity page has a response of its own, with its own short `max-age`, since its counters (§4.2) change without the entity's version changing; a page's budget of API calls to first byte and the bulk-labels route that meets it are in [18](18-api.md) and [20](20-web-tier.md).

## 11. Search

*Sources: [0014](../decisions/0014-caches-and-search.md) §7, §8, §10.*

### 11.1 Indexes and fields

*Sources: [0014](../decisions/0014-caches-and-search.md) §7; [0019](../decisions/0019-discussions.md) §11; [0038](../decisions/0038-page-metadata-and-categories.md) §12; [0039](../decisions/0039-files-and-media.md) §19; [0042](../decisions/0042-template-expansion-and-parsoid.md) §17; [0045](../decisions/0045-table-content-model.md) §12; [0049](../decisions/0049-boards.md) §12; [0051](../decisions/0051-page-redirects.md) §5; [0053](../decisions/0053-mirrored-pages.md) §7; [0060](../decisions/0060-scopes.md) §10; [0061](../decisions/0061-sprints-and-tasks.md) §11; [0064](../decisions/0064-entityschema-and-validation.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0066](../decisions/0066-lexemes.md) §5; [0069](../decisions/0069-synchronized-talk-pages.md) §10; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §2, §5.1.*

**Two indexes, one query.**

| Index | One document per | Fields |
|---|---|---|
| `entities` | Entity in the resolved view, under the ID its source wrote: local, foreign and keyed, of every type | `id`, `type`, `provider`, `namespace` (MediaWiki number); `cluster_id`; `labels.{lang}` with `.prefix` and `.near_match` subfields; `labels_all`; `descriptions.{lang}`; `aliases.{lang}`; `key`, `key_ulabel` and `key_parents` for keyed types; `statement_keywords`; `sitelink_count`, `statement_count`, `incoming_links`; `resolved_version`; on a shared document, `tenant_overlaid` (§7) |
| `pages` | Document page that is not deleted | `page_id`, `namespace`, `title` with `.prefix`, `text` (the content model rendered to plain text; for `wikitext`, the rendered text after expansion, and `source_text` the source, [0042](../decisions/0042-template-expansion-and-parsoid.md) §17), `content_model` (a content model registry ID; threads are `triplespace-thread`, [0041](../decisions/0041-content-models.md) §10), `origin`, `latest_offset`, `categories` (for `incategory:`) and `statement_keywords` over the page's statements (for `haswbstatement:`, [0038](../decisions/0038-page-metadata-and-categories.md) §12), `redirect_titles` (the titles of the page's redirects, [0051](../decisions/0051-page-redirects.md) §5; redirect pages themselves are not documents unless broken or external) and `short_description` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6) |
| `pages-{repo}` | A page a repository in `mirror` mode holds, one shared index per repository at instance scope ([0053](../decisions/0053-mirrored-pages.md) §7) | `title`, `text` (the HTML stripped), `categories`, `redirect_titles`, `origin`; searched beside the tenant's `pages` index when `search.inherited` is on |

Field semantics follow WikibaseCirrusSearch where a field exists there, so that its analysis configuration, its query builders and its ranking can be reused or compared:

- **`labels.{lang}`** holds the label and the aliases for that language, so a prefix match finds either. Language analyzers come from the same analysis configuration CirrusSearch uses, including the ICU plugin.
- **`labels_all`** holds every label and alias in every language, for a query with no language or a language with no label.
- **`statement_keywords`** holds `P31=Q5`-style tokens for item-valued and external-id statements in the resolved view, in source form, so `haswbstatement:` works; a `haswbstatement:` constant that names a clustered entity is expanded to the member set at query time, as every entry point expands its input ([04](04-entities-and-identifiers.md)). Properties excluded from it are registry configuration. On `pages` it covers the page's statements, so `haswbstatement:P40=Q812` finds the pages tagged for cleanup, whether the tag is asserted or projected ([0038](../decisions/0038-page-metadata-and-categories.md) §12).
- **`incoming_links`** is `view.entity.ref_count` (§4.2), summed over the cluster. `sitelink_count` and `statement_count` are read from the resolved JSON.
- **Keyed types** ([0009](../decisions/0009-keyed-entity-types-and-domain.md)): `id` is the prefixed ID, `domain:en.wikipedia.org`, as everywhere an entity ID appears ([0017](../decisions/0017-entity-id-grammar.md) §4); `key` keeps the bare key in its A-label form, `key_ulabel` the display form, and `key_parents` is the list of the key and every parent by removing labels from the left, so `wikipedia.org` matches `en.wikipedia.org` and a search for a zone finds its hosts. This is the DNS hierarchy of 0009 §5, made searchable.
- **Results collapse by cluster.** Every document carries `cluster_id`, maintained by the cluster projection and bounded by members, never by referrers; `wbsearchentities` and the search page return one result per cluster, the member the consumer's preference selects ([18](18-api.md)). A search for any member's ID is a `/resolve`, not a search, since every entry point accepts any member.

**What each page kind indexes.**

- **Threads.** A thread is one document in the `pages` index: subject plus post text, with the `Thread` namespace; hidden and erased text is not indexed, and the document is re-sent with the thread's latest revision ID as its external version ([0019](../decisions/0019-discussions.md) §11). A thread's document gains the IDs of all its attachments as a field, so a search can be limited to a board or talk page ([0049](../decisions/0049-boards.md) §12). Foreign threads are documents in the repository's `pages-{repo}` index with the thread fields, so a talk-page search covers both ([0069](../decisions/0069-synchronized-talk-pages.md) §10).
- **Redirects.** A redirect page is not indexed as a document. Its title is added to its target's document in the `pages` index, in a `redirect_titles` field, as CirrusSearch's `redirect` field does, so that a search for *Heart attack* finds *Myocardial infarction* and the suggester offers the redirect title with "→ Myocardial infarction". Broken and external redirects are indexed as documents of their own, so they can be found and fixed ([0051](../decisions/0051-page-redirects.md) §5).
- **Expanded text.** The `text` field of the `pages` index is the rendered text, as CirrusSearch indexes it, and `source_text` keeps the source. A `hastemplate:` keyword reads `view.transclusion` ([0042](../decisions/0042-template-expansion-and-parsoid.md) §17; the filter half of that section is in [09](09-security-and-moderation.md)).
- **File pages** are documents in the `pages` index like other pages, with the current readable version's fields: `file_media_type`, `file_mime`, `file_size`, `file_width`, `file_height`, `file_resolution`, `file_bits` and `file_text` (text the metadata projection extracts from PDFs and SVGs), supporting CirrusSearch's `filetype:`, `filemime:`, `filesize:`, `fileres:`, `filew:` and `fileh:` keywords. A version that becomes unreadable ([0039](../decisions/0039-files-and-media.md) §8) is removed from the document, as hidden text is. Foreign files are not indexed; a local page annotating a foreign file is ([0039](../decisions/0039-files-and-media.md) §11), with the foreign file's fields ([0039](../decisions/0039-files-and-media.md) §19).
- **Tables** are indexed as pages, with the text of [0045](../decisions/0045-table-content-model.md) §3 ([0045](../decisions/0045-table-content-model.md) §12).
- **Scopes and sprints** index the plain text of their definitions ([0060](../decisions/0060-scopes.md) §3, [0061](../decisions/0061-sprints-and-tasks.md) §3); member lists and tasks are not indexed ([0060](../decisions/0060-scopes.md) §10, [0061](../decisions/0061-sprints-and-tasks.md) §11).
- **Mirrored pages.** In `proxy` mode a foreign page is not indexed: its text is a cache entry, not something the tenant holds. The suggester and `list=prefixsearch` read the title index for served namespaces after the local hits, each row marked with its origin ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6), so a reader can reach any inherited title by typing it. In `mirror` mode a repository has one shared index, `pages-{repo}`, at instance scope like a provider's `entities` index, indexing each mirrored page's title, text (the HTML stripped), categories and redirect titles ([0053](../decisions/0053-mirrored-pages.md) §7).

**Other entity types.** The entity index gains lexeme documents with `lemma`, `language`, `lexical_category`, `representation` and `gloss` fields; `wbsearchentities&type=lexeme` matches lemmas and shows "lemma (language, category)" as Wikidata does, `type=form` matches representations and `type=sense` glosses, each returning the part's ID; the search box's entity suggester offers lexemes under their own heading ([0066](../decisions/0066-lexemes.md) §5). Entity schemas index their terms as any entity's and their ShExC text as a plain-text field, and `wbsearchentities&type=entityschema` finds them ([0064](../decisions/0064-entityschema-and-validation.md) §7; the special pages and API of that section are in [21](21-special-pages.md) and [18](18-api.md)). Captions index as the File page's terms ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §1, §4).

### 11.2 Queries

*Sources: [0014](../decisions/0014-caches-and-search.md) §7; [0053](../decisions/0053-mirrored-pages.md) §7.*

| Route | Query |
|---|---|
| `GET /suggest?q=` ([0010](../decisions/0010-site-ui.md) §3) | One `msearch` with a request per kind: items and properties, foreign entities by type, keyed entities, and pages. Entities match `labels.{lang}.prefix` with the viewer's language fallback chain, rescored by `incoming_links` and `sitelink_count`; pages match `title.prefix`. If the input parses as an ID or a key, the title resolver's answer is the first suggestion and the index is not asked for it. |
| `wbsearchentities` | The entity request of `/suggest`, with `type` and `language` as given and the same continuation as Wikibase |
| `list=search` | Full text over both indexes, with `srnamespace` selecting which. Entity results use `labels_all` and `descriptions`; page results use `text` and `title`. `haswbstatement:` is honoured, and `hastemplate:` reads `view.transclusion` ([0042](../decisions/0042-template-expansion-and-parsoid.md) §17). |
| `list=prefixsearch` | `title.prefix` on `pages`, and `labels.{lang}.prefix` on `entities` for entity namespaces |

`list=search` and the search box run one `msearch` across the tenant's `pages` index and the `pages-{repo}` indexes of the repositories it lists in `mirror` mode, local hits first; the `site` setting **`search.inherited`** (`on` by default where a mirrored repository is listed) turns the foreign indexes off for tenants that want search to mean "what this wiki holds" ([0053](../decisions/0053-mirrored-pages.md) §7).

**Sitelinks and resolvers add nothing to the indexes.** A denied host's sitelink is left out of the entity document because it is left out of the resolved view ([0026](../decisions/0026-sitelinks.md) §3); a resolver key is recognised by the title resolver before the index is asked ([0029](../decisions/0029-resolver-namespaces.md) §4), as an ID is.

### 11.3 Indexing is a projection

*Sources: [0014](../decisions/0014-caches-and-search.md) §7; [0056](../decisions/0056-security-model.md) §8; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

The search indexer is a tier-3 consumer (§6.1), with its own queue, rate and lag, off during bootstrap and bulk modes:

- It consumes the composition event `(id, tenant, resolved_version)`. The indexer reads the composed JSON and `ref_count` from `view`, builds the document, and sends it in bulk with `version_type: external` and `version: resolved_version`. Out-of-order deliveries are rejected by the index, so retries and parallel workers are safe.
- A page record does the same with `latest_offset`.
- An erased or tombstoned entity or a deleted page is deleted from the index with the same versioning.
- The index holds no moderation-hidden data: no hidden names, no erased bodies, nothing from an internal-only or private graph. Redaction is applied when the document is built, and the erasure path of §9.6 deletes documents. It does hold content under a **confidential** restriction (§11.5).
- **Bootstrap** indexes from `view` in one pass after the set-based composition, with the refresh interval raised and replicas set to zero for the duration, as CirrusSearch's reindex does.
- **Reindexing** into a new index and swapping an alias is the only way a mapping changes. The alias is what queries name.

**Mappings.** The mappings and analysis settings are committed as JSON under `docs/search/`, versioned, and named in the alias, so a running instance can report which mapping it serves. Building a document from the resolved JSON is pure and lives in `scatter-wikibase-model` as a function, so the audit tooling can produce the same documents without a server.

**Consistency.** The index is eventually consistent with `view`, by the consumer's lag. A suggestion may name an entity whose page has moved on; the page is served from `view`, and the difference is the lag reported by `ops.projection_work`.

### 11.4 Indexes per provider, tenant and repository; farm-wide search

*Sources: [0014](../decisions/0014-caches-and-search.md) §10; [0018](../decisions/0018-tenants.md) §6; [0053](../decisions/0053-mirrored-pages.md) §7; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1.*

**Indexes multiply by provider and tenant** ([0018](../decisions/0018-tenants.md) §6): one shared `entities` index per provider, holding that provider's entities as the shared view has them, and one index per tenant holding its local entities and its overlay documents. `/suggest` and `wbsearchentities` run one `msearch` across the tenant's own index and the indexes of its opted-in providers. **A tenant document replaces the shared one in the index**: the overlay projection sets `tenant_overlaid` on the shared document, naming the tenants that have overlaid the entity, and a tenant's query adds a filter excluding shared documents that name it, so the two indexes never return the same entity and no post-query dedup is needed (§7). `incoming_links` on a shared document is the instance-wide count. A repository in `mirror` mode has one shared `pages-{repo}` index at instance scope in the same way ([0053](../decisions/0053-mirrored-pages.md) §7).

**Farm-wide search** ([0028](../decisions/0028-tenancy-policy.md) §9): under `search.farm_wide`, `/suggest` and `list=search` at the farm base run one `msearch` across every tenant's index and every provider index, each hit carrying its tenant, gated to global groups or to everyone by policy. Redaction stays per tenant.

### 11.5 Restricted content, with a filter

*Sources: [0056](../decisions/0056-security-model.md) §8; [0014](../decisions/0014-caches-and-search.md) §7.*

**Documents carry `read_enclosures`.** Each document in a tenant's index gains `read_enclosures`: references to the enclosures its target is under (the namespace, the sets `set_member` names, the tenant), empty for a target under no restriction, and never a group list, since a stored group list goes stale when a namespace or set restriction changes or expires (§4.14). At query time the server evaluates from L0 which enclosures the principal satisfies and adds one filter: `read_enclosures` is empty, or every value in it is among the enclosures the principal may read, which OpenSearch expresses as a `terms_set` query whose `minimum_should_match` is the field's length. Anonymous queries match the empty case only. `/suggest`, `wbsearchentities`, `list=search` and the farm-wide `msearch` all apply it; the Postgres fallback (§11.6) applies the same predicate in SQL.

**Moderation stays out of the index.** A deleted, hidden or suppressed target is not indexed: the moderation kind has no readers who search for it. Only the confidential kind is indexed, so a private wiki's members can search their wiki.

**Consequences for the deployment.** The index holds text that not everyone may read. It is inside the boundary of [0056](../decisions/0056-security-model.md) §10, the server is its only client, and nothing else, no dashboard and no reporting tool, is pointed at it. The per-provider shared indexes hold public form only, because they are built from public data; the per-tenant indexes are where `read_enclosures` is non-empty.

**Re-indexing.** A `read` ACL change on a target re-indexes that target's document through the tier-3 visibility consumer (§4.14) on the trigger of §9.6, with the same versioning §11.3 uses, and a document whose target becomes moderation-hidden is deleted; a change on an enclosure rewrites no document, since the enclosure is evaluated at query time.

### 11.6 Postgres fallback

*Sources: [0014](../decisions/0014-caches-and-search.md) §8; [0056](../decisions/0056-security-model.md) §8; [0082](../decisions/0082-source-form-and-the-shared-view.md) §2, §6.*

Where OpenSearch is absent, the same routes are served from Postgres:

| Route | Fallback |
|---|---|
| `/suggest`, `wbsearchentities`, `list=prefixsearch` for entities | `view.term` with the `term_prefix` index (§4.3), partial over the tenant's rows: `lower(text) LIKE lower($q) || '%'` in the requested language and its fallbacks, ordered by `ref_count`, collapsed by `cluster_id` |
| `list=prefixsearch` for pages | `view.page` on `(ns, title)` |
| `list=search` | A `tsvector` column on a `view.page_text` table with a GIN index; entities by `term_prefix` |

The fallback has no `haswbstatement:`, no ranking beyond incoming links, and no analysis. It is the **small profile's** search: `term_prefix` is partial over local entities and overlays, so it is small on every profile and part of the schema everywhere, but the large profile never reads it, and an instance holding a full mirror has no prefix index over mirrored terms at all; `wbsearchentities` at Wikidata scale is the search index. The profile table of [0013](../decisions/0013-postgres-storage.md) §11 (in [22](22-crates-and-stack.md)) defines the sizes. `meta=siteinfo&siprop=triplespace` ([0012](../decisions/0012-api-requirements.md) §4) reports `search: opensearch` or `search: postgres` so the UI knows which features to show. The fallback applies the restriction of §11.5 after the scan: each hit's visibility set is computed at read time ([0056](../decisions/0056-security-model.md) §2) and a hit the principal may not read is dropped before paging. No visibility column is added to `view.term`, `view.page` or `view.page_text`; the fallback is for instances small enough that the wider scan does not matter.

## 12. The cache and index catalogue

*Sources: [0014](../decisions/0014-caches-and-search.md) §4, §10; [0018](../decisions/0018-tenants.md) §6; [0019](../decisions/0019-discussions.md) §11; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0028](../decisions/0028-tenancy-policy.md) §12; [0030](../decisions/0030-edit-filters.md) §11; [0031](../decisions/0031-property-constraints.md) §5; [0039](../decisions/0039-files-and-media.md) §20; [0049](../decisions/0049-boards.md) §12; [0051](../decisions/0051-page-redirects.md) §5; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §8; [0053](../decisions/0053-mirrored-pages.md) §7; [0056](../decisions/0056-security-model.md) §7, §14; [0058](../decisions/0058-packed-record-storage.md) §11; [0060](../decisions/0060-scopes.md) §10; [0061](../decisions/0061-sprints-and-tasks.md) §11; [0069](../decisions/0069-synchronized-talk-pages.md) §10.*

The ADRs after 0014 add to the layers of §9.2 without adding a layer. This section is the index, so that erasure (§9.6) and the privacy test have one list to walk. The first rows of each table are [0014](../decisions/0014-caches-and-search.md) §4's; the rest are the rows of [0014](../decisions/0014-caches-and-search.md) §10 and the "Caches" trailers of later ADRs, with the entries 0014 §10 does not list marked.

### 12.1 L1 keys

*Sources: [0014](../decisions/0014-caches-and-search.md) §4, §10; [0018](../decisions/0018-tenants.md) §6; [0019](../decisions/0019-discussions.md) §11; [0039](../decisions/0039-files-and-media.md) §20; [0049](../decisions/0049-boards.md) §12; [0051](../decisions/0051-page-redirects.md) §5; [0056](../decisions/0056-security-model.md) §7; [0060](../decisions/0060-scopes.md) §10; [0061](../decisions/0061-sprints-and-tasks.md) §11; [0069](../decisions/0069-synchronized-talk-pages.md) §10; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §2, §6; [0087](../decisions/0087-account-registration-and-challenges.md) §4; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2, §5, §9.*

Every key below is written with the instance-wide cache epoch in front of it, `c{epoch}:` (§9.4), omitted here for brevity; a projection rebuild bumps it and every key before it is unreachable.

| Key | Value | TTL ceiling | Added by | In 0014 §4 or §10 |
|---|---|---|---|---|
| `e:{id}:{gen}:{ver}` | Composed canonical JSON in source form, compressed; written by the composer after commit, rewritten to the consumer's preferred ID form on the way out (§9.3) | 24 h | [0014](../decisions/0014-caches-and-search.md) §4 | Yes |
| `t:{lang}:{id}:{gen}:{ver}` | Label and description in one language; the label cache that serves every language `view.term` does not hold, read from the record body (§4.3) | 24 h | 0014 §4 | Yes |
| `prov:{id}:{gen}:{ver}` | The provenance response ([0003](../decisions/0003-statement-ui.md) §6, [0012](../decisions/0012-api-requirements.md) §5) | 24 h | 0014 §4 | Yes |
| `p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}` | Rendered HTML of a document page; `{epoch}` is the render epoch ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10). A followed redirect's view is cached under the **target's** key, since its HTML is the target's with a notice whose text is the request's title; the notice is added outside the cached fragment ([0051](../decisions/0051-page-redirects.md) §5) | 24 h, or the render's `expires_at` if sooner | 0014 §4 | Yes |
| `d:{partition}:{offset}:{gen}` | A structured diff ([0012](../decisions/0012-api-requirements.md) §7) | 24 h | 0014 §4 | Yes |
| `sug:{lang}:{kinds}:{q}` | A suggestion list ([0012](../decisions/0012-api-requirements.md) §5) | 60 s | 0014 §4 | Yes |
| `up:{provider}:{id}:{from}:{to}` | Upstream edits fetched live ([0012](../decisions/0012-api-requirements.md) §6) | Configured; minutes | 0014 §4 | Yes |
| `rl:{class}:{key}` | Rate-limit counters, the key being the actor key or, for anonymous requests, the address key: the IPv4 address or the IPv6 /64 ([07](07-actors-and-accounts.md) §6.3; [0087](../decisions/0087-account-registration-and-challenges.md) §4) | The limit window | 0014 §4 | Yes |
| `s:{session}` | Session state; records the API key ID or OAuth token ID that opened it, so revoking either can end it, and the login it holds and, for a tenant session, the central session it came from ([0024](../decisions/0024-subsidiary-accounts.md) §4, [0025](../decisions/0025-oauth-server.md) §5) | The session lifetime | 0014 §4 | Yes |
| `sc:{central session}` | The tenant sessions a central session opened, so that logging out everywhere ends them ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §9) | The session lifetime | [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §9 | Yes |
| `authr:{handle hash}`, `authc:{code hash}` | A tenant's request at the auth host's `/auth/authorize`, and a login code ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5) | 10 min; 60 s | [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5 | Yes |
| `fp:{repo}:{ns}:{title}:{revid}` | A page repository's bundle: rewritten HTML and metadata ([0053](../decisions/0053-mirrored-pages.md) §4). The key has no tenant segment: a repository is instance-level and its stored rewrite is tenant-independent, so one bundle serves every tenant that selects the repository and the tenant pass is applied at serve time ([13](13-mirrored-pages.md)) | The repository's `cache_ttl`, or until an event purges it | 0014 §4 | Yes |
| `css:{pageid}:{gen}:{offset}` | The sanitized form of a `sanitized-css` page ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2) | 24 h | 0014 §4 | Yes |
| `post:{revid}:{gen}:{offset}` | A rendered post; `{offset}` is the record holding its current text | | [0019](../decisions/0019-discussions.md) §11 | Yes |
| `th:{pageid}:{gen}:{revid}` | A thread, keyed by its page ID and latest revision ID | | 0019 §11 | Yes |
| `tp:{pageid}:{gen}:{listing}` | A talk page's thread listing, keyed likewise by the talk page ID; `{listing}` is the **listing version**, the maximum activity ID over the threads attached to the page, local and foreign ([0049](../decisions/0049-boards.md) §12, [0069](../decisions/0069-synchronized-talk-pages.md) §10), computed in Postgres at read time, so a thread record or a followed `put` makes the old entry unreachable with no purge | | 0019 §11 | Yes |
| `t:`, `e:`, `prov:` and the rest of 0014 §4 | Gain a tenant component where the value is tenant-specific, and omit it where the value is shared | | [0018](../decisions/0018-tenants.md) §6 | Yes |
| `e:`, `t:`, `prov:`, `p:`, `d:`, `sug:`, `css:` | Gain a `{vis}` segment after `{gen}` (§9.4): `-{epoch}` for the public form of a tenant key, `-` for a shared key | | [0056](../decisions/0056-security-model.md) §7 | Yes (§4) |
| `fi:{tenant}:{page id}:{gen}` | A file's imageinfo | | [0039](../decisions/0039-files-and-media.md) §20 | Yes |
| `rf:{repo}:{name}` | A foreign file's imageinfo | The repository cache TTL | 0039 §20 | Yes |
| `ld:{tenant}:{pageid}:{gen}:{offset}` | A parsed data module ([0043](../decisions/0043-lua-modules.md) §6) | | [0042](../decisions/0042-template-expansion-and-parsoid.md) §10, [0043](../decisions/0043-lua-modules.md) §6 | Yes |
| `sc:{page id}:{definition revid}:{listing}` | A scope's member pages, `{listing}` being the listing version (the maximum activity ID over the members) at the last recomputation; the scope page itself follows the page rules | | [0060](../decisions/0060-scopes.md) §10 | **No** |
| `sp:{page id}:{definition revid}:{listing}` | A sprint page's counts and leaderboard, `{listing}` being the maximum activity ID over the sprint's tasks; the one hot key here, cached at L1 to absorb a busy drive | `sprints.cache_ttl` (default 30 s) | [0061](../decisions/0061-sprints-and-tasks.md) §11 | **No** |

Listing keys (`tp:`, `sc:`, `sp:`) are versioned like every other key: the listing version replaces the tag purges on ordinary writes, so principles 1 and 4 of §9.1 hold for them; tag purges remain for erasure and hiding (§9.6).

### 12.2 L0 entries

*Sources: [0014](../decisions/0014-caches-and-search.md) §2, §10; [0023](../decisions/0023-moderation.md) §10; [0028](../decisions/0028-tenancy-policy.md) §12; [0030](../decisions/0030-edit-filters.md) §11; [0031](../decisions/0031-property-constraints.md) §5; [0039](../decisions/0039-files-and-media.md) §20; [0056](../decisions/0056-security-model.md) §7, §14; [0058](../decisions/0058-packed-record-storage.md) §11.*

Beside the registries and role maps, property data types, cluster canonical IDs, hot-language labels and current versions of §9.2, all small, all invalidated by the record that changes them: the current ACLs of a target and its enclosures, including the `statement` and `property` targets ([0023](../decisions/0023-moderation.md) §2, §10); the compiled CEL rule of every enabled filter ([0030](../decisions/0030-edit-filters.md) §11); the parsed constraints of a property, keyed by its `resolved_version` ([0031](../decisions/0031-property-constraints.md) §5); the class chain walked by a *type* constraint check, to its depth limit ([0031](../decisions/0031-property-constraints.md) §2); the farm-account join behind global groups and blocks, per session ([0028](../decisions/0028-tenancy-policy.md) §12); the fragment cache of packed storage (§9.5); `file_block` held whole, since it is small and read on every request ([0039](../decisions/0039-files-and-media.md) §20); a session's effective permissions and group memberships, for seconds, the subset test of [0056](../decisions/0056-security-model.md) §6 and the visibility set of recently read targets, per target version and tenant epoch ([0056](../decisions/0056-security-model.md) §7, §14).

### 12.3 Valkey keys that are not caches

*Sources: [0014](../decisions/0014-caches-and-search.md) §4, §10; [0024](../decisions/0024-subsidiary-accounts.md) §10; [0025](../decisions/0025-oauth-server.md) §8; [0030](../decisions/0030-edit-filters.md) §11; [0083](../decisions/0083-write-path-in-three-tiers.md) §1; [0087](../decisions/0087-account-registration-and-challenges.md) §2, §3, §4.*

`rl:filter:{id}:{key}`, a filter's throttle counter with the filter's window as TTL ([0030](../decisions/0030-edit-filters.md) §4, §11); `oauth:code:{hash}` and `oauth:device:{hash}`, authorization and device codes with a ten-minute TTL ([0025](../decisions/0025-oauth-server.md) §8). The rate-limit counters `rl:{scope}:{key}` have the classes and windows of the site policy of [0024](../decisions/0024-subsidiary-accounts.md) §5 (`edit`, `create`, `move`, `link`, `job`, `read`, `stream`, `atom`, `upstream`, `notify`, `account`, `createaccount`), keyed by actor key or, for anonymous requests, by address key, the IPv4 address or the IPv6 /64 ([0087](../decisions/0087-account-registration-and-challenges.md) §4); counters are Valkey keys, never rows ([0024](../decisions/0024-subsidiary-accounts.md) §10). The `createaccount` class joins them, counted by address key whoever the caller is. `ch:{tenant}:{mac}` marks a challenge spent until its expiry, and `ch:{tenant}:bad:{key}` and `ch:{tenant}:badu:{actor}` count failed `password` logins by address key over 5 minutes and by account over 10 minutes, for the `badlogin` trigger ([0087](../decisions/0087-account-registration-and-challenges.md) §2, §3; [07](07-actors-and-accounts.md) §2.6). The `read` class is counted for authenticated principals only, once a per-process count passes a fraction of the limit, and **never for a response served from L0 or L1**, so a cache hit is not a round trip to Valkey ([07](07-actors-and-accounts.md)). Sessions (`s:`) record the API key ID or OAuth token ID that opened them, so revoking either can end them.

### 12.4 Cache tags

*Sources: [0014](../decisions/0014-caches-and-search.md) §5, §10; [0039](../decisions/0039-files-and-media.md) §20; [0057](../decisions/0057-web-tier.md) §6.*

Every anonymous API response carries `Cache-Tag: entity:{id}, page:{page id}` for each entity and page it is about (§9.6); every rendered page carries a tag for each page and entity in its manifest ([0042](../decisions/0042-template-expansion-and-parsoid.md) §10); every media response carries `blob:{scope}:{h}` and `file:{tenant}:{page id}` ([0039](../decisions/0039-files-and-media.md) §20); a page the web tier composes carries exact tags for its subjects and its render-manifest pages only, never for the entities it drew labels from (§10.3).

### 12.5 Search indexes

*Sources: [0014](../decisions/0014-caches-and-search.md) §7, §10; [0018](../decisions/0018-tenants.md) §6; [0053](../decisions/0053-mirrored-pages.md) §7; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1.*

| Index | Scope | Holds |
|---|---|---|
| `entities`, one per provider | Instance | That provider's entities as the shared view has them, under their source IDs; public form only; `tenant_overlaid` names the tenants whose own document replaces each |
| `entities`, one per tenant | Tenant | The tenant's local entities and its overlay documents; `read_enclosures` may be non-empty |
| `pages` | Tenant | Document pages that are not deleted, threads, file pages, tables, scope and sprint definitions |
| `pages-{repo}`, one per repository in `mirror` mode | Instance | Mirrored pages and foreign threads |

Thumbnails and proxied foreign bytes are caches kept in the blob store (§8), not in Valkey.

## 13. Stores beside the caches

*Sources: [0014](../decisions/0014-caches-and-search.md) §1, §10; [0028](../decisions/0028-tenancy-policy.md) §12; [0024](../decisions/0024-subsidiary-accounts.md) §10.*

Two stores sit beside Valkey in the deployment without being caches: the email and fediverse delivery queue in `ops` ([0021](../decisions/0021-notifications.md) §8), which is durable and survives a restart; and the rate-limit counters of §12.3.

**The query store** ([0059](../decisions/0059-query-service.md) §4, in [02](02-graphs-rdf-and-query.md)) is instance infrastructure like OpenSearch ([0028](../decisions/0028-tenancy-policy.md) §9): one store holding every public tenant's graphs under their names, isolated at query time by the SPARQL Protocol's dataset. A tenant may instead have an embedded store of its own (`query.isolation = store`), which the `isolated` preset of [0028](../decisions/0028-tenancy-policy.md) §1 selects. A private tenant has no graphs in the shared store ([0028](../decisions/0028-tenancy-policy.md) §12); with `query.isolation = store` it has a store of its own, fed with the form its `tenant` ACL's group may read and queried only by principals that satisfy that ACL, so that it lacks the public endpoint, the stream and the dump and nothing else ([09](09-security-and-moderation.md)).

**A small instance** runs with no shared cache and no search service (principle 5 of §9.1), with the Postgres fallback of §11.6 in place; OAuth codes, request handles and sessions are then held in `private` through `triplespace-accounts` ([0025](../decisions/0025-oauth-server.md) §8). The profiles themselves are in [22](22-crates-and-stack.md).
