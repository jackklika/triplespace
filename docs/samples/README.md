# Samples

Captures of other software's output — Wikibase, Wikidata, MediaWiki, DiscussionTools — that
Triplespace tests against and that other projects can use as they are: golden renders, real
dumps, real API responses. Each set is a directory with a README saying where it came from, how
it was made and under what licence.

**These files are not covered by `docs/LICENSE`.** The rest of `docs/` is CC0-1.0; each set
here keeps the licence of its source, given below and in its README (22 §3.3).

| Set | What it is | Licence | Read by |
|---|---|---|---|
| [`wikibase-1.43/`](wikibase-1.43/README.md) | A reference install of MediaWiki 1.43.9 with Wikibase REL1_43: `paraminfo`, `siteinfo`, the REST API's OpenAPI document, the ontology, RDF dumps and entity JSON | GPL-2.0-or-later for the software's own documents; see its README | `scatter-wikibase-model`, `scatter-vocab`; `docs/api/*-compat.md` |
| [`wikidata/`](wikidata/README.md) | Four Wikidata entities as `Special:EntityData` serves them; `big/` (not in git) a 50,000-item random sample | CC0-1.0 | `scatter-wikibase-model`, `scatter-wikibase-shape`, `triplespace-ui` |
| [`librarybase/`](librarybase/README.md) | `dump/`: XML dumps of librarybase.org (a 254-entity sample with history, and its wikitext pages). `parse/`: every revision of its wikitext pages with Librarybase's own `action=parse` render | CC BY-SA 4.0 | `triplespace-server`, `triplespace-cli`, `scatter-wikitext`; `tools/client_compat.sh`, `acl_check.sh`, `skin_corpus.sh` |
| [`enwiki-talk/`](enwiki-talk/README.md) | Ten English Wikipedia talk pages (one with its whole history) beside DiscussionTools' parse of each | CC BY-SA 4.0 (older revisions also GFDL) | `scatter-talk`, `scatter-talk-import` |

## Rules

- **A sample is a capture, not an expectation.** What Triplespace is expected to do with it
  stays with the test that checks it (for example `crates/scatter-wikitext/tests/fixtures/librarybase/known-differences.txt`).
  Fixtures written by hand for Triplespace (`crates/triplespace-server/tests/fixtures/`) and
  recordings of Triplespace itself (`crates/triplespace-ui/tests/`) are not samples.
- **Every set is reproducible.** Its README names the tool and command that made it and the
  source's version; regenerating it with the same inputs gives the same bytes.
- **Tests read samples in place,** from `$CARGO_MANIFEST_DIR/../../docs/samples/…`, at run
  time; nothing here is compiled into a crate.
- **Large sets stay out of git.** A file GitHub refuses (over 100 MB) goes in a `big/`
  directory, which `.gitignore` excludes, with its manifest committed beside it.

## History

Moved here on 2026-11-03 from where the ADRs' older log entries cite them:
`docs/api/snapshots/` (now `wikibase-1.43/` and `wikidata/`, with `big/` under `wikidata/`),
`crates/triplespace-server/tests/fixtures/librarybase-*.xml.gz` (now `librarybase/dump/`),
`crates/scatter-wikitext/tests/fixtures/librarybase/` (now `librarybase/parse/`) and
`crates/scatter-talk/tests/fixtures/` (now `enwiki-talk/`). The files are unchanged.
