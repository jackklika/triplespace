# Gotchas: the sandbox, the test environment and the checkout

What is not derivable from the ADRs, the chapters or the code and bites anyway. Add what you learn;
strike what stops being true. Implementation status is not kept here (24 §15: `tools/coverage.py
--todo`), and neither are decisions (the ADRs' logs and open questions, listed in
`docs/decisions/INDEX.md`).

## Known test-environment failures

- `triplespace-db --test db` `the_public_role_cannot_read_private` fails in the cloud sandbox
  ("permission denied to set role ts_server") on the base commit too; environmental, not a
  regression. `cargo test --workspace` stops at the first failing test binary; use
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
- The tree tarred into the sandbox leaves out `docs/api/snapshots` (139 MB), but
  `triplespace-ui`'s `entity` tests and the parity corpus's `wikidata` set read it
  (`tests/common/fixture_api.rs`): without it seven entity tests panic on a missing file under
  every renderer. Stage it separately (`tar czf target/snapshots.tgz docs/api/snapshots` on the
  mount) when running those.
- `tools/client_compat.sh` runs `target/debug` binaries: build `-p triplespace-cli -p
  triplespace-server` first, or it runs stale ones.

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
