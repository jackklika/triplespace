# Triplespace: notes for coding agents

Triplespace is Wikibase reimagined as an append-only log, in Rust: high-speed ingest, foreign
entities (`WDQ42`) alongside local ones, Merkle-verified history, full attribution, erasure, and
compatibility with Wikibase clients (WikibaseIntegrator, Pywikibot, WikidataIntegrator,
QuickStatements). James Hare (Scatter LLC) owns it; he decides, and agents draft.

## Where things are

| Path | Holds | Rule |
|---|---|---|
| `docs/architecture/` | **What holds now**, by subject: 25 chapters (`00`–`24`). Start with `00`, then `22` (crates and stack). `24` is how the docs themselves work. | A chapter describes; it decides nothing on its own. Every `##`/`###` section opens with a `*Sources:*` line naming the ADR sections it holds: that line is the only map between ADRs and chapters. |
| `docs/decisions/` | **Why and when**: ADRs. Every Decision section is a one-line pointer (*Current text: [03] §4.5.*) into a chapter, from the day the ADR is written; the ADR holds Context, Alternatives, Consequences, Open questions and the Amendment log. | Never read an ADR for its text: follow the pointer (`tools/adr.py show`). A new ADR's text is written into the chapters, then `architecture/pointers.py NNNN` fills its pointers. |
| `docs/registry/` | Names and codes the crates embed (`include_str!`) and parse at build time: providers, graphs, namespaces, content models, special pages, settings, groups… | A change here is a commit. Codes and numbers are never reused. See its `README.md`, including *Pending allocations*. |
| `docs/testing/` | How to test against real dumps (`real-dumps.md`, with its results log) and write-path performance: measurements, the `pg_stat_statements` recipe, Postgres settings, what a Wikidata-sized load needs (`performance.md`); the sandbox, test-environment and checkout pitfalls (`gotchas.md`). | Add a results row for every real-dump run; add pitfalls as you meet them. |
| `docs/api/` | Compatibility contracts with Wikibase and MediaWiki, log test vectors, the OpenAPI file. | Contracts are tested where they are implemented. |
| `docs/samples/` | Captures of other software's output, reusable outside Triplespace: Wikibase 1.43 and Wikidata entities, Librarybase dumps and `action=parse` goldens, English Wikipedia talk pages with DiscussionTools' reading. `docs/samples/README.md`. | Each set keeps its own licence (its README); `docs/LICENSE` does not cover them. Tests read them in place; expectations about Triplespace stay in the crates. |
| `crates/` | One workspace: `scatter-*` (shared with Scatterbase, knows nothing of Triplespace) and `triplespace-*` (the product). | See the crate rules below. |
| `tools/` | Scripts outside the build: dump slicing, client-compatibility checks, talk-page prototypes. `tools/README.md`. | |
| `xtask/` | `cargo xtask deps` (dependency graph vs. `docs/registry/crates.toml`), `wasm`, `manifest`, `skin-parity` (the skin migration's harness: `crates/triplespace-ui/tests/parity.rs` over `tests/corpus/`, with its report). | |
| `ui/`, `i18n/` | The site front end (Codex, Vite) and interface messages. | |

**Looking things up:** `tools/adr.py` answers the common questions in one call:
- `show 0013 §7` prints an ADR section, following its pointer to the chapter text; `show 0098 --full` prints a whole ADR assembled with its chapter text (what CI posts on a pull request that adds an ADR).
- `where <phrase>` lists the chapter sections, ADRs and registry files that cover a subject.
- `log 0008 [A6]` prints an Amendment log, or one entry.
- `questions NNNN` prints an ADR's open questions.
- `outline NN|NNNN` prints a document's headings.
- `code 0013 §7` lists the code that cites a section.

**Is it built? The code says (24 §15).** Two kinds of citation in code comments: `0013 §7` means the
code is *about* the section; `implements 0013 §7 (A41)` claims it is built in full as it stood at
entry A41 (written once, in src, at the code that completes it; `§7.3` or `§7 rule 2` for a part;
the entry is required once the section has a provenance line). A section is **done** when a src
claim covers it, a test cites it, and no entry changed it since. `tools/coverage.py` reads both:
alone for one line per ADR with its computed status, `NNNN` for each section, `NNNN §N` for the
citing lines, `--todo` for everything not done and what it needs (this is the backlog), `--check`
for malformed claims. The ADR's `Status` header and `INDEX.md`'s *Built* column are written from
this by `check_adrs.py --index`: never edit a Status by hand. Finishing a section means adding its
`implements` line and a test that cites it, in the same change. **What waits on a decision** is
`docs/decisions/OPEN.md` (open questions, unconfirmed entries, pending allocations; generated);
**what waits on work** is `tools/coverage.py --todo`. Nothing of either kind is kept anywhere else.

**Generated files: never edit by hand.** `docs/decisions/INDEX.md` (`check_adrs.py --index`),
the settings tables in chapter 23 (`settings.py --write`, from `registry/settings.toml`), and the
crate table in chapter 22 §2.1 with `docs/architecture/CRATES.md` (`crates.py --write`, from
`registry/crates.toml`), and the route, Action API, permission and log-event tables in 18 §3.2, 18 §2.3,
09 §2.2 and 16 §2.3 (`catalogues.py --write`, from `registry/routes.toml`, `action-api.toml`,
`permissions.toml` + `groups.toml`, `log-events.toml`). A new crate, route, module, permission or
log event is an edit to its registry file. `docs/decisions/OPEN.md` is generated with `INDEX.md`.

**A merge or rebase conflict in a generated file is resolved by regenerating it, never by hand.**
Most changes regenerate `INDEX.md` and `OPEN.md`, so two branches that each did will conflict once
either merges, however current each was when pushed. Take either side (`git checkout --theirs --
<file>`), run `docs/check.sh --write`, then `git add` the result. Where the conflict also touches a
chapter's hand-written text around a generated table, resolve the text by hand and let
`--write` rebuild the table. `.gitattributes` marks the wholly generated files, so pull requests
collapse their diffs.

**Secrets.** `owner.pw` and `triplespace.key` at the root are local, gitignored secrets. Do not
read, print, copy or commit them.

## Crate rules (chapter 22 §1.2, abridged)

1. Dependencies point only downward through four layers: substrate, Wikibase domain, ingest,
   Triplespace surfaces. No `scatter-*` crate depends on a `triplespace-*` crate; only adapters
   name a specific provider.
2. The crates `crates.toml` marks `pure` (22 §1.2 rule 2) are pure: no I/O, no async runtime, no tokio.
3. Policy is data: registries, role maps and rules are values passed in, never enums or globals.
4. Each hashed type has exactly one canonical encoding function.
5. The crates `crates.toml` marks `wasm` (rule 7) build for `wasm32-unknown-unknown`, and so does everything they
   depend on.
6. The site's crates (`triplespace-ui`, `-client`, `-web`) reach the instance only through its
   public API.
7. A new crate or dependency edge goes into the crate map (`docs/registry/crates.toml`, shown in 22 §2.1) through an ADR.

## Checks before calling work done

Run what the change touches; CI (`.github/workflows/ci.yml`) runs all of it.

```sh
cargo fmt --all --check
cargo xtask deps
cargo build --workspace --all-targets
cargo nextest run --workspace          # DB tests need TRIPLESPACE_TEST_DATABASE_URL, else vacuous
cargo test --workspace --doc
cargo clippy --workspace --all-targets -- -D warnings
cargo xtask wasm
(cd ui && npm run lint && npm test && npm run build)
(cd ui && npm run e2e)                  # Playwright + axe; needs the three binaries and a test DB
tools/client_compat.sh                  # WBI, Pywikibot, WDI against a fresh server
tools/acl_check.sh                      # the permission model's attack battery, likewise

docs/check.sh                           # every doc checker; exits non-zero on any failure
docs/check.sh --write                   # regenerate INDEX.md and chapter 23's tables, then check
```

In a fresh Linux sandbox, run `eval "$(tools/dev_env.sh)"` first. It starts Postgres, exports
`TRIPLESPACE_TEST_DATABASE_URL` and the client-compatibility `PYTHON`, and warns if it had to
fall back from the pinned Rust toolchain to stable.

Say which checks you ran and which you could not run (no Postgres, no browser, a stable
toolchain instead of the pinned one), rather than calling the work verified. Without
`TRIPLESPACE_TEST_DATABASE_URL` the database tests print "skipping" and pass vacuously.

## Working rules

- **Never commit or push without James's explicit go-ahead.** Leave the work uncommitted for
  his review.
- If a non-`main` branch is checked out, work on it. Otherwise make a named feature branch.
- Keep history linear: rebase onto `main` when other work merges; no stacked or orphaned
  branches; avoid force-pushes.
- Flag assumptions and open choices explicitly for James to confirm. Do not settle them silently.
  A call you have to make while building goes into the owning ADR's Amendment log in the same
  change, folded into the chapter, with Source `Direct: Claude, while building <what>, <date>;
  unconfirmed` (24 §6); `OPEN.md` lists these for James. Never keep such calls in notes.
- Sandbox and test-environment pitfalls are `docs/testing/gotchas.md`; add what you learn there.
- Prefer real data and real clients for testing: the Librarybase samples (`docs/samples/librarybase/`), `tools/*_check.py`.
  Watch for Pywikibot's API cache masking whether a fix worked.

## Design principles James applies

- Prefer a general principle over a narrow mechanism. Extend or retire an existing concept
  before adding a parallel one.
- Handle the common case well first; cover edge cases without compromising it.
- Local pages and data always take precedence in any inheritance stack.
- Default to the simpler, most permissive, reversible option (tombstones, read-time
  resolution) unless isolation, privacy or consistency argues otherwise.
- Size limits are sensible defaults against surprises, and waivable.
- "Running out of codes" is a weak reason for complexity; autonomy and privacy are strong ones.
- Keep wikitext extraction to template/parameter mapping and links; free-text NLP is a
  separate, swappable module.

## Skills

- `.claude/skills/triplespace-architecture/`: finding what the design says about a subject, and
  where in the code it lives.
- `.claude/skills/triplespace-adr/`: writing or changing an ADR, a chapter or a registry file.
