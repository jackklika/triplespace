# Testing against real dumps

- **Goal:** take Milestone 1 from "passes on sample dumps" to "runs on the real ones", in three
  stages: adopt Librarybase → sync internetdomains.wiki as a key-mapped provider → sync Wikidata as a
  prefixed provider.
- **History:** written 2026-10-04 against `4b8d63a`; stage 1 completed 2026-10-06 and its checks run
  in CI (§1.5). Moved into the repo on 2026-10-21 from a working note. Measurements and the
  write-path analysis are in [`performance.md`](performance.md). Revised 2026-11-01: stage 1's
  baseline is now a **full-history** adoption ([0035](../decisions/0035-adopting-a-wikibase.md)
  A12), so the dump is `--full` and `lastrevid` and each page's history must equal the source's;
  the 2026-10-06 run was a current-state adoption, the shape `adopt --current-only` keeps.
- **Where to run:** a Linux VM (Debian) or a Mac, from the repo checkout, with a local Postgres.
  Work files go in `~/ts-test/`. Commands assume Linux; macOS differences are noted.

## What the code can and cannot do today

Read this before stage 1, because one of these points changes the stage 1 commands.

1. **The set of providers is fixed when the instance is created.** `instance create --provider …`
   opens the mirror partitions, and no command adds one later. If an instance should ever sync
   Wikidata, pass `--provider wikidata` to `instance create` as well as `--provider internetdomains`.
   The stage 1 commands below do that.
2. **`triplespace sync` reads every Wikibase provider** since
   [0091](../decisions/0091-one-wikibase-adapter.md): it dispatches on the provider's `adapter`
   field, and `scatter-adapter-wikibase` reads Wikidata, Librarybase and internetdomains alike, with
   the key-map rules of the registry entry. Until 0091 it worked for `internetdomains` only.
3. **`sync` loads the entire dump into memory first.** It collects every `SyncItem` into a `Vec`
   before `run_sync` begins, even though `run_sync` accepts any `IntoIterator`. That is fine for
   internetdomains and the 50k Wikidata sample, and impossible for `latest-all.json.gz` (about 115M
   entities). 0013 §9 bootstrap mode (COPY, deferred indexes) is not built either. The full
   Wikidata sync is therefore blocked on code; stage 3 measures what is needed and stops at a slice.
4. **Known Milestone 1 gaps these tests will hit:** the snapshot sweep tombstones a Domain that is no
   longer in the dump, where 0009 §9 wants an empty `put`; `wbsetsitelink`, `wbgetclaims` and
   `wbmergeitems` are not served; `verify` is not written.
