# Write-path performance

How fast the write path is, how it was measured, what was fixed, and what a Wikidata-sized load
still needs. The procedure for the real-dump runs themselves is [`real-dumps.md`](real-dumps.md).

Moved into the repo on 2026-10-21 from working notes of 2026-10-05/06. Figures are from those
runs; re-measure before relying on them.

## 1. Librarybase adoption, before and after

Full Librarybase dump (`librarybase-20261005.xml.gz`, 247,011 entities), `adopt --batch 200`,
one connection. "Before" is `main` before #56; "after" is `3d84c87` + #56 + migration 0008.

| | before | after |
|---|---|---|
| wall | 51m44s | **11m56s** (4.3×) |
| per entity | 12.6 ms | **2.9 ms** |
| entities/s | 80 | **345** |
| user CPU | 10m50s (2.6 ms/entity) | 4m58s (1.2 ms/entity) |
| sys CPU | 11m31s (2.8 ms/entity) | 1m58s (0.5 ms/entity) |
| SQL statements | 44.5 per entity | **17.05 per entity** (4,212,753) |
| server exec / plan | 9.1 s / 13.6 s per 10k entities | 102.8 s / 16.8 s for the full run |
| `view.identifier` rows | 0 (bug, fixed in `3d84c87`) | 246,453 |

`rebuild` of the same log took **10m01s** (3m45s user, 1m28s sys) and produced an identical `view`
schema (1,481,456 rows). The catch-up path costs about what inline adoption does.

**Where the 716 s went after the fixes:** client CPU 416 s (58%), Postgres 120 s (17%), the wire
about 180 s (25%). The database is no longer the bottleneck; the client's per-entity CPU is, and it
has not been profiled.

The per-entity statement mix on the full run matched the 10k slice, so nothing grew with table
size. The remaining planning time is the `read_by_key` select on the partitioned `log.record`,
which Postgres re-plans per call despite the prepared statement (it prefers a custom plan for
plan-time pruning). That is 2% of the run; `plan_cache_mode = force_generic_plan` on the writer's
connection would remove it.

## 2. What the cost was, and the fixes

Before #56, every entity cost about 45 SQL statements, each sent unprepared, so each was two round
trips (Parse+Describe, then Bind+Execute): about 90 round trips per entity at roughly 140 µs each
over loopback TCP. Postgres spent more time *planning* these point lookups than running them (60% of
server time), and `page_id_of` alone was 15% of server time in planning.

| Fix | Statements removed per entity | What it did |
|---|---|---|
| 3.1 Projection positions in memory | 12 | `set_applied` writes to a map on `PgCx`; `commit` flushes all positions in one `unnest` upsert (`projection_state::set_applied_many`). |
| 3.2 Merkle frontier in memory | 8–10 | `Frontier::push_leaf_completing` returns the nodes a leaf completes; `merkle::put_nodes` inserts them in one statement; `Frontiers` carries one frontier per partition per unit of work, checked against `next_offset` under the row lock. |
| 3.3 Partition metadata cached | 6 | The `Partitions` trait: `PgCx` answers `partition_info`/`partition_of` from a per-transaction cache (positive results only). |
| 3.4 Prepared statements | (planning) | `scatter_log_postgres::PgClient`; deadpool's `prepare_cached`. `PgCx` no longer derefs to the raw client, so a forgotten trait import fails to compile. |
| 3.5 Multi-row inserts, pipelining | T + I − 4 | Terms and identifiers are one `DELETE` + one `unnest` `INSERT` each; the entity upsert and both writes are pipelined with `tokio::try_join!`. |
| 3.6 Dead work | 1 | `read_by_key` no longer re-reads the partition row. |
| Migration 0008 | — | `entity_by_id`, `entity_source_by_entity`, `identifier_entity`. `DELETE FROM view.identifier` went from a sequential scan (about 300 µs and growing) to 6 µs. |

