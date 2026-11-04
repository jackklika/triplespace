# Wikibase data model and ontology compatibility contract

- **Target:**
  - Wikibase **REL1_43** (`d561f2905229`, 2026-09-22) on MediaWiki 1.43.9
  - RDF format version **1.0.0**, ontology version **1.0**
  - **WikibaseEDTF 3.0.0**, using `professional-wiki/edtf` 3.1.0
- **Generated:** 2026-09-24
- **Companion documents:**
  - [mediawiki-compat.md](mediawiki-compat.md), which covers the HTTP API contract and the same reference install
  - [0000 — Initial proposition](../decisions/0000-init.md)

This document records the Wikibase data model as it is exposed to clients: the entity model, the JSON serialization and the RDF ontology and mapping. It also covers the EDTF data type added by the WikibaseEDTF extension. It is the reference Triplespace measures itself against when it claims to produce Wikibase-compatible JSON or RDF. It does not decide how much of the model Triplespace adopts.

## 1. Sources and provenance

### 1.1 Which sources are authoritative

| Topic | Authoritative source | Status |
|---|---|---|
| Ontology terms | `docs/ontology.owl` in the Wikibase tree. It is published at `http://wikiba.se/ontology`, and every RDF dump imports it as `<http://wikiba.se/ontology-1.0.owl>` | Normative for *names*. It is **incomplete** relative to what Wikibase emits (§4.5). |
| RDF mapping | The RDF builders in `repo/includes/Rdf/` and `RdfVocabulary.php` (`FORMAT_VERSION = '1.0.0'`). The human-readable version is mediawiki.org's *Wikibase/Indexing/RDF Dump Format*, which the ontology links via `rdfs:seeAlso` | The code is normative. Changes are governed by the *Wikidata Stable Interface Policy* (`docs/topics/rdf-binding.md`). |
| JSON serialization | `docs/topics/json.md` in the Wikibase tree | Normative |
| Data types | `repo/WikibaseRepo.datatypes.php` (the repository side) | Normative for the 12 built-in types |
| EDTF syntax | Library of Congress, *Extended Date/Time Format (EDTF) Specification*, final 2019-02-04. Level 0 is ISO 8601-1; levels 1 and 2 are ISO 8601-2 | Normative for EDTF strings |
| EDTF in Wikibase | The WikibaseEDTF source (`src/HookHandlers.php`, `src/Services/*.php`) and its README | Normative for the EDTF data type |

### 1.2 How the reference was produced

As with the API contract, statements here were checked against running software, not taken from documentation alone:

1. **Install.** The same MediaWiki 1.43.9 + Wikibase REL1_43 install described in `mediawiki-compat.md` §1.2 was used. WikibaseEDTF was cloned at its latest commit (`23d233c`, which is the 3.0.0 release plus translation updates), and `professional-wiki/edtf` 3.1.0 was installed with its dependencies. A local site, `testwiki` in group `wikipedia`, was registered so that sitelinks and badges could be exercised.
2. **Test data.** One test property was created for each data type (P1–P13). Two test items were then created:
   - **Q8** exercises every non-Commons data type, preferred, normal and deprecated ranks, qualifiers, a reference, `somevalue`, `novalue`, a badged sitelink, and nine EDTF values.
   - **Q9** holds 17 EDTF edge cases.
3. **RDF dumps.** RDF was produced with `repo/maintenance/dumpRdf.php` (the `full-dump` and `truthy-dump` flavors) and with `Special:EntityData/Q9.ttl?flavor=full`.
4. **Comparison.** The emitted vocabulary was compared mechanically against `ontology.owl` using rdflib.

Statements marked **(observed)** come from these outputs. Snapshots are committed in `docs/api/snapshots/`:

| File | Contents |
|---|---|
| `wikibase-ontology-1.0.owl` | `docs/ontology.owl` at REL1_43 |
| `wikibase-rdf-full-dump.ttl` | `dumpRdf.php --flavor full-dump` output for the whole reference install (Q1–Q9, P1–P13) |
| `wikibase-rdf-truthy-dump.ttl` | `dumpRdf.php --flavor truthy-dump` output |
| `wikibase-Q8.json` | `Special:EntityData/Q8.json`, the canonical JSON for the main test item |
| `wikibase-edtf-edge-cases-Q9.ttl` | Full RDF for the EDTF edge-case item |

### 1.3 Caveats

- **Commons-backed data types.** `commonsMedia`, `geo-shape` and `tabular-data` values are validated against Wikimedia Commons over the network, which the reference environment could not reach. Their RDF mapping is taken from the code (`CommonsMediaRdfBuilder`, `GeoShapeRdfBuilder`, `TabularDataRdfBuilder`) and was not observed.
- **Normalized values.** Normalized values (`wdtn:`/`psn:`) need configuration that the reference install does not have: `canonicalUriProperty` for external IDs and a unit-conversion table for quantities. The predicates are declared but carry no values in the snapshots.
- **URIs in the snapshots.** They use the reference install's base URI, `http://localhost:8080/entity/`. On Wikidata the same structures use `http://www.wikidata.org/entity/`.

## 2. Conceptual data model

- **Entities.** Wikibase REL1_43 defines two entity types: **item** (`Q<n>`) and **property** (`P<n>`). Other extensions add more: lexemes, forms and senses (WikibaseLexeme) are covered by §5.6 since ADR 0066; entity schemas (EntitySchema) have terms only and no RDF of their own (ADR 0064 §8); MediaInfo is covered by ADR 0041 §7 and 0065.
- **Terms.** Every entity carries a *fingerprint*: labels and descriptions (at most one per language) and aliases (several per language).
- **Sitelinks.** Items also carry sitelinks: at most one page per site, each with an optional set of badge items.
- **Data types.** Properties carry a data type, which is fixed at creation.

