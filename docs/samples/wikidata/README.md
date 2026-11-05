# Wikidata

Entities from [Wikidata](https://www.wikidata.org/), as `Special:EntityData/<id>.json` serves
them, chosen for their statement shapes (ADR 0003 §10): qualifiers, references, ranks, ranges and
points in time.

| File | Entity | Revision |
|---|---|---|
| `wikidata-Q65.json` | Los Angeles | 2550751121 (2026-09-29) |
| `wikidata-Q339.json` | Pluto | 2539604885 (2026-09-03) |
| `wikidata-Q731.json` | manganese | 2552513285 (2026-10-03) |
| `wikidata-Q1520.json` | Astana | 2552673948 (2026-10-03) |

`big/` is not in git (GitHub refuses files over 100 MB) and is shared out of band:
`wikidata-20261004-sample-50000.json.gz` is 50,000 items drawn uniformly at random from those
that existed on 2026-10-04, with all 13,972 properties, in the JSON dump's shape. `tools/wikidata-sample.py`
made it; its manifest gives the seed. The audit in
[`docs/audits/0003-shapes-2026-10-04.md`](../../audits/0003-shapes-2026-10-04.md) uses it.

**Licence.** Wikidata's structured data is [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/).