5. **Found by stage 1 and fixed:**
   - The stored JSON in an XML dump has no `datatype` on snaks, so adopted snaks were untyped and
     `view.identifier` stayed empty (`3d84c87`; [`performance.md`](performance.md) §2).
   - The Postgres-backed test files dropped every scratch database the process had created, with
     `FORCE`, at the end of each test: a race that terminated sibling tests' connections (`57P01`).
     Each test now drops only its own (#60).
   - Pywikibot and WikidataIntegrator needed server fixes, recorded in
     [`../clients.md`](../clients.md).

## Stage 0: build, preflight and a rehearsal adoption

### 0.1 Build and run the suite against a real database

```sh
cd ~/git/triplespace
git switch main && git pull && git status
cargo build --release --workspace
(cd ui && npm ci && npm run build) && cargo build --release -p triplespace-server -p triplespace-web   # embeds ui/dist

# The tests connect as an admin and CREATE DATABASE one scratch DB per test, so the role needs
# CREATEDB (and CREATEROLE: triplespace-db's tests create roles). tokio-postgres reads neither
# PGPASSWORD nor ~/.pgpass. On Debian, TCP to localhost wants a password ("password missing"), so use
# the Unix socket and peer auth instead:
sudo -u postgres psql -c "ALTER ROLE $USER CREATEDB CREATEROLE;"   # or: createuser --createdb --createrole
# Keep it URL-shaped: several test suites and ui/e2e/run.sh swap the database by replacing the text
# after the last "/", which breaks key=value strings and ?host=/path forms. A percent-encoded socket
# directory keeps that working:
export TRIPLESPACE_TEST_DATABASE_URL="postgresql://$USER@%2Fvar%2Frun%2Fpostgresql/postgres"
# (macOS/Postgres.app with trust auth: postgres://localhost/postgres also works)
# Use the same shape for TRIPLESPACE_DATABASE_URL below.
cargo nextest run --workspace          # or: cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask deps

export PATH="$PWD/target/release:$PATH"
mkdir -p ~/ts-test && cd ~/ts-test
```

**Pass:** everything is green. Check that the database suites actually ran: without the env var they
print "skipping" and pass vacuously. The suites that need the database:
`scatter-log-postgres/tests/postgres.rs`, `triplespace-db/tests/db.rs`,
`triplespace-projections/tests/{step1,steps}.rs`, `triplespace-api-ingest/tests/end_to_end.rs`,
`triplespace-accounts/tests/accounts.rs`, `triplespace-server/tests/{acceptance,forwarded,site}.rs`.

### 0.2 Rehearsal on the Open Miscellaneous Wikibase dump

`OPEN_MISC_DUMP-2026-01-16.xml.gz` is a 30 KB full-history XML dump of other.wikibase.cloud (James
has it; it is not in the repo). It exercises the whole adoption path in seconds, including
multi-revision pages, a redirect and non-entity pages, before the one-way door. Copy it to
`~/ts-test/`.

The dump has 57 pages; the highest page ID is 57 and the highest revision ID 206. There are 34 items
(Q1–Q34) and 12 properties (P1–P12). Main Page is a redirect, and there are 10 other wikitext pages.
Three accounts appear: Harej (1), Zblace (3) and R74n (4).

```sh
createdb ts_rehearsal
export TRIPLESPACE_DATABASE_URL="postgresql://$USER@%2Fvar%2Frun%2Fpostgresql/ts_rehearsal"
echo 'rehearsal passphrase' > owner.pw

triplespace instance create --key-file rehearsal.key \
    --tenant openmisc --base https://other.wikibase.cloud \
    --adopt https://other.wikibase.cloud/ \
    --owner 1 --owner-name Harej --owner-password-file owner.pw

triplespace adopt --tenant openmisc --dump OPEN_MISC_DUMP-2026-01-16.xml.gz \
    --source https://other.wikibase.cloud/ --version openmisc-20260116 --frozen
# expect: a warning about no --counters; adopted 46; rejected 0; accounts 3 (or 2 + owner)

triplespace adopt --tenant openmisc --dump OPEN_MISC_DUMP-2026-01-16.xml.gz \
    --source https://other.wikibase.cloud/ --version openmisc-20260116 --frozen
# expect: adopted 0, unchanged 46 (idempotent)

triplespace status
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT type, count(*) FROM view.entity WHERE tenant='openmisc' GROUP BY type;            -- item 34, property 12
SELECT id, page_id, local_revid FROM view.entity WHERE tenant='openmisc' ORDER BY page_id LIMIT 5;
SELECT actor_key, name, kind FROM view.actor WHERE tenant='openmisc' ORDER BY actor_key;  -- openmisc:1 Harej, :3 Zblace, :4 R74n
SELECT sequencename, last_value FROM pg_sequences WHERE schemaname='log' AND sequencename LIKE 'openmisc.%';  -- item≥34, property≥12, page≥57, revision≥206, user≥4
SELECT job_id, status, counts FROM view.job WHERE tenant='openmisc';
SQL
```

**Pass:**
- Every page ID matches the entity's page in the dump. Every revision keeps the source's revision
  ID (0035 A12), so `local_revid` is the dump's last revision ID for the page, below the revision
  floor. Spot-check three multi-revision items: the content is the last revision's, not the first,
  and `prop=revisions` lists every revision of the dump with its timestamp, author and summary.
- Every sequence floor is at or above the dump's maximum.
- The redirect and the wikitext pages appear as skipped, not as rejected.

**Rebuild equivalence** (repeat after every stage):

```sh
pg_dump --data-only --schema=view "$TRIPLESPACE_DATABASE_URL" | sort > view-before.sql
triplespace rebuild
pg_dump --data-only --schema=view "$TRIPLESPACE_DATABASE_URL" | sort > view-after.sql
diff view-before.sql view-after.sql && echo REBUILD-IDENTICAL
```

Some columns may legitimately differ, such as projection timestamps. If they do, write down which
ones; don't accept the diff wholesale. One difference that is not data: recent `pg_dump` writes a
`\restrict <token>` / `\unrestrict <token>` pair with a random token per run, and those two lines
always differ.

**Negative tests:**
- A second `instance create` on this database must refuse to run.
- `adopt` without `--frozen` must refuse to run.
- Edit one entity's JSON inside a copy of the dump, then re-run `adopt` with that copy. Expect exactly
  one reject, and the stored entity must be unchanged.

## Stage 1: adopt Librarybase (current state ✔ 2026-10-06; full history to rerun)

### 1.1 On the source: freeze, then dump

The order matters: once the dump is taken, the source must stop accepting edits.

```sh
# librarybase.org host
# set $wgReadOnly = 'Migrating to Triplespace'; in LocalSettings.php and confirm an edit fails
php maintenance/run.php dumpBackup --full --include-files=no --quiet | gzip > librarybase-YYYYMMDD.xml.gz
mysql -N -e 'SELECT * FROM wb_id_counters' <db>          # → item=…, property=…
# After an importDump rebuild, wb_id_counters may sit below the highest IDs: fix it before dumping
mysql -N -e 'SELECT MAX(log_id) FROM logging' <db>       # → --log-floor
mysql -N -e 'SELECT user_id FROM user WHERE user_name="James"' <db>   # the owner's source user ID
# Did the wiki use federated properties / entity sources pointing at Wikidata? If so, note the prefix.
```

The dump must carry history: `adopt` refuses a current-state export (every page one revision
while revisions name parents) unless it is given `--current-only`, which is not the baseline.
Copy the dump to `~/ts-test/`. Record its sha256 and its size.

### 1.2 Create the instance and adopt

```sh
createdb ts_lb
export TRIPLESPACE_DATABASE_URL="postgresql://$USER@%2Fvar%2Frun%2Fpostgresql/ts_lb"
echo '<real passphrase>' > owner.pw && chmod 600 owner.pw

triplespace instance create --key-file triplespace.key \
    --tenant librarybase --base https://librarybase.org \
    --provider internetdomains --provider wikidata \
    --adopt https://librarybase.org/ \
    --owner <your source user_id> --owner-name James --owner-password-file owner.pw

/usr/bin/time -v triplespace adopt --tenant librarybase --dump librarybase-20261005.xml.gz \
    --source https://librarybase.org/ --version librarybase-20261005 --frozen \
    --counters item=<n>,property=<n> --log-floor <n> \
    2> adopt.stderr | tee adopt.out
#   add --entity-source wikidata=WD only if 1.1 said federated properties were used
```

`/usr/bin/time -v` reports wall time and peak RSS (`-l` on macOS). The 11m56s for 247k entities
in the results log was a current-state adoption, one record per entity; a full-history adoption
writes one record per revision and has no measured baseline yet, so record its time and the
survey's revision count. [`performance.md`](performance.md) §3 has the `pg_stat_statements`
recipe if it looks far off per revision. The final `untyped N snaks on M properties …` line, if present, names properties the dump
does not define; on a full dump it should be absent.

To rehearse on a prefix of the real dump:
`tools/dump_slice.py librarybase-20261005.xml.gz slice-10k.xml.gz --limit 10000`. A prefix slice's
survey does not see properties defined later in the dump, so it reports untyped snaks the full dump
will not; `--all-properties` carries every property page along, and `--sample N --seed S` draws a
random sample instead (`tools/README.md`).

### 1.3 Checks

```sh
triplespace status        # every projection "current"
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT type, count(*) FROM view.entity WHERE tenant='librarybase' GROUP BY type;
SELECT count(*) FROM view.term WHERE tenant='librarybase';
SELECT count(*) FROM view.identifier WHERE tenant='librarybase';   -- must be non-zero
SELECT count(*) FROM view.actor WHERE tenant='librarybase';
SELECT sequencename, last_value FROM pg_sequences WHERE schemaname='log' AND sequencename LIKE 'librarybase.%';
SELECT job_id, status, counts, started, finished FROM view.job WHERE tenant='librarybase';
SELECT line, reason FROM view.job_reject WHERE tenant='librarybase' ORDER BY line;
SQL
grep -c '^skipping page' adopt.stderr; grep '^skipping page' adopt.stderr | cut -d: -f2- | sort | uniq -c | sort -rn | head
```

**Pass:**
- adopted + rejected + unreadable add up to the survey's entity count (247,011 on the 2026-10-05 dump).
- Every reject and unreadable page has a reason you understand. Group the reasons; each distinct
  class is either a bug or an accepted limitation, so file it as one or the other.
- The floors equal the values passed to `--counters` and are at or above the survey's maximum page,
  revision and user IDs.
- Re-running the same adopt command gives `adopted 0` and is fast.
- Rebuild equivalence passes (0.2), and you record how long `rebuild` takes.

**Spot-check against the live (frozen) source.** After starting the server (1.4),
`tools/api_check.py` does the comparison:

```sh
tools/api_check.py --db "$TRIPLESPACE_DATABASE_URL" --sample 20 --compare https://librarybase.org/
tools/api_check.py --ids Q1 Q<max> P1 P<unusual datatype> …   --compare https://librarybase.org/
```

Per entity it checks:
- `Special:EntityData` serves it, and every snak (mainsnak, qualifiers, references) carries `datatype`.
- `wbgetentities` agrees and has `pageid`/`lastrevid`.
- `wbsearchentities` finds it by label. A label shared as a prefix by more than 50 records is a note,
  not a failure.
- `view.identifier` has a row per external-id mainsnak.
- The entity equals the source's `Special:EntityData` output minus `lastrevid/modified/pageid/ns/title`.
- `pageid` equals the source's.
- `lastrevid` equals the source's (0035 A12: the newest source revision is the current state,
  under its own ID, with no adoption revision after it).