```
Entity ─┬─ labels{lang → text}, descriptions{lang → text}, aliases{lang → [text]}
        ├─ (item)     sitelinks{siteId → (title, badges[])}
        ├─ (property) datatype
        └─ statements{propertyId → [Statement]}

Statement = mainsnak, qualifiers{propertyId → [Snak]}, qualifiers-order,
            references[ Reference = snaks{propertyId → [Snak]}, snaks-order, hash ],
            rank ∈ {preferred, normal, deprecated}, id = "<EntityId>$<UUID>"

Snak      = property, snaktype ∈ {value, somevalue, novalue}, datavalue (value only), datatype, hash
```

Each **data type** (a property-level concept) is backed by a **value type** (the kind of `datavalue` stored). Several data types can share one value type.

| Data type | Value type | Defined by | RDF `wikibase:propertyType` | `wdt:` property kind |
|---|---|---|---|---|
| `commonsMedia` | `string` | Wikibase | `wikibase:CommonsMedia` | ObjectProperty |
| `external-id` | `string` | Wikibase | `wikibase:ExternalId` | DatatypeProperty |
| `geo-shape` | `string` | Wikibase | `wikibase:GeoShape` | ObjectProperty |
| `globe-coordinate` | `globecoordinate` | Wikibase | `wikibase:GlobeCoordinate` ⚠ | DatatypeProperty |
| `monolingualtext` | `monolingualtext` | Wikibase | `wikibase:Monolingualtext` | DatatypeProperty |
| `quantity` | `quantity` | Wikibase | `wikibase:Quantity` | DatatypeProperty |
| `string` | `string` | Wikibase | `wikibase:String` | DatatypeProperty |
| `tabular-data` | `string` | Wikibase | `wikibase:TabularData` | ObjectProperty |
| `time` | `time` | Wikibase | `wikibase:Time` | DatatypeProperty |
| `url` | `string` | Wikibase | `wikibase:Url` | ObjectProperty |
| `wikibase-item` | `wikibase-entityid` | Wikibase | `wikibase:WikibaseItem` | ObjectProperty |
| `wikibase-property` | `wikibase-entityid` | Wikibase | `wikibase:WikibaseProperty` | ObjectProperty |
| `edtf` | `string` | **WikibaseEDTF** | `wikibase:Edtf` ⚠ | DatatypeProperty |

⚠ These type IRIs are not declared in `ontology.owl` (see §4.5). All values in the table were **(observed)** as `wikibase:propertyType` triples and `owl:ObjectProperty`/`owl:DatatypeProperty` declarations on `wdt:` properties in the full dump. The only exceptions are the three Commons types, which are taken from the code.

## 3. JSON serialization

This is the canonical JSON returned by `wbgetentities`, `Special:EntityData/<id>.json` and the JSON dumps. The authoritative reference is `docs/topics/json.md`. The Wikibase REST API uses a different, flattened shape, described in `mediawiki-compat.md` §6.2.

### 3.1 Top level

```json
{"type": "item", "id": "Q8",
 "labels":       {"en": {"language": "en", "value": "Douglas Adams (test)"}},
 "descriptions": {"en": {"language": "en", "value": "English author and humorist"}},
 "aliases":      {"en": [{"language": "en", "value": "DNA"}]},
 "claims":       {"P1": [ …Statement… ]},
 "sitelinks":    {"testwiki": {"site": "testwiki", "title": "Douglas Adams", "badges": ["Q2"],
                               "url": "http://127.0.0.1:8080/index.php/Douglas_Adams"}},
 "pageid": 20, "ns": 120, "title": "Item:Q8", "lastrevid": 26, "modified": "2026-09-25T01:07:15Z"}
```

- Properties add `"datatype": "<data type id>"` and have no `sitelinks`.
- `pageid`, `ns`, `title`, `lastrevid` and `modified` are page metadata, not model data. They are emitted by `Special:EntityData` and by `wbgetentities` with `props=info`.
- Statements are keyed `claims` in this format, for historical reasons.

### 3.2 Statement (observed, Q8)

```json
{"mainsnak": {"snaktype": "value", "property": "P1", "hash": "7c8cb822…",
              "datavalue": {"value": "Douglas Noël Adams", "type": "string"}, "datatype": "string"},
 "type": "statement",
 "qualifiers": {"P4": [ …Snak… ]}, "qualifiers-order": ["P4"],
 "id": "Q8$C13E7A23-11E7-4C91-A799-3D1806B65444",
 "rank": "preferred",
 "references": [{"hash": "eaf0a11b…", "snaks": {"P8": [ … ], "P4": [ … ]}, "snaks-order": ["P8", "P4"]}]}
```

- **Snak hashes** are content hashes. Identical snaks share a hash across statements, qualifiers and references.
- **`somevalue` and `novalue` snaks** have no `datavalue`.

#### 3.2.1 How the hashes are computed (verified)

Wikibase hashes PHP serializations, in the pre-PHP-7.4 `Serializable` wrapper `C:<len>:"<class>":<len>:{<data>}` that `getSerializationForHash()` reproduces by hand. `scatter-wikibase-model::hash` implements these; every snak hash, the reference hash and every `v:` value node in the snapshots are reproduced by its tests.

| Hash | Of |
|---|---|
| snak, `value` | SHA-1 of `C:41:"Wikibase\DataModel\Snak\PropertyValueSnak":N:{a:2:{i:0;s:L:"P1";i:1;<value>}}` |
| snak, `somevalue` / `novalue` | SHA-1 of `C:43:"Wikibase\DataModel\Snak\PropertySomeValueSnak":L:{P4}` (resp. `…NoValueSnak`), the data being the bare property ID |
| reference | SHA-1 of the distinct snak hashes, sorted, joined with `\|` (`MapValueHasher`; a `SnakList` holds each hash once) |
| value node `v:` (RDF) | MD5 of `<value>`, except a globe coordinate: MD5 of `latitude\|longitude\|precision\|globe` with PHP's `(string)$float` (`%.14G`) |
| statement (not in JSON) | SHA-1 of `sha1(mainsnak hash ‖ qualifiers list hash)\|rank\|sha1(reference hashes sorted, joined with \|)`, rank 0/1/2 for deprecated/normal/preferred |

