# 04. Entities and identifiers

This chapter describes what an entity ID is in Triplespace: the forms an ID takes, the grammar of each minted type, the keyed types whose ID is a natural key rather than a minted number, and the identity clusters that say when IDs from different namespaces denote one thing and which of them is canonical. It also covers lexemes and MediaInfo as entity types, the derived IDs of foreign pages, the per-tenant Lua ID space, and where an ID is stored and shown. It assumes the log and record model of [01](01-log-and-records.md), the graphs and resolved view of [02](02-graphs-rdf-and-query.md), and the `view` schema of [03](03-storage-caches-and-search.md). Who minted an ID is distinguished here from who asserts a triple about it; the source graphs, key-mapped providers and entity sources that the second question leads to are in [05](05-providers-and-ingest.md), and the statement model is in [06](06-statements-and-properties.md). The registry files `docs/registry/providers.toml`, `keyed-types.toml` and `notation-schemes.toml` are authoritative for every code, type and scheme this chapter names; the chapter cites them and does not repeat them.

## 1. Local and foreign entities

*Sources: [0000](../decisions/0000-init.md) §3; [0017](../decisions/0017-entity-id-grammar.md) §1.*

A foreign entity can be referenced directly, without first being reified as a local item. Identifiers distinguish the two: **local entities** keep Wikibase's single-letter prefixes (`Q`, `P`, and so on), and **foreign entities** use a three-letter prefix, a two-letter provider code followed by a one-letter type code. **A prefix records who minted an ID, not who asserts its triples.** `WDQ123` was minted by Wikidata, and `Q5` by this instance. Which named graph a triple lives in says who asserts it, so an entity with a foreign ID can have triples in a provider's mirror graph, in the local graph, or in both ([05](05-providers-and-ingest.md)).

**Provider codes and type codes are registry data.** `docs/registry/providers.toml` allocates them, and is authoritative. The first three providers were Wikidata (`WD`: `WDQ` items, `WDP` properties, `WDL` lexemes, `WDM` Commons MediaInfo), Librarybase (`LB`) and OpenAlex (`OA`: `OAW` works, `OAA` authors and the rest). After the type code, an ID follows its type's grammar, which need not be digits (§2.2), and OpenAlex keywords are a keyed type rather than `OAK` entities (§3.7).

