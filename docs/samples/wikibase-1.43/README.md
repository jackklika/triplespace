# Wikibase 1.43 reference install

Outputs of a reference install of **MediaWiki 1.43.9 with Wikibase REL1_43**, holding test items
Q1–Q9 and properties P1–P13, captured for the compatibility contracts in
[`docs/api/wikibase-compat.md`](../../api/wikibase-compat.md) §1.2 and
[`docs/api/mediawiki-compat.md`](../../api/mediawiki-compat.md) §1.2, which say how each was made
and treat them as normative wherever their prose summarises them.

| File | Contents |
|---|---|
| `mw-1.43.9-wb-REL1_43.paraminfo.json` | `action=paraminfo` for every Action API module, plus `main` |
| `mw-1.43.9-wb-REL1_43.siteinfo.json` | `meta=siteinfo` (general, extensions, namespaces) |
| `wikibase-rest-v1.openapi.json` | The Wikibase REST API's OpenAPI document (API version 1.1) |
| `wikibase-ontology-1.0.owl` | Wikibase's `docs/ontology.owl` at REL1_43 |
| `wikibase-rdf-full-dump.ttl`, `wikibase-rdf-truthy-dump.ttl` | `dumpRdf.php --flavor full-dump` and `truthy-dump` for the whole install |
| `wikibase-Q8.json` | `Special:EntityData/Q8.json`, the main test item |
| `wikibase-edtf-edge-cases-Q9.ttl` | Full RDF for the EDTF edge-case item |

**Licence.** The ontology, the OpenAPI document and the API help text in `paraminfo` are part of
MediaWiki and Wikibase, GPL-2.0-or-later. The test items were written for this project; their
JSON and RDF are CC0-1.0, like the rest of `docs/`. The RDF and JSON use the install's base URI,
`http://localhost:8080/entity/`.