`<value>` by value type, where `S(x)` is PHP `serialize()` (`s:<bytes>:"…";`) and `J(x)` is `json_encode()` with no flags (`/` as `\/`, non-ASCII as `\uXXXX`, floats in shortest round-trip form, `51` for `51.0`):

| Value type | `<value>` |
|---|---|
| `string` | `C:22:"DataValues\StringValue":L:{<raw string>}` |
| `monolingualtext` | `C:31:"DataValues\MonolingualTextValue":L:{a:2:{i:0;S(language)i:1;S(text)}}` |
| `time` | `C:20:"DataValues\TimeValue":L:{J([time, timezone, before, after, precision, calendarmodel])}` |
| `globecoordinate` | `C:42:"DataValues\Geo\Values\GlobeCoordinateValue":L:{J([latitude, longitude, null, precision, globe])}` |
| `quantity`, bounded | `C:24:"DataValues\QuantityValue":L:{a:4:{i:0;D(amount)i:1;S(unit)i:2;D(upperBound)i:3;D(lowerBound)}}` with `D(x) = C:24:"DataValues\DecimalValue":L:{S(x)}` |
| `quantity`, unbounded | `C:33:"DataValues\UnboundedQuantityValue":L:{a:2:{i:0;D(amount)i:1;S(unit)}}` |
| `wikibase-entityid` | `C:39:"Wikibase\DataModel\Entity\EntityIdValue":L:{C:<len>:"<ID class>":L:{Q3}}`, the class `Wikibase\DataModel\Entity\ItemId`, `…\NumericPropertyId`, `Wikibase\Lexeme\Domain\Model\LexemeId` / `FormId` / `SenseId` or `Wikibase\MediaInfo\DataModel\MediaInfoId` by `entity-type` |
| `wikibase-entityid` with `entity-type` `entity-schema` | `C:50:"EntitySchema\Wikibase\DataValues\EntitySchemaValue":L:{O:40:"EntitySchema\Domain\Model\EntitySchemaId":1:{S("\0EntitySchema\Domain\Model\EntitySchemaId\0id")S(E3)}}`: the EntitySchema extension's value is a `DataValueObject`, not an `EntityIdValue`, and its ID is a plain PHP object with one private property (`\0` is a NUL byte). Checked against Wikidata's hashes (2026-10-26 dump) |

The ID inside an entity value is the one the source wrote: Wikidata hashes `Q3`, so a mirrored `WDQ3` is hashed as `Q3` (`Hasher::mirrored_from("WD")`). Everything else in a hash is bytes the JSON carries verbatim, which is why the guard of 0006 §2 can compare on every entity.

### 3.3 Data values by value type (observed)

| Value type | `datavalue` |
|---|---|
| `string` | `{"value": "n80076765", "type": "string"}`. This is also used by `url`, `external-id`, `commonsMedia`, `geo-shape`, `tabular-data` and **`edtf`**. |
| `wikibase-entityid` | `{"value": {"entity-type": "item", "numeric-id": 3, "id": "Q3"}, "type": "wikibase-entityid"}` |
| `globecoordinate` | `{"value": {"latitude": 51.5, "longitude": -0.1275, "altitude": null, "precision": 0.0001, "globe": "http://www.wikidata.org/entity/Q2"}, "type": "globecoordinate"}` |
| `quantity` | `{"value": {"amount": "+1.96", "unit": "http://localhost:8080/entity/Q3", "upperBound": "+1.97", "lowerBound": "+1.95"}, "type": "quantity"}`. A unitless quantity uses `"unit": "1"`, and the bounds are optional. |
| `monolingualtext` | `{"value": {"text": "Douglas Adams", "language": "en"}, "type": "monolingualtext"}` |
| `time` | `{"value": {"time": "+1952-03-11T00:00:00Z", "timezone": 0, "before": 0, "after": 0, "precision": 11, "calendarmodel": "http://www.wikidata.org/entity/Q1985727"}, "type": "time"}` |

**Time values** (per `json.md`):

- **`time`:** an ISO 8601-like string with a mandatory sign and at least four year digits.
  - Month and day are `00` when unknown, as in `+1500-00-00T00:00:00Z`.
  - Years use **historical numbering**: there is no year 0, and 1 BCE is `-0001`. The RDF mapping instead uses XSD 1.1 astronomical numbering, where 1 BCE is `0000`.
  - Hours, minutes and seconds are "currently unused and should always be 00".
- **`timezone`:** minutes offset from UTC, "currently unused, and should always be 0".
- **`before` / `after`:** uncertainty in units of `precision`. Currently unused.
- **`precision`:**

  | Value | Unit | Value | Unit |
  |---|---|---|---|
  | 0 | 1 Gigayear | 8 | decade |
  | 1 | 100 Megayears | 9 | year |
  | 2 | 10 Megayears | 10 | month |
  | 3 | Megayear | 11 | day |
  | 4 | 100 Kiloyears | 12 | hour (unused) |
  | 5 | 10 Kiloyears | 13 | minute (unused) |
  | 6 | millennium | 14 | second (unused) |
  | 7 | century | | |

- **`calendarmodel`:** a URI. Only Gregorian (`…/Q1985727`) and Julian (`…/Q1985786`) are supported.

**Hard-coded Wikidata IRIs.** The calendar URIs, the default globe (`http://www.wikidata.org/entity/Q2`, Earth) and the RDF "unit one" (`http://www.wikidata.org/entity/Q199`) are **Wikidata IRIs hard-coded in the DataValues libraries and in `RdfVocabulary`**. Every Wikibase emits them, whatever its own concept base URI (**observed** on the reference install).