- The page's whole history equals the source's, revision by revision: `revid`, `parentid`,
  `timestamp`, `user`, `userid`, `comment` and `minor`, read through `prop=revisions` on both
  sides by page ID. The output's `N revisions equal to the source's` says how many.
- With `--current-only` (a tenant adopted that way), `lastrevid` is instead above the source's,
  the one adoption revision over the floor (0035 §4), and the history is not compared.

Both equalities hold only while neither side has changed since the dump. Run `--compare` before
§1.4's writes, or leave the entities they edit out, and keep the source read-only until it is
done.

Pick IDs that include the lowest Q, the highest Q, every P with an unusual datatype, and a few
entities with heavy statements, qualifiers, references and many languages.

### 1.4 Serve and exercise the API

```sh
triplespace-server --database "$TRIPLESPACE_DATABASE_URL" --key-file triplespace.key \
    --mode development --dev-tenant librarybase --listen 127.0.0.1:8080 &

A=http://localhost:8080/w/api.php
curl -s "$A?action=query&meta=siteinfo&siprop=general|triplespace|providers&format=json" | jq .
curl -s "$A?action=wbgetentities&ids=Q1|P1&format=json" | jq '.entities|keys'
curl -s "$A?action=wbsearchentities&search=<a known label>&language=en&format=json" | jq '.search[].id'
curl -s "$A?action=query&list=logevents&letype=import&format=json" | jq '.query.logevents|length'