## 2. The forms of an entity ID

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1, §2, §6; [0044](../decisions/0044-tenant-relative-ids.md) §1, §2; [0066](../decisions/0066-lexemes.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §2.*

### 2.1 The ID forms

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1; [0066](../decisions/0066-lexemes.md) §4; [0044](../decisions/0044-tenant-relative-ids.md) §1; [0082](../decisions/0082-source-form-and-the-shared-view.md) §2.*

Every entity ID has one of these forms, told apart by its first character, by the presence of a colon and, for a colon form, by the kind of its prefix in the tenant's table of names ([0078](../decisions/0078-entity-sources.md) §3, in [05](05-providers-and-ingest.md)):

| Form | Shape | Examples | Minted by |
|---|---|---|---|
| **Local** | One uppercase letter and digits | `Q5`, `P12`, `L3` | This instance ([0000](../decisions/0000-init.md) §3) |
| **Local, lexeme part** | `L`, digits, then `-F` or `-S` and digits; on `L` only | `L3-F1`, `L3-S2` | This instance; the part belongs to the lexeme's record stream (§5.1). Foreign: `WDL3-F1`; input form `LLL3-F1` |
| **Foreign** | Two-letter provider code, one-letter type code, and the rest in the type's **ID grammar** (§2.2) | `WDQ42`, `OAW123`, `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d` | A provider ([0000](../decisions/0000-init.md) §3, [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §1) |
| **Keyed** | The keyed type's name, a colon, and the normalized key | `domain:en.wikipedia.org`, `keyword:p53` | The thing itself (§3) |
| **Source** | An entity source's name, a colon, and the source's own ID in the type's grammar | `mhc:Q1`, `mhc:P31` | An entity source the tenant declares ([0078](../decisions/0078-entity-sources.md) §1–2). A foreign ID in the colon shape, relative to the tenant |

No local or provider-form ID contains a colon, and every keyed and source ID does, so they never collide, whatever a key looks like; a colon form's prefix names one kind of thing on a tenant, so keyed and source IDs never collide either ([0078](../decisions/0078-entity-sources.md) §3). A registry provider's slug before a colon, `wikidata:Q42`, is an input form of the provider-form ID, `WDQ42` ([0078](../decisions/0078-entity-sources.md) §3). The colon form is the one Wikibase already uses for entities from another repository (`wikidata:Q42` in a federated install), so clients that parse federated IDs parse these.

The lexeme-part suffix is part of the ID wherever an ID is stored, compared or shown; `L1-F1` and `L1` are different IDs of one record stream. No other type takes a suffix; a provider type that minted one would need an ID grammar of its own ([0066](../decisions/0066-lexemes.md) §4).

**Statement IDs** keep Wikibase's form, `<EntityId>$<UUID>`: `domain:en.wikipedia.org$C7C2A847-…`, `L1-F1$<uuid>`. **Entity values** carry the same ID: `{"entity-type": "domain", "id": "domain:en.wikipedia.org"}`. How a statement ID reads when its entity belongs to an identity cluster is §4.9.

**Titles are unchanged.** In an entity namespace the title is the namespace and the bare key or ID, as [0008](../decisions/0008-namespaces-and-document-pages.md) §1 and [0009](../decisions/0009-keyed-entity-types-and-domain.md) §11 say (both in [10](10-pages-and-content-models.md)): `Domain:en.wikipedia.org`, `Keyword:p53`, `Item:MBAb10bbbfc-…`. The namespace supplies what the colon supplies in an ID. `Special:EntityPage/{id}` takes the ID form.

**Bare keys are not IDs.** `wbgetentities&ids=en.wikipedia.org` is an `invalid-entity-id`. The title resolver and `GET /resolve` ([0012](../decisions/0012-api-requirements.md) §5) still recognise a bare string that normalizes to a valid key of some keyed type and offer it as a suggestion, so the search box's "Go to" ([0010](../decisions/0010-site-ui.md) §3) keeps working for a pasted domain name.

**Derived IDs.** `M` and digits, a local-form ID, names the statements of the File page with that page ID (WikibaseMediaInfo's contract). It is derived, not minted: never in `view.entity`, never in a cluster, never the value of an entity data type (§5.2). A foreign page's page ID is derived in the same sense (§5.3).

**An input form.** A type letter written three times and the rest of a local ID, `QQQ5`, is an **input form** of the local ID on the tenant that reads it (§2.3). It is canonicalized to `Q5` wherever IDs are accepted and never stored or returned, except as Lua output under a remapped letter (§5.4).

**Any member of a cluster is accepted wherever an entity ID is accepted.** API parameters, change sets, titles, the search box, `wbgetentities`, `GET /resolve` and SPARQL constants all take `Q9`, `WDQ5` or any other member of one cluster (§4) for the same entity, and the entry point normalizes what it is given to the internal form before anything executes, as it canonicalizes the tenant-relative form and a provider slug. Where an input names a cluster, the lookup is expanded to the member set: a lookup by `Q9` over `entity_ref`, `identifier`, `sitelink` or `term` is a lookup over `{Q9, WDQ5, …}`, one indexed probe of `view.cluster_member` per constant with L0 in front of it, and a SPARQL constant `lb:Q9` becomes `VALUES ?v { lb:Q9 wd:Q5 … }` in the query service's rewriting layer ([02](02-graphs-rdf-and-query.md)). Property paths are not expanded: `wdt:P279*` follows stored triples, and a query that needs `owl:sameAs` closure writes it. Search results collapse by cluster: every search document carries `cluster_id`, maintained by the cluster projection, and `wbsearchentities` and the search page return one result per cluster, the member the consumer's preference selects (§4.12). The map is a table, `view.cluster_member`, never a structure assumed to fit in a process. What the consumer gets back, and in which form, is §4.12.

### 2.2 ID grammars for minted types

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §2; [0017](../decisions/0017-entity-id-grammar.md) §6; [0044](../decisions/0044-tenant-relative-ids.md) §2; [0066](../decisions/0066-lexemes.md) §4.*

Each foreign entity type declares an **ID grammar** in `providers.toml`. It says what may follow the three-letter prefix and what the canonical spelling is.

| Grammar | Rest of the ID | Canonical form | Used by |
|---|---|---|---|
| `digits` | One or more decimal digits | As given; leading zeros are significant where upstream has them (a CAMEO code is `0311`, [0037](../decisions/0037-gdelt-provider.md) §2). Providers that never mint them, such as Wikidata, never produce them | Wikidata, Librarybase, OpenAlex (the default) |
| `uuid` | An RFC 4122 UUID in its 36-character hyphenated text form | Lowercase | MusicBrainz |
| `gdelt-record` | 14 digits, `-`, an optional uppercase `T`, digits | As given | GDELT documents ([0037](../decisions/0037-gdelt-provider.md) §2) |
| `token` | Uppercase letters, digits and underscores, starting with a letter, at most 128 characters | As given | GDELT themes ([0037](../decisions/0037-gdelt-provider.md) §2) |

Input is case-insensitive in every position. **Canonicalizing an ID uppercases its prefix and applies the grammar's canonical form to the rest**, so `mbab10BBBFC-…` becomes `MBAb10bbbfc-…`. The `entity-id` normalizer of [0008](../decisions/0008-namespaces-and-document-pages.md) §3 therefore does not uppercase the whole ID, and the ID rewriting of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4 is: strip or add the prefix, and canonicalize the rest by the type's grammar. The rewriting of upstream IDs also honours `upstream_prefix`, so `keywords/…` and MusicBrainz's bare MBIDs both round-trip.

A grammar is declared per provider and may be set on a single type, overriding the provider's ([0037](../decisions/0037-gdelt-provider.md) §2). A new grammar is added by ADR, as a value the registry accepts. Nothing else in the system needs to know a grammar's rules: the ID parser in `scatter-wikibase-model` ([0005](../decisions/0005-crate-organization.md) §2) reads them from the registry passed in as data (0005 §3, rule 3).

**Lexeme parts.** The `-F` and `-S` suffix of §2.1 belongs to the lexeme entity type, not to a grammar ([0066](../decisions/0066-lexemes.md) §4). A foreign ID whose provider type has entity type `lexeme` may carry, after the rest in that type's grammar, `-F` or `-S` and digits, with the part letter canonical uppercase: `wdl1-f1` is `WDL1-F1`. The rewriting of upstream IDs keeps the suffix (`L252247-F2` ↔ `WDL252247-F2`, concept IRI `http://www.wikidata.org/entity/L252247-F2`), and a type of any other entity type rejects it. So Wikidata's `L` type keeps grammar `digits`, and a provider whose lexemes have another grammar gets the suffix without a grammar of its own.

**Reserved codes.** The twenty-six provider codes `AA` through `ZZ` are reserved in `providers.toml` and are never allocated, so that the tenant-relative input form (§2.3) is unambiguous: a provider with code `QQ` would make `QQQ5` mean two things. No allocated code is a doubled letter (`WD`, `LB`, `OA`, `MB`, `XD`, `OS`, `OW`, `GD`), so nothing moves. Reserving all twenty-six, not only the letters local types use today, means a type letter added later already has its tenant-relative form.

**MusicBrainz** is the provider whose grammar is `uuid`. It is registered in `providers.toml` with code `MB`, slug `musicbrainz` and provider number 4 ([0015](../decisions/0015-record-format-and-partition-registry.md) §2, §5). Every type has ID grammar `uuid`, so an ID is the prefix and the MBID verbatim: `MBAb10bbbfc-cf9e-42e0-be17-e2c3e1d2600d`. Keeping the MBID intact was preferred to a shorter encoding because the ID stays recognisable and pastes into musicbrainz.org unchanged.

| Code | Type | Code | Type |
|---|---|---|---|
| `A` | Artist | `P` | Place |
| `G` | Release group | `E` | Event |
| `R` | Release | `S` | Series |
| `C` | Recording | `I` | Instrument |
| `W` | Work | `N` | Genre |
| `L` | Label | `D` | Area |

MusicBrainz `url` entities are not mirrored as items; the adapter turns them into URL-valued statements on the entity that links them. Concept IRIs are `https://musicbrainz.org/{type}/{mbid}`. MusicBrainz has global edit numbers but no per-entity revision IDs, so `revision_ids` is false and a mirror record's revision ID uses its offset (0015 §2). Its issuer, `musicbrainz`, is provider-only ([0007](../decisions/0007-actor-identity.md) §6) until edit history is mirrored, since the data the adapter reads carries no editors. The adapter is the crate `scatter-adapter-musicbrainz` ([22](22-crates-and-stack.md)).

### 2.3 Tenant-relative IDs

*Sources: [0044](../decisions/0044-tenant-relative-ids.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §2; [0017](../decisions/0017-entity-id-grammar.md) §1; [0080](../decisions/0080-tenants-as-entity-sources.md) §7.*

**A type letter written three times, then the rest of a local ID, names that local entity of the tenant where it is read:**

| Written | Means, on the tenant reading it |
|---|---|
| `QQQ5` | `Q5` |
| `PPP31` | `P31` |
| `LLL3` | `L3` |
| `MMM1234` | `M1234`, the MediaInfo ID of File page 1234 ([0041](../decisions/0041-content-models.md) §7) |

It has the shape of a foreign ID whose provider code is the type letter doubled. **It is an input form of the local ID, not a fourth form:**

- **It is never stored.** It is canonicalized to the local form wherever an ID is accepted, before anything is validated, hashed, appended or cached. No record, `view` row, cache key, search document or dump contains it.
- **It is never the canonical output.** Responses use the local form, `Q5`. The one place the tenant-relative form is written out is Lua under a mapped letter (§5.4), where the bare form already means something else.
- **It is relative.** `QQQ5` means a different entity on each tenant, as `Q5` does. To name a tenant's entity from elsewhere, use its IRI; on a tenant that reads it, use that tenant's name for it, the registry code where there is one (`LBQ6`) and otherwise the tenant's source name (`lb:Q6`) ([0018](../decisions/0018-tenants.md) §5, in [08](08-tenants-and-instances.md); [0080](../decisions/0080-tenants-as-entity-sources.md) §7).

Input is case-insensitive, as every ID is (§2.2): `qqq5` is `Q5`. **The rest of the ID follows the local type's grammar**, which for every local type is `digits`. `QQQb10bbbfc-…` is not an ID.

## 3. Keyed entity types

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1, §2, §3, §4, §5, §6, §7, §8, §10; [0017](../decisions/0017-entity-id-grammar.md) §3, §5; [0026](../decisions/0026-sitelinks.md) §5; [0033](../decisions/0033-backend-stack.md) §9.3; [0036](../decisions/0036-openstreetmap-providers.md) §3, §6; [0048](../decisions/0048-notation.md) §1, §2, §3, §4, §6.*

### 3.1 What a keyed type is

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1; [0017](../decisions/0017-entity-id-grammar.md) §3; [0048](../decisions/0048-notation.md) §6.*

A **keyed entity type** is an entity type whose ID is a normalized natural key, not a minted identifier. Each keyed type is a registry entry, recorded in the log as configuration (§4.10). The entry records:

- the **type name**, used as the `type` in Wikibase JSON;
- the **namespace** its pages live in ([0008](../decisions/0008-namespaces-and-document-pages.md) §1);
- the **key grammar**: which strings are valid keys;
- the **normalizer**: how input is turned into the one canonical key;
- the **IRI template** (§3.5);
- the **display rule**: how the key is shown as a label, **derived** or **editable** (§3.6, §3.7);
- the **data type** used to refer to entities of this type (§3.2);
- the **ID prefix**: the type's name and a colon (§3.2);
- whether the type's entities **join identity clusters**, `clusters` (default true; false for keyword and notation, §4.1);
- a **label** for the type.

**Domain is the first keyed type registered.** The mechanism is general so that others can be added as registry entries: Keyword (§3.7) and Notation (§3.8) have been. `docs/registry/keyed-types.toml` is authoritative. Normalizers come from `scatter-normalize` (§3.10).

### 3.2 Keyed IDs, JSON and values

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §3; [0017](../decisions/0017-entity-id-grammar.md) §3.*

**The entity ID is the type's prefix and the key**, `domain:en.wikipedia.org`. The canonical ID is the prefix, in lowercase, and the key in the form the type's normalizer produces. A key may itself carry a scheme: the `notation` type's key is a scheme name, a colon and the string in the scheme's canonical form, so its ID is `notation:osm:amenity=cafe` (§3.8). In Wikibase JSON a Domain is:

```json
{"type": "domain", "id": "domain:en.wikipedia.org", "labels": {…}, "descriptions": {…}, "claims": {…}, "lastrevid": …}
```

**Referring to a keyed entity uses the type's data type,** `wikibase-domain` for a Domain. Its value type is `wikibase-entityid`, with an `id` and no `numeric-id`:

```json
{"value": {"entity-type": "domain", "id": "domain:en.wikipedia.org"}, "type": "wikibase-entityid"}
```

Wikibase already has entity values without a `numeric-id`: the Lexeme extension's forms and senses (`L1-F1`, `L1-S1`) are written this way.

**Statement IDs follow the Wikibase form** `<EntityId>$<UUID>`, so a local statement on a Domain has an ID like `domain:en.wikipedia.org$C7C2A847-…`. `$` cannot occur in a key.

The concept IRI is `https://scatter.red/domain/{key}` (§3.5): the prefix is part of the ID, not of the key. In a change set the prefixed form is the subject (§3.3), and a key-mapped upstream item is written under `domain:{key}` ([05](05-providers-and-ingest.md)).

### 3.3 A keyed entity exists because its key is valid

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §10; [0048](../decisions/0048-notation.md) §1.*

**Every valid key denotes a Domain, whether or not anything has been said about it.** There is no operation that creates one. The same holds for every keyed type: a notation exists because its key is valid, with no create, redirect or delete ([0048](../decisions/0048-notation.md) §1).

- **The first `add` is enough.** Change sets refer to a Domain by its key. They never allocate an ID, need a temporary ref, or need a match key ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5). Re-running an ingest is idempotent by construction.
- **Valid and present are different.** A Domain is **present** when some graph asserts something about it. `wbgetentities` returns an empty entity (its derived label, §3.6, and no statements) for a valid ID that is not present, such as `domain:example.org`, and an `invalid-entity-id` error for an invalid one or a bare key. It never returns `missing` for a valid ID. Page listings such as `list=allpages` show only present Domains.
- **There is no delete.** Retracting every local assertion leaves the Domain valid and, if no mirror says anything, not present. Erasure by key works as for any other entity ([0006](../decisions/0006-log-integrity-and-erasure.md) §7, in [01](01-log-and-records.md)).
- **There are no merges or redirects between Domains,** and no `same-as` between two of them. Two different keys are two different names. That `wikipedia.com` forwards to `wikipedia.org` is a fact about HTTP, recorded as a statement, not an identity.
- **`convert` does not apply.** A Domain is already canonical (§4.4).

The operations of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2 and §4.10 apply to Domains as follows:

| Operation | Applies to a Domain |
|---|---|
| `put` | Yes, from a key-mapped adapter ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)) |
| `tombstone` | No. Upstream deletion is an empty `put` (0009 §9) |
| `redirect` | No |
| `create`, `convert` | No |
| `add`, `remove`, `override` | Yes |
| `retain` | Yes (0009 §9) |
| `same-as`, `different-from` | Yes, with an entity of another namespace (§4.1). Never between two Domains |

A local bulk job:

```
{"job":{"source":"citation-domains","graph":"local"}}
{"op":"add","id":"domain:en.wikipedia.org","claims":{"P12":[…]}}
{"op":"add","id":"Q5","claims":{"P40":[{"mainsnak":{"datavalue":{"value":{"entity-type":"domain","id":"domain:doi.org"},"type":"wikibase-entityid"},…}}]}}
{"op":"same-as","ids":["Q77","domain:en.wikipedia.org"]}
```