## 4. The Wikibase ontology (`http://wikiba.se/ontology#`)

The ontology is written in OWL (RDF/XML, CC0). Its header reads `rdfs:label "Wikibase system ontology"` and `rdfs:seeAlso <https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format>`. The tables below list every term declared at REL1_43.

### 4.1 Classes

| Class | Superclass | Comment (from ontology) |
|---|---|---|
| `wikibase:Entity` | | Wikibase entity. |
| `wikibase:Item` | `wikibase:Entity` | Wikibase item. |
| `wikibase:Property` | `wikibase:Entity` | Wikibase property. |
| `wikibase:Statement` | | A reified statement. |
| `wikibase:Reference` | | A reference to sources supporting the claim. |
| `wikibase:Value` | | Wikibase extended value. |
| `wikibase:TimeValue` | `wikibase:Value` | Extended value representing time. |
| `wikibase:QuantityValue` | `wikibase:Value` | Extended value representing quantity. |
| `wikibase:GlobecoordinateValue` | `wikibase:Value` | Extended value representing geographic coordinate. |
| `wikibase:BestRank` | | Marks a statement that has the best rank for its property. |
| `wikibase:Rank` | | Wikibase statement rank. |
| `wikibase:PropertyType` | | Type of a Wikibase property. |
| `wikibase:Dump` | | A dump of Wikidata content. |
| `wikibase:GeoAutoPrecision` | | Coordinate value using automatic precision. |

The ontology also asserts `owl:AllDisjointClasses (wikibase:Item, wikibase:Property)`.

### 4.2 Named individuals

- **Ranks** (`a wikibase:Rank`): `wikibase:PreferredRank`, `wikibase:NormalRank`, `wikibase:DeprecatedRank`.
- **Property types** (`a wikibase:PropertyType`):
  - Built into Wikibase: `wikibase:CommonsMedia`, `ExternalId`, `GeoShape`, `Globecoordinate`, `Monolingualtext`, `Quantity`, `String`, `TabularData`, `Time`, `Url`, `WikibaseItem`, `WikibaseProperty`.
  - Supplied by other extensions: `wikibase:Math` (Math extension) and `wikibase:MusicalNotation` (Score extension).

### 4.3 Object properties

| Property | Domain → Range | Meaning |
|---|---|---|
| `wikibase:rank` | Statement → Rank | Statement rank |
| `wikibase:badge` | → Item | Badge on a sitelinked page |
| `wikibase:claim`, `directClaim`, `statementProperty`, `statementValue`, `statementValueNormalized`, `qualifier`, `qualifierValue`, `qualifierValueNormalized`, `reference`, `referenceValue`, `referenceValueNormalized`, `novalue` | Property → (predicate) | Link a property entity to its family of generated predicates (§5.3) |
| `wikibase:geoGlobe` | GlobecoordinateValue → Item | Globe |
| `wikibase:quantityUnit` | QuantityValue → Item | Unit |
| `wikibase:quantityNormalized` | QuantityValue → QuantityValue | Value in standard units |
| `wikibase:timeCalendarModel` | TimeValue → Item | Calendar model |

### 4.4 Datatype properties

| Property | Domain | Declared range |
|---|---|---|
| `wikibase:timeValue` | TimeValue | `xsd:dateTime` |
| `wikibase:timePrecision` | TimeValue | `xsd:nonNegativeInteger` |
| `wikibase:timeTimezone` | TimeValue | `xsd:decimal` |
| `wikibase:quantityAmount`, `quantityUpperBound`, `quantityLowerBound` | QuantityValue | `xsd:decimal` |
| `wikibase:geoLatitude`, `geoLongitude`, `geoPrecision` | GlobecoordinateValue | `xsd:decimal` |
| `wikibase:wikiGroup` | | Group a site belongs to |

### 4.5 Where the emitted RDF departs from `ontology.owl` (observed)

The emitted vocabulary was compared mechanically with the ontology. **Clients and query services are built against the emitted RDF, so the emitted form is the de facto contract.**

| Kind | Term | What happens |
|---|---|---|
| Used but not declared | `wikibase:propertyType` | Links every property to its type individual |
| Used but not declared | `wikibase:statements`, `wikibase:sitelinks`, `wikibase:identifiers` | `xsd:integer` counts on the `schema:Dataset` node |
| Used but not declared | `wikibase:directClaimNormalized` | Links to the `wdtn:` predicate (only for types with normalization) |
| **Name mismatch** | `wikibase:GlobeCoordinate` | Emitted as the type of `globe-coordinate` properties. The ontology declares **`wikibase:Globecoordinate`**, with a lower-case *c*. |
| Used but not declared | `wikibase:Edtf` | WikibaseEDTF declares no `rdf-data-type`, so Wikibase falls back to a camel-cased name (`RdfVocabulary::getDataTypeURI`). |
| Range mismatch | `geoLatitude`, `geoLongitude`, `geoPrecision` | Emitted as `xsd:double`; declared as `xsd:decimal` |
| Range mismatch | `timePrecision` | Emitted as `xsd:integer`; declared as `xsd:nonNegativeInteger` |
| Range mismatch | `timeTimezone` | Emitted as `xsd:integer`; declared as `xsd:decimal` |
| Declared, never emitted here | `wikibase:Entity`, `Value`, `Rank`, `PropertyType` | These are used only as superclasses or types in the ontology. |
| Declared, never emitted here | `wikibase:GeoAutoPrecision`, `quantityNormalized` | Emitted only when the relevant configuration exists |
| Declared, never emitted here | `wikibase:Math`, `MusicalNotation` | Emitted only when the Math or Score extension is installed |

## 5. RDF mapping (format 1.0.0)

### 5.1 Namespaces