triplespace subsidiary create --tenant librarybase --name "James bot" --operator James --group bot
triplespace subsidiary key --tenant librarybase --name "James bot" --label test --grant editentity --grant highvolume
# --json prints {key_id, login, secret, bearer} for scripts
```

**WikibaseIntegrator** (the script does steps 1–3 and reads every write back):

```sh
tools/api_check.py --db "$TRIPLESPACE_DATABASE_URL" --ids Q1 \
    --wbi-login "James bot@test" --wbi-secret "$SECRET" --wbi-create-property
```

1. Edit a label, add a statement and remove a statement on an adopted item, passing `baserevid`; a
   stale `baserevid` must be refused (`editconflict`, 0006 §8).
2. Create a new item. Its ID must be above the item counter floor, never a reused number.
3. Create a new property, likewise above the property floor. (These writes are permanent.)
4. Open `/wiki/Item:Q<n>` in the browser: tabs, `uselang=de`, the term editor and the group editor.
5. Log in as the owner with `clientlogin` (plain `action=login` with the owner's password answers
   `ts-use-oauth`; bot passwords are for subsidiaries).
6. Run `rebuild` again and confirm the edits survive it.

**Pywikibot:** the same story through Pywikibot's own probes. The script writes its family file and
config into a temporary directory and uses a fresh API cache, so it tests the server as it is now
(Pywikibot's 30-day API cache otherwise masks whether a fix worked):

```sh
tools/pwb_check.py --login "James bot@test" --secret "$SECRET" --item Q1 --item-floor <item counter>
```

**WikidataIntegrator** (`setuptools<80`):

```sh
tools/wdi_check.py --login "James bot@test" --secret "$SECRET" --item Q1 --item-floor <item counter>
```

Each prints `N passed, M failed, K skipped` and exits non-zero on any failure. What each library
needs from the server is in [`../clients.md`](../clients.md).

### 1.5 The same, in CI

`tools/client_compat.sh` does 1.2–1.4 unattended against the Librarybase sample fixture (made from
the full dump, `tools/README.md`) and the internetdomains fixture, then runs all three client checks;
any failure fails the run. CI's *Client compatibility* job runs it on every push with the clients
pinned in `tools/requirements-compat.txt`. Bumping a pin is a deliberate change whose result goes in
the results log below and in `docs/clients.md`.

Locally: `python3 -m venv .venv && .venv/bin/pip install -r tools/requirements-compat.txt`, then
`TRIPLESPACE_TEST_DATABASE_URL=… PYTHON=.venv/bin/python tools/client_compat.sh` (about 90 seconds
after the build; `CHECKS="pwb"` for one, `KEEP_DB=1` to inspect).

## Stage 2: sync internetdomains.wiki (keyed entities)

Use the same `ts_lb` database, so that Domains sit next to the adopted entities. Migration 0008 must
be in: it keeps the per-mirror-record `overlay_tenants` lookup from scanning the adopted rows.

### 2.1 Get the dump

On the internetdomains.wiki host:

```sh
php maintenance/run.php dumpJson --entity-type item --entity-type property | gzip > internetdomains-20261005.json.gz
# or an XML dump (Special:Export works without shell access); sync accepts either
```

### 2.2 First sync (upsert)

```sh
/usr/bin/time -v triplespace sync --provider internetdomains \
    --dump internetdomains-20261005.json.gz --version 20261005 2> sync-xd.stderr | tee sync-xd.out