Upstream items can be key-mapped onto any keyed type; the mapping is in [05](05-providers-and-ingest.md).

### 3.4 Log keys are surrogates

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §7; [0017](../decisions/0017-entity-id-grammar.md) §3; [0048](../decisions/0048-notation.md) §1.*

A domain name can be personal data: a person's own name registered as a domain, for example. A log header key survives erasure ([0006](../decisions/0006-log-integrity-and-erasure.md) §3, §7), so a key must not be the domain name itself. A hash of the name is no better, because domain names can be enumerated from zone files and any hash can be checked against them.

**Each present keyed entity gets a surrogate for log headers.** The rules are:

- The instance mints the surrogates in sequence, one per keyed type, the first time a key is written to. The allocation happens in the appending transaction, under an advisory lock on `(keyed_type, key)`, so two writers meeting a new key cannot mint two surrogates for it; a bulk job reserves a block of surrogates up front, as it reserves ID blocks ([05](05-providers-and-ingest.md) §3.5), so a key-mapped bootstrap of millions of keys is not millions of serialized appends.
- The mapping from surrogate to key is itself a log record, whose body can be erased. It is appended in the same transaction as the first record that uses the surrogate.
- Change-set payloads refer to the entity by its key. They are in the body, which can be erased.
- An index projected from the mapping records resolves keys to surrogates when a change set is appended.

This is the same pattern as the actor surrogates in [0007](../decisions/0007-actor-identity.md) §5 ([07](07-actors-and-accounts.md)). Erasing a Domain by key erases the mapping record along with everything else keyed to the surrogate, and only an opaque number remains; the mapping is erased only when no live record in any partition of the instance is keyed to the surrogate, so one tenant's erasure never strips the key from another tenant's assertions. The log never sees the key, only the surrogate; this holds for every keyed type, with one sequence per type.

**The forms.** The header key of a keyed entity's records is `{type}#{n}`, `domain#17`: the `#` keeps it from being any entity ID, since no ID form contains one. The mapping record is a `scatter:v0/keyed-surrogate` record in the **instance `log`**, keyed by the same surrogate, with content `{keyed_type, surrogate, key}` ([payloads.md §3.4](../api/payloads.md)); surrogates are instance-wide, so the mapping does not belong to any tenant, and the record is committed with the first record under the surrogate. `view.keyed_surrogate` is projected from it at the instance tenant (`''`); a projection that folds a keyed entity's records looks the surrogate up there, and fails rather than guess when it is missing, which makes a rebuild that orders the instance `log` after a tenant partition fail loudly instead of silently dropping the tenant's assertions. The table is in [03](03-storage-caches-and-search.md).

A source entity (§2.1) needs no surrogate: its log header key is its ID, as `WDQ42`'s is ([0078](../decisions/0078-entity-sources.md) §2).

### 3.5 IRIs under `scatter.red`

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §6; [0017](../decisions/0017-entity-id-grammar.md) §3.*

**A Domain's concept IRI is `https://scatter.red/domain/{key}`,** for example `https://scatter.red/domain/en.wikipedia.org`. The key is used in A-label form, so the IRI is plain ASCII.

The IRI depends only on the key, so every instance mints the same one. Under [0005](../decisions/0005-crate-organization.md) §5, identifiers derived only from content are not instance data and may live under `scatter.red`, as Scatterbase's claim and triple IRIs do. Two Triplespace instances that describe the same domain therefore describe the same RDF subject, and their data can be merged without an alignment step.

**The document node is still instance data.** Revision metadata attaches to an entity's document node, not to the concept ([0001](../decisions/0001-revision-metadata-rdf.md) §1). A Domain's document node is `{base}/wiki/Special:EntityData/{id}` with the prefixed ID, `…/Special:EntityData/domain:en.wikipedia.org`, on each instance, as for every entity ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4, [0017](../decisions/0017-entity-id-grammar.md) §1), so each instance's history stays its own.

**The keyed type's IRI template is registry data.** The template for Domain is `https://scatter.red/domain/{key}`. A later keyed type may use an external IRI instead, such as `https://doi.org/{key}` for DOIs. The templates in `keyed-types.toml` are `https://scatter.red/keyword/{key}` and `https://scatter.red/notation/{key}` for the other two types (§3.7, §3.8).

### 3.6 Domain

*Sources: [0009](../decisions/0009-keyed-entity-types-and-domain.md) §2; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §5; [0026](../decisions/0026-sitelinks.md) §5; [0048](../decisions/0048-notation.md) §3.*

**The key is the domain name in lowercase A-label form** (the ASCII form, with internationalized labels in Punycode), with no trailing dot.

The normalizer:

1. strips one trailing dot;
2. applies [UTS #46](https://www.unicode.org/reports/tr46/) processing, non-transitional, with `CheckHyphens`, `CheckBidi`, `CheckJoiners` and `UseSTD3ASCIIRules` on;
3. converts every label to its A-label;
4. lowercases the result.

A key is valid when:

- the whole name is at most 253 octets and each label at most 63;
- it has at least two labels, **or** it is a single label that is alphabetic ASCII or begins with `xn--`. A single label is therefore always shaped like a top-level domain.

**`www.` is not stripped.** `www.example.org` and `example.org` are different names.

**Names with underscores are not keys.** `UseSTD3ASCIIRules` rejects them, so service labels such as `_dmarc.example.org` are not Domains. Domain means a host or zone name.

**Either form is accepted as input.** `Domain:bücher.example`, `Domain:XN--BCHER-KVA.example` and `Domain:xn--bcher-kva.example.` all normalize to `xn--bcher-kva.example`. Page titles redirect to the normalized form ([0008](../decisions/0008-namespaces-and-document-pages.md) §3). The U-label form is used for display.

**Terms.**

- **The label is derived from the key and cannot be edited.** This is the Domain's **derived** display rule; a keyword's labels follow the **editable** rule instead (§3.7). The resolved view emits one label in the language `mul`, whose value is the U-label form of the key. Wikidata uses `mul` for labels that hold in every language. Labels in source graphs are kept there, and the per-graph views ([0003](../decisions/0003-statement-ui.md) §6) still show them, but the resolved view ignores them. `wbsetlabel` on a Domain fails with a Triplespace error code, `derived-label`.
- **The A-label is a derived alias** when it differs from the U-label.
- **Descriptions and other aliases** are stored and reconciled like an item's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3).
- **Domains have no sitelinks.** Every sitelink's host is a valid Domain key by construction, though ([0026](../decisions/0026-sitelinks.md) §1, in [06](06-statements-and-properties.md)). A sitelink does **not** make its host's Domain present: linking a page asserts nothing about its host. But the host index of 0026 §6 answers "which entities link to this site", and the Domain page's header shows the count with a link to the list, beneath the DNS hierarchy it already shows. A `key_parents` query gives the count for a zone.

**The page shows the DNS hierarchy.** The header of `Domain:en.wikipedia.org` links its parents, `org` and `wikipedia.org`, by removing labels from the left. This is navigation, not data: no statements are derived and nothing is emitted in RDF. The Public Suffix List is not used, because the core should not depend on an external list that changes.

**Domain stays its own type** rather than a notation scheme (§3.8), for three reasons: Domains join identity clusters, and notations never do; its derived label is a conversion (the U-label), not the string itself; and sitelink hosts and source domains ([0037](../decisions/0037-gdelt-provider.md) §3) depend on it.

The Domain namespace (210, `Domain talk` 211) and its content model `triplespace-domain` are in the namespace table of [10](10-pages-and-content-models.md).

### 3.7 Keyword

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §5; [0048](../decisions/0048-notation.md) §3.*

A **keyword** is a short phrase describing what a work is about. It is the phrase, not the thing the phrase denotes.

| | |
|---|---|
| Type name, ID prefix | `keyword`, `keyword:` |
| Namespace | `Keyword`, 212, with `Keyword talk` at 213 ([0008](../decisions/0008-namespaces-and-document-pages.md) §2) |
| Data type | `wikibase-keyword`, with value type `wikibase-entityid` as `wikibase-domain` has |
| IRI | `https://scatter.red/keyword/{key}`, content-derived and so shared by every instance ([0005](../decisions/0005-crate-organization.md) §5, §3.5) |
| Grammar | 1 to 200 characters of letters, digits and single hyphens; at least one letter; no leading or trailing hyphen |
| Normalizer | Unicode NFKC; casefold; every run of whitespace or punctuation becomes one hyphen; leading and trailing hyphens are removed |

**Labels are editable, not derived.** A Domain's label is derived from its key because the key is the name. A keyword's key loses case and punctuation (`dna`, `type-1-diabetes`), so its labels and descriptions are stored and reconciled like an item's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3). When no label exists in the requested language, the UI shows the key with hyphens as spaces. The keyed-type registry therefore distinguishes a **derived** display rule from an **editable** one.

**Keywords never join identity clusters** (§4.1): a Wikidata item about machine learning is the concept, and `keyword:machine-learning` is a phrase. A keyword may carry an ordinary statement pointing at the concept it usually denotes, through a role in the role map ([0003](../decisions/0003-statement-ui.md) §7); that is data, not identity. **Keyword stays its own type** beside notation: convergence is the point of a keyword, and a notation preserves the distinctions a keyword erases ([0048](../decisions/0048-notation.md) §3).

**OpenAlex keywords are mapped by key** ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)), as internetdomains.wiki's items are mapped to Domains. The adapter turns `https://openalex.org/keywords/{slug}` into the key by running the slug through the normalizer, writes the entity's `display_name` as the mirror graph's English label, emits `<https://openalex.org/keywords/{slug}> owl:sameAs <https://scatter.red/keyword/{key}>`, and turns a work's `keywords` list into `wikibase-keyword` values with the score as a qualifier by role. The `OAK` type code stays reserved in the registry and no `OAK` entity is ever minted.

DOI is not a keyed type but a resolver ([0029](../decisions/0029-resolver-namespaces.md) §1, in [06](06-statements-and-properties.md)), and resolver names and keyed-type names share one namespace, so `doi:10.1000/xyz` can mean only one thing ([0029](../decisions/0029-resolver-namespaces.md) §2).