The entity namespace prefixes shown here are the defaults for the local repository. On Wikidata the base is `http://www.wikidata.org/`, and federated or foreign sources get their own prefixes, such as `wd`/`wdt` equivalents per entity source (see Wikibase `docs/topics/entitysources.md`). The URIs below are for concept base `http://localhost:8080/entity/` **(observed)**.

| Prefix | URI | Role |
|---|---|---|
| `wikibase` | `http://wikiba.se/ontology#` | Ontology |
| `wd` | `{base}/entity/` | Entity concept URIs |
| `data` | `{script}/Special:EntityData/` | Entity *document* URIs (`schema:Dataset`) |
| `s` | `{base}/entity/statement/` | Statement nodes (`Q8-<UUID>`; the `$` is replaced by `-`) |
| `ref` | `{base}/reference/` | Reference nodes (named by the reference hash) |
| `v` | `{base}/value/` | Full value nodes (named by the value hash) |
| `wdt` | `{base}/prop/direct/` | Truthy entity → simple value |
| `wdtn` | `{base}/prop/direct-normalized/` | Truthy entity → normalized value |
| `p` | `{base}/prop/` | Entity → statement |
| `ps` | `{base}/prop/statement/` | Statement → simple value |
| `psv` | `{base}/prop/statement/value/` | Statement → full value node |
| `psn` | `{base}/prop/statement/value-normalized/` | Statement → normalized value node |
| `pq`, `pqv`, `pqn` | `{base}/prop/qualifier/`, `…/value/`, `…/value-normalized/` | Qualifiers |
| `pr`, `prv`, `prn` | `{base}/prop/reference/`, `…/value/`, `…/value-normalized/` | Reference snaks |
| `wdno` | `{base}/prop/novalue/` | novalue classes |
| `schema`, `skos`, `rdfs`, `owl`, `prov`, `cc`, `geo`, `xsd` | Standard vocabularies (`http://schema.org/`, SKOS core, `http://www.opengis.net/ont/geosparql#` and so on) | |

Prefix names such as `wd` and `wdt` are conventional and can be configured. **The URIs are the contract; the prefix names are not.**

### 5.2 Dump header, entity document and entity

**(observed)**

```turtle
wikibase:Dump a schema:Dataset, owl:Ontology ;
    cc:license <http://creativecommons.org/publicdomain/zero/1.0/> ;
    schema:softwareVersion "1.0.0" ;                          # RDF FORMAT_VERSION
    schema:dateModified "2026-09-25T01:07:23Z"^^xsd:dateTime ;
    owl:imports <http://wikiba.se/ontology-1.0.owl> .

data:Q8 a schema:Dataset ;
    schema:about wd:Q8 ;
    schema:version "26"^^xsd:integer ;                        # revision ID
    schema:dateModified "2026-09-25T01:07:15Z"^^xsd:dateTime ;
    wikibase:statements "23"^^xsd:integer ;
    wikibase:sitelinks "1"^^xsd:integer ;
    wikibase:identifiers "1"^^xsd:integer .                   # count of external-id statements

wd:Q8 a wikibase:Item ;
    rdfs:label "Douglas Adams (test)"@en ; skos:prefLabel "Douglas Adams (test)"@en ; schema:name "Douglas Adams (test)"@en ;
    schema:description "English author and humorist"@en ;
    skos:altLabel "DNA"@en .
```

- **Labels** are emitted three times, as `rdfs:label`, `skos:prefLabel` and `schema:name`.
- **Sitelinks (observed):**

  ```turtle
  <http://127.0.0.1:8080/index.php/Douglas_Adams> a schema:Article ;
      schema:about wd:Q8 ; schema:inLanguage "en" ; schema:isPartOf <http://127.0.0.1/> ;
      schema:name "Douglas Adams"@en ; wikibase:badge wd:Q2 .
  <http://127.0.0.1/> wikibase:wikiGroup "wikipedia" .
  ```

### 5.3 Property entities

Each property gets its own family of predicates **(observed, P5)**:

```turtle
wd:P5 a wikibase:Property ;
    wikibase:propertyType <http://wikiba.se/ontology#Quantity> ;
    wikibase:directClaim wdt:P5 ;  wikibase:claim p:P5 ;
    wikibase:statementProperty ps:P5 ;  wikibase:statementValue psv:P5 ;
    wikibase:qualifier pq:P5 ;  wikibase:qualifierValue pqv:P5 ;
    wikibase:reference pr:P5 ;  wikibase:referenceValue prv:P5 ;
    wikibase:novalue wdno:P5 ;
    wikibase:directClaimNormalized wdtn:P5 ;  wikibase:statementValueNormalized psn:P5 ;
    wikibase:qualifierValueNormalized pqn:P5 ;  wikibase:referenceValueNormalized prn:P5 .
p:P5 a owl:ObjectProperty .
wdt:P5 a owl:DatatypeProperty .          # ObjectProperty for IRI-valued types (§2 table)
wdno:P5 a owl:Class ; owl:complementOf [ a owl:Restriction ; owl:onProperty wdt:P5 ; owl:someValuesFrom owl:Thing ] .
```

The `…Normalized` links are emitted only for data types that support normalization: `quantity` and `external-id`.

### 5.4 Statements, ranks and the truthy projection

```turtle
wd:Q8 p:P1 s:Q8-C13E7A23-… .
s:Q8-C13E7A23-… a wikibase:Statement, wikibase:BestRank ;
    wikibase:rank wikibase:PreferredRank ;
    ps:P1 "Douglas Noël Adams" ;
    pq:P4 "1952-03-11T00:00:00Z"^^xsd:dateTime ;
    pqv:P4 v:426df902… ;
    prov:wasDerivedFrom ref:eaf0a11b… .
ref:eaf0a11b… a wikibase:Reference ;
    pr:P8 <https://example.org/source> ;
    pr:P4 "1952-03-11T00:00:00Z"^^xsd:dateTime ; prv:P4 v:426df902… .
```

