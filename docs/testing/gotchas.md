# Gotchas: the sandbox, the test environment and the checkout

What is not derivable from the ADRs, the chapters or the code and bites anyway. Add what you learn;
strike what stops being true. Implementation status is not kept here (24 §15: `tools/coverage.py
--todo`), and neither are decisions (the ADRs' logs and open questions, listed in
`docs/decisions/INDEX.md`).

## Known test-environment failures

- `triplespace-db --test db` `the_public_role_cannot_read_private` fails in the cloud sandbox
  ("permission denied to set role ts_server") on the base commit too; environmental, not a
  regression. It fails the same way on a Debian 13 VM prepared by `tools/dev_env.sh`
  (Postgres 17, 2026-11-04). `cargo test --workspace` stops at the first failing test binary; use
  `--no-fail-fast` to run the rest.

## The cloud sandbox

- Its disk fills with `target/debug/incremental` (~10 GB); build with `CARGO_INCREMENTAL=0` or
  clear it. A full disk shows up as linker "Bus error". A full `cargo test --workspace` with debug
  info leaves ~28 GB of test binaries; `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`
  keeps it near 15 GB.
- Its Postgres stops when the container is recycled; a "Connection refused" in a test means rerun
  `eval "$(tools/dev_env.sh)"`.
- It cannot install the pinned Rust toolchain and falls back to stable (`dev_env.sh` says so);
  rustfmt and clippy may differ from CI's, and there is no wasm32 target, so `cargo xtask wasm`
  cannot run there. Say so when reporting.
- Its Playwright browsers (build 1243, `/opt/pw-browsers`) do not match the repo's pinned
  `@playwright/test` 1.56 and cannot be downloaded. To run `ui/e2e` there, either
  `npm i -D @playwright/test@1.63.0 --no-save` first and restore `package.json`/`package-lock.json`
  after, or, leaving the repo untouched, set `PLAYWRIGHT_BROWSERS_PATH` to a folder whose
  `chromium_headless_shell-1194/chrome-linux/` holds symlinks to the 1243 build's files plus
  `headless_shell` → `chrome-headless-shell`, with empty `INSTALLATION_COMPLETE` and
  `DEPENDENCIES_VALIDATED` files beside it.
- `pkill -f triplespace-server` from a sandbox shell kills the calling shell too (its command line
  contains the name); match on the binary path (`pkill -f "^/home/.*/triplespace-server"`), and
  start a test server and query it within one call, since background processes die when the call
  ends.
- The tree tarred into the sandbox must leave out `docs/samples/wikidata/big` (140 MB,
  gitignored) but keep the rest of `docs/samples` (about 9 MB): many tests read it, among them
  `triplespace-ui`'s `entity` tests and the parity corpus's `wikidata` set
  (`tests/common/fixture_api.rs`), which panic on a missing file without it.
- Moving `target/` into a fresh copy of the tree (to reuse a build) leaves binaries that
  bake `env!("CARGO_MANIFEST_DIR")` pointing at the old copy, and Cargo does not rebuild
  them: `triplespace-ui`'s tests then panic on missing fixtures, and `cargo xtask skin-parity`
  writes its report under the old path. `cargo clean -p` the crates that use it (`grep -rl
  'env!("CARGO_MANIFEST_DIR")' crates xtask`) and `xtask`, or build in place.
- `tools/client_compat.sh` runs `target/debug` binaries: build `-p triplespace-cli -p
  triplespace-server` first, or it runs stale ones.
- `pkill -f '<pattern>'` inside a longer shell command matches that command's own `bash -c`
  line and kills it (exit 144, no output). Stop a test server by process name instead
  (`kill $(pgrep -x triplespace-ser)`; the name is cut to 15 characters).
- `triplespace instance create` refuses an owner password under 8 characters only after it has
  written the instance, so a short test password leaves an instance nobody can log in to.
  Use 8 or more.
- A test adoption that fails leaves an empty `view`, and an empty `view` passes the rebuild
  equivalence diff vacuously. Check `adopt`'s stderr and the row counts before trusting
  `REBUILD-IDENTICAL`.

## Wikidata dumps

- `latest-all.json.*` is not in ID order: it is written in shards by page ID and opens Q31, Q8,
  Q23, with properties scattered through it. `head -n N` gives neither the first N items nor
  the properties they use; cut slices by ID with `tools/wikidata_zindex.py` (real-dumps.md §3.2).
- `latest-all` changes under a long download; fetch the dated file from
  `wikidatawiki/entities/<date>/` and check it against that directory's `sha1sums.txt`.
- dumps.wikimedia.org gives one connection about 5 MB/s and limits connections per IP. The
  your.org mirror (`dumps.wikimedia.your.org/wikidatawiki/entities/`) serves the same files;
  `aria2c` with 2 connections to each runs about 16 MB/s. ftp.acc.umu.se did not carry the
  entity dumps when looked for (2026-11-03).