### 3.8 Notation and its schemes

*Sources: [0048](../decisions/0048-notation.md) §1; [0048](../decisions/0048-notation.md) §2; [0048](../decisions/0048-notation.md) §6; [0036](../decisions/0036-openstreetmap-providers.md) §3.*

A **notation** is a string that names something in an outside vocabulary, taken exactly as that vocabulary's own rule says. Its **scheme** says which vocabulary. The name follows SKOS, where `skos:notation` is "a string of characters … used to uniquely identify a concept within the scope of a given concept scheme".

| | |
|---|---|
| Type name, ID prefix | `notation`, `notation:` |
| Key | The scheme name, a colon, and the string in the scheme's canonical form: `osm:amenity=cafe` |
| ID | `notation:osm:amenity=cafe` (§3.2: the prefix and the key) |
| Namespace | `Notation`, 216, with `Notation talk` at 217 ([0008](../decisions/0008-namespaces-and-document-pages.md) §2). Titles read `Notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, value type `wikibase-entityid` |
| Content model | `triplespace-notation` ([10](10-pages-and-content-models.md)) |
| IRI | `https://scatter.red/notation/{key}`, content-derived and shared by every instance (§3.5): `https://scatter.red/notation/osm:amenity=cafe` |
| Grammar and normalizer | The scheme's |
| Display | Derived: the string, without the scheme, as the `mul` label; `wbsetlabel` fails with `derived-label` as on a Domain (§3.6). Descriptions and aliases are stored and reconciled like an item's |
| Clusters | Never (§3.9) |

**Parsing a key.** A scheme name never contains a colon, so the first colon of the key ends the scheme name, and everything after it is the string. A string may contain colons of its own, as `osm:addr:street=Main Street` does. The scheme name is matched case-insensitively and written in lowercase. The string is then normalized by the scheme's normalizer and checked against the scheme's grammar. A key whose scheme is not registered is not a valid key.

The rest of what §3 says of a Domain applies unchanged: a notation exists because its key is valid, with no create, redirect or delete (§3.3); log headers carry a surrogate, one sequence for the type, not the string (§3.4); upstream items can be key-mapped onto notations ([05](05-providers-and-ingest.md)).

**Schemes** are registry data, in `docs/registry/notation-schemes.toml`, embedded by `scatter-normalize`. Each entry has:

| Field | Meaning |
|---|---|
| `name` | Lowercase ASCII letters, digits and single hyphens, starting with a letter, at most 32 characters. No colon |
| `label` | The vocabulary's name, shown in the UI's identity line ("OSM tag or key") |
| `normalizer` | One of a fixed set: `exact` (the input must already be NFC and is not changed), `nfc`, `casefold` (NFC, then full Unicode case folding, as §3.7 and [0029](../decisions/0029-resolver-namespaces.md) §1 use it) and `ascii-lowercase` (NFC, then ASCII letters lowercased) |
| `grammar` | The name of a grammar implemented in `scatter-normalize`, checked after normalizing: `osm-tag` (below), or `text`, which is 1 to 255 characters with no control characters and no leading or trailing whitespace |
| `documentation` | Optional. A URL template for the vocabulary's own page about a string, `{string}` being the string |
| `defined_in` | The ADR that registered the scheme |
| `retired` | Optional. A retired scheme stays in the file so that its name is never given to another vocabulary |

**A new scheme is a registry change, and a new grammar is code.** A scheme that names a grammar `scatter-normalize` does not implement is rejected when the file is parsed, as an unimplemented keyed type is. The normalizers are a closed set so that the rule that decides identity is never free text.

**Schemes are allocated in the registry only, never by an instance or a tenant.** A notation's ID and IRI are shared by every instance, so two instances that gave the name `foo` to different vocabularies would describe different things with one IRI. This differs from resolvers, which a tenant may add ([0029](../decisions/0029-resolver-namespaces.md) §8), because a resolver mints no shared identifier. A vocabulary a tenant wants for itself is modelled with ordinary items.

**Scheme names join the one namespace of keyed-type and resolver names** ([0029](../decisions/0029-resolver-namespaces.md) §2). `osm:amenity=cafe` typed into `/resolve` or the search box means the notation, as `doi:10.1000/x` means the DOI resolver. `/resolve` tries such input as a notation key after keyed IDs and before resolver strings. No scheme may share a name with a keyed type or a resolver, and the registry tests check it. Entity sources a tenant declares join the same table of names on that tenant ([0078](../decisions/0078-entity-sources.md) §3, in [05](05-providers-and-ingest.md)).

**The `osm` scheme.** OSM keys (`amenity`) and tags (`amenity=cafe`) are notations in the `osm` scheme: one scheme covers both, as Wikidata's P1282 is "OpenStreetMap tag or key". Keys never contain `=`, so the two forms cannot collide, and the wiki's `Key:` and `Tag:` page titles are the same strings.

```toml
[[scheme]]
name = "osm"
label = "OSM tag or key"
normalizer = "exact"
grammar = "osm-tag"
defined_in = "0036 §3, 0048 §6"
```

| | |
|---|---|
| Type, scheme, ID | `notation`, scheme `osm`: `notation:osm:amenity`, `notation:osm:amenity=cafe` |
| Data type | `wikibase-notation`, on properties whose `notation-scheme` is `osm` (§3.9) |
| IRI | `https://scatter.red/notation/osm:{key}` |
| Grammar | The scheme's `osm-tag` grammar: a key of 1 to 255 characters, or a key, `=` and a value of 1 to 255 characters. No control characters, no `=` in the key, no leading or trailing whitespace, Unicode NFC |
| Normalizer | The scheme's `exact` normalizer: the exact string, case-sensitive. Input that is not NFC is not a valid key |

**The keyword normalizer is not used.** Keyword (§3.7) casefolds and turns punctuation into hyphens, so `fast_food`, `fast-food` and `fast:food` would become one entity, and `Name` would merge with `name`. OSM tags are case-sensitive exact strings and those forms are different tags.

**Grammar over policy.** The grammar admits any valid OSM string. Which strings become entities is decided by the adapters ([0036](../decisions/0036-openstreetmap-providers.md) §4, §5, in [05](05-providers-and-ingest.md)), not the grammar.

The `osm` scheme has no `documentation` template. The wiki's pages are `Key:amenity` and `Tag:amenity=cafe`, two forms one template cannot produce, and a mapped notation already carries the wiki's documentation-page statements (0036 §5). The roles `osm-tag`, `osm-tag-text` and `osm-key` of 0036 §4 take values `notation:osm:k=v` and `notation:osm:k`, and the properties bound to `osm-tag` and `osm-key` carry `notation-scheme` `osm` (§3.9); the `OW` items of `Q7` and `Q2` are key-mapped to `notation`, with the key prefixed `osm:`, and the key map gains `key_scheme = "osm"`.

One scheme shares one namespace with every other scheme, which leaves 218 to 219 free. Splitting keys from tags into two schemes is a registry change, not a design change, if a use for the difference appears.

**Not yet.** One scheme ships, `osm`. The file drafts two candidates, commented out as `resolvers.toml` drafts its candidates ([0029](../decisions/0029-resolver-namespaces.md) §8): `hashtag`, `casefold`, for ActivityPub hashtags on federated posts ([0022](../decisions/0022-federation.md) §8), which needs a `hashtag` grammar; and `media-type`, `ascii-lowercase`, for IANA media types such as `text/html`, the `mime` field of an upload ([0039](../decisions/0039-files-and-media.md) §2), which needs a `media-type` grammar.

### 3.9 What a notation is, and what it is not

*Sources: [0048](../decisions/0048-notation.md) §3; [0048](../decisions/0048-notation.md) §4; [0036](../decisions/0036-openstreetmap-providers.md) §6.*

Four mechanisms give strings an identity. The question that tells them apart is who decides that two strings are the same thing.

| Mechanism | Use it when | Example |
|---|---|---|
| **Notation** | An outside system defines the string, and its own rule decides sameness. The set is open: anyone may coin a new string. No authority mints an ID for it. | `notation:osm:amenity=cafe` |
| **Keyword** (§3.7) | The string is a phrase about a work, and variants should converge whoever writes them. Labels are editable because folding loses case | `keyword:machine-learning` |
| **Provider item** ([0037](../decisions/0037-gdelt-provider.md) §2) | One provider publishes and identifies a finite list | `GDTTAX_FNCACT_MAYOR`, a GDELT theme |
| **Resolver** ([0029](../decisions/0029-resolver-namespaces.md)) | The string identifies a thing that already has an item | A DOI |

**Notations never join identity clusters** (§4.1), as keywords do not, for the same reason: `notation:osm:amenity=cafe` is a tag, and a Wikidata item about cafés is the concept. A notation may carry an ordinary statement pointing at the concept it usually denotes; `P7` and `P12` stay statements, so a user can follow a tag to its concept without the phrase becoming canonical over the concept.

**A property's scheme.** With one data type for every scheme, the data type no longer limits which strings a property takes. A property whose values should be OSM tags must not accept hashtags. A role, `notation-scheme`, names the property that restricts the scheme ([0003](../decisions/0003-statement-ui.md) §7, in [06](06-statements-and-properties.md)). It is a `string` property, and a `wikibase-notation` property may carry one or more statements with it whose values are scheme names. A value whose scheme is not among them is refused on the write path with `ts-notation-scheme`, in the same check that refuses a value of the wrong data type. A `wikibase-notation` property with no such statement takes any registered scheme, which is what a general "has tag" property needs.

**This is a type check, not a constraint** ([0031](../decisions/0031-property-constraints.md)). Constraints report and never refuse (0031 §2), and they read the graph after the fact. The scheme belongs with the data type: it is part of what kind of value the property takes, read from the property itself when its data type is read. A change to the scheme statement applies to later writes, and values already stored are left as they are.

The UI's value editor ([0003](../decisions/0003-statement-ui.md) §8) offers the property's schemes. When it has exactly one, the editor hides the scheme, so an editor types `amenity=cafe` and the editor stores `notation:osm:amenity=cafe`.