**Partial dumps need every property page.** XML dumps store entity JSON without `datatype` on snaks;
`adopt` types snaks from the dump's own property pages (`3d84c87`). Pywikibot and
WikidataIntegrator also fetch every property an item uses, so a slice must carry all property pages
(`tools/dump_slice.py --all-properties`, with `--defined-only` to drop items a prefix slice cannot
type).

## 3. Measuring

1. In `postgresql.conf`: `shared_preload_libraries = 'pg_stat_statements'` and
   `pg_stat_statements.track_planning = on`. Restart.
2. `CREATE DATABASE ts_lb_10k;`, run `instance create` into it, then `CREATE EXTENSION
   pg_stat_statements;` there.
3. `python3 tools/dump_slice.py data/librarybase-20261005.xml data/librarybase-slice-10k.xml.gz --limit 10000`
4. `SELECT pg_stat_statements_reset();`, then `adopt` the slice with the same flags as the full run.
5. `pg_stat_statements` is cluster-wide, so filter by database:

```sql
SELECT sum(calls) AS statements, round(sum(calls)/10000.0, 1) AS per_entity,
       round(sum(total_exec_time)) AS exec_ms, round(sum(total_plan_time)) AS plan_ms
FROM pg_stat_statements WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'ts_lb_10k');

SELECT calls, round(calls/10000.0, 1) AS per_entity,
       round(total_exec_time) AS exec_ms, round(total_plan_time) AS plan_ms,
       left(regexp_replace(query, '\s+', ' ', 'g'), 90) AS query
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = 'ts_lb_10k')
ORDER BY calls DESC LIMIT 40;
```

Re-run into a fresh database after each change; the per-entity column is the scoreboard. With
prepared statements, `plan_ms` should be near zero. A point lookup whose `exec_ms` is out of
proportion to its `calls` is the signature of a missing index.

Also worth trying: connect over the Unix socket instead of loopback TCP (a percent-encoded socket
directory in the URL, as in [`real-dumps.md`](real-dumps.md) §0.1). While round trips dominate, the
lower per-message latency shows up directly. `top` on the client alongside the run shows whether the
client or Postgres is busy.

## 4. Postgres settings

Measured effect on the load: about 3% (the load is round-trip bound); it helps the running site
more. For a machine with `R` of RAM:

| Setting | Default | Suggest | Why |
|---|---|---|---|
| `shared_buffers` | 128MB | R/4 | 128MB cannot hold the hot indexes (`merkle_node` PK, `term` PK and `term_prefix`, `identifier` PK, `entity_source` PK, the record partitions' key indexes) |
| `effective_cache_size` | 4GB | R/2 to 3R/4 | planner hint only |
| `max_wal_size` | 1GB | 8GB or more during loads | with 1GB a load checkpoints every minute or two, and each checkpoint brings full-page images for randomly touched pages across about eight indexes |
| `checkpoint_timeout` | 5min | 30min | same |
| `wal_compression` | off | `zstd` | compresses those full-page images |
| `work_mem` / `maintenance_work_mem` | 4MB / 64MB | 32MB / 1GB | the latter for index builds and vacuum after a load |
| `synchronous_commit` | on | leave on | about 1,235 commits per full adoption; fsyncs are not the problem |
| `random_page_cost` | 4.0 | 1.1 on SSD | planner |

## 5. Open problems on the write path

1. **`page_id_of` probes every record partition** (`triplespace-api-ingest/src/store.rs`): the join
   on `log.partition` gives the planner nothing to prune by. Resolve the tenant's partitions first
   (free, from the 3.3 cache) and query `partition = ANY($2)`.
2. ~~**`sync` materializes the whole dump in a `Vec<SyncItem>`** before `run_sync`.~~ Since
   2026-11-04 it streams (`SyncItems` in `triplespace-cli/src/sync.rs`): the dump is read one
   entity at a time as the job takes them. The job still keeps the set of IDs seen (in every mode,
   for the snapshot sweep), so a full Wikidata sync holds every ID, not every entity, in memory.
3. ~~**`sync` has only the internetdomains adapter.**~~ Since 0091, `sync` dispatches on the
   registry's `adapter` field and reads every Wikibase provider, Wikidata included, through
   `scatter-adapter-wikibase`; a JSON dump without key maps is read in one pass.
4. **The NDJSON job endpoint and the interactive API share `write_to`**, so a bulk import over the
   API runs at the per-entity rate (about 345 entities/s per job on Librarybase-shaped entities).
5. **Client CPU (1.2 ms/entity) is unprofiled.** Per entity the op is parsed from XML, validated,
   cloned, converted to JSON then to the CBOR value, encoded and hashed; the projection decodes it
   again and builds canonical JSON even when only its length is kept. The survey pass also reads and
   gunzips the whole dump before the job pass does it again. Profile with `perf record -g` or
   `samply` on the 10k slice before changing anything.

## 6. A Wikidata-sized load

**Why the Librarybase rate does not carry over.** Wikidata has about 117M items, 1.3M lexemes and
the properties. At 345/s that is about 4 days, if entities were Librarybase-sized. They are not: a
Librarybase citation item is 1–2 KB, and the mean Wikidata item is around ten times that, and every
per-entity cost scales with bytes or rows. On the present per-entity path, expect 10–20 ms per
Wikidata entity: **2–4 weeks**.

**The term table.** Wikidata has 1.5–2 billion labels, descriptions and aliases. One `view.term` row
per term, with its PK and `term_prefix` on `lower(text)`, is hundreds of GB of index: structurally
the `wb_terms` table Wikidata itself abandoned. 0014 §8 names `term_prefix` the small profile's
fallback. Before any full mirror, decide whether mirror entities populate `view.term` at all (all
languages, a configured subset such as `entities.term_languages`, or none, with search delegated).
[0070](../decisions/0070-shallow-entity-mirroring.md) keeps shallow mirrors to the configured term
languages; a full sync needs the same decision. Related: `view.activity` gets a row per mirrored
record, which a bulk sync probably should not produce.

**What gets to roughly 3,000 entities/s** (each independent; items 1–2 alone are about a week,
all five plus not materializing mirror terms is about overnight):

1. **Overlap parsing and writing.** Parsing, validating and encoding (1.2 ms) currently alternate
   with waiting on the database (1.7 ms). Worker threads feeding a bounded channel to the database
   task approach the maximum of the two instead of the sum: 1.5–2×, and it helps `adopt` and `sync`
   too. This is the only parallelism within one partition, since `FOR UPDATE` on the partition row
   serializes appends by design (gapless offsets, 0006 §3).
2. **Batch-shaped writes (`write_many`).** Per block of 200: lock the partition once, allocate
   offsets and revision IDs in one statement (`SELECT nextval(…) FROM generate_series(1, $1)`), one
   multi-row `INSERT` or `COPY … FROM STDIN BINARY` per table, and the block's Merkle nodes from the
   in-memory `Frontier` (the groundwork is in 3.2). About 10 statements per 200 entities. Used by
   `run_adoption`, `run_sync` and the NDJSON job; interactive edits keep `write_to`.
3. **Deferred indexes** (0013 §9 bootstrap mode): load `log.record` and the view tables without
   secondary indexes, build them after. Building `term_prefix` over a billion rows is itself hours.
4. **Set-based projections for bootstrap.** Resolution as one pass over the loaded log instead of
   117M round trips; `entity_source`, `activity` and `keyed_map` as one `INSERT … SELECT` each.
5. **`sync` plumbing:** stream instead of collecting a `Vec`, and skip `overlay_tenants` in bootstrap.

**Next, in order:** profile the client on the 10k slice; overlap parsing and writing; decide the
mirror-term question as an amendment to 0013/0014; then `write_many`, deferred indexes and set-based
projections as the stage 3 work, measured against the 1M slice of [`real-dumps.md`](real-dumps.md) §3.2.

## 7. The hidden set (read side)

Every read on a tenant with confidential targets starts by computing what the caller may not see
(09 §5.3; 0056 A19): one query over the tenant's confidential ACLs, then the subpages and threads
they enclose. `tools/hidden_set_bench.py` measures it: an outsider (a non-empty set, recomputed
every request) and a member of the group (an empty set, remembered by `pages::NothingHidden`
until the ACL frontier moves), at 0, 200 and 1,000 confidential pages, p50/p90 of 20 requests
after a warm-up, on a release build in the cloud sandbox (2026-10-24, `BENCH=1 BIN=target/release
tools/acl_check.sh`).

| Confidential pages | Probe | Outsider ms | Member ms |
|---|---|---|---|
| 0 | `prop=info`, one public page | 5.9 / 7.9 | 9.4 / 11.1 |
| 0 | `list=recentchanges`, 50 | 6.9 / 9.7 | 13.8 / 18.2 |
| 0 | `list=allpages`, 50 | 5.9 / 6.7 | 10.7 / 12.1 |
| 200 | `prop=info` | 10.1 / 13.1 | 12.3 / 16.1 |
| 200 | `list=recentchanges` | 11.8 / 13.7 | 15.3 / 17.2 |
| 200 | `list=allpages` | 10.7 / 12.3 | 11.4 / 14.8 |
| 1,000 | `prop=info` | 13.8 / 17.1 | 13.2 / 17.4 |
| 1,000 | `list=recentchanges` | 19.6 / 21.4 | 22.6 / 28.5 |
| 1,000 | `list=allpages` | 15.7 / 21.3 | 12.6 / 18.4 |

The member's baseline is higher because a logged-in request evaluates the tenant's groups; the
outsider here is anonymous. The set itself costs about 7 ms at 1,000 pages (1,000 ACL rows
decoded, then two indexed queries), linear in the number of confidential targets.

**Two fixes the measurement found**, both in the first run (1,000 pages: 160–220 ms per
outsider request, 90–170 ms per member `recentchanges`):

1. **The subpage query scanned every page per hidden root.** `left(p.title, length(r.title) + 1)
   = r.title || '/'` cannot use an index, so 1,000 roots × 1,000 pages was 150 ms. It is now a
   range of the title index, `Root/` up to the character after `/`, which is a prefix only in
   byte order: migration `0020_page_title_bytes` adds `view.page (tenant, ns, title COLLATE "C")`
   and the query compares under `COLLATE "C"`. 3 ms at 1,000 roots.
2. **The feed's page lateral cast the key** (`x.page_id::text = a.target_id`), which scanned
   `view.page` for every activity row the feed considered; the comparison now casts the row's
   target instead, behind a digits guard, and uses the primary key. This was not an ACL cost at
   all: every `recentchanges` paid it in proportion to the page count.

The debug-build figures in the implementation backlog (12→34 ms, 20→58 ms at 200 pages) predate
both fixes. **Still open:** a non-empty set is recomputed on every request; caching it needs a
key that also moves with page and thread writes under hidden targets (the pages frontier beside
the ACL frontier), which is the next step if a tenant with thousands of confidential pages
appears. The feed also computes its lateral joins before sorting and limiting when the planner
scans `view.activity` sequentially (a small table); on a large one it walks the time index.

## 8. The 2M Wikidata baseline (2026-11-04)

**What ran.** `sync --provider wikidata` of `wd-bench-2m.json.gz` (sha256 `4ceed74c…`), at `main`
`26f7754`, into a fresh database on a 16-core, 62 GB Debian 13 VM with Postgres 17 tuned per §4
(`shared_buffers` 16 GB, `max_wal_size` 16 GB, `checkpoint_timeout` 30 min, `wal_compression`
zstd). The set is every property (14,017), the first million items that exist (Q1–Q1084808) and a
seeded sample of a million scholarly articles (P31 = Q13442814, seed 1, from 46,010,359), all from
the 2026-10-26 dump, cut with `tools/wikidata_zindex.py` (real-dumps.md §3.2): 54.8 GB of JSON,
27 KB per entity.

**Result.** 2,014,017 created, 0 rejected, 0 unreadable, 0 hash mismatches, in 9 h 28 min
(59 entities/s: 54/s over the properties and dense early items, 63/s over the articles). Peak
RSS 406 MB, flat in the dump's size since sync streams. Database 39 GB: `view` 26 GB, of which
`view.term` 18 GB and `view.identifier` 4.3 GB; `log` 13 GB.

**Where the time goes, per entity:**

| | ms | share of wall |
|---|---|---|
| Wall clock | 16.9 | 100% |
| Client CPU (user + sys) | 11.8 | **70%** |
| Postgres execution | 2.2 | 13% |
| … `INSERT INTO view.term` alone | 1.3 | 8% |
| Postgres planning | 0.3 | 2% |
| Round trips and the rest | ~2.6 | 15% |

20.0 statements per entity. Sampled over 30 s mid-run, the client used 78% of a core and its
backend 18%, active in 9 of 30 one-second samples. **On Wikidata-sized entities the load is bound
by client CPU, not by round trips**: the reverse of Librarybase (§2), whose entities are a tenth
the size.

**Where the client's CPU goes** (callgrind over 1,000 dense items and 1,000 articles, 54M
instructions per entity; shares overlap where functions nest):