- **Best rank.** A statement is typed `wikibase:BestRank` when it is non-deprecated and has the highest rank present for its property on that entity. That means the preferred statements if there are any, and otherwise all normal ones. In Q8, the preferred P1 statement is `BestRank` and the deprecated P1 statement is not **(observed)**.
- **Truthy triples.** `wdt:` triples are emitted **only for BestRank statements**. They appear in both the `full-dump` and `truthy-dump` flavors.
- **The `truthy-dump` flavor** contains *only* the truthy triples, entity and property metadata and terms. It has no `p:`/`ps:`/`s:`/`ref:`/`v:` nodes (**observed**: zero statement nodes).
- **Node naming.** Reference and value nodes are named by content hash, so they are shared. For example, `v:426df902…` is referenced from a qualifier, a reference and a main value.
- **`somevalue`** becomes a **blank node** as the object of `ps:`/`wdt:`, for example `wdt:P4 _:82688c58…` **(observed)**. It has no stable identity across dumps.
- **`novalue`** is emitted as class membership. The statement node gets `a wdno:P2`, and so does the entity if the statement is best-rank.

### 5.5 Values by data type

The simple form is the object of `wdt:`, `ps:`, `pq:` and `pr:`. The full form is the value node reached via `psv:`, `pqv:` and `prv:`.

| Data type | Simple value | Full value node |
|---|---|---|
| `string` | `"Douglas Noël Adams"` (plain literal) | — |
| `external-id` | `"n80076765"` (plain literal) | — (`wdtn:`/`psn:` IRI if `canonicalUriProperty` is set) |
| `monolingualtext` | `"Douglas Adams"@en` | — |
| `url` | `<https://douglasadams.com/>` | — |
| `wikibase-item` / `-property` | `wd:Q3` / `wd:P1` | — |
| `commonsMedia` | `<http://commons.wikimedia.org/wiki/Special:FilePath/{rawurlencoded file}>` *(code)* | — |
| `geo-shape`, `tabular-data` | `<http://commons.wikimedia.org/data/main/{urlencoded page}>` *(code)* | — |
| `time` | `"1952-03-11T00:00:00Z"^^xsd:dateTime`. Unknown month or day become `01`, and **Julian dates are converted to Gregorian only at day precision or finer** (`JulianDateTimeValueCleaner`); `+1500-00-00` at year precision emits `1500-01-01`. | `a wikibase:TimeValue ; wikibase:timeValue …^^xsd:dateTime ; timePrecision "11"^^xsd:integer ; timeTimezone "0"^^xsd:integer ; timeCalendarModel <…Q1985727>` |
| `quantity` | `"+1.96"^^xsd:decimal` | `a wikibase:QuantityValue ; quantityAmount ; quantityUpperBound ; quantityLowerBound ; quantityUnit <unit IRI>`. A unitless quantity has unit **`<http://www.wikidata.org/entity/Q199>`**. |
| `globe-coordinate` | `"Point(-0.1275 51.5)"^^geo:wktLiteral`. The order is longitude then latitude, and the WKT is prefixed with the globe IRI when the globe is not Earth. | `a wikibase:GlobecoordinateValue ; geoLatitude ; geoLongitude ; geoPrecision` (all `xsd:double`) `; geoGlobe <…Q2>` |
| `edtf` | **Two or more literals**; see §6.4 | `wikibase:TimeValue` nodes derived from the EDTF value; see §6.4 |

### 5.6 Lexemes (ADR 0066 §6)

*To be filled from the observed dump, as §5.2–5.5 were.* The mapping ADR 0066 §6 fixes: a lexeme is an `ontolex:LexicalEntry` with `wikibase:lemma` and `rdfs:label` per lemma, `dct:language`, `wikibase:lexicalCategory`, `ontolex:lexicalForm` and `ontolex:sense`; a form is an `ontolex:Form` with `ontolex:representation` and `wikibase:grammaticalFeature`; a sense is an `ontolex:LexicalSense` with `skos:definition`; statements on all three as §5.4. Part IRIs carry the `-F`/`-S` suffix in the entity's local part.

## 6. EDTF extension (WikibaseEDTF)

### 6.1 What it is

