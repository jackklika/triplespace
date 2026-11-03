# tools/

Scripts that are not part of the build. The Python ones need only the standard library unless
they say otherwise; every script prints its own usage with `--help`.

| Script | What it does |
|---|---|
| `wikidata-sample.py` | Draws a uniformly random sample of Wikidata items from a JSON dump, in the dump's shape, for the classifier audit (0003 §10). |
| `dump_slice.py` | Cuts a smaller, valid MediaWiki XML dump out of a Wikibase dump: the first N entities (`--limit`), or a reproducible random sample of items (`--sample N --seed S`, with `--history N` adding N items whose pages have more than one revision), with `--all-properties` to carry every property page so each snak keeps its datatype, `--include Q1,P5` to force entities in, and `--defined-only` to drop items that use a property the slice lacks. Streams the input; `.gz` or plain `.xml` either side. The Librarybase sample fixture (below) is made with it. |
| `librarybase_parse.py` | Downloads a wiki's own `action=parse` renders of its wikitext pages (default Librarybase), pinned by `oldid`, as golden fixtures for the built-in renderer: per revision the source (`.wiki`), the HTML (`.html`) and the rest of the parse (`.json`), plus a `manifest.json`. `--history` parses every revision; reruns fetch only what is missing. |
| `talk_dumps.py` | Downloads wiki talk pages as XML dumps for the talk-page conversion (0054 §5): the latest revision of some pages through the Action API's `export`, every revision of others through `prop=revisions` written in the same export-0.11 shape, and DiscussionTools' `discussiontoolspageinfo` reading of each dumped revision, pinned by `oldid`. Defaults to a sample of odd English Wikipedia talk pages. Dumps are kept once written, so reruns fetch only what is missing. |
| `talk_split.py` | The reference prototype of the talk-page splitter (0090 §2–§3, §9): splits a dump's or a `.wiki` file's talk pages into sections and signed comments, with depth, parents, undated signatures and DiscussionTools names and IDs; `--page TITLE` prints a comment tree. The Rust splitter in `scatter-talk` is measured against it. |
| `talk_compare.py` | Compares `talk_split.py` with DiscussionTools on the samples `talk_dumps.py` writes: comments, parents, names and IDs per page (`--show TITLE` lists one page's disagreements); `--attribution` replays `history.xml.gz` with a line-level blame and counts who added each signature (0090 §13). |
| `talk_fixtures.py` | Turns the samples `talk_dumps.py` writes into `scatter-talk`'s committed golden-test fixtures (0090 §13): the dumps recompressed with bzip2, DiscussionTools' responses without their rendered HTML, and a README with each page's licence and attribution. Deterministic. |
| `skin_corpus.sh`, `skin_corpus.py` | Records the parity corpus of the skin migration (19 §5.10): a test instance on the server's fixtures (the e2e entities, Librarybase's own wikitext pages and talk threads, the internetdomains sample), seeded through the API and the site's forms with a week's activity (accounts, edits, a markdown page, a redirect, a move, a deletion, threads and replies, watches, a protection, a block, a group, a reader's language and appearance), then every page the site serves recorded as three viewers (anonymous; a user reading in German with dark mode; the owner) and again with the tenant private. Writes `crates/triplespace-ui/tests/corpus/exchanges/*.json` and `cases.recorded.json`; `cargo xtask skin-parity` replays them. Needs the binaries, `ui/dist` and a test database. |
| `api_check.py` | Checks adopted entities through the Action API after `adopt` (test plan §1.3–1.4): `Special:EntityData`, `wbgetentities`, `wbsearchentities`, `view.identifier`, snak datatypes, and with `--compare` the source wiki's: the entity, `pageid`, `lastrevid` and the page's whole history revision by revision, all equal for a full-history adoption (`--current-only` for one adopted with `adopt --current-only`: `lastrevid` above the source's, history not compared). `--compare-dump <xml>` does the same against the adopted XML dump itself, with no network. With `--wbi-login` it runs the WikibaseIntegrator section: bot login, label edit, statement add/remove, a stale `baserevid` refused, a new item (and property) above the counter floor. |
| `pwb_check.py` | The same story through Pywikibot: generates a family file and config in a temporary directory, then site info, paraminfo, data repository, login, read, property, search, `editLabels`, `addClaim`, `removeClaims`, stale `baserevid`, new item; then a Project page through `Page.save`, created, edited, a stale save refused, and rendered by `action=parse`. Needs `pywikibot`. |
| `wdi_check.py` | The same through WikidataIntegrator, with its SPARQL backoff bounded and its endpoint pointed at the server: login, read, search, property, `set_label` + `write()`, `WDString` append, stale and current `delete_statement`, new item. Needs `wikidataintegrator` and `setuptools<80`. |
| `client_compat.sh` | Compares every adopted entity with the fixture (`api_check.py --compare-dump`), then runs all three against a server it sets up itself (database, `instance create`, `adopt` of the sample fixture, `sync`, a bot key with `editentity`, `createeditmovepage` and `highvolume`, `triplespace-server --ui off`), each as a hard pass/fail; CI's *Client compatibility* job. Needs the binaries built, `psql`, and a Python with `requirements-compat.txt` installed (`PYTHON=` to name it). Everything it writes is in a temporary directory removed on exit. `CHECKS="wbi pwb wdi"` selects, `KEEP_DB=1` keeps the database, `ITEM=Q5` names the item to edit (default: the lowest-numbered item), `FIXTURE=` another dump. |
| `acl_check.py` | The permission model's attack battery (09 §4–§5; 0056) over real HTTP: the negative half probes what an outsider must not see or do (every read route for a confidential page, entity and thread; talk pages and threads through every surface (listings, composite histories, revisions and diffs, rendering, departures, hidden posts, the site's pages and forms, a protected talk page); write-side oracles; self-grants, ACL writes and lock-out bypass; scoped-group escape; delegation ceilings and stale delegation; inclusion chains; a hidden name; tokens; a private tenant; a member's bot; the owner outside `suppress`), the positive half what every legitimate role must be able to do (member, admin in the group, owner by inclusion, bureaucrat, delegated and scoped admin, `read-all`, bot by bearer, a private tenant's users, the CLI). Each probe is a check; a failure is a finding, and any finding exits 1. `--sections` selects, `--list` names them. Needs `requests` and a fresh instance whose owner is Alice; `acl_check.sh` provides one. |
| `acl_check.sh` | Runs `acl_check.py` against a server it sets up itself (database, `instance create`, `adopt` of the sample fixture, `triplespace-server --ui embedded`, so the site probes read real pages, which a check at the start confirms); CI's *Permission model* step. Needs the binaries built, `psql`, and a Python with `requests`. `SECTIONS=leaks,positive` selects, `KEEP_DB=1` keeps the database. |
| `adr.py` | Looks things up in the ADRs and chapters in one call: `show 0013 §7` (an ADR section, following its pointer to the chapter text), `show 0098 --full` (a whole ADR with its chapter text, what CI posts on a pull request that adds an ADR), `code 0013 §7` (the citing lines), `where <phrase>` (chapter sections, ADRs and registry files, best first), `log 0008 [A6]`, `questions 0090`, `outline 05`. Prints links between docs as plain citations to keep output short (`--raw` keeps them). |
| `coverage.py` | What the code says it has built (24 §15), from two kinds of citation in comments across `crates/`, `xtask/`, `ui/` and `tools/`: `0013 §7` (the code is about the section) and `implements 0013 §7 (A41)` (built in full as of that entry). Alone: one line per ADR with its computed status and section counts (done, amended since the claim, untested, partial, not built, uncited). `coverage.py 0013`: per section. `coverage.py 0013 §7`: the citing lines. `--todo`: every section not done and what it needs (the backlog). `--status`: the per-ADR status as JSON, which `check_adrs.py --index` writes into the headers and `INDEX.md`. `--check`: malformed claims (run by `docs/check.sh`). `--stale`: citations of sections that don't exist. |
| `dev_env.sh` | Prepares a Debian or Ubuntu sandbox for the full suite: installs and starts Postgres, creates a role that may create databases and roles, proves it can, and installs the pinned client libraries into `.venv`. Falls back to stable Rust, with a warning, when the pinned toolchain can't be downloaded. Prints `export` lines: `eval "$(tools/dev_env.sh)"`. `--ui` adds `npm ci`, `--nextest` installs cargo-nextest, `--no-compat` skips the Python clients. Idempotent. |
| `hidden_set_bench.py` | The hidden set's cost (09 §5.3): request latency for an outsider and a member as confidential pages accumulate (`BENCH=1 BIN=target/release tools/acl_check.sh`). Results in `docs/testing/performance.md` §7. |
| `requirements-compat.txt` | The pinned client versions `client_compat.sh` is run against (and `requests`, for `acl_check.py`). A bump is deliberate: run the script, then record the versions and verdict in the test plan's results log and `docs/clients.md`. |