| | share |
|---|---|
| Decoding the record just appended, once in each of five projections (`common::operation` from `activity`, `entities` ×2, `sources` ×2) | **44%** |
| … of which re-encoding the body to check it is canonical CBOR (`cbor::decode_prefix` → `cbor::write`, 10 per entity) | 19% |
| … of which deserializing it into an `Operation` | 13% |
| Parsing the dump line (`ParsedEntity::deserialize`) | 12% |
| The entity-source and activity projections' own work | ~10% each |
| Canonical JSON (`Resolved::equals`, sizes) | 7% |
| Encoding the record (`Value::from_json`, body parts) | 8% |

`malloc` and `free` are about a third of all instructions, spread over the rows. `perf` is not
usable on Debian without lowering `kernel.perf_event_paranoid` (3 by default); callgrind on a
binary built with `CARGO_PROFILE_RELEASE_DEBUG=line-tables-only` needs no privileges.

**What this changes in §6.** Overlapping parsing and writing (§6 item 1) is worth at most the 12%
parse share here. Decoding each record once per write and handing the `Operation` to every
projection removes four of the five decodes, about a third of client CPU, with no change to what
is verified (one canonical check still runs). `view.term` is 46% of the database and half of
Postgres's execution time: the mirror-term question of §6 is now measured, not projected. At
59/s the full dump (122M entities) is about 24 days and `view.term` about 1.1 TB.