### 3.10 Normalizers and the libraries behind them

*Sources: [0033](../decisions/0033-backend-stack.md) §9.3; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1.*

Normalizers come from `scatter-normalize` ([22](22-crates-and-stack.md)), which also embeds `notation-schemes.toml`. The backend stack fixes the libraries that identifiers, URLs and case depend on:

| Concern | Choice |
|---|---|
| URLs (0026 §1) | `url` (WHATWG parsing) |
| IDNA / UTS 46 (0009) | `idna` |
| Case folding (DOI resolver, 0029) and plural rules | `icu_casemap`, `icu_plurals` (ICU4X; wasm-friendly) |
| Unicode normalization | `unicode-normalization` |

## 4. Identity clusters

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §1, §2, §3, §4, §5, §6, §7, §8, §9, §10; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8, §10; [0018](../decisions/0018-tenants.md) §7; [0066](../decisions/0066-lexemes.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §3, §4, §5, §5.1.*

### 4.1 Clusters and namespaces

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §1; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8; [0017](../decisions/0017-entity-id-grammar.md) §5; [0036](../decisions/0036-openstreetmap-providers.md) §6; [0048](../decisions/0048-notation.md) §3; [0066](../decisions/0066-lexemes.md) §4.*

IDs that denote the same thing form an **identity cluster**. Each member of a cluster belongs to a **namespace**, meaning whoever minted the ID: the local instance, Wikidata or OpenAlex. The namespace is the provider, not the type code. A Wikidata item can share a cluster with an OpenAlex work (`OAW`) or an OpenAlex author (`OAA`). An entity source a tenant declares is a namespace in this sense too ([0078](../decisions/0078-entity-sources.md) §2).

**Aliases are not members.** An ID retired by a redirect is an alias of its target. This covers an upstream merge recorded in a mirror graph and a local merge. Aliases are resolved to their targets before clusters are built.

**A keyed type is a namespace of its own.** Its keys are minted by the thing itself, not by a provider, and a cluster holds at most one member from it, as from any other namespace. The cluster rules do not apply to the provider mapping of key-mapped providers ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §9, in [05](05-providers-and-ingest.md)): that mapping defines which Domain an upstream item is, so it is resolved by the adapter before any cluster is built.

**Keyed entities that are names never join a cluster.** Keywords (§3.7) and notations (§3.9) are phrases, not concepts. Treating a phrase and the concept it denotes as one thing would make the phrase canonical over the concept, since keyed members rank first (§4.4). So `same-as` with a keyword or notation on either side is rejected at write time, as a data-type mismatch between properties is (§4.6), and no tier-2 or tier-3 link may produce one. Ranking first is for keyed types that name a thing, as a Domain names a host.

**Clusters are of lexemes, never of their parts.** `same-as` between lexemes forms an identity cluster as between items; `same-as` between forms or senses, or between a part and a lexeme, is refused with `ts-cluster-part`. A non-canonical lexeme's forms and senses keep their IDs under the canonical lexeme's page and are not merged into its own (§5.1).

### 4.2 Clusters are strictly one-to-one

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §2.*

**A cluster holds at most one member from each namespace.** A set of links that would put two members from one namespace into a cluster is a conflict (§4.11). The links involved are held, not applied. For example:

- `WDQ1 = OAW1` and `OAW1 = WDQ2` would join two Wikidata items.
- Both links are held, so `WDQ1`, `OAW1` and `WDQ2` stay out of each other's clusters until the conflict is resolved.

Holding every conflicting link, not only the later one, makes the result independent of the order in which links arrive.

**Duplicates within one namespace are merges.** Two IDs from the same provider that denote one thing are a duplicate within that provider. That is expressed by a `redirect`, an explicit merge ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7). It is never inferred from links.

**Clusters stay small.** A cluster can hold no more members than there are namespaces the tenant reads: the local instance, each registry provider, each keyed type (§4.1) and each entity source (§2.1). Chains of links cannot grow, and a cluster can be recomputed from its links whenever one of them changes.

### 4.3 Where links come from

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.1.*

Links come from three tiers of source. A higher tier beats a lower one when they conflict. Conflicting links within one tier are all held (§4.2).

| Tier | Source | Example | Default |
|---|---|---|---|
| 1. Asserted | A local-graph `same-as` (§4.10) | An editor links `Q456` and `OAW123` | Always on |
| 2. Mirror-derived | A mirrored statement of a configured link property | Wikidata's P10283 (OpenAlex ID) on a Wikidata item; OpenAlex's `ids.wikidata`, on the entity types that carry it | On for configured properties |
| 3. Inferred | Two entities from different namespaces share a value of a configured identifier property | Both carry the same DOI | Off |

**A mirror-derived link lasts only as long as its statement.** It disappears when:

- the mirror drops the statement;
- the entity is tombstoned;
- the local graph deprecates or suppresses the statement.

A link value that points at an alias follows the redirect. If Wikidata says an item's OpenAlex ID is `W1` and OpenAlex has merged `W1` into `W2`, the link goes to `OAW2`. Mirror-derived links can join a Domain to a mirrored entity through a configured link property, as for any other cluster ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §8).

**Inferred links need a unique value.** A value that appears on more than one entity in a namespace produces no link. Values are compared by their normalized keys (§4.7). An inferred link is applied at once, for the identifier properties the **instance's** `reconcile` record names (§4.10); nothing is queued for review, and the registry ships none enabled. A tenant's `reconcile` record may decline inferred links in its own view, and may add link properties for its own entity sources, but cannot add inference properties or change the link properties of a shared provider, so every tenant of a farm infers the same links from a shared graph.

**On a farm,** tier-2 and tier-3 links come from shared graphs and are instance-wide: they are shared rows (`tenant = ''`) of `view.link`, `view.cluster` and `view.cluster_member`, computed under the instance policy (§4.10). Tier-1 links and `different-from` blocks are each tenant's own: `view.cluster`, `view.cluster_member`, `view.link`, `view.different_from` and `view.entity_source` carry the `tenant` column with the `''`-then-tenant lookup of `view.entity` ([03](03-storage-caches-and-search.md)), and `cluster_member`'s unique key is `(tenant, entity_id)`, so a tenant's tier-1 link on `WDQ42` sits as a tenant row beside the shared cluster without touching it, and two tenants' local assertions on one foreign entity no longer collide in `entity_source`. A provider tenant's `same-as` and `convert` records are tier-2 links for every other tenant ([0018](../decisions/0018-tenants.md) §5–6, in [08](08-tenants-and-instances.md)).

**A `different-from` block beats links from every tier.** Two IDs joined by a local `different-from` never share a cluster. Wikidata's P1889 ("different from") needs no special handling. It relates two Wikidata items, which can never share a cluster anyway.

### 4.4 The canonical ID

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §4; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §8; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §4, §5.*

**The canonical ID of a cluster is its highest-ranked member.**

- A keyed member ranks first, and a local member next. The canonical order is: keyed type, then local, then the providers in the instance's order.
- The instance's `reconcile` record, the policy record under which every shared row is computed (§4.10), sets the order of the registry providers after them; the default is provider-number order in `providers.toml` (Wikidata first). The record ([0015](../decisions/0015-record-format-and-partition-registry.md) §3, in [23](23-configuration-and-registry.md)) may set a different order per entity type, `order_by_type`; a cluster's members share a type, so the order is always defined. A tenant's `reconcile` record orders only the tenant's own entity sources, after every registry provider ([05](05-providers-and-ingest.md) §6.6); it cannot reorder shared providers.

So if a local item `Q77` about en.wikipedia.org existed before Domains did, `same-as` between `Q77` and `domain:en.wikipedia.org` makes the Domain canonical: the Domain's IRI is the cluster's canonical concept IRI, and `Q77` gets the redirect-form `owl:sameAs` in RDF. A cluster whose links would join two Domains is a conflict, and the links are held (§4.11).

**The canonical ID is an attribute of the cluster, never applied to data.** `view.cluster` names it. The internal graph is in **source form**: every stored form of an entity keeps the IDs its source wrote. Statement subjects, entity values, qualifier and reference values, `view.entity_ref`, `view.identifier`, the search documents, the RDF dump and the update stream carry `WDQ5` where Wikidata wrote `Q5` and `Q9` where a local editor wrote `Q9`, and nothing rewrites an ID in storage because a cluster formed, changed or dissolved. The resolved view ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, in [05](05-providers-and-ingest.md) §1.4) is the cluster's **fused body**, composed once per cluster from the members' graph states ([03](03-storage-caches-and-search.md)); the per-graph views ([0003](../decisions/0003-statement-ui.md) §6) still show each member's own data. A `materialized` row of `view.entity` means two or more contributing graphs or local deltas, nothing else: a mirrored entity that merely references a clustered entity is `source`, served from its record, so what is materialized is bounded by what tenants have asserted about an entity, never by what references it. **A cluster change recomposes the cluster's members and nothing else**: there is no delta per referrer in the update stream, no re-resolution of referrers, and `entity_ref` is not needed for correctness. The form in which a consumer sees the IDs of a fused body is a property of the response, not of the store (§4.12).

**In RDF the fused body is emitted under every member.** The resolved graph carries the cluster's fused statements under each member's concept IRI, with `owl:sameAs` between the members and the redirect-form `<member IRI> owl:sameAs <canonical IRI>` from each non-canonical member to the canonical one. Duplication is bounded by cluster size, never by referrers, and a query that reaches `wd:Q5` through a value and asks for its label or its statements succeeds without rewriting, which is why §2.1 leaves property paths alone. The update stream and the dump carry the same ([02](02-graphs-rdf-and-query.md)).

**The canonical ID changes when membership changes.** Examples:

- A Wikidata item is created for an OpenAlex work, and the new item becomes canonical.
- A local member is minted (§4.5) and becomes canonical.
- The canonical member is tombstoned under `cascade` or `orphan`. It leaves the cluster, and the next member becomes canonical. A retained member stays in the cluster.
- A local member is deleted. It leaves the cluster as a member tombstoned under `orphan` does. `same-as` and `different-from` records naming it are kept but not applied, and apply again if it is undeleted ([0023](../decisions/0023-moderation.md) §4, in [09](09-security-and-moderation.md)).

