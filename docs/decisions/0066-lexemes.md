# 0066. Lexemes

- **Status:** Proposed
- **Date:** 2026-10-05
- **Updated:** 2026-11-04 (A3)
- **Author:** James Hare / Claude Fable
- **Changes:** [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0041](0041-content-models.md), [0043](0043-lua-modules.md), [0047](0047-special-pages.md), [0060](0060-scopes.md)
- **Uses:** [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0009](0009-keyed-entity-types-and-domain.md), [0010](0010-site-ui.md), [0015](0015-record-format-and-partition-registry.md), [0016](0016-permissions-and-access-control.md), [0023](0023-moderation.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0035](0035-adopting-a-wikibase.md), [0045](0045-table-content-model.md), [Wikibase data model and ontology contract](../api/wikibase-compat.md)

## Context

Wikidata has held lexicographical data since 2018: **lexemes** (`L1`), each with lemmas, a language and a lexical category, **forms** (`L1-F1`) with representations and grammatical features, and **senses** (`L1-S1`) with glosses, all three carrying statements. Over a million lexemes are in the dumps, and every Wikidata test import so far has skipped them: `Lexeme` (146) is `reserved` ([0008](0008-namespaces-and-document-pages.md) §2), the `wikibase-lexeme` model is `reserved` ([0041](0041-content-models.md) §3), the Wikidata provider's type `L` says "mirroring lexemes requires implementing it" ([0017](0017-entity-id-grammar.md), `providers.toml`), and `wikibase-compat.md` §2 puts lexemes, forms and senses out of scope. The ID grammar of [0017](0017-entity-id-grammar.md) §1 — one letter and digits — has no place for `L1-F1`; [0009](0009-keyed-entity-types-and-domain.md) noted forms and senses as the one Wikibase precedent for entity values without a numeric ID. [0031](0031-property-constraints.md) §1 already names the `lexeme requires language` constraint; [0043](0043-lua-modules.md) Q4 waits for `mw.wikibase.lexeme`.

### Direction

James's direction, from the design discussion of 2026-10-05:

- **Forms and senses are treated the way Wikidata treats them.**
- **Lexemes in this round**, since they affect a Wikidata test import.

## Decision

### 1. Lexemes as Wikidata has them

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.1.*

### 2. The `Lexeme` namespace (amends 0008 §2)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.1.*

### 3. The `wikibase-lexeme` content model (extends 0041 §3)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.1.*

### 4. IDs and clusters (extends 0017 §1; extends 0004 §1)

*Changed by A1, A3.*

*Current text: [04](../architecture/04-entities-and-identifiers.md) §2.1, §2.2, §4.1, §4.10, §5.1.*

### 5. Terms, search and storage (extends 0013 §5.6; extends 0014 §7; extends 0060 §4)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.2, §4.3, §4.4, §5, §6.1, §11.1.*

### 6. RDF (extends wikibase-compat.md §5)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §5.8.*

### 7. The UI and special pages (extends 0047 §9)

*Current text: [19](../architecture/19-site-ui.md) §2.1, §6.8; [21](../architecture/21-special-pages.md) §6.5, §7, §8.*

### 8. API (extends 0012 §5)

*Current text: [18](../architecture/18-api.md) §2.1, §2.3, §2.4, §3.2, §3.4, §4.*

### 9. Mirroring, constraints, Lua and tables (settles 0043 Q4)

*Current text: [04](../architecture/04-entities-and-identifiers.md) §5.1.*

### 10. Crates (amends 0005 §2)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Forms and senses as entities of their own**, with pages and cluster membership. Rejected by direction and by Wikibase: a form has no existence apart from its lexeme, and tools address parts through the lexeme's JSON.
- **Lemmas as labels.** Tempting for the UI, wrong for the data: a lexeme may have several lemmas in one language's variants, and `rdfs:label` is emitted beside `wikibase:lemma`, not instead of it.
- **A leaner RDF vocabulary.** Rejected: the dump has to load into a Wikidata-shaped QLever index unchanged, and OntoLex is what the ecosystem reads.
- **Merging parts across a cluster.** Renumbering a member's forms under the canonical lexeme would change IDs that statements elsewhere point at; kept as Q1.