**Decoding once (2026-11-04).** `common::operation` now keeps the last record it decoded per
thread, with a copy of its content bytes, and returns it again (cloned) for equal bytes: the five
projections and the entity projection's read-back of the record share one decode and one canonical
check. Against `26f7754` on the same slices: instructions per 2k-entity profile 108.7 G → 71.8 G
(−34%); on 10,000 dense items and 10,000 articles, client CPU 187.6 s → 128.3 s (−32%) and wall
273 s → 217 s (73 → 92 entities/s, +26%). The next client costs, in the new profile: entity
resolution (`resolve_subject`) 31%, the entity-source projection 18%, SHA-256 for commitments 13%,
and `Resolved::equals` by canonical JSON 12%.

**The term-language rule (2026-11-04).** 05 §5.8 (0070 §6, settled for full mirrors by 0082 §6)
gives a mirrored entity `view.term` rows in the instance's term languages only; the baseline wrote
every language (110.8M rows in 629 languages for 2M entities). The entity projection now filters
shared rows (`tenant = ''`) inside the term insert, to the `tenancy:entities.term_languages`
ceiling or by default `mul`, `en` and every tenant's `site.language` (0082 A2). On the same 20k
slice, two runs each, alternating, against decode-once alone: term rows 811k → 50k (−94%),
`view.term` 127 MB → 9.7 MB, database 318 MB → 200 MB (−37%); Postgres execution 23 s → 15.5 s,
term inserts 9.7 s → 2.2 s; wall 225 s → 191 s (89 → 105 entities/s, +18%); client CPU unchanged.
With decoding once, 73 → 105 entities/s (+44%) on this slice against `26f7754`. A mirrored
entity's labels in other languages are in its record (`wbgetentities`) but no longer in
`view.term`, so a page shows its ID in those languages until the label cache (0070 §4, 05 §5.6)
is built.