head -3 sync-xd.stderr      # "N entities, M mapped to domains, K keys in conflict"
```

### 2.3 Checks

```sh
psql "$TRIPLESPACE_DATABASE_URL" <<'SQL'
SELECT type, count(*) FROM view.entity WHERE id LIKE 'domain:%' OR id LIKE 'XD%' GROUP BY type;
SELECT count(*) FROM view.keyed_map WHERE provider='internetdomains';
SELECT key, count(*) FROM view.keyed_map GROUP BY key HAVING count(*)>1;                 -- must be empty
SELECT reason, count(*) FROM view.job_reject r JOIN view.job j USING (tenant, job_id)
  WHERE j.source='internetdomains' GROUP BY reason;
SELECT entity_id, upstream_version FROM view.entity_source WHERE graph='mirror/internetdomains' LIMIT 5;
SELECT upstream_id, key FROM view.keyed_map WHERE key ~ '^xn--' LIMIT 5;                   -- IDN normalized to A-label
SQL

curl -s "$A?action=wbgetentities&ids=domain:wikipedia.org&format=json" | jq .
curl -s "$A?action=wbgetentities&ids=XDP1|XDQ<an unmapped item>&format=json" | jq '.entities|keys'
curl -s "$A?action=wbgetentities&ids=domain:never-heard-of-this.example&format=json" | jq .   # empty-but-valid Domain (0009 §4)
```

**Pass:**
- mapped + unmapped + rejected equals the entity count, and the "K keys in conflict" figure matches
  the duplicate rejects.
- Every rejected item still exists as an `XDQ` item that keeps its `P1`.
- A mapped Domain has no `P1`.
- A statement upstream that pointed at a mapped item is now a `wikibase-domain` value. Find one
  `Q`→`Q` link in the source and check it.
- `Domain:` pages render in the site (`/wiki/Domain:wikipedia.org`, Identifiers tab).
- If the input is an XML dump, snaks still carry `datatype`.

### 2.4 Local assertions on a foreign key (resolution)

With the bot account:
1. Add a statement and an English label to `domain:wikipedia.org`.
2. Read it back. You should get the union of the statements, with the local label winning (0013 §5.1).
3. Confirm the mirror's partition is untouched: the local edit went to `local`, not to
   `mirror/internetdomains`.
4. Edit an `XDQ` item locally. Same expectations.

### 2.5 Re-sync and snapshot

```sh
triplespace sync --provider internetdomains --dump internetdomains-20261005.json.gz --version 20261005
# expect: created 0, updated 0, unchanged = all (the cursor works)