Each of these recomposes the cluster's members; no referrer's row, triple or cache entry changes.

**The stable handle is the document node.** External consumers are told to cite the document node of any member, `{base}/wiki/Special:EntityData/{id}` ([0015](../decisions/0015-record-format-and-partition-registry.md) §6), which exists for every member and never moves. The resolved view carries `schema:about` from each document node to the current canonical concept IRI, and the update stream ([0032](../decisions/0032-sparql-update-stream.md), in [02](02-graphs-rdf-and-query.md)) keeps that triple current.

### 4.5 Conversion joins a cluster

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §5.*

`convert` mints a local entity and links it to the foreign entity with `same-as`. The local member is canonical, so the behavior of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6 (in [05](05-providers-and-ingest.md)) follows from the cluster rules:

- `WDQ123` and `Q456` are one entity: a request for either returns the fused body under the requested ID, with `Q456` as its `canonical` (§4.12);
- mirrored statements under `WDQ123` are part of the fused body, and stay stored under `WDQ123` (§4.4);
- the fused body is emitted under both IRIs, with the redirect-form `owl:sameAs` from `WDQ123` to `Q456`.

**A local alias that conflicts with an upstream redirect.** Suppose `WDQ123` has been converted to `Q456`, and Wikidata then merges `Q123` into `Q789`.

- `WDQ123` becomes an alias of `WDQ789`, so `WDQ789` takes its place as the cluster's Wikidata member.
- If `WDQ789` already belongs to another cluster, the merge would join the two clusters. One-to-one applies.
- For example, if `WDQ789` is already linked to `Q999`, the joined cluster would have two local members. The links are held as a conflict.
- The editor resolves it with a local `redirect` from `Q456` to `Q999`, or the reverse.

`convert` does not apply to a keyed entity, which is already canonical (§3.3).

### 4.6 Properties

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §6.*

**Wikidata properties are used as they are,** as mirrored `WDP` entities.

**Adapters never map onto a tenant's properties.** A mirror partition is an instance partition read by every tenant, a local `P12` means a different entity on each tenant, and an instance job cannot mint from a tenant's sequence; so no adapter writes a local property or item ID into a mirror graph, and none creates a local entity on import. **OpenAlex properties are mapped by the adapter.** OpenAlex has no property entities. Its fields have names, not identifiers, and the set of fields is small and changes little. The OpenAlex adapter therefore maps each field:

- onto a Wikidata property, where one fits;
- otherwise onto a property of OpenAlex's own **provider property type**.

A provider that needs vocabulary of its own has a property type in the registry: one `[[provider.type]]` row with `entity_type = "property"` under its code ([05](05-providers-and-ingest.md) §8.1), its properties minted by the adapter under deterministic IDs so that every instance mints the same ones. OpenAlex, OpenStreetMap and GDELT each have one.

- **Enumerated values** such as work type are mapped in the same way, onto Wikidata items where one fits; a provider that needs items of its own for them mints those under a provider type of its own, as GDELT's themes and CAMEO codes are ([05](05-providers-and-ingest.md) §8.10).
- **Roles are read at instance scope.** Where an adapter names a property by role, as the OpenStreetMap key map and the GDELT roles do ([05](05-providers-and-ingest.md) §8.8, §8.11), the binding it reads is the instance-scope `role` record ([06](06-statements-and-properties.md) §1), never a tenant's, and the instance binds those roles to registry-provider properties or to the provider's own property type.
- **The mapping is versioned with the adapter** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3), the minted property IDs with it.

**Any graph may use any property.** The OpenAlex mirror graph holds statements whose predicates are `WDP` or OpenAlex's own properties, and the local graph may use either. A graph records who asserts a triple, not whose vocabulary the triple uses. Mapped properties are affected by changes as follows:

- **A provider's own properties** change only with its adapter's mapping version.
- **Wikidata properties** can change upstream. If one that an adapter maps onto is deleted upstream, or its data type changes, that mapping stops and the change is reported. The mapping then has to be revised.

**Properties form clusters as items do.** A local property may duplicate a Wikidata property, for example because it was created before Wikidata had one. A local `equivalent-property` (§4.10) links the two. Property clusters follow the same rules as item clusters:

- They are one-to-one.
- The local property is canonical.
- `different-from` blocks a link.

Property links come only from local assertions. Wikidata's P1628 ("equivalent property") points at external vocabularies, not at entities this instance holds, so it is ordinary data.

- **Data types must match.** A link between properties with different data types is rejected when it is written.
- **The fused body treats the cluster as one property.** Statements on `P12` and on `WDP585` fuse as statements on one property (§4.8); stored predicates keep their IDs (§4.4), and a response shows the group under the property ID the consumer's preference selects (§4.12), `equivalent-property` being an exact match.
- **RDF output:** the fused statements are emitted under each property's predicates and property entity IRIs get the redirect-form `owl:sameAs`, as items do. OWL equivalence holds between predicates, not between entity IRIs. So `owl:equivalentProperty` is emitted for each pair of predicates in the same family: `wdt:`, `p:`, `ps:`, `psv:`, `pq:`, `pqv:`, `pr:`, `prv:`, and the normalized forms.

### 4.7 Values

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §7.*

The resolved view compares values by a canonical form:

- **Entity values** compare by their cluster: two values are one value when their IDs are members of one cluster. The stored IDs are not changed (§4.4).
- **Literal values** compare by a **normalized key**, derived per data type. A property can override its data type's normalizer.

**The `unit` of a quantity, the `globe` of a coordinate and the `calendarmodel` of a time are entity IDs in the stored form:** `"unit": "WDQ11573"`, `"unit": "Q7"`, `"unit": "1"` unchanged for a dimensionless quantity, never a concept IRI under some base. They are expanded through the current IRI template on output, as every ID is ([08](08-tenants-and-instances.md)); the adapter rewrites them on ingest as it rewrites every other ID ([05](05-providers-and-ingest.md) §3.4); the content hash is over the stored form; and they participate in `entity_ref` and in rewriting (§4.12) as entity values do. [wikibase-compat.md §4](../api/wikibase-compat.md) describes the serialization.

| Data type | Normalized key |
|---|---|
| `external-id` | Per property. For example, a DOI is lowercased, and an ORCID loses its URL prefix. The default is the exact value. |
| `string` | Unicode NFC |
| `monolingualtext` | Unicode NFC, together with the language code |
| `time` | The value, precision and calendar model. Values with different precisions are different values. |
| `quantity` | The amount, unit and bounds |
| `url` | The normalized URL that sitelinks also use ([0026](../decisions/0026-sitelinks.md) §1) |
| Everything else | The exact value |

Stored values are never changed. A fused statement (§4.8) shows the value asserted by the highest-ranked graph.

**A normalizer change is a rebuild, not a rewrite.** The normalized keys are stored beside the values (`view.value_key`, [03](03-storage-caches-and-search.md)), and each row carries the version of the normalizer that produced it. Changing a normalizer, whether a data type's, a property's override or a resolver's ([06](06-statements-and-properties.md) §3.1), enqueues a per-property rebuild job that recomputes the keys of every value of the affected properties, and bumps the rendering version of the update stream ([02](02-graphs-rdf-and-query.md)), since the normalized (`wdtn:`) triples change lexical form; until the job completes, rows of the old version are compared by the old key.

**A coarser time does not fuse with a finer one.** "2019" and "2019-03-04" stay two values. The statement UI marks the coarser one as consistent with the finer one in the same group, and the resolved view keeps both, so it never claims a precision no source asserted.

### 4.8 Fusing equivalent statements

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §8.*