The three checks take `--api` (default `http://127.0.0.1:8080`), the bot login as `--login
name@label` (`--wbi-login` for `api_check.py`), and the secret as `--secret` or
`TRIPLESPACE_WBI_SECRET`. Their writes are permanent on the tenant they run against, so point
them at a test database; `--no-write` (Pywikibot and WDI checks) keeps to reads.

## The Librarybase sample fixture

`crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz` is 200 random Librarybase
items, 10 more items whose pages have more than one revision, every property page and Q1, each
page with its whole history, made from the full-history dump of the rebuilt librarybase.org
(`librarybase-20261101.xml.gz`, sha256 `f722e518…5b8af2`): 254 entities, 295 revisions.

```sh
tools/dump_slice.py librarybase-20261101.xml.gz \
    crates/triplespace-server/tests/fixtures/librarybase-sample.xml.gz \
    --sample 200 --seed 1 --history 10 --all-properties --include Q1 --defined-only
```

Only 518 of the dump's 246,967 items have more than one revision, so a random 200 would hold
almost none; `--history 10` adds ten of them, which is what lets `client_compat.sh`'s `dump`
step test history. Nearly every revision is attributed to an `imported>` name, as the rebuilt
wiki has it, and one to the account `Librarybase` (user 3); the scripts' owner (user 7, Alice)
clashes with neither (0035 A16).

Made from the full dump, `--defined-only` drops nothing and `Q1` (the Librarybase item itself)
is in; made from a prefix slice it drops the items whose properties the slice does not
define. The same seed on the same dump gives the same fixture (the gzip header's timestamp
differs; the XML does not). `adopt` is run on it without `--counters`, so the floors come from
the sample's highest IDs; the checks only require new entities to land above them.

## The Librarybase wikitext fixtures

`crates/scatter-wikitext/tests/fixtures/librarybase/` holds every revision of Librarybase's
wikitext pages with Librarybase's own `action=parse` render of each, made with:

```sh
tools/librarybase_parse.py crates/scatter-wikitext/tests/fixtures/librarybase --history
```

The golden test (`crates/scatter-wikitext/tests/golden.rs`) renders each revision and compares
it with Librarybase's after both are parsed as HTML5 and normalized. Revisions that are known
to differ are listed with their reason in `known-differences.txt` there; the test fails when
one of them starts matching, so the list stays true. The MediaWiki-namespace pages are
fetched but not compared: Triplespace keeps that namespace reserved (11 §2.3).
