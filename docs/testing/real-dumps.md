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
3. **`sync` streams the dump** (since 2026-11-04): `prepare_wikibase` hands `run_sync` an
   iterator that reads one entity at a time, so memory no longer grows with the dump. A first
   pass, when the provider has key maps or the dump is XML, still holds the key index and the
   property types; the job keeps the set of IDs it has seen in every mode (`seen` in
   `scatter-ingest`'s `sync.rs`, which the snapshot sweep reads, along with the mirror's entity
   list), which grows with the number of IDs, not with the entities. Until then `sync` collected every
   `SyncItem` into a `Vec` first, which was impossible for `latest-all.json.gz` (about 122M
   entities). 0013 §9 bootstrap mode (COPY, deferred indexes) is not built, so the full Wikidata
   sync is still blocked on write speed; stage 3 measures what is needed and stops at a slice.
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

## Stage 1: adopt Librarybase (current state ✔ 2026-10-06; full history ✔ 2026-11-02, in CI)

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
the full-history dump, `tools/README.md`) and the internetdomains fixture. Before any client
writes, its `dump` step compares every adopted entity with the fixture itself
(`api_check.py --compare-dump`): content, page ID, `lastrevid` and every revision's history, the
offline form of §1.3's `--compare`. Then it runs all three client checks; any failure fails the
run. CI's *Client compatibility* job runs it on every push with the clients
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
- ~~Make the sync stream: pass `run_sync` an iterator instead of a `Vec`.~~ Done 2026-11-04
  (`SyncItems`). The job still keeps a set of the IDs it has seen (in every mode; the snapshot
  sweep reads it with the mirror's entity list), so a full Wikidata sync holds 122M IDs, not
  122M entities.

### 3.1 The 50k sample

`docs/samples/wikidata/big/wikidata-20261004-sample-50000.json.gz` holds 50,000 uniformly random items
and all 13,972 properties, in the dump's shape (see its manifest).

```sh
cp ~/git/triplespace/docs/samples/wikidata/big/wikidata-20261004-sample-50000.json.gz ~/ts-test/
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

The dump is **not** in ID order: it is written in shards by page ID and opens Q31, Q8, Q23, with
properties scattered through it, so a prefix (`head -n`) is neither the first N items nor carries
the properties its items use. Cut slices by ID instead, from an indexed copy of the dump that
`tools/wikidata_zindex.py` makes once (`tools/README.md`):

```sh
# Pin a dated dump, not latest-all, and take the bz2 (~104 GB): lbzip2 decompresses it on every
# core, where gzip is single-threaded. aria2c from dumps.wikimedia.org and the your.org mirror
# (2 connections each) runs ~16 MB/s, about 1¾ h; one wget, ~5 MB/s.
D=20261026
aria2c --max-connection-per-server=2 --split=4 --min-split-size=256M --max-tries=0 \
    --checksum=sha-1=<from wikidata-$D-sha1sums.txt> -o wikidata-$D-all.json.bz2 \
    https://dumps.wikimedia.org/wikidatawiki/entities/$D/wikidata-$D-all.json.bz2 \
    https://dumps.wikimedia.your.org/wikidatawiki/entities/$D/wikidata-$D-all.json.bz2

# Once per dump (~2 h on 16 cores, bound by bzip2): the archive and its ID index
python3 tools/wikidata_zindex.py build wd-$D --input wikidata-$D-all.json.bz2 --sha1 <sha1>
python3 tools/wikidata_zindex.py verify wd-$D

# The first million items that exist, and every property, in numeric order
{ python3 tools/wikidata_zindex.py ids wd-$D --kind Q --limit 1000000
  python3 tools/wikidata_zindex.py ids wd-$D --kind P; } |
    python3 tools/wikidata_zindex.py get wd-$D --ids-file - | gzip > wd-slice-1m.json.gz
triplespace sync --provider wikidata --dump wd-slice-1m.json.gz --version slice-1m-$D
```

`get` writes JSON Lines, which the dump reader takes as it takes the array form. The dump holds
only entities that exist, so redirects, deletions and gaps in the numbering are already left out:
`ids --limit 1000000` ends where the millionth existing item does, well above Q1000000. Any
other slice (a 5M prefix, a sample, a class of items) is likewise a list of IDs. Keep the archive
with the dump date in its name; `info` prints the source, sha1 and counts it was built from.

For the 2026-10-26 dump: 122,042,968 items and 14,017 properties; 1.85 TB of JSON in a 104.3 GB
archive (bzip2: 103.6 GB) with a 2.6 GB index, built in 2 h 1 min at 4.8 GB peak RSS on 16 cores;
the millionth item is Q1084808; one entity comes back in 0.5 ms.

The first million items are old, dense, heavily linked items: the worst case for size per entity.
Record the same measurements as in 3.1 at 1M, then repeat with 5M if the 1M run looks linear.

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
| 1.2 Librarybase, full history | `librarybase-20261101.xml.gz` (the rebuilt librarybase.org), 247,011 entities, 248,115 revisions | 8m46s (3m00 user, 1m34 sys) | 17 MB | | 248,097 revisions of 247,011 entities | 0 | 0 | | 2026-11-01, Mac, release build. 555 pages with history; 37 document pages; 6 talk pages deferred. The rebuild's `importDump` left every author but `Librarybase` as `imported>Name` (246,450 `imported>Database import`, 1,655 `imported>Harej`), so 1 account and 6 surrogates. Found: surrogates served as `actor:N` with userid N, not as the source shows them; the owner created as a second `Librarybase` stopped `addressing` (0035 A16). Rerun 2026-11-02 with the owner as source user 3: users, IDs and timestamps equal; every summary had `/* adopt */` before the source's (fixed: none on an adopted history revision, 0035 A12); `rebuild` re-projected it, no new adoption needed |
| 1.3 rebuild equivalence, full history | `librarybase-20261101.xml.gz` | `rebuild` 7m50s (2m30 user, 1m33 sys) | | | | | | **no**: 37 `view.page` rows | 2026-11-02. Every document page's `random` was redrawn (fixed: derived from tenant and page ID, 0047 A29, migration 0025); Main Page, deleted on the site that afternoon, came back not deleted because `rebuild` replayed the tenant's `log` before its `pages` (fixed: a tenant's partitions replay interleaved by append time, as 03 §6.1 says). Rerun after both fixes, migration 0025 applied: the first rebuild restored Main Page's `deleted`, and a second rebuild from that state was **identical** |
| 1.3 api_check, 2,500 random, full history | `librarybase-20261101.xml.gz` | | | | | 0 failures | | | 2026-11-02, after `rebuild`, against the live librarybase.org: for 2,500 entities, content, `pageid` and `lastrevid` equal to the source's, and every page's history equal revision by revision (revid, parentid, timestamp, user, userid, summary, minor) |
| 1.3 api_check, 20 random (current state) | | | | | | | | | 2026-10-06: **0 failures**. Content identical to source for 20/20; pageid equal; lastrevid = source + 247,617 (fresh IDs above the floor, 0035 §3); 2 notes for labels shared as a prefix by >50 records, matching the live wiki |
| 1.4 WBI + owner + rebuild | Q1 | | | | Q250001, P51 | | | **yes**: second rebuild identical with the edits in it | 2026-10-06: WikibaseIntegrator 0.12.15 bot login; fr label set (lastrevid 248238 → 495249); stale baserevid → `editconflict`; P3 statement added and removed; item above floor 250000, property above floor 50; browser tabs and editors work; owner `clientlogin` PASS |
| 1.4 Pywikibot | Q1 | | | | 1 item | | | | 2026-10-06: **14 passed, 0 failed** (Pywikibot 11.8.0) after server fixes for `thumblimits`/`imagelimits`/`magiclinks`, `generator` version, structured `paraminfo`, `meta=wikibase`, `query+tokens`/`wbsearchentities` params. Stale baserevid surfaces as `OtherPageSaveError` wrapping `editconflict` |
| 1.4 WikidataIntegrator | Q1 | | | | 1 item | | | | 2026-10-06: **11 passed/info, 0 failed** (WDI 0.9.30) with `setuptools<80`, `BACKOFF_MAX_TRIES=2`, endpoint at `<api>/sparql` (404, continues). `write()` sends no baserevid (info); stale `delete_statement` refused |
| 1.2 re-adopt, full history | `librarybase-20261101.xml.gz` | | | | adopted 0, unchanged 248,097 | 0 | 0 | | 2026-11-02: idempotent, entity and document-page revisions alike, after the fix that recognises a history revision by its adoption job |
| 1.5 client_compat.sh, full history | `librarybase-sample.xml.gz` (254 entities, 295 revisions: 200 sampled items, 10 with history, every property, Q1; cut from `librarybase-20261101.xml.gz`) | | | | 254 | 0 | 0 | | 2026-11-02, sandbox, Postgres 16: `dump` 254/254, WBI 0 failures, Pywikibot 23/23, WDI 11/11; `acl_check.sh` 612 checks, 0 findings |
| 1.5 client_compat.sh | `librarybase-sample.xml.gz` (225 entities, the earlier fixture cut from the 10k slice) | ~90 s incl. adopt | | | 225 | 0 | 0 | | 2026-10-06, Postgres 16: all three checks pass (WBI 0 failures; Pywikibot 14/14; WDI 11/11). The fixture has since been regenerated from the full dump |
| 2.2 internetdomains | 312,926 entities | | | | | | | | Synced 2026-10-10; measurements not recorded |
| 2.5 snapshot | | | | | | | | | |
| 3.1 WD sample | | | | | | | | | |
| 3.2 WD first-10k + all properties, `main` vs streaming | `wikidata-20261026-all.json.bz2` (sha1 `e46332c3…`) via `wikidata_zindex.py`: Q1–Q10348 (first 10,000 items) and 14,017 properties, 24,017 entities | 8m49s streaming (alone); 11m19s `main` (3 runs in parallel) | 160 MB streaming; 3.75 GB `main` | | 23,862 | 0 | 155 | | 2026-11-04. All 155 unreadable are lexeme form/sense values (`L470-S1`), 0066 §4 not built. |
| 3.2 WD first-10k + all properties, with lexeme part IDs | same 24,017 entities | 10m21s | 156 MB | | 24,017 | 0 | 0 | | 2026-11-04, after 0066 §4's IDs (A3): the 155 entities with form or sense values now sync, as `WDL…-F…`/`WDL…-S…`. 7 entity-value hash mismatches, the same 7 as before the change: all EntitySchema values (`E1`–`E13` as qualifiers on properties), hashed as `EntityIdValue`s; fixed in `hash.rs` the same day (0 mismatches on the 10k and 50k slices). |
| 3.2 WD 2M benchmark: all properties, first 1M items, 1M scholarly articles | `wd-bench-2m.json.gz` (sha256 `4ceed74c…`), 2,014,017 entities, 54.8 GB JSON; from the 2026-10-26 dump, sha1 `e46332c3…` | 9h28m08s (59/s) | 406 MB | 39 GB | 2,014,017 | 0 | 0 | | 2026-11-04, `main` `26f7754`, Postgres tuned per performance.md §4. 0 hash mismatches. Client CPU 70% of wall; 20.0 statements/entity; `view.term` 18 GB. Analysis in performance.md §8. |
| 3.2 WD 2M benchmark, rerun | same file, 2,014,017 entities | 5h18m49s (105/s) | 386 MB | 19 GB | 2,014,017 | 0 | 0 | | 2026-11-05, `867c673` (decode once, term languages, cheaper comparison, no load activity rows, mimalloc). 0 hash mismatches. Client CPU 12,653 s (baseline 23,818); Postgres execution 1,862 s (4,463); 20.0 statements/entity; `view.term` 5.1M rows, 1.1 GB (110.8M, 18 GB); 3 activity rows. 110/s over the dense items, 99/s over the articles. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap mode | same file, 2,014,017 entities | 2h56m09s (190/s) | 1.44 GB | 19 GB | 2,014,017 | 0 | 0 | | 2026-11-05, bootstrap stage A (0013 §9, A70: records prepared on 14 threads, block `COPY`, projections still inline). Client CPU 11,259 s; Postgres execution 1,456 s; 11.0 statements/entity (20.0 before). 159/s over the dense items, 244/s over the articles. Against the baseline 9h28m: 3.2×. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap with set-based projections | same file, 2,014,017 entities | 1h44m41s (321/s) | 1.75 GB | 19 GB | 2,014,017 | 0 | 0 | | 2026-11-05 (0013 A71: projections per block, set-based for the entity source, entity rows, terms, identifiers and activity). 0.13 statements/entity; Postgres execution 719 s. 278/s over the dense items, 358/s over the articles. View rows identical to the record-by-record bootstrap. Against the baseline 9h28m: 5.4×. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap, each block decoded once | same file, 2,014,017 entities | 49m15s (681/s) | 1.68 GB | 19 GB | 2,014,017 | 0 | 0 | | 2026-11-05: each block's operations decoded once on every core before the projections (`Pipeline::before_block`), not once per projection on the coordinator. Client CPU 9,525 s. Row counts in `view.entity`, `view.term`, `view.identifier` and `view.entity_source` the same as the previous load. Against the baseline 9h28m: 11.5×. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap, hashes with the decoding, statistics refreshed | same file, 2,014,017 entities | 40m56s (820/s) | 1.81 GB | 19 GB | 2,014,017 | 0 | 0 | | 2026-11-05: content hashes computed with the block's parallel decoding; planner statistics refreshed as the tables double (0013 A73), which removed a quadratic term delete. Client CPU 9,954 s. Same row counts in `view.entity`, `view.term`, `view.identifier`, `view.entity_source`. Against the baseline 9h28m: 13.9×. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap, block commits not waiting for the disk | same file, 2,014,017 entities | 39m14s (856/s) | 1.77 GB | | 2,014,017 | 0 | 0 | | 2026-11-05: `synchronous_commit` off for each block's transaction, on for the last (0013 A72). Client CPU 9,563 s. Same row counts. Against the baseline 9h28m: 14.5×. performance.md §8. |
| 3.2 WD 2M benchmark, bootstrap, decoding cache by fingerprint, operations shared | same file, 2,014,017 entities | 31m51s (1,054/s) | 1.81 GB | | 2,014,017 | 0 | 0 | | 2026-11-06: the cache no longer hashes whole contents or clones operations. Client CPU 9,039 s. Same row counts. Against the baseline 9h28m: 17.8×. performance.md §8. |
| 3.2 WD first-50k + all properties, `main` vs streaming | same archive: Q1–Q52905 (first 50,000 items) and 14,017 properties, 64,017 entities, 2.85 GB JSON | 31m34s streaming; 34m04s `main` (3 runs in parallel) | **157 MB** streaming; **10.9 GB** `main` | | 63,757 | 0 | 260 | | 2026-11-04. Same counts both ways; RSS flat with streaming, linear without (≈220 KB/entity). ~35–45 entities/s on these dense early items. |
| 3.2 WD 1M slice | | | | | | | | | |