This extends the statement hash in [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5. A statement's **key** is a hash of two things, both in canonical form (§4.4, §4.6, §4.7):

- the main snak;
- the qualifiers, in any order.

In the resolved view, statements with the same key fuse into one statement. This applies both across graphs and within a graph. Source graphs are not changed.

**References** are combined, with duplicates removed by the reference hash. Reference snaks are put into canonical form before hashing.

**Rank:**

- The local graph wins, as [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3 says.
- Otherwise the rank comes from the highest-ranked mirror graph, in the same provider order as §4.4.

**Statement ID:**

- The fused statement takes its ID and IRI from the highest-ranked graph that asserts it.
- The provenance response ([0003](../decisions/0003-statement-ui.md) §6) lists every member statement and its graph.

**Overrides:**

- A rank override on any member statement applies to the fused statement.
- A suppression removes only one member's contribution. To remove the fused statement, every member is suppressed, which is what the UI does.

**Non-equivalent values** are kept side by side, as 0002 §3 says.

### 4.9 Statement IDs follow the entity ID a consumer sees

*Sources: [0018](../decisions/0018-tenants.md) §7; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §3.*

**Statement IDs in the log never change,** and the stored form keeps the entity part its source wrote (§4.4): the mirror writes `WDQ42$abc` and that is what `view` holds. What clients see is derived: **the response rewrite of §4.12 rewrites the entity-ID part of a statement GUID to the form the consumer prefers and keeps the UUID.** The UUID is the identity.

- example.wiki converts `WDQ42` to `Q9` (§4.5). The mirror keeps writing `WDQ42$abc`; example.wiki's site UI, which prefers the local form, shows `Q9$abc`, and a Wikidata-keyed bot that asked for `WDQ42` sees `WDQ42$abc`.
- Librarybase does the same; example.wiki, reading Librarybase, shows `LBQ9$abc`.
- An unconverted cluster {`WDQ42`, `OAW123`} shows the OpenAlex statements as `WDQ42$oa-uuid` to a consumer preferring Wikidata form and as `OAW123$oa-uuid` to one preferring OpenAlex form.

A `wbgetentities` request for a non-canonical member returns the fused body under the requested ID with its GUIDs prefixed by that ID (§4.12). The UUID alone identifies a statement, so overrides ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7), the provenance response (§4.8, which lists member statements in their source form) and API writes all resolve through it, whatever prefix the client sends. Every GUID a client sees has the entity's ID, in the form the client sees it, as its prefix, which is Wikibase's invariant. Re-minting statements on conversion was rejected: it would copy mirrored statements into the local graph and recreate the stale-copy problem 0002 §5 rejected for rescue.

### 4.10 Operations and configuration

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §9; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §10; [0066](../decisions/0066-lexemes.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5.*

| Operation | Graph | Meaning |
|---|---|---|
| `same-as` | Local | Links two items from different namespaces (§4.3, tier 1). |
| `different-from` | Local | Blocks two IDs from ever sharing a cluster (§4.3). |
| `equivalent-property` | Local | Links two properties from different namespaces (§4.6). |

`remove` retracts any of these. `same-as` and `different-from` apply to a Domain with an entity of another namespace, never between two Domains (§3.3), and `same-as` applies between lexemes but not their parts (§5.1).

`redirect` in the local graph records a merge within one namespace. **For properties only, a local `redirect` may cross namespaces:** `{"op":"redirect","from":"P12","to":"WDP585"}` retires `P12` and makes `WDP585` the canonical predicate. `P12` becomes an alias of `WDP585` (§4.1), existing local statements keep `P12` in the log and are served under `WDP585` as any redirect's are. It needs `ts-linkproperty` and a matching data type. An item never redirects across namespaces, and a local item stays canonical.

```
{"op":"same-as","ids":["Q456","OAW123"]}
{"op":"different-from","ids":["WDQ1","OAW7"]}
{"op":"equivalent-property","ids":["P12","WDP585"]}
```

**Configuration is recorded in the log, and the shared view is computed under the instance's record.** The instance's `reconcile` record, `reconcile:default` in the instance `config`, is the **policy record** under which every shared row (`tenant = ''`) of every `view` table is computed ([03](03-storage-caches-and-search.md)). It holds:

- the provider order among registry providers (§4.4);
- the link properties for each provider (§4.3, tier 2);
- the identifier properties used for inference (§4.3, tier 3);
- normalizer overrides (§4.7);
- the role bindings the constraint checker and the adapters read for shared rows ([06](06-statements-and-properties.md) §1, §2.4);
- the languages that populate shared `view.term` ([05](05-providers-and-ingest.md) §5.8).

**A tenant narrows; it does not reorder.** A tenant's `reconcile` record may hide a provider in its own view and API (a filter on read), restrict its own display languages, decline inferred links in its own view, and add link properties and an order for its own entity sources ([05](05-providers-and-ingest.md) §6), whose graphs are read per tenant anyway. It may not reorder shared providers, change a shared normalizer, add inference properties or rebind a role the instance record binds; a tenant-scope write that tries is refused with **`ts-instance-policy`**, and the tenancy presets ([08](08-tenants-and-instances.md)) say so.

**The shared row names the policy it was computed under.** `view.entity` and the shared rows of its dependents carry `policy`, the policy record's code. Only `default` exists; a shared view computed once per distinct policy record, with tenants bound to a profile, can be added later by adding records and rebuilding the projection, with no change to the log.

A change to any of these settings is appended to the log as a record, so the resolved view stays rebuildable from the log ([0000](../decisions/0000-init.md) §1). Keyed-type registry entries are recorded the same way (§3.1). The record kinds are in [23](23-configuration-and-registry.md).

### 4.11 Conflicts

*Sources: [0004](../decisions/0004-identity-clusters-and-equivalence.md) §10.*

A **conflict** is a set of links that is held instead of applied. Conflicts arise from:

- a one-to-one violation, whether from links or from an upstream redirect that joins two clusters;
- a link blocked by `different-from`.

Conflicts are listed for review, in the same way as the maintenance tooling for local corrections ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7). An editor resolves a conflict in one of these ways:

- with a local `same-as`, which outranks mirror-derived links;
- with `different-from`;
- with a `redirect` that merges a duplicate;
- by deprecating the mirrored statement a link came from.

**The forms.** The review list is `Special:IdentityConflicts`, with these resolutions as row actions. `Special:LinkEntities` is the form for `same-as`, `different-from` and `equivalent-property`; `Special:MergeItems` writes a merging `redirect` within one namespace and refuses a merge across namespaces ([0047](../decisions/0047-special-pages.md) §5–6, in [21](21-special-pages.md)).

### 4.12 Rewriting in responses

*Sources: [0082](../decisions/0082-source-form-and-the-shared-view.md) §3; [0004](../decisions/0004-identity-clusters-and-equivalence.md) §4; [0018](../decisions/0018-tenants.md) §7.*

**A layer on the way out rewrites entity identifiers to the form the consumer prefers.** It is the read-side rewrite that already turns a provider tenant's `Q6` into `LBQ6` or `lb:Q6` for a reader ([0018](../decisions/0018-tenants.md) §5, in [08](08-tenants-and-instances.md); [05](05-providers-and-ingest.md) §6.7), with the cluster map as one more table: where the consumer prefers the local namespace and `WDQ5` has an exact match `Q9`, the response says `Q9`. The store is never rewritten (§4.4); every entry point accepts every form (§2.1); the two together let a consumer round-trip what it was shown without the server remembering what form it gave out.

**Exact matches only.** The rewrite uses tier-1 links (`same-as`, `convert`), tier-2 links (an identifier statement of a configured link property, §4.3) and `equivalent-property` (§4.6). Tier-3 inference from shared identifier values is never used, nor is anything a `different-from` has split. The rewrite is therefore a bijection within a cluster and a pure function of the map, so two surfaces never disagree. A provider tenant's `same-as` records, tier-2 links for its readers ([08](08-tenants-and-instances.md)), count as exact matches.

**The preference is per request.** `prefer` is a namespace order (`local`, a provider code, a source name); the default is the form of the IDs in the request itself, so a request for `WDQ42` gets values in Wikidata form and a request for `Q9` gets them local, and the site UI asks for local. **An ID with no exact match in the preferred namespace is left as stored.**

**Where it applies:** Action API and REST JSON ([18](18-api.md)); SPARQL result bindings, through the concept-IRI template, since local IRIs dereference ([02](02-graphs-rdf-and-query.md)); search results; and HTML ([19](19-site-ui.md)). It rewrites the entity part of a statement GUID (§4.9) and the entity IDs in `unit`, `globe` and `calendarmodel` (§4.7). It never rewrites inside string literals or URLs. **It runs after the cache:** L1 holds the internal form once per entity version ([03](03-storage-caches-and-search.md)), and the rewrite is applied to a hit on the way out, so there is one cache entry per entity, not one per preference. **Dumps and the update stream are source form** (§4.4): the dump is the internal graph, and a rewritten dump profile, if ever wanted, is a serialization-time pass over a dump and not a stored form.

**`wbgetentities` answers under the requested ID.** A request for a non-canonical member returns the fused body under that ID, with a `canonical` field naming the cluster's canonical member and GUIDs prefixed with the requested ID. The Wikibase redirect shape (`redirects: {from, to}`) is reserved for true redirects: a `redirect` operation or an upstream merge (§4.1). A clustered Wikidata item is therefore never a redirect to a client library.

Two consumers of one tenant may see different IDs for one value, and a URL copied from one view resolves in the other because §2.1 accepts any member; normalization at every entry point is therefore not optional.

## 5. Other entity types and derived IDs

*Sources: [0066](../decisions/0066-lexemes.md) §1, §2, §3, §4, §9; [0017](../decisions/0017-entity-id-grammar.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §1; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6; [0043](../decisions/0043-lua-modules.md) §8.*

### 5.1 Lexemes

*Sources: [0066](../decisions/0066-lexemes.md) §1; [0066](../decisions/0066-lexemes.md) §2; [0066](../decisions/0066-lexemes.md) §3; [0066](../decisions/0066-lexemes.md) §4; [0066](../decisions/0066-lexemes.md) §9.*

**`lexeme` is an entity type**, local `L1`, foreign `WDL1`, hosted in namespace 146 under the `wikibase-lexeme` model. Its data model is WikibaseLexeme's, in its JSON serialization, unchanged:

| Part | Fields | Statements |
|---|---|---|
| **Lexeme** | `lemmas` (multilingual text, one per language code or variant), `language` (an item), `lexicalCategory` (an item) | Yes |
| **Form** `L1-F1` | `representations` (multilingual text), `grammaticalFeatures` (items) | Yes |
| **Sense** `L1-S1` | `glosses` (multilingual text) | Yes |

**Forms and senses are parts of the lexeme, not entities.** One page, one record stream, one revision history; a form's or sense's statements are in the lexeme's resolved JSON under `forms[].claims` and `senses[].claims`, with statement IDs `L1-F1$<uuid>`. They have no page, no terms table rows of their own beyond 0066 §5 ([03](03-storage-caches-and-search.md)), no cluster membership, and no ACL target of their own: the lexeme encloses them ([0023](../decisions/0023-moderation.md) §2). A form or sense **is an entity value**: the data types `wikibase-lexeme`, `wikibase-form` and `wikibase-sense` are added to `scatter-wikibase-model`, with `{"entity-type": "form", "id": "L1-F1"}` as Wikibase writes them, usable on items and lexemes alike. Lexeme **terms** are lemmas, not labels: a lexeme has no `labels`, and the places that show "the label" show the lemmas joined by `/` as Wikidata does.

**The `Lexeme` namespace.**

| Number | Canonical name | Kind | Allowed models | Default model |
|---|---|---|---|---|
| 146 | `Lexeme` | `pages` | `wikibase-lexeme` | `wikibase-lexeme` |
| 147 | `Lexeme talk` | `pages` | `triplespace-talk` | `triplespace-talk` |

WikibaseLexeme's number, implemented with its meaning. Titles are lexeme IDs (`Lexeme:L1`, `Lexeme:WDL1`); a form or sense ID in a title (`Lexeme:L1-F1`) resolves to the lexeme's page with the part's anchor, as `Special:EntityPage/L1-F1` does on Wikidata.

**The `wikibase-lexeme` content model.**

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `wikibase-lexeme` | WikibaseLexeme | entity (`lexeme`) | main | `application/json` | No | 146 |

An entity model; edits go through the modules of 0066 §8 ([18](18-api.md)) and the UI of 0066 §7 ([19](19-site-ui.md)). Both rows belong to the tables of [10](10-pages-and-content-models.md).

**IDs and clusters.** The ID forms, with the `-F` and `-S` suffix, are in §2.1; the cluster rule, that clusters are of lexemes and never of their parts, is in §4.1. The canonical lexeme's parts are the cluster's parts; a non-canonical member's forms and senses are **not merged** into the canonical lexeme's, so a reference to `WDL1-F1` resolves to that form under the canonical lexeme's page, never to a renumbered part. **Merging lexemes** (`Special:MergeLexemes`, 0066 §7, in [21](21-special-pages.md)) when both are local is a `redirect`, as `Special:MergeItems` writes for items (§4.11): the source's forms and senses are moved to the target with new part numbers, each moved part leaves a redirect from its old ID, and the source lexeme becomes a redirect; no cluster is formed within the local namespace (§4.2). A local lexeme and a foreign one are joined by `same-as` like any other pair.

**Mirroring, constraints, Lua and tables.**

- **The Wikidata adapter** parses `"type": "lexeme"` documents from the same JSON dumps and EventStreams it already reads, writing `put` records for `WDL` as for any entity ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4, in [05](05-providers-and-ingest.md)); the RevisionChest reader covers them. A Wikidata test import includes lexemes.
- **Constraints** ([0031](../decisions/0031-property-constraints.md), in [06](06-statements-and-properties.md)): `lexeme requires language`, `lexeme requires lexical category`, `lexeme value requires lexical category` and the form/sense scoped constraint types evaluate over the lexeme's resolved JSON; the `scope` constraint's `as qualifier`/`as reference` parameters gain WikibaseLexeme's `as main value on a form/sense` meaning.
- **Lua** ([11](11-rendering-templates-and-modules.md)): `mw.wikibase.lexeme` and the `mw.wikibase.lexeme.entity.*` classes as the extension documents them; usage tracking per lexeme ID.
- **Tables and scopes** ([15](15-structured-pages.md)): `L` and part IDs as rows; term columns `lemma:{lang}`, `representation:{lang}`, `gloss:{lang}`; the `language` and `lexicalCategory` fields as columns.
- **Adoption** ([0035](../decisions/0035-adopting-a-wikibase.md) §2, in [05](05-providers-and-ingest.md)): a Wikibase with lexemes adopts its `L` entities under their IDs.

### 5.2 MediaInfo and the `M` ID

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §1; [0044](../decisions/0044-tenant-relative-ids.md) §1.*

`M` and digits is the derived ID of §2.1: `M1234` names the statements of File page 1234 ([0041](../decisions/0041-content-models.md) §7), and is never the value of an entity data type. It is a cluster member in one case: a local `M{page ID}` and the `WDM` of the same file, where a local File page exists for a title a Commons repository serves, form an ordinary identity cluster with a fixed canonical that the file lookup sets and that no `same-as` or `different-from` record re-points; the member it fixes, and what the cluster means for resolution and rewriting, are in [12](12-files-and-media.md) §6.6. Its tenant-relative input form is `MMM1234` (§2.3), and a tenant's Lua may map the `M` letter to a provider (§5.4). Commons MediaInfo mirrored from Wikidata is the foreign type `WDM` (§1). The `mediainfo` slot, captions and Commons as a source are in [12](12-files-and-media.md).

### 5.3 Provider-ranged page IDs

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §6 (shared with [18](18-api.md)).*

**A foreign page has a derived page ID.** Its `pageid` is the **provider-ranged** form of the repository's own page ID, `provider_number << 40 | upstream page ID`, computed as mirror records' revision IDs are ([0015](../decisions/0015-record-format-and-partition-registry.md) §2, [0013](../decisions/0013-postgres-storage.md) §6, in [03](03-storage-caches-and-search.md)). It is derived, not minted, as a File page's `M` ID is (§5.2): nothing allocates it, it is the same on every tenant that reads the repository, and it never collides with a local page ID, which comes from the tenant's sequence below 2^40. Its `lastrevid` is the ranged form of the upstream revision ID the tenant currently holds. In `mirror` mode the same numbers are what the `pages/{repo}` records carry in header fields 9 and 7 ([0053](../decisions/0053-mirrored-pages.md) §5, in [13](13-mirrored-pages.md)).

In the API, `titles=`, `pageids=` and `redirects` resolve through the stack and through foreign redirects, and a ranged `pageid` resolves to the repository page it names; `GET /page/{id}` accepts a ranged ID. What else the API reports for a foreign primary is in [18](18-api.md).

**A mirrored entity's page ID is provider-ranged in the same way.** The `pageid` of an entity in a mirror partition is `provider_number << 40 | upstream page ID` where the provider publishes one (Wikidata does), and `provider_number << 40 | mirror offset` otherwise; the writer computes it from the record and allocates nothing, so a bulk sync takes no page-ID sequence and every tenant that reads the provider sees one number. The instance sequence `log."instance.page_id"` is not used for entities; the range below 2^40 is the tenant's own sequence, from which local entities and pages take theirs ([03](03-storage-caches-and-search.md)), and `view.entity`'s unique key on the page ID is `(tenant, page_id)`. Every number a client sees stays below 2^53 ([05](05-providers-and-ingest.md) §8.1).

**Sitelinks target local pages only.** A sitelink to the tenant's own host is stored by page ID ([0038](../decisions/0038-page-metadata-and-categories.md) §6), and a foreign page's ID changes when it is forked, so a title whose primary is foreign is refused as a sitelink target with `ts-sitelink-foreign`. An item that wants an article here gets a fork ([13](13-mirrored-pages.md)).

### 5.4 The Lua ID space, per tenant

*Sources: [0043](../decisions/0043-lua-modules.md) §8.*

**Each tenant says what a bare one-letter ID means in its Lua,** with the `site` setting **`lua.ids`**: a map from a type letter to `local` or to a provider code. The default is empty: every letter means `local`, so a bare `Q5` means the local Q5, as everywhere else in Triplespace. A mapped provider must mint that type letter (`providers.toml`) and be one the tenant reads (`providers`, [0018](../decisions/0018-tenants.md) §3); a record that names any other provider is refused.

**A Wikipedia mirror maps the Wikidata letters:**

```toml
# site setting lua.ids
Q = "WD"
P = "WD"
L = "WD"
M = "WD"
```

**`#property` and `#statements` read the same setting.** They are Wikibase Client surfaces like `mw.wikibase`, and a template that mixes `{{#property:P31}}` with a module call must mean one property by both.

**Translation is at the boundary,** both ways, for every ID that crosses it:

| Written in Lua (or a parser-function argument) | Means |
|---|---|
| A bare ID, `Q5`, whose letter is mapped to provider `WD` | `WDQ5` |
| A bare ID whose letter is unmapped | The local `Q5` |
| The tenant-relative form, `QQQ5` (§2.3) | The local `Q5`, **always**, whatever the mapping |
| A full foreign ID, `LBQ6`, or a keyed ID, `domain:example.org` | Itself |

| An entity reaching Lua | Is written |
|---|---|
| An entity of the provider its letter is mapped to (`WDQ5` under `Q = "WD"`) | Bare: `Q5` |
| A local entity whose letter is mapped elsewhere | Tenant-relative: `QQQ5` |
| A local entity whose letter is unmapped | Bare: `Q5` |
| Any other foreign or keyed entity | Its full ID: `LBQ6`, `domain:example.org` |

The output rule applies everywhere an ID appears in what the host returns: an entity's `id`; statement ID prefixes; the property keys of `claims`, `qualifiers` and reference `snaks`; `property` fields; `qualifiers-order` and `snaks-order`; entity values' `id`, with `numeric-id` its digits; and the results of `getEntityIdForCurrentPage`, `getEntityIdForTitle`, `resolvePropertyId`, `getReferencedEntityId`, `orderProperties` and `getPropertyOrder`.

**The mapping is a bijection** between the strings Lua sees and IDs in their stored form, so a module that reads an ID and passes it back always reaches the same entity. Nothing translated is ever stored: manifests, usage rows and caches hold IDs in the stored form.

`resolvePropertyId` resolves a label among the properties of the provider the `P` letter names, or the local ones when `P` is unmapped.

**On a Wikipedia mirror**, Module:Wd, Module:WikidataIB and infobox modules work unchanged against mirrored Wikidata, and the tenant's own items stay reachable as `QQQ…`. The Lua environment itself is in [11](11-rendering-templates-and-modules.md).

## 6. Where IDs are stored and shown

*Sources: [0017](../decisions/0017-entity-id-grammar.md) §4; [0044](../decisions/0044-tenant-relative-ids.md) §1; [0043](../decisions/0043-lua-modules.md) §8; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §3.*

The ID form of §2.1 is the one that appears everywhere an entity ID does: `view.entity.id` and every column that references it ([0013](../decisions/0013-postgres-storage.md) §5, in [03](03-storage-caches-and-search.md)), the `id` field of a search document ([0014](../decisions/0014-caches-and-search.md) §7, whose `key` field keeps the bare key), the `target_id` of an ACL ([0016](../decisions/0016-permissions-and-access-control.md) §4, in [09](09-security-and-moderation.md)), the ID chip in the UI ([0010](../decisions/0010-site-ui.md) §2, in [19](19-site-ui.md)), and canonical JSON. In storage, the ID is the one the source wrote (§4.4); in a response, it is the member of the entity's cluster that the consumer's preference selects (§4.12), and in a dump or the update stream it is the stored one. The UI's identity line names the kind of thing in words, "Domain" or "Keyword", and the chip shows the ID as it is; for a notation the identity line shows the scheme's `label` (§3.8).

The input forms are never stored: the tenant-relative form (§2.3) and a provider slug before a colon (§2.1) are canonicalized wherever an ID is accepted, and the one place a non-canonical form is written out is Lua under a mapped letter (§5.4), where nothing translated is stored either. In the log, a keyed entity appears only under its surrogate (§3.4).