[WikibaseEDTF](https://github.com/ProfessionalWiki/WikibaseEDTF) is maintained by Professional Wiki and was funded by the Luxembourg Ministry of Culture. It adds a single data type, **`edtf`**, whose values are strings in the Library of Congress **Extended Date/Time Format**. EDTF covers what the native `time` type cannot express:

- uncertain and approximate dates (`1984?`, `2004-06~`, `2004-06-11%`);
- unspecified digits (`201X`, `156X-12-25`);
- intervals, including open ones (`1964/2008`, `1985-04-12/..`);
- seasons and quarters (`2001-21`, `2001-34`);
- sets (`[1667,1668,1670..1672]` for one of them, `{…}` for all of them);
- very large years (`Y170000002`, `Y-17E7`);
- significant digits (`1950S2`);
- real times of day with a UTC offset.

EDTF is layered. **Level 0** is ISO 8601-1: dates, reduced precision, date-times and intervals. **Levels 1 and 2** are ISO 8601-2 extensions. WikibaseEDTF validates all three levels using the `professional-wiki/edtf` library.

**Versions.**

| WikibaseEDTF | MediaWiki / Wikibase | Note |
|---|---|---|
| **3.0.0** (2025-07-10) | 1.40–1.44 | **The version targeted here** |
| 2.1.2 | 1.37–1.43 | Also runs on 1.43 |

The repository's head (`23d233c`, 2026-09-21) adds only translation updates to 3.0.0.

### 6.2 Data type definition

The type is registered through the `WikibaseRepoDataTypes` and `WikibaseClientDataTypes` hooks as `PT:edtf`:

| Aspect | Definition |
|---|---|
| Data type ID | `edtf` |
| Value type | `string`, a `DataValues\StringValue` holding the EDTF string verbatim |
| Parser | Identity: wraps the input in a `StringValue` |
| Validator | `EdtfValidator::isValidEdtf`. The error text is "Invalid EDTF", surfaced through the API as `wikibase-validator-invalid`. |
| Formatters | HTML: an internationalized humanization, e.g. "circa June 2004". Wikitext and plain text: the raw string. |
| RDF builder | `Wikibase\EDTF\Services\RdfBuilder` (§6.4) |
| `rdf-data-type` | **Not set**, so `wdt:` is declared `owl:DatatypeProperty` and `propertyType` falls back to `wikibase:Edtf` |
| UI | `jquery.valueview.experts.edtf` (a text input) |
| Client | Formatter only. There is no parser, validator or RDF. |

JSON is identical to any string-valued snak, except for `datatype` **(observed)**:

```json
{"snaktype": "value", "property": "P13", "hash": "e9d46ea3…",
 "datavalue": {"value": "1985-04-12T23:20:30+04:00", "type": "string"}, "datatype": "edtf"}
```

**Validation (observed via `wbparsevalue&datatype=edtf&validate=1`).**
- All 27 distinct level 0–2 examples tried were accepted.
- `2019-02-30` (an impossible date) and `1985-13` (an impossible month) were rejected.
- Strings are stored exactly as entered. There is no normalization.

### 6.3 RDF contract as documented by the extension

From the README:

> Wikibase EDTF gives its value to the native RDF export mechanism using the datatypes `xsd:edtf` (e.g. `"1867-05-26"^^xsd:edtf`) and `xsd:dateTime`. For the latter datatype it turns EDTF values into standard Wikibase time (`xsd:dateTime`) values … Precision and time zone are retained. Qualifications and unspecified digits are discarded. **Set:** each date in the set is exported. **Season:** one date for each month is exported, each having month precision. **Interval:** nothing is exported.

The README also warns that querying an EDTF property returns both literals. It recommends `FILTER(datatype(?date) = xsd:edtf)` or `FILTER(datatype(?date) = xsd:dateTime)` to pick one.

### 6.4 RDF contract as emitted (observed)

For each EDTF snak, `RdfBuilder::addValue`:

1. emits the raw string once, as `"<edtf>"^^xsd:edtf`;
2. converts the EDTF value into zero or more native `TimeValue`s (`TimeValueBuilder`);
3. emits each of those through the standard `TimeRdfBuilder`. That gives a simple `xsd:dateTime` literal and, in full flavor, a `wikibase:TimeValue` node on `psv:`/`pqv:`/`prv:`.

```turtle
s:Q8-5F5C351A-… a wikibase:Statement, wikibase:BestRank ;
    wikibase:rank wikibase:NormalRank ;
    ps:P13 "[1667,1668,1670..1672]"^^xsd:edtf,
           "1667-01-01T00:00:00Z"^^xsd:dateTime ;  psv:P13 v:816ab642… ;
    ps:P13 "1668-01-01T00:00:00Z"^^xsd:dateTime ;  psv:P13 v:08258d0d… ;
    ps:P13 "1670-01-01T00:00:00Z"^^xsd:dateTime ;  psv:P13 v:c9cc548e… ;
    ps:P13 "1671-01-01T00:00:00Z"^^xsd:dateTime ;  psv:P13 v:c3681f7b… ;
    ps:P13 "1672-01-01T00:00:00Z"^^xsd:dateTime ;  psv:P13 v:6381f3fa… .
```

**The `xsd:edtf` IRI.** `xsd:edtf` expands to `http://www.w3.org/2001/XMLSchema#edtf`. That term is **not defined by XML Schema** or by any W3C vocabulary. Typed literals with it are valid RDF, but they are opaque to standard reasoners and to SPARQL value functions. Treat it as a Wikibase-EDTF-specific datatype IRI that happens to sit in the XSD namespace.

**Mapping observed for every EDTF form tried.** The last column of each row is what an EDTF-aware consumer loses.

| EDTF input | Level | `xsd:dateTime` literal(s) emitted | `timePrecision` | Loss or defect |
|---|---|---|---|---|
| `1952-03-11` | 0 | `1952-03-11T00:00:00Z` | 11 (day) | None |
| `1985-04` / `1985` | 0 | `1985-04-01…` / `1985-01-01…` | 10 / 9 | None |
| `1985-04-12T23:20:30Z` | 0 | `1985-04-12T23:20:30Z` | **13 (minute)** | Seconds precision is reported as minutes |
| `1985-04-12T23:20:30` | 0 | `1985-04-12T23:20:30Z` | 13 | A local time without an offset is emitted as UTC |
| `1985-04-12T23:20:30+04:00` | 0 | `1985-04-12T23:20:30Z`, with `timeTimezone 240` | 13 | **The clock time is not shifted to UTC.** The literal claims 23:20:30 UTC; the real instant is 19:20:30 UTC. |
| `1964/2008` | 0 | *none* | — | The interval is dropped entirely; there is no `psv:` node |
| `1985-04-12/..`, `../1985-04-12`, `1985-04-12/`, `2004-06-~01/2004-06-~20` | 1–2 | *none* | — | Open, unknown and qualified intervals are dropped |
| `2004-06~`, `1984?-06-02`, `2004-06-11%`, `?2004-06-~11` | 1–2 | The date without its qualifier | 10 / 11 | Uncertain and approximate markers are discarded |
| `19XX` | 1 | `1900-01-01T00:00:00Z` | **9 (year)** | "Some year in the 1900s" becomes the year 1900 |
| `156X-12-25` | 2 | `1560-12-25T00:00:00Z` | **11 (day)** | Becomes a specific, wrong day |
| `XXXX-12-XX` | 2 | *none* | — | A null year cannot be represented |
| `2001-21` (spring) | 1 | Three literals: `2001-03-01`, `-04-01`, `-05-01` | 10 each | The season becomes three unrelated month values |
| `2001-34` (quarter 2) | 2 | Three literals: April, May, June | 10 each | As above |
| `[1667,1668,1670..1672]` (one of) | 2 | Five literals, one per year | 9 each | "One of" semantics are lost; the values read as if all hold |
| `{1667,1668,1670..1672}` (all of) | 2 | Five literals, one per year | 9 each | Indistinguishable from the "one of" set |
| `-1985` | 1 | **`-1984-01-01T00:00:00Z`** | 9 | **Off by one year.** EDTF/ISO 8601 years are astronomical, so `-1985` is 1986 BCE. The extension passes the number into a Wikibase `TimeValue`, which reads it as *historical* (1985 BCE), and the RDF cleaner then converts it to astronomical `-1984`. |
| `Y-17E7` | 2 | `-169999999-01-01T00:00:00Z` | 9 | The same off-by-one error |
| `Y170000002` | 1 | `170000002-01-01T00:00:00Z` | 9 | A valid XSD 1.1 year, but many parsers (rdflib included) reject it |
| `1950S2` | 2 | *none* | — | Significant-digit forms are dropped |

**Structural consequences for consumers:**

- **More than one simple value per statement.** An EDTF statement can carry several `ps:` values: always one `xsd:edtf` literal, plus zero to N `xsd:dateTime` literals. Qualifiers and references behave the same way. This breaks the usual Wikibase invariant that a `ps:` value is single-valued per statement.
- **Unrelated values in the truthy projection.** `wdt:` for an EDTF property therefore carries several literals per statement, with nothing linking the `xsd:dateTime` literals to their source `xsd:edtf` literal or to one another. On Q8, `wdt:P13` has 23 objects for 9 statements.
- **Duplicated literals.** Two statements that differ only in UTC offset, such as `…Z` and `…+04:00`, produce the same `xsd:dateTime` literal. Their `psv:` nodes are kept apart only by `timeTimezone`.
- **Shared value nodes.** EDTF-derived `TimeValue` nodes use the Gregorian calendar and are content-hashed like native ones. For example, `v:426df902…` is shared by the native `P4` time `1952-03-11` and the EDTF `P13` value `1952-03-11`.

## 7. Compatibility checklist

A Triplespace implementation that claims Wikibase-compatible output should reproduce:

1. **The JSON shapes in §3.** That includes `claims` (not `statements`), hash fields, the `*-order` arrays, `"unit": "1"`, historical year numbering, and the `00` month/day convention.
2. **The emitted RDF of §5, not `ontology.owl` where the two differ (§4.5).** In particular:
   - `wikibase:GlobeCoordinate`;
   - `xsd:double` coordinates;
   - `xsd:integer` precision and timezone;
   - the undeclared `propertyType`, `statements`, `sitelinks` and `identifiers` predicates.
3. **The hard-coded Wikidata IRIs** for calendars, Earth and unit one, even on a non-Wikidata base URI. Changing them breaks queries and tools written for Wikidata.
4. **The truthy rule** (BestRank) and the two dump flavors.
5. **For `edtf`:**
   - JSON as a plain string value with `datatype: "edtf"`;
   - RDF with `^^<http://www.w3.org/2001/XMLSchema#edtf>` plus derived `xsd:dateTime` values;
   - `propertyType wikibase:Edtf`.

   Reproducing the §6.4 defects (off-by-one negative years, unshifted UTC offsets) is a policy choice for Triplespace. They should not be copied without a decision.

## 8. Regenerating

1. Starting from the `mediawiki-compat.md` §8 install, add WikibaseEDTF and `professional-wiki/edtf`.
2. Set `$wgWBRepoSettings['siteLinkGroups'] = ['wikipedia']`, register a site with `maintenance/run.php addSite`, and set `$wgRateLimits = []`.
3. Create one property per data type plus the Q8 and Q9 fixtures.
4. Run `extensions/Wikibase/repo/maintenance/dumpRdf.php --format ttl --flavor full-dump` (and `truthy-dump`).
5. Compare the dump with `docs/ontology.owl`: parse both with an RDF library, then set-difference the `wikibase:` IRIs and the literal datatypes per predicate.

## 9. Sources

Primary:

- Wikibase REL1_43: <https://github.com/wikimedia/mediawiki-extensions-Wikibase/tree/REL1_43>
  - `docs/ontology.owl`, `docs/topics/rdf-binding.md`, `docs/topics/json.md`, `docs/topics/datatypes.md`
  - `repo/includes/Rdf/`, `repo/WikibaseRepo.datatypes.php`
- DataValues libraries (calendar and globe constants): <https://github.com/DataValues/Time>, <https://github.com/DataValues/Geo>
- WikibaseEDTF: <https://github.com/ProfessionalWiki/WikibaseEDTF> (3.0.0, and head `23d233c`)
- EDTF PHP library: <https://github.com/ProfessionalWiki/EDTF> (3.1.0)
- EDTF specification: [Library of Congress, Extended Date/Time Format](https://www.loc.gov/standards/datetime/)
- Snapshots in `docs/api/snapshots/` (§1.2)

Secondary, not fetched for this revision (mediawiki.org and wikidata.org are unreachable from the build environment), but cited by the primary sources:

- [Wikibase/Indexing/RDF Dump Format](https://www.mediawiki.org/wiki/Wikibase/Indexing/RDF_Dump_Format)
- [Wikidata:Stable Interface Policy](https://www.wikidata.org/wiki/Wikidata:Stable_Interface_Policy)
- [Wikibase/DataModel](https://www.mediawiki.org/wiki/Wikibase/DataModel)
