# Demofarm: one instance, four tenants, two real Wikibases

- **Goal:** exercise what Triplespace has built so far, end to end on one instance, with two real
  Wikibases:
  - tenancy;
  - adoption with history;
  - foreign entities under a **global** prefix (a registry provider) and under a **local** prefix
    (a tenant's entity source);
  - the sync: upsert, snapshot, redirects, rejects, the cursor;
  - local statements on foreign entities;
  - the Action API and the three client libraries;
  - the site, discussions and watchlists;
  - rebuild equivalence.
- **Layout:** instance (farm) `demofarm`, with four tenants:

  | Tenant | Role |
  |---|---|
  | `control` | The primary tenant. Holds nothing of either wiki, so it shows what leaks across tenants |
  | `library` | **Adopts** Librarybase from `data/librarybase-20261005.xml` |
  | `creditunions` | **Adopts** the Credit Unions Wiki from `data/creditunionswiki-20261004.xml` |
  | `reader` | **Reads both as foreign wikis**: Librarybase through the registry provider `LB` (global prefix, `LBQ1`, `LBP5`), and the Credit Unions Wiki through its own entity source `opencredit` (local prefix, `opencredit:Q8`), synced first from the dump and then from the live wiki |

  The Credit Unions Wiki, OpenCredit on Miraheze, is not registered as a provider: it is a
  foreign wiki under a tenant-local prefix, `opencredit:` (James, 2026-10-26).
- **History:** written 2026-10-25 against `main` at `93941ca`. Revised 2026-10-26 for
  [0091](../decisions/0091-one-wikibase-adapter.md), the entity-source route and the Librarybase
  dump. Companion to [`real-dumps.md`](real-dumps.md): its stage 0 (build, database, rebuild
  equivalence) is assumed here, and its stage 1 is the single-tenant Librarybase adoption this
  plan repeats on a farm. Add rows to the results log at the end of this file.
- **Where to run:** James's Mac, from the checkout, with a local Postgres. The dumps are in `data/`
  (gitignored). Phase 5 needs network access to Miraheze, which the cloud sandbox does not have.
  Work files go in `~/ts-test/demofarm/`.

## The dumps

### Librarybase: `data/librarybase-20261005.xml`

769 MB, uncompressed. The same dump as `real-dumps.md` stage 1, MediaWiki 1.39.1.

| | |
|---|---|
| Pages | 247,055 |
| Entities | 247,011: 246,968 items (Item, ns 120) and 43 properties (Property, ns 122; highest P44) |
| Revisions | 248,114 |
| Redirects | 5 (merged items) |
| Other pages | 27 Project, 5 User, 6 User talk, 4 MediaWiki, 2 Template |
| Accounts | Database import (4, 246,450 revisions), Harej (3, 1,655), Librarybase (1), Egon Willighagen (20), Example (7), Colby (24) |

The dump's `<base>` is a container host (`http://792abbf5ae5f/…`), not `librarybase.org`, so
`--source https://librarybase.org/` is always given explicitly. The six user talk pages are
converted into threads after the adoption job (0090 §10; built 2026-10-26): two are redirects,
three belong to users the dump never names as accounts, and one (Harej's welcome to himself)
becomes a thread.

### The Credit Unions Wiki: `data/creditunionswiki-20261004.xml`

37 MB, uncompressed (a `.gz` copy sits beside it). A full-history export of OpenCredit,
MediaWiki 1.46.2.

| | |
|---|---|
| Pages | 5,083; page IDs 1–5,083, no gaps |
| Revisions | 5,088; revision IDs 1–5,088. Every entity has one revision; Main Page has six |
| Items | 5,070, Q1–Q5095 (25 numbers unused or deleted) |
| Properties | 12, P1–P12: `string` ×8, `quantity` ×4 (P6–P9), `wikibase-item` ×2 (P4, P12) |
| Wikitext | Main Page only (ns 0); no talk pages, no redirects, no files |
| Accounts | MediaWiki default (1, 1 edit), Harej (2, 24), Harej bot (12, 5,063); no IP edits |
| Statements | 58,337, every one with one reference (P12 → Q7, the NCUA dataset); no qualifiers |
| Terms | English labels on everything; descriptions on the class and unit items; no sitelinks |

What it exercises that Librarybase does not:

- **Miraheze's namespace numbers.** Item is 860 and Property 862, not 120/122. Entity pages are
  recognized by content model, so this is harmless; the sandbox run (results log) confirms it.
- **Unit IRIs.** 20,252 quantity values have a unit pointing at the wiki's own items:
  `https://creditunions.miraheze.org/entity/Q6` (U.S. Dollars, 15,189) and `…/Q4` (members,
  5,063). Units are stored as raw IRIs today; audit item G21 (prefixed unit and globe IDs) is not
  applied.
- **Empty maps serialized as lists.** 7 items have `"claims": []`, 5,066 have `"aliases": []`,
  all have `"sitelinks": []`.
- **A system account.** "MediaWiki default" (user 1) authored Main Page's first revision.
- **Full history on a wikitext page** (Main Page, six revisions by two users).

## What the build can and cannot do today

Read this first: points 2–3 decide what phase 0 has to build before phases 4 and 5 can run.

1. **The adapter is ready.** Since [0091](../decisions/0091-one-wikibase-adapter.md), `sync`
   chooses the adapter by the provider's `adapter` field, and `scatter-adapter-wikibase` reads any
   Wikibase dump, XML or JSON, with or without key-map rules. Phases 2 and 3 run on today's code.
2. **Entity sources (0078) are not built at all.** `reader`'s source is an `entity-source` record
   in its own `config`:
   - a name, `opencredit`;
   - a number from the tenant's range 2^12–2^13 − 1;
   - a `source/opencredit` partition in the tenant;
   - IDs of the form `opencredit:Q8` that only `reader` reads ([05](../architecture/05-providers-and-ingest.md) §6).

   None of the record, the partition, the colon-form IDs, the source namespace or the reads exists
   yet. Phase 0 builds the part this test needs.
3. **Loading a source and following it are designed, not built.**
   [0092](../decisions/0092-following-recent-changes.md) lets a source be loaded from a dump
   (`sync --tenant reader --source opencredit`) and follows a wiki without an event stream by
   polling its `list=recentchanges`, handing over from the load at the dump's newest timestamp.
   Phase 0 builds both.
4. **A registry provider's mirror is the instance's, not a tenant's.** Every tenant reads every
   mirror: the per-tenant `providers` opt-in (0018 §6) is not built. So `LBQ…` entities are visible
   on `control`, `library` and `creditunions` as well as on `reader`. Record that as an observation,
   not a failure. `opencredit:` entities, by contrast, are `reader`'s alone (05 §6.4).
5. **The set of providers is fixed at `instance create`.** `--provider librarybase` must be on
   that command; no command adds a provider later.
6. **Miraheze has no event stream**, so the follower polls recent changes (0092 §1). It needs the
   dump to be younger than the wiki's recent-changes window (MediaWiki's default is 90 days); the
   2026-10-04 dump is well inside it until early January 2027.
7. **Adoption expects a frozen source.** `adopt --frozen` is the operator's declaration. Neither
   wiki is frozen, so `library` and `creditunions` are forks as of their dump dates. That is fine
   for a test, and it is what makes phase 5's upstream edits possible.
8. **The 0080 §3 binding is not enforced.** An instance should refuse to sync a provider whose
   registry `issuer` is one of its own tenants, since that tenant's `local` partition is the
   provider. `library` adopts under its own derived issuer code, not `librarybase`, so the main
   path never meets the rule: the LB mirror and the `library` fork sit side by side. Phase 6 probes
   the rule on a scratch database.
9. **Known gaps this will hit:**
   - G21: unit IRIs stay raw;
   - `wbgetentities` `modified` is append time;
   - ~~the sync loads the whole dump into memory~~ (it streams since 2026-11-04);
   - no `record` ACLs from adoption;
   - no SPARQL endpoint.

## Decisions for James

- ~~**D1. A dump sync into an entity source.**~~ *Settled by 0092 §4: a source is loaded by a
  dump sync or a crawl, as well as filled by its fetch job.*
- **D2. Names.** I proposed these slugs; they can change before phase 1:
  - The tenants are `library`, `creditunions` and `reader`.
  - The adopting tenant is not called `opencredit`, so that `opencredit` means only the source.
  - The source is `opencredit`, labelled "OpenCredit (Credit Unions Wiki)".
  - A source has no issuer (05 §6.4), so the Credit Unions Wiki's upstream actors are not recorded
    on `reader`. Librarybase's are, as `librarybase:{n}`.
- ~~**D3. How to fetch from the live wiki.**~~ *Settled by 0092 §1: the follower polls recent
  changes; no fetch tool is needed.*

## Phase 0: prerequisite code (one branch, `demofarm-sources`)

- **P1. The entity-source slice of 0078 this test needs:** **built** 2026-10-27 (Stage B). What
  landed, by crate:
  - **`scatter-providers`**: `EntitySource` (the `entity-source` record, validated: the name
    grammar, no registry name, typed, numbered in 2^12..2^13) and `to_provider`, so a source is a
    `Provider` with `source = true`, no code and `source/{name}` as its mirror graph;
    `Registry::with_sources`, the tenant's table of names (0078 §3), which parses and
    canonicalizes `{name}:{upstream ID}` (`OpenCredit:q8` → `opencredit:Q8`) and refuses a name
    the registry has; `from_upstream` rewrites `Q8` → `opencredit:Q8`.
  - **`scatter-log`**: `per_source` and the `{name}` placeholder; `graphs.toml` registers
    `source/{name}` (the README's pending entry is struck; the derived graph needs a key of its
    own).
  - **`scatter-wikibase-changeset`**: `Graph::Source`, `JobHeader::source`, the validator's
    `WrongProvider` on a source graph by prefix, source IRIs from the type's template.
  - **`scatter-ingest`**: `run_sync_in(tenant, …)` (the source's job, partition, cursors and
    records under the tenant); a source's entity takes no surrogate and tombstones on deletion;
    `run_follow` under a tenant.
  - **`triplespace-projections`**: `entity_sources` (the declared records; `registry_for`, the
    tenant's table); `source/` partitions count as mirrors, the keyed map skips them, and a colon
    prefix that is no keyed type files under its own ID.
  - **`triplespace-api-ingest`**: `sources::write`, the one writer of `entity-source:{name}`
    (`ts-config`; the number assigned; the partition opened with its `graph:` record on the first
    record; `null` retires); `ProviderFollower` under a tenant and kind `source`.
  - **`triplespace-api-action` / `-rest`**: `App::registry_for(tenant)`, the cached table of
    names; `wbgetentities`, `Special:EntityData` and the REST entity routes canonicalize source
    IDs, answer `Item:opencredit:Q8` as the title, `missing` for an unheld source ID and
    `invalid-entity-id` for a colon prefix that is neither a keyed type nor a source; provenance
    `minted_by: {kind: source, source, upstream_id, iri}`; search finds the tenant's rows.
  - **`triplespace-ui`**: the identity line "Foreign item, from the source {name}" and the
    source's name as the graph chip.
  - **`triplespace-cli`**: `source add|list|show|retire --tenant`, `sync --tenant --source`
    (recording `loaded_to` under `(tenant, source, name)`), `follow --tenant --source`,
    `upstream-login … --tenant --source`.
  - Tests: `scatter-providers` (validation, the tenant table), `triplespace-api-ingest`
    `end_to_end::a_tenant_declares_and_loads_an_entity_source` (the writer, the refusals, a sync
    into the source, a deletion, the instance refused a source graph, retirement) and
    `triplespace-server` `tests/sources.rs` (the CLI path end to end: declare, load the
    Librarybase sample as source `lb`, read through the Action and REST APIs).

  **Checked** in the sandbox on `ts_follow`: `source add` on `reader` → number 4096,
  `source/opencredit` opened; `sync --tenant reader --source opencredit` of the 5,082-entity Credit
  Unions dump in 1m28s (debug build), all as `reader` rows (`view.entity` item 5070, property 12),
  `loaded to 2026-08-30T02:53:48Z`; `follow --tenant reader --source opencredit --once` against the
  mock wiki (the P3 scenario on Q8–Q12) gave the same counts as the provider run, starting
  `loaded_to` less `follow.tail`; the entity page renders statements with the source's property
  labels.

  **Live run, 2026-10-27 (James, Mac):** the dump load (5,082 created, 30 s release), the
  first follow (`596 rows … 3 created, 3 updated, 576 unchanged, 1 tombstoned, 1 redirected`
  for edits A–H plus the wiki's own September edits), the quiet second follow, and every read
  of §4 passed. **Undoing the edits found a bug:** an upstream *undelete* restores the item's
  old revisions, so its state comes back at a revision the mirror had already recorded, and
  the follower's `put` tried to claim that revision ID twice (`activity_revid`, `(tenant,
  revid)`). Fixed in `scatter-ingest::sync` (`undo.patch`): the sync now takes the highest
  revision any record under the entity's key has claimed (`IngestStore::highest_revid`), a
  tombstone or redirect in between notwithstanding, and writes the returning state as
  `{revid}+{version}`, the "revision already claimed" form that key-map moves already used.
  Test `an_undeleted_entity_comes_back_without_claiming_its_revision_twice`
  (mutation-checked). Projection errors now carry the server's message
  (`triplespace_projections::common::describe`), so the next such failure names its
  constraint instead of saying `db error`.

  **Deviations to confirm** (flagged, not settled):
  - `source add` with a name already declared **updates** the record (label, API, licence) and
    keeps the number, as a config write does; the plan's "refuses" below was my guess. The types
    of a source that already holds records can be changed this way, which nothing checks yet.
  - The table-of-names parsing lives in `scatter-providers` (`Registry::with_sources`), not in
    `scatter-wikibase-model` as 22 §2.1 has it: the model's parser still takes any colon form as
    `IdForm::Keyed`, and the registry says which prefix is a source. The write path, the
    projections and the reads consult the registry at their edges; a few sites still treat every
    colon form as keyed (`rank`, the UI's `display_title`, `canonical_iri` of a keyed ID in the
    changeset's `rewrite_id`), which a source ID never reaches on these paths.
  - Not built: the source's own namespace (0078 §5), `entity_sources` in siteinfo and
    `GET /entity-sources` (0078 §10), `entities.mirror`/`entities.closure` taking a source name,
    `source promote` and `registry check`, the `tenant`/`base` fields (0080), `namespace`,
    `events` and `role` on the record (the writer rejects them), `GET /resolve` for source IDs,
    and the search `concepturi` for a source entity (it is `{base}/entity/{id}`, as for every
    foreign entity).
  - The sync's `Hasher::mirrored_from(code)` is given the empty code for a source; both the
    load and the reads use it, so hashes agree, but a source should have a hasher of its own
    (0078 §2 says nothing about it).
- **P2. ~~A generic Wikibase path in `sync`.~~** Done by [0091](../decisions/0091-one-wikibase-adapter.md).
- **P3. The follower** (0092 §1–§3, §5, §10): **built** against a registry provider, 2026-10-27;
  the `--tenant … --source` forms arrive with P1. What landed, by crate:
  - **`scatter-adapter-mediawiki`** (new crate, as the crate map has it): `recentchanges::Poller`,
    the query and batch reading over the namespaces a caller names, with dedupe by `rcid`,
    coalescing by title and the settle and tail windows, as pure functions with tests;
  - **`triplespace-upstream`** (new crate, partial): `Wiki`, one `api.php` client per followed wiki
    with a User-Agent, `maxlag`, `Retry-After` and `429`/`503` retries, one request in flight, and
    the optional bot login (`login`), after which the higher limits (5,000 rows, 500 IDs) are used;
  - **`scatter-adapter-wikibase`**: `follow`, the entity namespaces from `siteinfo` by content
    model, a row's title as an entity ID, and a `wbgetentities` response as states, redirects and
    misses; `Index::restore` from the projected key map, and re-observing an item is not a claim
    against itself;
  - **`scatter-ingest`**: `follow::run_follow` over a `ChangeSource` trait, one `upsert` sync job
    per batch, the position saved after the batch's job;
  - **`triplespace-api-ingest`**: `follow::ProviderFollower`, the `ChangeSource` over the three
    crates above: the position in `ops.repo_cursor`, the gap check against the wiki's oldest
    recent change, the handover from a load (`loaded_to` less `follow.tail`), stale states kept
    to read again, `late`/`stale` counted;
  - **`triplespace-accounts`**: `upstream_login`, the sealed bot password;
  - **`triplespace-db`**: migration `0022_followers` (`ops.repo_cursor`, `private.upstream_login`);
  - **`triplespace-cli`**: `follow --provider {slug} [--once] [--from {iso}] [--interval {s}]
    [--api {url}] [--anonymous]`, `upstream-login set|unset|show`, and `sync` recording the load's
    end as `loaded_to`.

  **Checked** in the sandbox against a mock `api.php` serving the Librarybase sample (an edit, an
  unchanged row, a deletion, a merge, a new item, a state served stale once, and a row outside
  the entity namespaces): after `sync` of the sample, `follow --once` printed `1 created, 1
  updated, 1 unchanged, 1 tombstoned, 1 redirected; 1 stale`; a second run re-read only the
  stale item (`1 updated`) and wrote nothing else. A position older than the wiki's oldest
  change stops with the gap error. The live Credit Unions wiki is reached from the Mac only.

  **Deviations to confirm** (flagged, not settled):
  - 0092 §3 says the position is written in the batch's transaction; it is written after the
    batch's job commits, which the "hint, not guarantee" rule makes equivalent for a crash
    between the two (a replay), but the text should say which.
  - 0092 §3's recovery of a lost position from `view.entity_source` is not built: that table
    holds no upstream time, so a lost row needs `--from` or a new load.
  - 0092 §5's warning at the end of `sync` when the load is already behind the wiki's window is
    not built; the follower reports the gap when it starts.
  - A wiki whose API omits snak datatypes (none known) would make every state "updated": the
    follower applies no property typing, unlike the XML dump path.

**Pass:**
- fmt, clippy, the workspace tests with a database, and `docs/check.sh`.
- `sync --source opencredit` on a ten-entity slice prints created 10.
- `follow --once` against a recorded `list=recentchanges` response replays to the same records
  twice, and the second run writes nothing.

## Phase 1: the farm

```sh
cd ~/git/triplespace && git switch demofarm-sources
cargo build --release --workspace && (cd ui && npm ci && npm run build) \
  && cargo build --release -p triplespace-server -p triplespace-web
export PATH="$PWD/target/release:$PATH" DATA=$PWD/data
mkdir -p ~/ts-test/demofarm && cd ~/ts-test/demofarm
shasum -a 256 $DATA/librarybase-20261005.xml $DATA/creditunionswiki-20261004.xml

# /etc/hosts: 127.0.0.1 demofarm.test control.demofarm.test library.demofarm.test creditunions.demofarm.test reader.demofarm.test auth.demofarm.test
createdb ts_demofarm
export TRIPLESPACE_DATABASE_URL=postgres://localhost/ts_demofarm
export TRIPLESPACE_KEY_FILE=$PWD/demofarm.key
export TRIPLESPACE_AUTH_BASE=http://auth.demofarm.test:8080
echo 'demofarm owner passphrase' > owner.pw && chmod 600 owner.pw

triplespace instance create --key-file demofarm.key \
    --farm-slug demofarm --farm-base http://demofarm.test:8080 \
    --tenant control --base http://control.demofarm.test:8080 --name Control \
    --provider librarybase \
    --owner-name James --owner-password-file owner.pw

triplespace tenant create --tenant library --base http://library.demofarm.test:8080 \
    --name "Librarybase (fork)" --adopt https://librarybase.org/ \
    --owner 3 --owner-name Harej --owner-password-file owner.pw

triplespace tenant create --tenant creditunions --base http://creditunions.demofarm.test:8080 \
    --name OpenCredit --adopt https://creditunions.miraheze.org/ \
    --owner 2 --owner-name Harej --owner-password-file owner.pw

triplespace tenant create --tenant reader --base http://reader.demofarm.test:8080 \
    --name Reader --owner-name James --owner-password-file owner.pw

triplespace source add --tenant reader --name opencredit --label "OpenCredit (Credit Unions Wiki)" \
    --type Q=item --type P=property \
    --iri 'https://creditunions.miraheze.org/entity/{upstream_id}' \
    --api https://creditunions.miraheze.org/w/api.php --revision-ids --licence CC-BY-SA-4.0

triplespace tenant list && triplespace status
```

Check the Credit Unions Wiki's licence before writing the source record; CC BY-SA 4.0 is
Miraheze's default.

**Pass:**
- **Tenants.** Four tenants, each with its own issuer code (0079), `owner` group and owner account.
- **Refusals:**
  - the farm slug `demofarm` is refused as a tenant slug;
  - `librarybase` is refused as a slug for any tenant that does not adopt `librarybase.org` (0079 §3);
  - a second `instance create` on `ts_demofarm` refuses;
  - `tenant create` without `TRIPLESPACE_AUTH_BASE` refuses for the second tenant;
  - `source add` with a name the registry has (`wikidata`, `librarybase`, `domain`) refuses
    (`ts-source-name-taken`, 05 §6.3); with the same name again it updates the record and keeps
    the number (P1 deviation).
- **`status`.** The instance has `mirror/librarybase`; `reader` alone has `source/opencredit`,
  numbered 4096, the first of its range. Every projection is current.

Start the server for the later phases:

```sh
triplespace-server --key-file demofarm.key --mode development --dev-tenant control \
    --listen 127.0.0.1:8080 &
for t in control library creditunions reader; do
  curl -s "http://$t.demofarm.test:8080/w/api.php?action=query&meta=siteinfo&siprop=general|triplespace|providers&format=json" \
    | jq -c '{t: .query.general.sitename, providers: .query.providers}'
done
```

Each answers with its own `sitename`. All four list `librarybase` among the instance's providers
(point 4). Only `reader` lists `opencredit`, among its sources (05 §6.1).

## Phase 2: adoption, small and large

### 2.1 The Credit Unions Wiki on `creditunions`

```sh
/usr/bin/time -l triplespace adopt --tenant creditunions --dump $DATA/creditunionswiki-20261004.xml \
    --source https://creditunions.miraheze.org/ --version creditunionswiki-20261004 --frozen \
    --counters item=5095,property=12 2> adopt-cu.stderr | tee adopt-cu.out
```

`wb_id_counters` is not readable without shell access on Miraheze, so 5,095 and 12 are the dump's
highest IDs. If items have been created on the wiki since the dump, use the live highest Q
instead.

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT type, count(*) FROM view.entity WHERE tenant='creditunions' GROUP BY type;   -- item 5070, property 12
SELECT actor_key, name FROM view.actor WHERE tenant='creditunions' ORDER BY actor_key; -- 1 MediaWiki default, 2 Harej, 12 Harej bot
SELECT sequencename, last_value FROM pg_sequences
  WHERE schemaname='log' AND sequencename LIKE 'creditunions.%';  -- item ≥5095, property ≥12, page ≥5083, revision ≥5088, user ≥12
SELECT job_id, status, counts FROM view.job WHERE tenant='creditunions';
SQL
```

**Pass:**
- **Counts.** Adopted 5,082 entities, 0 rejected, 0 unreadable. Main Page is adopted as a wikitext
  page with all six revisions, attributed to MediaWiki default and Harej.
- **IDs.** Every page ID equals the dump's; `local_revid` is above the floor; the source revision
  is kept as `source_revid`.
- **Idempotence.** Re-running the command gives adopted 0, unchanged 5,082, and is fast.
- **Negative tests** (`real-dumps.md` §0.2): without `--frozen` it refuses, and one entity edited
  in a copy of the dump gives exactly one reject and leaves the stored entity unchanged.
- **Unit IRIs.** `wbgetentities&ids=Q8` shows `P6`/`P7` units as
  `https://creditunions.miraheze.org/entity/Q4`/`Q6`, not the tenant's own concept base. That is
  G21; record it.

### 2.2 Librarybase on `library`

This is `real-dumps.md` stage 1 again, on a tenant of a farm instead of a primary tenant. Use
stage 1's results row as the baseline: 11m56s, 247,011 adopted.

```sh
/usr/bin/time -l triplespace adopt --tenant library --dump $DATA/librarybase-20261005.xml \
    --source https://librarybase.org/ --version librarybase-20261005 --frozen \
    --counters item=<wb_id_counters item>,property=<wb_id_counters property> \
    2> adopt-lb.stderr | tee adopt-lb.out
grep '^skipping page' adopt-lb.stderr | cut -d: -f2- | sort | uniq -c | sort -rn
```

Use the counters recorded for stage 1. Without them, the warning names the dump's highest IDs
(Q246968, P44).

**Pass:**
- **Counts.** Adopted + rejected + unreadable is 247,011, less the 5 redirects, which are
  skipped: local entity redirects are not served yet (backlog). The 27 Project pages, the 5 User
  pages and the 2 Templates are adopted as wikitext; the 4 MediaWiki pages are reported as
  deferred, not rejected.
- **Talk pages.** `talk 1 pages converted into 1 threads, 1 posts`, and 5 deferred: 2 redirects
  (`User talk:Librarybase`, `User talk:Database import`) and 3 whose users have no account in
  the dump. `Special:RecentChanges` on `library` shows the thread's creation at the source's
  time, by Harej; `/wiki/User_talk:Harej` lists "Welcome to Librarybase!" as an archived thread
  with its 2025-08-31 post.
- **Speed.** Wall time is within about 10% of stage 1's. A multi-tenant instance should cost
  nothing extra per entity; if it does, the `pg_stat_statements` recipe of `performance.md` §3
  says where.
- **Floors.** The floors are at or above the counters. `library`'s sequences are independent of
  `creditunions'`: both have a `Q8`, and they are different items.

### 2.3 API, clients and the site, on both adopted tenants

```sh
for t in creditunions library; do
  A=http://$t.demofarm.test:8080
  triplespace subsidiary create --tenant $t --name "Harej bot 2" --operator Harej --group bot
  triplespace subsidiary key --tenant $t --name "Harej bot 2" --label test \
      --grant editentity --grant highvolume --json > $t-key.json
done
CU=http://creditunions.demofarm.test:8080; LB=http://library.demofarm.test:8080
tools/api_check.py --api $CU --tenant creditunions --db "$TRIPLESPACE_DATABASE_URL" --sample 20 \
    --ids Q1 Q4 Q6 Q7 Q8 Q5095 P4 P6 P12 --compare https://creditunions.miraheze.org/
tools/api_check.py --api $LB --tenant library --db "$TRIPLESPACE_DATABASE_URL" --sample 20 \
    --compare https://librarybase.org/
SECRET=$(jq -r .secret creditunions-key.json)
tools/pwb_check.py --api $CU --tenant creditunions --login "Harej bot 2@test" --secret "$SECRET" --item Q8 --item-floor 5095
tools/wdi_check.py --api $CU --login "Harej bot 2@test" --secret "$SECRET" --item Q8 --item-floor 5095
SECRET=$(jq -r .secret library-key.json)
tools/api_check.py --api $LB --tenant library --ids Q1 --wbi-login "Harej bot 2@test" --wbi-secret "$SECRET" --wbi-create-property
```

- **Comparison.** `--compare` needs network access to the source wiki. Expect content equal for
  the sample, `pageid` and `lastrevid` equal, and each page's history equal to the source's
  revision by revision (0035 A12; `real-dumps.md` §1.3). Run it before the WBI writes.
- **New IDs.** New items and properties take numbers above each tenant's own floor, never a
  reused one.
- **Search.** `wbsearchentities&search=TEXARKANA` on `creditunions` finds Q8 ("MORRIS SHEPPARD
  TEXARKANA (TX)"); the same search on `library` finds nothing.
- **Site.** Log in as Harej with `clientlogin`, then:
  - open `/wiki/Item:Q8` on `creditunions`: statement groups, quantities with units, references
    to Q7;
  - open `/wiki/Main_Page` and its History: six revisions, with diffs;
  - on `library`, open `/wiki/User_talk:Harej` and its one converted thread;
  - start a thread on `Item talk:Q8` and watch Q8;
  - reply from a second account that mentions Harej;
  - check the bell and `Special:Watchlist`.
- **Permissions.** Run `tools/acl_check.sh` once on the branch to confirm P1 regressed nothing.
- **Rebuild equivalence** as in `real-dumps.md` §0.2. Record `rebuild`'s wall time: it now
  replays two adopted tenants.

## Phase 3: a foreign provider with a global prefix (Librarybase as `LB`)

The same Librarybase dump, synced as the registry provider `LB` into the instance's
`mirror/librarybase`: the remote-wiki reading of Librarybase, beside the `library` fork.

```sh
/usr/bin/time -l triplespace sync --provider librarybase --dump $DATA/librarybase-20261005.xml \
    --version 20261005 --rejects lb-rejects.tsv 2> sync-lb.stderr | tee sync-lb.out
head -3 sync-lb.stderr      # "indexing …", then "247011 entities, 43 property types"
```

**Checks:**

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT tenant, type, count(*) FROM view.entity WHERE id LIKE 'LB%' GROUP BY 1, 2;  -- tenant '' only: item 246968, property 43
SELECT entity_id, upstream_version FROM view.entity_source WHERE graph='mirror/librarybase' ORDER BY entity_id LIMIT 5;
SELECT job_id, status, counts FROM view.job WHERE source='librarybase';
SQL
R=http://reader.demofarm.test:8080/w/api.php
curl -s "$R?action=wbgetentities&ids=LBQ1|LBP1&format=json" | jq .
```

**Pass:**
- **Counts.** Created + redirected + rejected + unreadable is 247,011, with 5 redirected (mirror
  redirects between `LBQ` items) and 0 rejected.
- **Typing.** Every snak carries `datatype`, typed from the dump's own properties.
- **IDs are rewritten everywhere:** values `LBQ…`, properties `LBP…`, statement GUIDs `LBQ1$…`.
- **Revisions and actors.** Revision IDs are `2 << 40 | upstream revid` (LB is provider number 2).
  The revisions' actors are `librarybase:3` (Harej), `librarybase:4` (Database import) and so on.
- **Same content, two identities.** `LBQ1` on `reader` and `Q1` on `library` have the same content
  and are different entities. Nothing links them; clusters (0004) are not built.
- **Shared rows.** The rows are the instance's (`tenant = ''`), so `LBQ1` reads the same on all
  four tenants (point 4).
- **Re-sync and snapshot.** Re-sync: unchanged 247,011 (the cursor). Snapshot on the same dump
  with `--threshold 0`: swept 0.
- **Measurements.** Record wall time and peak RSS. The sync streams the dump (point 9), so RSS
  should stay flat as the dump grows; a rising RSS is a finding.

**Local statements on `LB` entities, from `reader`** (`real-dumps.md` §2.4):

1. On `reader`, create a local string property "local note" (`reader`'s `P1`).
2. Add `P1 = "overlay test"` and an English description to `LBQ1`.
3. Read `LBQ1` on `reader`: the union, with the local description winning.
4. Read it on `control`: the shared row, without either.
5. `view.entity` has a `(reader, LBQ1)` row and a `('', LBQ1)` row, and `mirror/librarybase` has no
   new record.

## Phase 4: a foreign wiki with a local prefix (OpenCredit as `opencredit:`)

```sh
/usr/bin/time -l triplespace sync --tenant reader --source opencredit \
    --dump $DATA/creditunionswiki-20261004.xml --version 20261004 \
    --rejects oc-rejects.tsv 2> sync-oc.stderr | tee sync-oc.out
```

**Checks:**

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT tenant, type, count(*) FROM view.entity WHERE id LIKE 'opencredit:%' GROUP BY 1, 2;  -- only reader: item 5070, property 12
SELECT entity_id, upstream_version FROM view.entity_source
  WHERE tenant='reader' AND graph='source/opencredit' ORDER BY entity_id LIMIT 5;
SQL
R=http://reader.demofarm.test:8080/w/api.php
curl -s "$R?action=wbgetentities&ids=opencredit:Q8|opencredit:P6&format=json" | jq .
```

**Pass:**
- **Counts.** Created 5,082, rejected 0, unreadable 0; Main Page is skipped as a non-entity.
- **Typing.** Every snak carries `datatype`, typed from the dump's own properties (the XML has none).
- **IDs are rewritten everywhere:**
  - `opencredit:Q8`'s `opencredit:P4` value is `opencredit:Q2`;
  - its references cite `opencredit:P12` → `opencredit:Q7`;
  - statement GUIDs carry the `opencredit:Q8$` prefix;
  - `snaks-order` names `opencredit:P…`.
- **Units.** Record what `opencredit:Q8`'s `opencredit:P6` unit is. Today it will still be
  `https://creditunions.miraheze.org/entity/Q4`; G21 wants `opencredit:Q4`.
- **Revisions and actors.** Revision IDs are `4096 << 40 | upstream revid`. No upstream actor is
  recorded, since a source has no issuer; the job's actor is `reader`'s operator.
- **Tenant rows only.** There is no shared row and nothing in a provider index (05 §6.4).
  `opencredit:Q8` is invalid on the other three tenants, where `opencredit` names nothing.
- **Site and search.** `/wiki/Item:opencredit:Q8` renders with the source chip, and
  `wbsearchentities&search=TEXARKANA` on `reader` finds `opencredit:Q8`.
- **Re-sync and snapshot.** Re-sync: unchanged 5,082. Snapshot on the same dump with
  `--threshold 0`: swept 0.
- **Rebuild equivalence** passes with two adopted tenants, a mirror and a source in the database.

**Local statements, and a statement that crosses both prefixes:**

1. Add `P1 = "overlay test"` to `opencredit:Q8`, and an English description to `opencredit:Q100`.
   Read both on `reader`: the union, with the local description winning. `source/opencredit` has
   no new record.
2. On `reader`, create a local `wikibase-item` property "see also" (`reader`'s `P2`), and add
   `P2 = LBQ1` to `opencredit:Q8`. One statement then holds a local subject's foreign ID under a
   local prefix, a local property, and a foreign value under a global prefix. Read it back through
   `wbgetentities`, `Special:EntityData` (`.json` and `.ttl`) and the entity page. Each ID must keep
   its own form, and `LBQ1`'s label must resolve in the page.

## Phase 5: `reader` syncs OpenCredit from the live wiki

**Upstream edits.** As Harej on `creditunions.miraheze.org`, keeping a list with times and revision
IDs:

| # | Edit on Miraheze | Expected in the next sync |
|---|---|---|
| U1 | Change Q8's English label | `opencredit:Q8` updated; `reader`'s `P1` and `P2` statements survive |
| U2 | Change Q100's P6 (member count) | `opencredit:Q100` updated; `reader`'s description still wins |
| U3 | Create a new credit union item | created, `opencredit:Q5096` or above |
| U4 | Delete Q5095 | snapshot: tombstoned; upsert: unchanged (upsert never deletes) |
| U5 | Merge Q101 into Q102 (`Special:MergeItems`) | `opencredit:Q101` a redirect to `opencredit:Q102`; `opencredit:Q102` updated |
| U6 | Create P13 with datatype `url` or `external-id`, use it on Q8 | `opencredit:P13` created; an `external-id` lands in `view.identifier` |
| U7 | Edit Main Page | skipped |

**Follow to the present.** Phase 4's load recorded where the dump ends (2026-08-30T02:53:48Z, the
dump's newest revision). The source must be at `all`, so that new items are followed too.

```sh
triplespace settings set --tenant reader 'entities.mirror' '{"opencredit": {"item": "all", "property": "all"}}'
triplespace follow --tenant reader --source opencredit --once 2> follow1.stderr | tee follow1.out
triplespace follow --tenant reader --source opencredit --once 2> follow2.stderr | tee follow2.out
curl -s "http://reader.demofarm.test:8080/rest.php/triplespace/v0/provider/opencredit/mirror" | jq .
```

The `settings set` form is a proposal; any way of writing the `site` record will do.

**Pass:**
- **First run.** Every upstream change since the dump arrives, the seven edits among them:
  - created 2 (U3, U6);
  - updated 3 (U1, U2, U5's target);
  - redirected 1 (U5);
  - tombstoned 1 (U4: the follower reads Q5095's state, finds it missing, and tombstones it
    without a snapshot);
  - U7 is not in the entity namespaces and is never read.
- **Second run.** Nothing new upstream, so nothing is written; the position moves only by the
  re-read window.
- **The mirror report.** `recentchanges`, the position near now, `late` and `stale` at or near 0,
  and the load marked as handed over.
- **A lost position.** Delete `reader`'s row from `ops.repo_cursor` and run `follow --once` again.
  It resumes from the newest upstream time in `view.entity_source`, less `follow.tail`,
  and writes nothing.
- **Continuous.** Run `follow` without `--once` and make one more edit on Miraheze. It arrives
  within `follow.interval` (a minute) plus the settle window.
- **Redirects.** `opencredit:Q101` resolves to `opencredit:Q102` through `wbgetentities`
  (`redirects`). If redirects are not followed on read yet (the backlog's "following mirror
  redirects on read"), record it.
- **Local statements.** `reader`'s statements survive both syncs; `opencredit:Q8`'s label is the
  new upstream one.
- **Forks unaffected.** On `creditunions`, Q8 keeps its 2026-10-04 label: it is a fork.
- **Activity.** `GET /activity?source=source/opencredit` on `reader` lists the sync's changes. A
  watch on `opencredit:Q8` from phase 4 shows it as unseen; entity `watch` notifications for syncs
  are not built, so record whether the watchlist row appears.

## Phase 6: cross-cutting probes

**Isolation matrix.** Fill in each cell:

| Probe | control | library | creditunions | reader |
|---|---|---|---|---|
| `wbgetentities` Q8 | missing | Librarybase's Q8 | OpenCredit's Q8 | missing |
| `wbgetentities` LBQ8 | shared row (by design: absent, 0018 §6) | shared row (by design: absent) | shared row (by design: absent) | shared row |
| `wbgetentities` opencredit:Q8 | invalid | invalid | invalid | the source entity, with `reader`'s statements |
| `wbsearchentities` TEXARKANA | none | none | Q8 | `opencredit:Q8` |
| `list=allusers` Harej | absent | present (3) | present (2) | absent |
| `list=recentchanges` | its own; LB syncs? | adoption; LB syncs? | adoption; LB syncs? | LB syncs, source syncs, local edits |
| `/wiki/Main_Page` | its own | Librarybase's `Project:Home`? | adopted, six revisions | its own |

**The 0080 §3 binding, on a scratch database** (not `ts_demofarm`):
1. Create an instance with `--provider librarybase` and primary tenant `librarybase`, adopting
   `https://librarybase.org/`. The 0079 §9 exception lets the tenant take the registered issuer's
   name, because the host matches.
2. Adopt the dump.
3. Run `sync --provider librarybase`. By design it is refused: the tenant is the provider, so a
   mirror of it would be a stale copy (0002 A39; 05 §8.3).

Today it will likely succeed. Record what happens, since this is the test for when 0080 is built.

**Clustering.** `creditunions`' Q8 and `opencredit:Q8` are the same credit union, and `library`'s
Q1 and `LBQ1` are the same item, but nothing links either pair: clusters (0004) are not built.
Record them as the motivating cases, not as bugs.

## Results log

One row per run.

| Run | Input (sha256, entities) | Wall | Peak RSS | DB size after | Created / adopted | Rejected | Unreadable | Rebuild identical? | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 0 sandbox: OpenCredit under a scratch `CU` provider | `creditunionswiki-20261004.xml.gz`, 5,082 entities | 21.5 s | | | 5,082 | 0 | 0 | | 2026-10-26, release build, cloud sandbox. Re-sync: unchanged 5,082 (4.9 s); snapshot swept 0. Tests only the 0091 adapter path, before any source code; the `CU` entry was never committed |
| 1 farm | | | | | | | | | |
| 2.1 adopt `creditunions` | `creditunionswiki-20261004.xml`, 5,082 entities / 5,083 pages | | | | | | | | |
| 2.2 adopt `library` | `librarybase-20261005.xml`, 247,011 entities / 247,055 pages | | | | | | | | baseline: `real-dumps.md` 1.2, 11m56s |
| 2.3 api_check, clients | | | | | | | | | |
| 3 sync `LB` | `librarybase-20261005.xml` | | | | | | | | |
| 3 re-sync, snapshot | same | | | | | | | | |
| 4 sync `opencredit` | `creditunionswiki-20261004.xml` | | | | | | | | |
| 5 follow, first run | | | | | | | | | |
| 5 follow, second run and lost position | | | | | | | | | |
| 6 binding probe | | | | | | | | | |