# Edit 2 items upstream, delete 1, add 1. Take a new dump:
triplespace sync --provider internetdomains --dump internetdomains-20261006.json.gz --version 20261006 --snapshot --threshold 10
# expect: created 1, updated 2, tombstoned 1
```

Then check:
- The local statement added in 2.4 survives the upstream update to the same Domain.
- If the deleted item was a mapped Domain, it is tombstoned. That is the known gap (0009 §9 wants an
  empty `put`); record the behaviour, since the fix will be tested against it.
- `--threshold 0` with one deletion fails the job rather than sweeping.
- Rebuild equivalence passes.

## Stage 3: sync Wikidata

Read [`performance.md`](performance.md) §6 first: it sets expectations (2–4 weeks for the full dump
on the per-entity path) and names the design question (whether mirror entities populate `view.term`
at all) to settle before 3.2. Note also that `rebuild` replayed Librarybase's 247k records in
10 minutes, nearly the adoption's 12: the catch-up path costs about what the inline path does, and it
is the path a Wikidata resolution pass would take, 500 times over.

### 3.0 Prerequisite code

On one branch, before any testing:
- ~~Add `prepare_wikidata` to `sync.rs`.~~ Done by 0091: `prepare_wikibase` serves every Wikibase
  provider, and reads a JSON dump without key maps in one pass.
- Make the sync stream: pass `run_sync` an iterator instead of a `Vec`. This works for upsert mode;
  snapshot mode needs a set of the IDs it has seen, so the full dump cannot be swept yet.

### 3.1 The 50k sample

`docs/api/snapshots/big/wikidata-20261004-sample-50000.json.gz` holds 50,000 uniformly random items
and all 13,972 properties, in the dump's shape (see its manifest).

```sh
cp ~/git/triplespace/docs/api/snapshots/big/wikidata-20261004-sample-50000.json.gz ~/ts-test/
/usr/bin/time -v triplespace sync --provider wikidata \
    --dump wikidata-20261004-sample-50000.json.gz --version sample-20261004 2> sync-wd.stderr | tee sync-wd.out
