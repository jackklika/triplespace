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
3. `python3 tools/dump_slice.py librarybase-20261005.xml.gz librarybase-slice-10k.xml.gz --limit 10000`
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