**The state comparison and loads' activity rows (2026-11-04).** `Resolved::equals` turned
both states into canonical JSON to compare them, and the projection then built the resolved
state's a third time; it now builds it once and compares the contribution against it
(`equals_given`), which stays exact (a structural comparison would not be: `StatementId`
compares without case and `-0.0 == 0.0`). Instructions on the 2k profile 71.8 G → 67.4 G (−6.2%).
A load's mirror records no longer write `view.activity` rows (03 §4.6; 0013 §5.5): a mirror
change-set record whose job is not marked `follow` is skipped by the activity projection, so a
load is its job's rows alone, while the follower's records keep theirs. On the 20k slice the two
together, against the term rule alone: wall 191 s → 181 s (105 → 110 entities/s), client CPU
about 142 s → 132 s, activity rows 20,003 → 3, database 200 MB → 175 MB. **Against `26f7754` on
this slice, the four changes of this section take 273 s to 181 s (73 → 110 entities/s, +51%).**

**The allocator, measured (2026-11-04).** `malloc` and `free` were about a third of the client's
instructions. The same binary (all the changes above) under glibc, jemalloc 5.3.0 and mimalloc
3.0.1 by `LD_PRELOAD` (Debian's `libjemalloc2`, `libmimalloc3`), on the 20k slice, alternating,
two runs each: CPU 130 s / 104 s (−20%) / 96 s (−26%); wall 185 s / 169 s / 156 s (−9% / −16%;
runs vary by ±6%); peak RSS 49 MB / 128 MB / 85 MB. mimalloc is now the binaries' allocator
(0033 A24; 22 §4.2).

**The 2M set again, at `867c673` (2026-11-05).** Same file, settings and machine as the
baseline: 2,014,017 created, 0 rejected, 0 unreadable, 0 hash mismatches, in **5 h 18 min
(105 entities/s), against 9 h 28 min (59/s): −44% wall, +78% throughput**. Client CPU 12,653 s
against 23,818 (−47%); Postgres execution 1,862 s against 4,463 (−58%), still 20.0 statements
per entity (the job lookup replaced the activity insert); database 19 GB against 39 GB, with
`view.term` 5.1M rows and 1.1 GB against 110.8M and 18 GB, and `view.activity` 3 rows. Unlike
the baseline, the articles went slower than the dense items (99/s against 110/s). Per entity:
9.5 ms wall, of which client CPU 6.3 ms (66%), Postgres execution 0.9 ms (10%), round trips
and the rest about 2.3 ms (24%). The client is still the larger share, so preparing entities
on worker threads (§6 item 1, for parsing, ID rewriting, hash checks and CBOR encoding) and
then batch-shaped writes (§6 items 2–4) are next.

**Bootstrap mode, stage A (2026-11-05).** 0013 §9 for an empty instance mirror without a key
map and a JSON dump (A70; 05 §3.8): each record is a new subject's `put`, so it is prepared
from the dump alone on worker threads (parsing, the hash check, the operation, its encoding,
the part hashes), and one coordinator gives offsets and page IDs in dump order and appends a
block at a time by binary `COPY` of the records and the tree nodes (`append_many`), with one
partition lock, one page-ID reservation and one commit per block. The projections still run
per record, inline, so the key index stays; set-based projections and the deferred index build
are stage B. On the 20k slice: 178 s → 89 s (112 → 225 entities/s), with the same records,
tree and view rows as the ordinary sync and a rebuild identical to the load. **On the 2M set:
2 h 56 min (190 entities/s), against 5 h 18 min at `867c673` and 9 h 28 min at `26f7754`
(3.2×)**: 159/s over the dense items and 244/s over the articles; 11.0 statements per entity
against 20.0, Postgres execution 1,456 s against 1,862, the database the same 19 GB. Peak RSS
rose to 1.44 GB (386 MB in the ordinary sync; 658 MB on the 20k slice), not yet explained: the
queues between the threads and the coordinator are bounded, so the growth with the load's size
is worth a look before a 122M-entity load. What is left is the per-record projection work on the coordinator's thread
(the client was at about 110% of one core at the end): stage B's set-based projections.

**Bootstrap mode, set-based projections (2026-11-05).** The projections run over each block at
once (`Projection::apply_many`, `Pipeline::apply_inline_many`; 0013 A71): the entity source is
one multi-row upsert per block; a mirror's `put` of a subject no other graph or overlay holds is
resolved from its own state on worker threads, and its entity rows, terms (in the instance's term
languages) and identifiers are written with one statement per table; activity looks a load's job
up once per block. Any other record goes record by record. On the 20k slice: 89 s → 62 s
(225 → 321 entities/s), the same rows as the ordinary sync, a rebuild identical. **On the 2M set:
1 h 44 min (321 entities/s), against 2 h 56 min with projections record by record, and 9 h 28 min
at `26f7754` (5.4×)**: 278/s over the dense items, 358/s over the articles; 0.13 statements per
entity, Postgres execution 719 s; every view row the same as the record-by-record load's. The
client was at about 180% of one core and Postgres at about 12%: the coordinator's thread (the
append, the block's encoding for `COPY`, the rows' assembly) is the limit now. Peak RSS was
1.75 GB; the next paragraph explains it.