- Take the `.bz2` over the `.gz`: `lbzip2` decompresses it on every core (~500 MB/s on 16),
  where gzip runs on one. A partly downloaded `.bz2` decompresses up to where it stops, which is
  enough to try a tool on the dump's real opening.

## The device bridge and James's checkout

- The bridge's shell runs in a Linux VM with the folder mounted, not on the Mac: no `cargo` there.
  Builds and tests run in the sandbox; the Mac gets patches.
- Deleting files in the mounted folder is refused unless James grants it, so git commands that
  write (`fetch`, `commit`, `pull`, `checkout`) leave `.lock` files behind or fail on the files
  they change. Avoid them on the mount; `git show HEAD:path > path` restores a file.
- `git apply` on the mount prints "unable to unlink … Operation not permitted" for every file but
  writes the files; check with `shasum -a 256 -c` against a sums file from the sandbox.
- The Mac's Python is 3.10: `docs/check.sh` adds the vendored `tomli` wheel itself; a checker
  run directly needs `PYTHONPATH=docs/tomli-2.4.1-py3-none-any.whl`.

## The code

- A `PgCx` dropped after a failed write without `store.rollback(cx)` can leave the pooled
  connection mid-transaction and hang the next write on the same key. Always roll back explicitly,
  as `editpage` does.
- Partition IDs are 64-bit and stored as `bigint` bit for bit (`ids::partition_from_db`): half of
  them read back negative. Anything built from a partition read as `i64` must `cast_unsigned`
  first.
- `owner` holds every permission and reads every group but the suppression group
  (`includes = ["*"]`); it is still in no other group for edit restrictions and the lock-out. A
  test that needs a logged-in reader outside a group uses `suppress` as the group, or a non-owner
  account; tests that need the owner in a group grant it (`grant` helper).
- Every request evaluates against the tenant's groups (`triplespace_accounts::groups::registry`),
  not `GroupRegistry::default_registry()`; new code that evaluates or lists groups should do the
  same.
- Membership records project into `view.activity`, whose `logid` is unique per tenant: a test
  writing membership records needs distinct log IDs.
- Projections in steps 6–7 (addressing; later RDF) never run inside a write: a test that reads
  their output calls `App::catch_up_async` first, or reads through a module that does
  (`meta=notifications`).
- Actor records share their key (the account) across payloads: a projection looking up "the
  previous record under the key" must filter by payload type. Actor keys of adopted accounts use
  the tenant's derived issuer code (`ActorKey::numeric(issuer, id)`), not the tenant slug.
- A real instance has no `board` group: `action=protect read=board` on it is `ts-locked-out`,
  where `/acl` says `ts-unknown-group`. Define a group over `/group` first. The tests' `grant`
  helper bypasses this.
- The attack battery is `tools/acl_check.sh`. Registration is throttled at six accounts a day per
  address and the challenge rises a bit per account: a live script that registers users must stay
  under six and lower `challenge.bits` on its test tenant. `autoconfirmed` is implied, never
  granted. Needles in a leak probe must be specific: the fixture holds an item labelled "The
  Secret life of Adolf Hitler", and a page title that contains the hidden user's name echoes it.
- A test that inserts into `view.acl` directly (bypassing records) must move the ACL frontier
  (`offset` and row count), or the hidden-set cache answers "nothing hidden" from before.
- `tests/moderation.rs` has helpers for more accounts: `instance(name)` (a fresh database per
  test), `account(router, name, password)` (`action=createaccount`, solving the challenge) and
  `grant(app, name, group)` (a membership record written through the pipeline).
- REST handlers that return the Action API's `ApiError` need `#[allow(clippy::result_large_err)]`
  (as `triplespace-api-rest/src/acl.rs` does module-wide).
- Entities have no `view.page` row: code that checks "does this page ID exist" must also look in
  `view.entity`.
- An XML dump's entity JSON carries no snak `datatype`; `sync` fills them from the dump's property
  pages (`DumpPropertyTypes`). The API's `wbgetentities` carries them, so the follower applies no
  typing; a mock wiki built from dump text must add them or every state reads as changed.
- `Index::observe` is idempotent (a follower re-reads items): the same item with the same key is
  not a conflict, and a changed key releases the old one.
- A colon-form ID that is neither a registered keyed type nor one of the tenant's sources is
  `invalid-entity-id` on reads (it used to answer as an empty keyed entity of an unknown type).
- Applied migrations are checksummed (`ops.migration`): never edit one, even a comment; add a new
  migration.
- Pywikibot's API cache can mask whether a fix worked; clear it between runs.