```

**Pass:**
- created + rejected + unreadable equals 63,972.
- Every unreadable entity and reject is grouped by cause. This is the datatype-coverage test:
  lexeme, form and sense values, `musical-notation`, `geo-shape`, `tabular-data`, `entity-schema`
  and Commons media should all parse.
- `SELECT type, count(*) FROM view.entity WHERE id LIKE 'WD%' GROUP BY type` gives 50,000 items and
  13,972 properties.
- `wbgetentities&ids=WDQ<sampled>` round-trips against `Special:EntityData` on wikidata.org with IDs
  rewritten: every `Q…` and `P…` becomes `WDQ…`/`WDP…`, including statement GUID prefixes, qualifier
  and reference snaks and `snaks-order`.
- `/wiki/Item:WDQ<sampled>` renders. Labels for entities outside the sample show as IDs, which is
  expected.
- **Cross-graph:**
  - A sampled item's label is found by search, from the Wikidata mirror, next to local results.
  - If Librarybase was adopted with `--entity-source wikidata=WD`, adopted statements that point at
    `WDQ…` now resolve labels for the sampled entities. If not, Librarybase's external-ID links to
    Wikidata are just strings, and that is expected.
- Re-sync: everything comes back unchanged.
- Record entities per second, peak RSS, database size
  (`SELECT pg_size_pretty(pg_database_size(current_database()))`) and the `pg_stat_statements`
  per-entity table ([`performance.md`](performance.md) §3).

### 3.2 A real-dump slice

Download `latest-all.json.gz` (about 140 GB) from dumps.wikimedia.org and cut a prefix rather than
writing a new reader:

```sh
zcat latest-all.json.gz | head -n 1000001 | sed '$ s/,$//' | { cat; echo ']'; } | gzip > wd-slice-1m.json.gz
triplespace sync --provider wikidata --dump wd-slice-1m.json.gz --version slice-1m-<dumpdate>
```

The dump is ordered by ID, so the first million entities are old, dense, heavily linked items: the
worst case for size per entity. Record the same measurements as in 3.1 at 1M, then repeat with 5M if
the 1M run looks linear.

### 3.3 Extrapolate and decide

From 3.1 and 3.2, project the full sync's wall time, database size and log size.

- **Stop here** unless the projection is acceptable through the normal append path. On the evidence
  so far it will not be ([`performance.md`](performance.md) §6).
- The full run then needs 0013 §9 bootstrap mode (batch-shaped writes, deferred indexes, set-based
  projections) and a streaming snapshot sweep. Those are separate pieces of work, not something the
  test can push through.
- Keep the slice instance: it is the baseline the bootstrap work is measured against.

## Results log

One row per run.

| Run | Input (sha256, entities) | Wall | Peak RSS | DB size after | Created / adopted | Rejected | Unreadable | Rebuild identical? | Notes |
|---|---|---|---|---|---|---|---|---|---|
| 0.2 rehearsal | | | | | | | | | |
| 1.2 Librarybase (before) | `librarybase-20261005.xml.gz`, 247,011 entities / 247,055 pages | 51m44s (10m50 user, 11m31 sys) | | | 247,011 | 0 | 0 | | 2026-10-05, `main` before #56. 44.5 SQL statements/entity, `view.identifier` empty (untyped snaks) |
| 1.2 Librarybase 10k slice | first 10,000 entities | 2m02s → 35s → 34s | | | 10,000 | 0 | 0 | | successive runs: baseline → after 3.1–3.6 → with typed snaks; 44.5 → 17.0 statements/entity |
| 1.2 Librarybase (after) | same dump | **11m56s** (4m58 user, 1m58 sys) | | | 247,011 | 0 | 0 | **yes**: `rebuild` 10m01s (3m45 user, 1m28 sys), 1,481,456 view rows, diff empty but for pg_dump's `\restrict` token | 2026-10-05/06, after `3d84c87` + #56 + migration 0008. 17.05 statements/entity; `view.identifier` 246,453 rows |
| 1.2 Librarybase, full history | new deployment's `--full` dump | | | | | | | | to rerun on a dump of the rebuilt librarybase.org (page and user IDs reassigned by its import) |
| 1.3 api_check, 20 random, full history | | | | | | | | | |
| 1.3 api_check, 20 random (current state) | | | | | | | | | 2026-10-06: **0 failures**. Content identical to source for 20/20; pageid equal; lastrevid = source + 247,617 (fresh IDs above the floor, 0035 §3); 2 notes for labels shared as a prefix by >50 records, matching the live wiki |
| 1.4 WBI + owner + rebuild | Q1 | | | | Q250001, P51 | | | **yes**: second rebuild identical with the edits in it | 2026-10-06: WikibaseIntegrator 0.12.15 bot login; fr label set (lastrevid 248238 → 495249); stale baserevid → `editconflict`; P3 statement added and removed; item above floor 250000, property above floor 50; browser tabs and editors work; owner `clientlogin` PASS |
| 1.4 Pywikibot | Q1 | | | | 1 item | | | | 2026-10-06: **14 passed, 0 failed** (Pywikibot 11.8.0) after server fixes for `thumblimits`/`imagelimits`/`magiclinks`, `generator` version, structured `paraminfo`, `meta=wikibase`, `query+tokens`/`wbsearchentities` params. Stale baserevid surfaces as `OtherPageSaveError` wrapping `editconflict` |
| 1.4 WikidataIntegrator | Q1 | | | | 1 item | | | | 2026-10-06: **11 passed/info, 0 failed** (WDI 0.9.30) with `setuptools<80`, `BACKOFF_MAX_TRIES=2`, endpoint at `<api>/sparql` (404, continues). `write()` sends no baserevid (info); stale `delete_statement` refused |
| 1.5 client_compat.sh | `librarybase-sample.xml.gz` (225 entities, the earlier fixture cut from the 10k slice) | ~90 s incl. adopt | | | 225 | 0 | 0 | | 2026-10-06, Postgres 16: all three checks pass (WBI 0 failures; Pywikibot 14/14; WDI 11/11). The fixture has since been regenerated from the full dump |
| 2.2 internetdomains | 312,926 entities | | | | | | | | Synced 2026-10-10; measurements not recorded |
| 2.5 snapshot | | | | | | | | | |
| 3.1 WD sample | | | | | | | | | |
| 3.2 WD 1M slice | | | | | | | | | |