**A bootstrap load's memory (2026-11-05).** Peak RSS does not grow with the length of a load: it
is reached in the first minute and follows the size of the entities then in flight. RSS sampled
every 5 s over 300k entities: the dense start of the 2M file (38 KB a line) peaked at 1,744 MB
within a minute, then held 600–1,000 MB and ended near 300 MB; the articles at its end (16 KB)
peaked at 853 MB, then fell to 140–340 MB. The 2M run's 1.75 GB is the first peak: the dump
opens with Wikidata's largest items (Q1 onwards average 160 KB of JSON; record bodies there
average 76 KB, the largest 1.9 MB). On the first 60k lines: mimalloc as shipped 1,641 MB;
`MIMALLOC_PURGE_DELAY=0` 1,430 MB; the system allocator 1,580 MB, flat at about 1.5 GB to the
end and 38% slower. So the peak is live data, which mimalloc's delayed purge adds about an
eighth to and returns as the load goes on. massif on the first 2,000 items put 69% of the peak
in prepared record bodies waiting between the workers and the coordinator, 8% in copies of the
block's records made per projection, 7% in entities decoded for the block's projections. The
bodies piled up because the collector's reorder buffer was unbounded: while the next unit in
order was still being prepared (a unit holding megabyte-sized items), the collector kept taking
later units into it, freeing the channels for the other workers; halving the channels lowered
the steady state by a quarter but the peak by 7%. Three changes: the units in flight anywhere
between the reader and the coordinator are bounded (`UNITS_PER_WORKER`, 3 per worker: the
reader takes a permit per unit, the collector returns it when it takes the unit up); an encoded
part is trimmed to its length (`Part::of`: the encoder's buffer grows by doubling); a projection
that accepts every record of a block takes the block without a copy (`apply_inline_many`). On
the same 60k lines: peak 1,641 → 1,286 and 1,231 MB (two runs), steady state 900–1,300 → 600–800
MB, time 472 s → 515 s and 474 s (runs vary by about 6%). What remains of the peak is a window
of 42 units of the largest items, the projections' working set for a block of them, and
mimalloc's retention; bounding the window by bytes rather than units would take it lower still.

**`term_prefix` is not yet as 03 §4.3 has it.** The chapter's index is partial over tenant rows
(`WHERE tenant <> ''`); the migration's (`0002_view.sql`) covers every row, shared ones included,
and `wbsearchentities` reads shared rows through it (`t.tenant IN ($1, '')`). Making it partial
before the search index (0014) serves mirror search would leave that half of the query to a
sequential scan, so it stays as built; with the term-language rule the shared part is small.