## Consequences

- **A Wikidata test import is complete**: every entity type in the dumps mirrors.
- **Lexicographical data is first-class** — pages, statements, search, RDF, Lua, tables and scopes — with nothing bent: forms and senses are parts, lemmas are not labels, the vocabulary is OntoLex.
- **The ID grammar has one suffix form**, on one letter, and clusters stay at the lexeme level.
- **Test plan.** JSON round-trip of Wikidata lexemes through `put`, `wbgetentities` and the dump against the originals; the ShEx-free compat checklist of `wikibase-compat.md` §7 gains lexeme items; part-ID parsing and refusal of suffixes on other letters; `same-as` on parts refused; search by lemma, representation and gloss; each WikibaseLexeme module against the extension's own API tests; the lexeme page renders a 50-form lexeme within the page budget.

## Open questions

- **Q1. Parts across a cluster.** Whether the canonical lexeme's page should list a non-canonical member's forms and senses alongside its own, and how a `wikibase-form` value pointing at a member's form displays.
- **Q2. Lemma-based title resolution.** `Lexeme:run` is not a title today; whether a lemma resolver ([0029](0029-resolver-namespaces.md)) should disambiguate by language and category.
- **Q3. Senses as scope subjects in the builder** ([0062](0062-workspaces.md) §5): by-example over senses finds `P5137` patterns; whether the builder should offer part subject types.
- **Q4. Lexeme-specific notation** for languages without items (`mis`-coded lemmas), which Wikidata handles with special items; whether a Notation ([0048](0048-notation.md)) fits better here.

## References

- [Extension:WikibaseLexeme](https://www.mediawiki.org/wiki/Extension:WikibaseLexeme); [Wikibase/DataModel](https://www.mediawiki.org/wiki/Wikibase/DataModel) §Lexeme; the extension's `docs/` on JSON and RDF
- [Wikidata:Lexicographical data](https://www.wikidata.org/wiki/Wikidata:Lexicographical_data) and the [RDF dump format](https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format#Lexemes)
- [OntoLex-Lemon](https://www.w3.org/2016/05/ontolex/)
- [0017](0017-entity-id-grammar.md) §1, [0004](0004-identity-clusters-and-equivalence.md) §1: the grammar and cluster rules this ADR extends

## Amendment log

### A1. Merging local lexemes is a redirect

- **Date:** 2026-10-08
- **Source:** Direct: James, design discussion of 2026-10-08
- **Change:** corrects §4
- **Summary:** Merging two local lexemes is a `redirect`, as for items ([0004](0004-identity-clusters-and-equivalence.md) §2, unchanged): the source's forms and senses move to the target with new part numbers and each part leaves a redirect; no cluster is formed within the local namespace. (PENDING B7)

Replaced text (§4):

> **Merging lexemes** (`Special:MergeLexemes`, §7) is `same-as` plus the conversion of [0004](0004-identity-clusters-and-equivalence.md) §5 when both are local, which is what WikibaseLexeme's merge does by hand: the source's forms and senses are appended to the target with new part numbers and the source becomes a redirect; here the source becomes a cluster member and its parts keep their IDs.

### A2. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§10
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A3. The part suffix belongs to the lexeme entity type, not to a grammar

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while building lexeme part IDs, 2026-11-04; unconfirmed
- **Change:** extends §4
- **Summary:** A foreign ID of a provider type whose entity type is `lexeme` may carry the part suffix (`-F` or `-S` and digits, part letter canonical uppercase) after the rest in the type's ID grammar, whatever that grammar is; a type of any other entity type rejects it. No grammar is added to `providers.toml`, and Wikidata's `L` type keeps `digits`. The alternative, a `lexeme` grammar named on the type, would make every provider of lexemes declare it and would tie the suffix to digits. Built for the Wikidata mirror, whose item statements name senses and forms as values (`L499143-S1`, about 0.04% of entities in the 2026-10-26 dump), so that they sync instead of being unreadable.

