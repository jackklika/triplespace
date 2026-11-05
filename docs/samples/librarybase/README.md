# Librarybase

Pages of [Librarybase](https://librarybase.org/), a Wikibase of bibliographic and library data.
[`tools/README.md`](../../../tools/README.md) has the commands that made each part and the
choices behind them.

- **`dump/`**: MediaWiki XML dumps, as `triplespace adopt` and the client-compatibility checks
  read them.
  - `librarybase-sample.xml.gz`: 200 random items, ten more items with more than one revision,
    every property and Q1, each with its whole history (254 pages, 295 revisions), cut by
    `tools/dump_slice.py` from the full-history dump of 2026-11-01 (MediaWiki 1.46.2).
  - `librarybase-wikitext.xml.gz`: the wiki's own wikitext pages, every revision (39 pages,
    147 revisions), exported from MediaWiki 1.39.1.
- **`parse/`**: every revision of the wikitext pages, each with Librarybase's own
  `action=parse` render, fetched by `tools/librarybase_parse.py` on 2026-10-19.
  `manifest.json` records the API, the generator and the parse parameters;
  `pages/<pageid>/<revid>.wiki` is the revision's wikitext, `.json` the rest of the parse
  response (links, templates, categories, sections) and `.html` the rendered HTML.
  Golden renders for any wikitext renderer.

**Licence.** Librarybase's text is available under the
[Creative Commons Attribution-ShareAlike 4.0 License](https://creativecommons.org/licenses/by-sa/4.0/).
The authors of each page are named in its history on librarybase.org and, revision by
revision, in the dumps. The files are unchanged apart from being cut from a larger dump
(`dump/`) or fetched one revision at a time (`parse/`).
