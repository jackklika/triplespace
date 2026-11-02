# 02. Graphs, RDF and the query service

This chapter describes the RDF that Triplespace produces: the named graphs its data is spread across and how they are named, the IRIs it mints for documents, revisions, records and instance-scope things, the vocabulary it defines under `scatter.red/terms/v0/` and the vocabularies it reuses, what each kind of thing in the system emits as triples, the dumps, the SPARQL Update stream that keeps a dump current, and the query service that is the only reader of a local quad store. It assumes the log, records and partitions of [01](01-log-and-records.md), the `view` schema and `view.rdf_delta` of [03](03-storage-caches-and-search.md), the source graphs, providers and reconciliation of [05](05-providers-and-ingest.md), the tenant model of [08](08-tenants-and-instances.md) and the read ACLs and erasure of [09](09-security-and-moderation.md). Tenant isolation inside the query service, the storage of deltas, and the raw SPARQL endpoint for people are in [08](08-tenants-and-instances.md), [03](03-storage-caches-and-search.md) and [18](18-api.md) respectively.

## 1. Named graphs

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §2; [0005](../decisions/0005-crate-organization.md) §4.1; [0015](../decisions/0015-record-format-and-partition-registry.md) §5.*

Triplespace's RDF projection is a set of quads spread across named graphs. The main graph and the metadata graph are projections; the local, mirror, derived and other graphs are sources, which log records write to; and the registry names them all ([0015](../decisions/0015-record-format-and-partition-registry.md) §5).

### 1.1 Graphs are registry entries

*Sources: [0005](../decisions/0005-crate-organization.md) §4.1.*

`scatter-log` does not enumerate graphs. Each deployment registers its own. A registry entry records:

- the graph IRI, and the **tenant** it belongs to, or none for an instance-level graph ([0018](../decisions/0018-tenants.md) §2);
- its **kind**: a source, which log records write to, or a projection, which is derived;
- its **ACL**: which groups may write to it ([0016](../decisions/0016-permissions-and-access-control.md) §4);
- its **history policy**: `full` or `latest` ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2);
- its **integrity policy**, `logged` or `hashed` ([0006](../decisions/0006-log-integrity-and-erasure.md) §4);
- its **export policy**: `public`, published to external stores; `internal`, served by the API but never exported; or `private`, never projected, placed on a feed or exported ([0007](../decisions/0007-actor-identity.md) §8).

Triplespace and Scatterbase register different sets:

| Role | Triplespace | Scatterbase |
|---|---|---|
| Local assertions | `local` (source, `logged`, public) | `claim` |
| Data from elsewhere | `mirror/{provider}` (source, one per provider, `hashed`, public) | `foreign` |
| Provenance about records | `metadata` (projection) | `meta` (asserted) |
| Log events | `log` (source, `logged`, internal) and `log/{provider}` (source, `hashed`, internal) ([0011](../decisions/0011-logs.md) §2) | part of `meta` |
| Document pages | `pages` (source, `logged`, public) ([0008](../decisions/0008-namespaces-and-document-pages.md) §4) | none |
| Destructible data | `accounts` (source, `logged`, private); `actors` (source, `logged`, internal); `actors/{provider}` (source, `hashed`, internal) ([0007](../decisions/0007-actor-identity.md) §8); `logins` (source, `logged`, private) at the instance ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2); and, where a farm has identity, `accounts/{farm}`, `actors/{farm}` and `log/{farm}` at the instance ([0028](../decisions/0028-tenancy-policy.md) §2) | `mutable` |
| Instance configuration | `config` (source, `logged`, public) ([0015](../decisions/0015-record-format-and-partition-registry.md) §3) | `server` |
| Reconciled view | `resolved` (projection) | none; current state is a policy projection over claims |

The table shows a correspondence, not an identity. Triplespace's `metadata` graph is derived and Scatterbase's `meta` graph is asserted, so they are not interchangeable.

### 1.2 The graph catalogue

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §5; [0005](../decisions/0005-crate-organization.md) §4.1; [0001](../decisions/0001-revision-metadata-rdf.md) §2; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2.*

**Graph names are fixed by convention.** They are the partition names of [0013](../decisions/0013-postgres-storage.md) §2, and the registry ([0015](../decisions/0015-record-format-and-partition-registry.md) §3, kind `graph`), keyed by (tenant, name), stores both name and IRI. Each tenant has its own `config`, `local`, `pages`, `log`, `actors` and `accounts`; the instance holds its `config`, `log` and `logins` ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2), every provider's partitions and, with farm identity, `actors/{farm}`, `accounts/{farm}` and `log/{farm}` under the per-issuer names ([0018](../decisions/0018-tenants.md) §2, [0028](../decisions/0028-tenancy-policy.md) §2, [0039](../decisions/0039-files-and-media.md) §10):

| Name | Kind | Defined in |
|---|---|---|
| `config` | Source | [0015](../decisions/0015-record-format-and-partition-registry.md) §3 |
| `local` | Source | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2 |
| `pages` | Source | [0008](../decisions/0008-namespaces-and-document-pages.md) §4 |
| `log` | Source | [0011](../decisions/0011-logs.md) §2 |
| `actors`, `accounts` | Source | [0007](../decisions/0007-actor-identity.md) §8 |
| `logins` (instance) | Source | [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2 |
| `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | Source, one set per provider | 0002 §2, 0007 §8, 0011 §2 |
| `log` (instance) | Source: the instance's own log, for operator records | [0039](../decisions/0039-files-and-media.md) §10 |
| `files/{repo}` | Source, one per mirrored file repository | [0039](../decisions/0039-files-and-media.md) §11 |
| `derived/{source}` | Source, one per extraction source of a tenant | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1 |
| `source/{name}` | Source, one per entity source of a tenant: its mirrored entities | [0078](../decisions/0078-entity-sources.md) §4 |
| `resolved` | Projection: the main graph of [0001](../decisions/0001-revision-metadata-rdf.md) §2 as [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3 computes it | 0002 §3 |
| `metadata` | Projection | 0001 §2 |

`graphs.toml` also lists `pages/{repo}`, the mirrored-page partition of a page repository in `mirror` mode ([0053](../decisions/0053-mirrored-pages.md) §5); see §1.4.

`{provider}` is the provider's **slug**, a lower-case name such as `wikidata`, `openalex`, `librarybase` or `internetdomains`. The provider registry entry ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4) records the slug alongside the two-letter code used in IDs.

Every graph's kind, integrity policy and export policy are those `graphs.toml` records; for `files/{repo}`, `derived/{source}`, `source/{name}`, `pages/{repo}` and the instance `log` the registry file is at present the only statement of them.

### 1.3 Graph IRIs

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §5; [0046](../decisions/0046-primary-tenant.md) §7.*

**A graph's IRI is `{base}/graph/{name}`** for a tenant's partitions, where `{base}` is the tenant's base URI: the origin that serves its `/wiki/`, `/w/api.php` and `/entity/`, the same base that [0001](../decisions/0001-revision-metadata-rdf.md) §5 gives its data ([0018](../decisions/0018-tenants.md) §2). The instance's own partitions, every name in `graphs.toml` with `scope = "instance"` or `"both"`, are under the reserved path `{farm base}/instance/graph/{name}`, so that they cannot be confused with a tenant's when the farm base is a tenant's base ([0046](../decisions/0046-primary-tenant.md) §7). The IRI is therefore per instance, which it has to be: the metadata graph attributes triples to this instance's revisions, and two instances' full dumps (§4.1) must be loadable together without their `local` graphs colliding, especially since [0009](../decisions/0009-keyed-entity-types-and-domain.md) §6 gives Domain subjects the same IRI everywhere.

**Checkpoint origin lines stay `{host}/log/{name}`** ([0006](../decisions/0006-log-integrity-and-erasure.md) §6), with the tenant's host for a tenant's partitions and `{farm host}/instance/log/{name}` for the instance's ([0018](../decisions/0018-tenants.md) §2, [0046](../decisions/0046-primary-tenant.md) §7). They are a transparency-log convention, not a graph IRI, and the two are deliberately different.

### 1.4 The registry of record

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §5; [0085](../decisions/0085-settings-and-preferences.md) §1; [0086](../decisions/0086-theming.md) §1; [0093](../decisions/0093-fallback-typefaces.md) §1; [0094](../decisions/0094-accessibility-alternates.md) §1.*

**The repository is the authority for names and codes.** The files under `docs/registry/` in the Triplespace repository are the registry of record; [23](23-configuration-and-registry.md) describes how an instance's configuration starts from them.

| File | Lists | Defined in |
|---|---|---|
| `graphs.toml` | The reserved graph names of §1.2, with each one's kind, policies and payload type; `pages/{repo}`, the mirrored-page partition of a page repository in `mirror` mode, with its five-part payload type `scatter:v0/mirrored-page` ([0053](../decisions/0053-mirrored-pages.md) §5); `derived/{source}`, a tenant's derived graph per extraction source, with the four-part `scatter:v0/derivation` ([0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §1, §3); `source/{name}`, an entity source's mirror partition of tenant scope ([0078](../decisions/0078-entity-sources.md) §4) | [0015](../decisions/0015-record-format-and-partition-registry.md) §5, §3 |
| `providers.toml` | Each provider's two-letter code, slug, **provider number** ([0015](../decisions/0015-record-format-and-partition-registry.md) §2), type codes with their upstream prefixes and IRI templates, issuer, whether it publishes revision IDs, and its `trust` mode and key-chain URL ([0022](../decisions/0022-federation.md) §2) | [0000](../decisions/0000-init.md) §3, [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4 |
| `issuers.toml` | The issuer codes and actor models | [0007](../decisions/0007-actor-identity.md) §1 |
| `namespaces.toml` | The default namespace numbers and kinds | [0008](../decisions/0008-namespaces-and-document-pages.md) §2 |
| `keyed-types.toml` | The keyed entity types | [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1 |
| `groups.toml` | The default groups, their permissions, and the default graph ACLs | [0016](../decisions/0016-permissions-and-access-control.md) §2–4 |
| `grants.toml` | The API-key grants and the permissions each covers | [0024](../decisions/0024-subsidiary-accounts.md) §4 |
| `sites.toml` | Site aliases: MediaWiki site IDs, hosts, article paths and languages, for sitelink compatibility | [0026](../decisions/0026-sitelinks.md) §2 |
| `thread-statuses.toml` | The default thread statuses, with category and order | [0019](../decisions/0019-discussions.md) §6 |
| `talk-markup.toml` | The conventions of wikitext talk pages: timestamp formats, month names by language, undated-signature marks and template names by role, universal across wikis | [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §5 |
| `preferences.toml` | The registered preference keys, types, groups and defaults, and which follow the farm account or may be held in the browser | [0027](../decisions/0027-preferences-and-portability.md) §1; [0085](../decisions/0085-settings-and-preferences.md) §1 |
| `settings.toml` | Every `site` setting, a tenant's and the instance's: scope, bound, group, type, choices and default; the catalogue of [23](23-configuration-and-registry.md) §3.2–3.3 is generated from it | [0085](../decisions/0085-settings-and-preferences.md) §1 |
| `tenancy.toml` | The tenancy switches, the three presets and their global groups | [0028](../decisions/0028-tenancy-policy.md) §1 |
| `resolvers.toml` | The default resolvers (`doi`, `url`) and drafted candidates | [0029](../decisions/0029-resolver-namespaces.md) §8 |
| `file-types.toml` | The permitted file types: extensions, MIME and media types, magic signatures, inline or attachment, thumbnailer | [0039](../decisions/0039-files-and-media.md) §5 |
| `wikitext-functions.toml` | The variables, parser functions, tags and switches of template expansion | [0042](../decisions/0042-template-expansion-and-parsoid.md) §5 |
| `special-pages.toml` | Every special page name with its MediaWiki name, aliases, scope and status | [0047](../decisions/0047-special-pages.md) §1 |
| `css-properties.toml` | The CSS properties and at-rules the `scatter-css` sanitizer allows, with the module each came from | [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2 |
| `version.toml` | What `Special:Version` credits and lists that nothing else records: developer, contributors, funders, AI agents, services, feature switches, and the extensions an `origin` names | [0077](../decisions/0077-special-version.md) §14 |
| `content-models.toml` | Content models: ID, origin, source, slot, entity type, serialization format and direct editing; the reserved MediaWiki and Wikibase model IDs | [0041](../decisions/0041-content-models.md) §3 |
| `notation-schemes.toml` | Notation schemes: name, label, normalizer and grammar, documentation template | [0048](../decisions/0048-notation.md) §2, §6 |
| `themes.toml` | Palettes, each with its `light`, `dark`, `light-more` and `dark-more` colour variants, shape tokens and accessibility alternates, the alternates' keys, type sets with the typefaces each serves, and fallback layers of typefaces for the scripts a type set's own lack: the palettes `parchment`, with the alternates `rg` and `by`, `encyclopedia`, `gruvbox`, `nord`, `catppuccin`, `greenbar` and `cyanotype`, the type sets `default26`, `hyperlegible`, `opendyslexic`, `basic`, `retro`, `nordic`, `cozy`, `mainframe` and `drafting`, and the layer `noto` | [0034](../decisions/0034-frontend-stack.md) §1; [0086](../decisions/0086-theming.md) §1; [0093](../decisions/0093-fallback-typefaces.md) §1; [0094](../decisions/0094-accessibility-alternates.md) §1 |
| `fragments.toml` | Fragment paths for packed record storage: per payload type, the CBOR path patterns stored once per dedup domain, the history condition and size bounds | [0058](../decisions/0058-packed-record-storage.md) §3 |

`scatter-log`, `scatter-providers`, `scatter-actors`, for special pages `triplespace-titles`, and for `version.toml` `triplespace-api-rest` embed these files and ship them as defaults; an instance's `config` partition ([0015](../decisions/0015-record-format-and-partition-registry.md) §3) starts from them and may diverge. Allocating a new provider code, slug, number or graph name is a change to the file, in a commit; a code is never reused. This is the same rule [0005](../decisions/0005-crate-organization.md) §5 applies to the `scatter:` vocabulary through `scatter-vocab`. The code for internetdomains.wiki is allocated there when its adapter is written; the slug `internetdomains` is reserved now. MusicBrainz (`MB`, number 4) and the other providers registered since are allocated the same way ([0017](../decisions/0017-entity-id-grammar.md) §6).

### 1.5 The main graph and the metadata graph

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §2; [0013](../decisions/0013-postgres-storage.md) §8.*

| Graph | Contents | Consumers |
|---|---|---|
| **Main graph** (`resolved`) | The Wikibase-compatible RDF described in [wikibase-compat.md](../api/wikibase-compat.md), computed by reconciling the source graphs ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3) | External triplestores (QLever) and any Wikibase or Wikidata tool, through the Wikibase-compatible dump (§4.1) |
| **Metadata graph** | Revision nodes, actors, summaries, tags, flags, log events, and the links from main-graph nodes to the revisions that produced them | The instance's own SPARQL endpoint and its full dumps (§4.1) |

**Exporting to QLever means exporting the main graph only.** The Wikibase-compatible dump never contains the metadata graph. "Not published to external RDF stores" and "never exported to external stores" ([0011](../decisions/0011-logs.md) §2) mean the Wikibase-compatible dump; a full dump is the instance's own export and carries both ([0013](../decisions/0013-postgres-storage.md) §8). The application's history views, attribution, diffs and moderation read the `view.activity` projection and the records, not RDF.

**Consumers can stay current.** For every graph an instance publishes, it also publishes the changes to that graph as a stream of SPARQL 1.1 Update requests that continues from a dump (§6).

**Attributing triples to revisions.** Wikibase already reifies statements, references and values as IRI nodes (`s:`, `ref:`, `v:`). The metadata graph can therefore refer to them directly, for example `s:Q8-C13E7A23-… scatter:addedIn <…/revision/26>`. This attributes the main graph's content to revisions without adding anything to the main graph.

## 2. Nodes and IRIs

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §1; [0015](../decisions/0015-record-format-and-partition-registry.md) §6; [0046](../decisions/0046-primary-tenant.md) §7; [0078](../decisions/0078-entity-sources.md) §7.*

Instance data IRIs belong to each instance's own base URI; only ontology terms live under `scatter.red/terms/` (§3.1). This section lists the nodes an instance mints.

### 2.1 Document nodes and revision nodes

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §1; [0015](../decisions/0015-record-format-and-partition-registry.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §4.*

Revision metadata attaches to the entity's **document node** (`data:Q8`) and to **revision nodes** hanging off it. It never attaches to the concept (`wd:Q8`). A revision describes a version of the wiki record. It says nothing about the person, work or place the record describes.

Revisions are first-class nodes with their own IRIs, minted under the instance's base URI alongside Wikibase's `s:`, `ref:` and `v:` namespaces. For example, `{base}/revision/{revid}`. Each revision node:

- is linked to its document with `prov:specializationOf`;
- is linked to its parent with `prov:wasRevisionOf`.

**Every entity on the instance has a document node under the instance's base, whatever minted its ID:** `data:Q6`, `data:WDQ42` and `data:en.wikipedia.org` are all `{base}/wiki/Special:EntityData/{id}`, as [0009](../decisions/0009-keyed-entity-types-and-domain.md) §6 states for Domains. It is what local revisions of the entity specialize, and where the instance's own metadata about the record lives.

For a foreign entity, the document node also points upstream:

```turtle
# graph <{base}/graph/metadata>
data:WDQ42 pav:importedFrom <https://www.wikidata.org/wiki/Special:EntityData/Q42> ;
    pav:hasCurrentVersion <https://www.wikidata.org/wiki/Special:Redirect/revision/123> ;
    pav:retrievedFrom <{base}/job/17> ; pav:importedOn "…"^^xsd:dateTime .
```

`pav:importedFrom` is the term §3.2 reserves for this. Upstream revisions specialize the upstream document node ([0015](../decisions/0015-record-format-and-partition-registry.md) §4); the instance's document node reaches them through `pav:hasCurrentVersion` and the job.

**The document node is the stable handle.** In the resolved view, every member of a cluster keeps its own document node, and each carries `schema:about` pointing at the cluster's **current canonical** concept IRI ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4). The concept IRIs themselves are all live: the cluster's fused statements are emitted under **every member's** concept IRI, with `owl:sameAs` between the members and the redirect-form `owl:sameAs` from each non-canonical member to the canonical one, so a query that reaches `wd:Q5` through a value and asks for its label or statements succeeds without any rewriting, and the duplication is bounded by the cluster's size, never by what references it ([0082](../decisions/0082-source-form-and-the-shared-view.md) §4). A document node never moves, so `{base}/wiki/Special:EntityData/{id}` for any member ID is what external consumers are told to cite; the SPARQL Update stream (§6) rewrites the `schema:about` triple when the canonical member changes.

**A post's revision node is its record's IRI,** `{base}/record/{partition}/{offset}`, and carries the Activity Streams properties of [0019](../decisions/0019-discussions.md) §7.

### 2.2 Instance IRIs have their own path

*Sources: [0046](../decisions/0046-primary-tenant.md) §7.*

Every IRI and origin line the instance mints for something of instance scope is under the reserved path segment **`instance`** of the farm base:

| Thing | IRI |
|---|---|
| Instance graph, every name in `graphs.toml` with `scope = "instance"` or `"both"` | `{farm base}/instance/graph/{name}` |
| Checkpoint origin line of an instance partition ([0006](../decisions/0006-log-integrity-and-erasure.md) §6) | `{farm host}/instance/log/{name}` |
| Farm account ([0028](../decisions/0028-tenancy-policy.md) §2, §10) | `{farm base}/instance/user/{id}` |
| Operator actor ([0040](../decisions/0040-instance-prerogatives.md) §2) | `{farm base}/instance/operator` |

Tenant IRIs and origin lines are unchanged, and no tenant route, article path or IRI template may begin with `/instance/`. The farm base may then be any host, including the base of the primary tenant or of any other tenant. Record IRIs `{base}/record/{partition}/{offset}` and job IRIs `{base}/job/{id}` are unchanged, because partition IDs and job IDs are unique on the instance. Special pages and REST routes served at the farm base are addresses for people and clients, not identifiers, and keep their paths. Where the farm base is also a tenant's base, a special page with a tenant and an instance form, such as `Special:Log` or `Special:Jobs`, shows both, with a `scope` filter ([0047](../decisions/0047-special-pages.md) §3); the registry's names are unique, so nothing else collides.

### 2.3 Concept IRIs of source entities

*Sources: [0078](../decisions/0078-entity-sources.md) §7.*

**A source entity's concept IRI is its type's `iri` template applied to the upstream ID**, `https://communities.miraheze.org/entity/Q1`. This is the triple's meaning outside the tenant. RDF and every dump write it as the subject and as every value. The source's name appears in no concept IRI, as Wikibase's own prefix names are not the contract ([wikibase-compat.md](../api/wikibase-compat.md) §5.1); only the tenant's own document node for the entity carries the ID form.

**A source declared without an `iri` is unpublished.** Its concept IRI is then `{base}/entity/{name}:{upstream ID}` under the tenant's base, which means nothing outside the tenant; [0078](../decisions/0078-entity-sources.md) §8 treats its references as unreadable elsewhere.

The document node is `data:mhc:Q1`, `{base}/wiki/Special:EntityData/mhc:Q1`, with `pav:importedFrom` pointing at the source's `entity_data` URL where there is one (§2.1). The tenant's Wikibase-compatible JSON dump writes the ID form; the tenant's source table is published at `GET /entity-sources` ([0078](../decisions/0078-entity-sources.md) §10), so that a reader of the dump can map it.

## 3. Vocabulary

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §5, §6; [0005](../decisions/0005-crate-organization.md) §5; [0007](../decisions/0007-actor-identity.md) §9.*

### 3.1 Triplespace's own terms live under `https://scatter.red/terms/v0/`

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §5; [0005](../decisions/0005-crate-organization.md) §5.*

Terms Triplespace defines are minted under `https://scatter.red/terms/v0/`, with the prefix `scatter:`, the namespace Scatterbase already uses. They are never minted under `http://wikiba.se/ontology#`, which belongs to Wikimedia Deutschland. Both products use that one namespace:

- **`scatter-vocab` is the only place terms are defined.** Both products take their term IRIs from it.
- **`v0` means the vocabulary makes no stability promise yet.** Moving to `v1` is a compatibility commitment, made for both products at once.

The **vocabulary** namespace is distinct from **instance data** IRIs. Revision, user and graph IRIs belong to each Triplespace instance's own base URI. Only ontology terms live under `scatter.red/terms/`. Identifiers derived only from content, which are the same on every instance, are not instance data, and may also live under `scatter.red`. Scatterbase's `https://scatter.red/claim/` and `https://scatter.red/triple/` IRIs are of this kind, and so are the keyed-type IRIs of [0009](../decisions/0009-keyed-entity-types-and-domain.md) §6, such as `https://scatter.red/domain/{key}`, and the skolem IRIs of §4.3.

### 3.2 Reuse existing vocabularies first

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §6; [0007](../decisions/0007-actor-identity.md) §9.*

New terms are minted only where no established vocabulary fits.

| Need | Vocabulary | Terms |
|---|---|---|
| Revision chain, time, author | PROV-O | `prov:wasRevisionOf`, `prov:specializationOf`, `prov:generatedAtTime`, `prov:wasAttributedTo`, `prov:SoftwareAgent`, `prov:invalidatedAtTime` |
| Versions and current version | PAV | `pav:version`, `pav:previousVersion`, `pav:hasCurrentVersion` |
| Provenance of foreign entities ([0000](../decisions/0000-init.md) §3) | PAV | `pav:importedFrom`, `pav:retrievedFrom`, `pav:importedOn` |
| User accounts | SIOC | `sioc:UserAccount`, `sioc:has_creator`, `sioc:has_modifier` |
| Account names, holders and delegation ([0007](../decisions/0007-actor-identity.md) §9) | SIOC, FOAF, PROV-O | `sioc:name`, `sioc:account_of`, `foaf:Agent`, `prov:Organization`, `prov:actedOnBehalfOf` |
| Log events: delete, protect, move, import and the rest (§5.3). A merge is its revisions and a redirect, not a log event | Activity Streams 2.0, with `scatter:` subclasses of `as:Activity` where none fits | `as:Delete`, `as:Update`, `as:Move`, `as:Undo`, `as:actor`, `as:object`, `as:published` |
| Posts, threads and talk pages ([0019](../decisions/0019-discussions.md) §7) | Activity Streams 2.0 | `as:Note`, `as:OrderedCollection`, `as:inReplyTo`, `as:context` |
| General metadata | Dublin Core Terms, schema.org | `dcterms:isVersionOf`, `dcterms:replaces`, `schema:version` |

For actors, SIOC supplies accounts and names (`sioc:UserAccount`, `sioc:name`, `sioc:account_of`), FOAF the holder of linked accounts (`foaf:Agent`), and PROV-O attribution and delegation (`prov:wasAttributedTo`, `prov:Organization`, `prov:SoftwareAgent`, `prov:actedOnBehalfOf`) ([0007](../decisions/0007-actor-identity.md) §9).

Terms to mint under `scatter:`:

- the parsed edit-summary operation and its arguments
- the full comment text
- change tags, which a record keeps in its attestation part ([0030](../decisions/0030-edit-filters.md) §5)
- revert details
- the minor, bot and patrolled flags
- slot roles and content models
- size and SHA-1
- `scatter:TemporaryAccount`, `scatter:AnonymousActor` and `scatter:ImportedActor`, as subclasses of `prov:Agent`, and `scatter:attributionHidden`, a flag on a revision whose actor is hidden ([0007](../decisions/0007-actor-identity.md) §9)

The list is illustrative: later sections of this chapter use further terms (`scatter:Revision`, `scatter:addedIn`, `scatter:logType`, `scatter:logAction`, the activity subclasses, `scatter:authority`, `scatter:listedOn`, `scatter:subject`, `scatter:cursor`), and `scatter-vocab` ([22](22-crates-and-stack.md)) is the catalogue of record for every term minted under `scatter:`.

## 4. RDF as an output

*Sources: [0013](../decisions/0013-postgres-storage.md) §8; [0001](../decisions/0001-revision-metadata-rdf.md) §3, §4; [0032](../decisions/0032-sparql-update-stream.md) §7; [0033](../decisions/0033-backend-stack.md) §8.*

### 4.1 RDF is an output, not the read path; the three dumps

*Sources: [0013](../decisions/0013-postgres-storage.md) §8; [0033](../decisions/0033-backend-stack.md) §8; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3.*

- **The API and UI read only `view`.** No request path queries a triplestore.
- **The RDF projection streams.** The resolved view (the main graph of §1.5, computed as [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3 describes), the source graphs and the metadata graph are produced as N-Quads from `view` and `log` by `triplespace-rdf` ([0005](../decisions/0005-crate-organization.md) §2). No intermediate quad store is needed to produce them. **A dump is taken from a replica under `pg_export_snapshot()`**, with parallel workers sharing the one snapshot, so that every file of a dump describes one instant and the primary is not read. **Every dump is in source form**: entity IDs are the IDs their source wrote, `WDQ5` where Wikidata wrote `Q5`, and the response-time rewriting of [04](04-entities-and-identifiers.md) never touches a dump; a rewritten dump profile, if ever wanted, is a serialization-time pass over a dump and not a stored form ([0082](../decisions/0082-source-form-and-the-shared-view.md) §3). Three dump products are offered:
  - **A Wikibase-compatible dump** holds the resolved view only. It is what QLever and any Wikibase or Wikidata tool load, and it never contains the metadata graph. Source graphs may be added to it for consumers who want to compare what a provider asserts with what the instance asserts ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3).
  - **A full dump** holds the resolved view, every source graph whose export policy allows it (§1.1), and the metadata graph. It is the RDF form of the instance's own history: for the instance's own SPARQL endpoint, for backups, and for another instance that wants the whole record.
  - **A local-graph source dump** holds the `local` partition's current state as N-Quads and as canonical JSON, one file per snapshot, with the record coordinates of each entity's newest record. It is what another instance reads for verified sync ([0022](../decisions/0022-federation.md) §1).

  Every dump is stamped with the cursor of the SPARQL Update stream it continues into, and is offered in a second form with blank nodes skolemized; the plain form is unchanged (§4.3, §6). None is the log export bundle of [0006](../decisions/0006-log-integrity-and-erasure.md) §9, which carries records, not RDF.
- **A local quad store is a deployment option** (§6.5). `scatter-quadstore` keeps its contract for Scatterbase, whose drivers implement it; in Triplespace it is a projection target, not the store projections write into. **Nothing reads from it but the query service** (§7): compiled queries for scope membership and raw SPARQL for people, under the dataset isolation of [0059](../decisions/0059-query-service.md) §4. The application's own views still come from `view`.

`oxrdf` and `oxttl`, the Oxigraph project's standalone model and serializer crates, are used in `scatter-wikibase-rdf` and `triplespace-rdf`. They serialize N-Quads, N-Triples and Turtle for dumps and for the SPARQL Update stream without a store. The `oxigraph` store is used only behind `scatter-quadstore`: Scatterbase, and the optional local quad store that consumes the update stream ([0033](../decisions/0033-backend-stack.md) §8).

### 4.2 The standard Wikibase output is unchanged

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §3.*

No vocabulary is added to the main graph or to `Special:EntityData`-equivalent output. That output stays identical in shape to Wikibase's RDF format 1.0.0, including its known departures from `ontology.owl` (see [wikibase-compat.md §4.5](../api/wikibase-compat.md)). All new vocabulary lives in the metadata graph.

**The main graph gains subjects, not vocabulary.** Statements about document pages and threads are output in it, in Wikibase's statement shape, with the page node `{base}/page/{page ID}` as subject. Every predicate is Wikibase's or schema.org's (§5.4).

**The plain dump keeps Wikibase's blank nodes.** A second form of the dump, with blank nodes replaced by skolem IRIs, is what the update stream continues from (§4.3).

### 4.3 Blank nodes are skolemized

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §7.*

Wikibase's RDF gives an *unknown value* snak a blank node object. `DELETE DATA` cannot name a blank node, and a diff between two renderings of one entity cannot match blank nodes at all. In the stream, and in every dump the stream continues from, a blank node is replaced by a **skolem IRI**:

```
https://scatter.red/genid/{H(statement UUID ‖ snak role ‖ property ‖ index)}
```

The **statement UUID** is the UUID part of the statement ID and never its entity part, so the node is the same under every member ID the fused body is emitted under (§2.1) and whatever form a consumer asked for; the **snak role** is main snak, qualifier or reference; the **index** is the snak's position within its role on the statement; a reference snak hashes the **reference hash** in place of the index, so that identical references share a node as they share a `ref:` IRI. The hash function and the byte layout of the preimage are fixed in [wikibase-compat.md](../api/wikibase-compat.md). The IRI is derived from what the blank node stands in for and from nothing about the instance, so it is the same on every re-resolution, every rebuild and **every instance**: two tenants' streams about one Wikidata item agree on the node, and a store fed by both holds one. It lives under `scatter.red` for the reason Domain IRIs do (§3.1): an identifier derived only from content is not instance data. The UUID is upstream's for a mirrored statement and the tenant's for a local one, so the inputs never collide. A store that does not care sees an IRI and treats it as opaque.

**The plain dump keeps its blank nodes.** The Wikibase-compatible dump of §4.1 is unchanged and identical in shape to Wikibase's (§4.2). Each dump is also offered with `?bnodes=skolem`, and it is that form the stream continues from; `manifest.json` says which form a file is. The batch files and every event use skolem IRIs always. A consumer that loaded the blank-node form and subscribes is told so at its first event (`412`), because its deletes would never match.

Nothing else in the resolved view's RDF changes. Statement, reference and value nodes are already IRIs, which is what makes a triple-level diff possible at all.

### 4.4 Deleted and suppressed data is an application-policy concern

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §4.*

Revision deletion and suppression are handled as policy by the Triplespace application. The RDF model does not encode them:

- Deleted or suppressed content, summaries and usernames are **not exported to RDF stores**.
- They are **not shown to users**, except administrators.
- The append-only log still holds them. Administrator views read from the log or the application, not from exported RDF.

Hiding is set by a `record` ACL on the parts it hides ([0023](../decisions/0023-moderation.md) §5). A legal takedown that requires erasure from the log itself is the `erase` record of [0006](../decisions/0006-log-integrity-and-erasure.md) §7. How hiding and erasure reach the stream and the quad store is §6.3.

## 5. What each kind of thing emits

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §1, §6; [0040](../decisions/0040-instance-prerogatives.md) §7; [0011](../decisions/0011-logs.md) §8; [0049](../decisions/0049-boards.md) §9; [0038](../decisions/0038-page-metadata-and-categories.md) §11; [0026](../decisions/0026-sitelinks.md) §8; [0048](../decisions/0048-notation.md) §5; [0066](../decisions/0066-lexemes.md) §6; [0064](../decisions/0064-entityschema-and-validation.md) §8; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §4.*

Entities emit the Wikibase RDF of [wikibase-compat.md](../api/wikibase-compat.md) in the main graph (§4.2), in source form: the subject and every entity value carry the ID the source wrote, and a cluster's fused body is emitted under each member's concept IRI with `owl:sameAs` between them (§2.1), in the dump and in the stream alike ([0082](../decisions/0082-source-form-and-the-shared-view.md) §1, §4). This section gathers what the other kinds of thing emit, and in which graph.

### 5.1 Revisions

*Sources: [0001](../decisions/0001-revision-metadata-rdf.md) §6, §1; [0040](../decisions/0040-instance-prerogatives.md) §7.*

Revision nodes are in the metadata graph (§2.1). Illustrative shape, using a revision from the reference install:

```turtle
# graph <…/graph/metadata>
data:Q1 pav:hasCurrentVersion rev:9 .
rev:9 a prov:Entity, scatter:Revision ;
    prov:specializationOf data:Q1 ;
    prov:wasRevisionOf rev:8 ;
    prov:generatedAtTime "2026-09-25T00:49:03Z"^^xsd:dateTime ;
    prov:wasAttributedTo <…/user/…> ;
    scatter:summaryOperation "wbsetlabel-add" ; scatter:summaryArgs "1|es" ;
    scatter:comment "/* wbsetlabel-add:1|es */ Douglas Adams, test" ;
    scatter:minor false ; scatter:size 853 ; scatter:sha1 "eb5635fd…" ; scatter:contentModel "wikibase-item" .
```

**Every revision node carries `scatter:contentModel`, whatever the page:** entity revisions their entity type's model, thread revisions `triplespace-thread`, and statement revisions of File pages `wikibase-mediainfo` ([0041](../decisions/0041-content-models.md) §10).

**An instance act** ([0040](../decisions/0040-instance-prerogatives.md)): the record's revision node has `prov:wasAttributedTo` the operator IRI, and `scatter:authority` the authority record's IRI `{farm base}/record/{partition}/{offset}`. The person never appears. The display and log-event side of instance acts is in [19](19-site-ui.md) and [16](16-logs-feeds-and-notifications.md).

### 5.2 Log events

*Sources: [0011](../decisions/0011-logs.md) §8; [0049](../decisions/0049-boards.md) §9.*

Log events are projected into the **metadata graph** only.

**IRIs:**

| Event | IRI |
|---|---|
| Upstream | `{article path}Special:Redirect/logid/{id}` on the provider's wiki, for example `https://www.wikidata.org/wiki/Special:Redirect/logid/{id}`. This parallels the upstream actor IRIs of [0007](../decisions/0007-actor-identity.md) §2. |
| Local | `{base}/record/{partition}/{offset}`, the IRI of the record the event comes from |

Local event IRIs are record IRIs, not log IDs, although every local event has a global log ID ([0015](../decisions/0015-record-format-and-partition-registry.md) §2). Partition numbers and offsets are fixed at append time and never reused. They are the same coordinates that inclusion proofs use ([0006](../decisions/0006-log-integrity-and-erasure.md) §9).

**Types.** Every log event is a `prov:Activity`. Activity Streams 2.0 types are used where they fit, and `scatter:` subclasses of `as:Activity` are minted where they don't:

| Log | Type |
|---|---|
| `delete/delete` | `as:Delete` |
| `delete/restore` | `as:Undo`, whose `as:object` is the deletion when the instance holds it |
| `create/create`, `newusers/create` | `as:Create` |
| `move/move` | `as:Move` |
| `thread/attach` | `as:Add`, with `as:object` the thread and `as:target` the talk page or board ([0049](../decisions/0049-boards.md) §9) |
| `task/claim`, `task/release` | `as:Add` and `as:Remove`, with `as:target` the sprint page and `as:object` the task's subject ([0061](../decisions/0061-sprints-and-tasks.md) §5) |
| `proposal/propose`, `proposal/submit`, `proposal/withdraw` | `as:Offer` with `as:target` the upstream IRI; `as:Announce`; `as:Undo` ([0067](../decisions/0067-proposals.md) §3) |
| `thread/detach` | `as:Remove`, with `as:origin` the talk page or board it left |
| `contentmodel/*` | `as:Update` |
| `protect/*` | `scatter:Protect` |
| `delete/revision`, `delete/event` | `scatter:ChangeVisibility` |
| `import/*` | `scatter:Import` |
| `renameuser/*` | `scatter:Rename` |
| `link/link` | `scatter:LinkAccount` |
| `retention/set` | `scatter:SetRetention` |
| `convert/convert` | `scatter:Convert` |
| `erase/erase` | `scatter:Erase` |
| `job/*` | `scatter:JobEvent`. The job itself is a `prov:Activity` at `{base}/job/{id}`. |

Activity Streams has exactly the terms `thread/attach` and `thread/detach` use.

**Properties:**

- `as:actor`: the performer's actor IRI. It is omitted when the performer is hidden, and the event then carries `scatter:attributionHidden` ([0007](../decisions/0007-actor-identity.md) §9).
- `as:object`: the target's **document node**, as §2.1 requires. That is `data:Q123` for an entity, `{base}/page/{id}` for a document page, and the actor IRI for an account. For `delete/revision`, it is the affected revision nodes.
- `as:result`: for `convert`, the minted entity's document node.
- `as:published`: the time of the event at its source.
- `scatter:logType`, `scatter:logAction`: the MediaWiki strings, always kept.
- `scatter:comment`: omitted when hidden.
- The typed parameters of [0011](../decisions/0011-logs.md) §7, as `scatter:` properties or blank nodes. Each protection restriction is a node with a restriction, a level and an expiry.

```turtle
# graph <{base}/graph/metadata>
<https://www.wikidata.org/wiki/Special:Redirect/logid/{id}>
    a as:Delete, prov:Activity ;
    as:actor <https://www.wikidata.org/wiki/Special:Redirect/user/{uid}> ;
    as:object <https://www.wikidata.org/wiki/Special:EntityData/Q123> ;
    as:published "…"^^xsd:dateTime ;
    scatter:logType "delete" ; scatter:logAction "delete" ;
    scatter:comment "…" .
```

Merges are not log events. A Wikibase merge is represented by its revisions and by the redirect ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2). The log events themselves, their parameters and who may see them are in [16](16-logs-feeds-and-notifications.md).

### 5.3 Threads, talk pages and boards

*Sources: [0049](../decisions/0049-boards.md) §9; [0001](../decisions/0001-revision-metadata-rdf.md) §1.*

A post's revision node is its record's IRI, `{base}/record/{partition}/{offset}`, and carries the Activity Streams properties of [0019](../decisions/0019-discussions.md) §7. The nodes of threads with several listings extend that:

- A thread's `as:context` stays its home. It gains `scatter:listedOn` for each listing.
- A talk page's `as:OrderedCollection` holds every thread attached to it, home or listing.
- A **board** is `{base}/page/{board page ID}`, an `as:OrderedCollection` of its threads with `as:name` the title and `as:summary` the description. It has no `scatter:subject`.

The attach and detach log events of boards are in [16](16-logs-feeds-and-notifications.md); their RDF types are in §5.2.

### 5.4 Pages and page statements

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §11.*

**Page statements are in the main graph,** in Wikibase's statement shape (`p:`, `ps:`, `pq:`, `prov:wasDerivedFrom`, the truthy direct claims and statement nodes named from the statement ID), with the page's node `{base}/page/{page ID}` of [0008](../decisions/0008-namespaces-and-document-pages.md) §10 as subject. That node gets `a schema:WebPage`, `schema:name` (the current title), `schema:url` and, where the page sets a short description, `schema:description` from its `wikibase-shortdesc` page property ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6). For a page paired with an item ([0038](../decisions/0038-page-metadata-and-categories.md) §6), the sitelink's `schema:Article` node is this same page node, not the URL, so a query can join an item's article with the article's statements. The URL remains available through `schema:url`.

This adds subjects to the main graph, not vocabulary: every predicate is Wikibase's or schema.org's, and a Wikibase consumer that reads `p:`/`ps:` reads page statements unchanged. Projected statements are output like asserted ones. Categories themselves are not RDF; the triples they produce come through mappings. The SPARQL Update stream (§6) carries page statements' deltas like any other.

### 5.5 Sitelinks

*Sources: [0026](../decisions/0026-sitelinks.md) §8.*

The RDF shape of [wikibase-compat.md §5.2](../api/wikibase-compat.md) is kept for every sitelink, so QLever and Wikidata tooling see what they expect:

```turtle
<https://collections.example.museum/object/1234> a schema:Article ;
    schema:about wd:Q42 ;
    schema:isPartOf <https://collections.example.museum/> ;
    schema:name "object/1234" ;
    wikibase:badge wd:Q17437798 .
```

`schema:inLanguage` is emitted only where an alias supplies a language; `schema:name` carries a language tag only then. `wikibase:sitelinks` on the entity counts links in the resolved view.

For a page paired with an item ([0038](../decisions/0038-page-metadata-and-categories.md) §6, §11; [06](06-statements-and-properties.md)) the `schema:Article` node is the page node `{base}/page/{page ID}`, also typed `schema:WebPage`, with the URL as `schema:url`; every other sitelink keeps the shape above.

### 5.6 Foreign, source and MediaInfo entities

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §6; [0078](../decisions/0078-entity-sources.md) §7; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §4.*

A foreign entity's document node carries the `pav:` provenance triples of §2.1 in the metadata graph, and a source entity's concept IRI and document node follow §2.3. For MediaInfo: captions are on the `M` node ([0065](../decisions/0065-mediainfo-captions-and-commons.md) §1); `WDM` entities are in the Wikidata mirror graph and, through resolution, in the resolved view of the tenant's own File pages for foreign files, as any mirrored entity. The storage and API side of MediaInfo is in [03](03-storage-caches-and-search.md) and [18](18-api.md).

### 5.7 Notations

*Sources: [0048](../decisions/0048-notation.md) §5.*

A notation's concept IRI is its subject ([0048](../decisions/0048-notation.md) §1). Beside the triples every keyed entity gets, it carries:

```turtle
<https://scatter.red/notation/osm:amenity=cafe>
    skos:notation "amenity=cafe"^^<https://scatter.red/notation-scheme/osm> ;
    skos:inScheme <https://scatter.red/notation-scheme/osm> .
```

SKOS types a notation's literal with a datatype named for its scheme, and this follows it. `https://scatter.red/notation-scheme/{name}` is the scheme's IRI, which is content-derived like the notation's own. A notation is **not** typed `skos:Concept`: it is the string, not the concept ([0048](../decisions/0048-notation.md) §3), and SKOS gives `skos:notation` and `skos:inScheme` no domain that would imply otherwise.

### 5.8 Lexemes

*Sources: [0066](../decisions/0066-lexemes.md) §6.*

Lexemes are emitted as Wikidata emits them, with the OntoLex vocabulary, so the Wikibase-compatible dump and the stream carry them unchanged and QLever's Wikidata index layout applies:

| Subject | Triples |
|---|---|
| Lexeme `wd:L1` | `a ontolex:LexicalEntry`; `wikibase:lemma` and `rdfs:label` for each lemma; `dct:language wd:Q1860`; `wikibase:lexicalCategory wd:Q1084`; `ontolex:lexicalForm wd:L1-F1`; `ontolex:sense wd:L1-S1`; statements as on any entity |
| Form `wd:L1-F1` | `a ontolex:Form`; `ontolex:representation` per language; `wikibase:grammaticalFeature wd:Q…`; statements |
| Sense `wd:L1-S1` | `a ontolex:LexicalSense`; `skos:definition` per language; statements |

Part IRIs follow the entity IRI rules of [0015](../decisions/0015-record-format-and-partition-registry.md) §5 with the suffix in the local part. `wikibase-compat.md` gains §5.6 "Lexemes" with the mapping as observed from Wikidata's dump, and §2's exclusion of lexemes, forms and senses is lifted; entity schemas stay out of its RDF.

### 5.9 EntitySchemas

*Sources: [0064](../decisions/0064-entityschema-and-validation.md) §8.*

Entity schemas have **no RDF**: no terms on an entity node, no type, nothing in any dump or in the update stream, as Wikidata exports none. The ShExC text and the terms are served at `Special:EntitySchemaText` and `GET /entity/E1` ([18](18-api.md)).

Not yet: whether the ShExC text belongs in the full dump is open ([0064](../decisions/0064-entityschema-and-validation.md) Q3).

## 6. The SPARQL Update stream

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §1, §2, §5, §6, §8; [0013](../decisions/0013-postgres-storage.md) §8; [0033](../decisions/0033-backend-stack.md) §8, §11.*

For every graph an instance publishes, it also publishes the changes to that graph as a stream of SPARQL 1.1 Update requests that continues from a dump. The storage of deltas in `view.rdf_delta` is in [03](03-storage-caches-and-search.md); shared and overlay deltas across tenants are in [08](08-tenants-and-instances.md); the crates are in [22](22-crates-and-stack.md).

### 6.1 The stream is the dump, kept current

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §1; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3.*

For every graph an instance publishes in RDF, it also publishes the **sequence of changes** to that graph as SPARQL 1.1 Update requests. A consumer loads a dump, notes the **cursor** the dump was taken at, and applies every event after that cursor in order. The result is, at every cursor, the same set of triples the dump would contain if it were taken then. This is the contract. It is why the stream, like the dump, is **source form** (§4.1): the dump is the internal graph, and the response-time rewriting of entity IDs to a consumer's preferred form ([04](04-entities-and-identifiers.md); [0082](../decisions/0082-source-form-and-the-shared-view.md) §3) never applies to either.

The stream carries the **public form** only, exactly as a dump does: nothing behind a read ACL ([0023](../decisions/0023-moderation.md)), nothing erased ([0006](../decisions/0006-log-integrity-and-erasure.md) §7), nothing from an `internal` or `private` partition (§1.1). The graphs a subscription may name are those of §4.1:

| `graphs=` | Carries | Continues from |
|---|---|---|
| `resolved` (default) | The resolved view of [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3, including the truthy and normalized triples, as a single default graph | The Wikibase-compatible dump |
| `resolved,local`, `resolved,mirror/wikidata`, … | The resolved view and the named source graphs, each in a `GRAPH` clause | The Wikibase-compatible dump with those source graphs added |
| `full` | Every public source graph, the resolved view and the metadata graph, all in `GRAPH` clauses. A modifier `metadata=local` (the default) carries revision, actor and event nodes for the tenant's own records and the provenance triples of §2.1 for mirrored entities, but not a mirror's per-revision nodes; `metadata=all` carries every node the full dump does | The full dump, which takes the same modifier |

When exactly one graph is subscribed, its triples are emitted without a `GRAPH` clause, as the Wikibase-compatible dump is a single-graph dump; with more than one, every triple is inside `GRAPH <{base}/graph/{name}> { … }`, with the IRIs of §1.3. A consumer that loads the single-graph dump into its default graph and subscribes to `resolved` never sees a graph name.

### 6.2 The delta is computed by a consumer of composition

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §2; [0082](../decisions/0082-source-form-and-the-shared-view.md) §1; [0083](../decisions/0083-write-path-in-three-tiers.md) §3, §7.*

A **delta** is a pair of triple sets, deleted and inserted, for one graph, caused by one event. It is computed by the **delta consumer**, a tier-3 consumer of composition events ([0083](../decisions/0083-write-path-in-three-tiers.md) §3; [03](03-storage-caches-and-search.md) §6) that exists only while the `site` setting **`updates.enabled`** is on (default off), with its own queue, rate and lag; it runs in no write's transaction and is off during bootstrap and bulk modes. It has the old state and the new because the write path stores them:

| Graph | Event | Old state | New state |
|---|---|---|---|
| `resolved` | The composer writes an entity's composed row ([0083](../decisions/0083-write-path-in-three-tiers.md) §2) | The previous composed row, kept until the delta consumer has read it | The new composed row |
| `local`, `mirror/{provider}` | A change set is applied to the entity's state in that graph ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2) | The graph's previous state, from `view.graph_state` for a delta graph and the previous `put` for a mirror graph ([0083](../decisions/0083-write-path-in-three-tiers.md) §4) | The new state |
| `metadata` | A record is appended, hidden or erased | The revision, actor, event and provenance nodes the record projected before | The nodes it projects now |

Each side is rendered to triples, the `resolved`, `local` and `mirror/{provider}` sides by `scatter-wikibase-rdf` ([wikibase-compat.md](../api/wikibase-compat.md) §4–5) and the `metadata` sides by `triplespace-rdf` (§4.1), and the delta is the set difference, so a change that touches one statement of a large entity is a few triples, not the entity. A **cluster change** ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §4) recomposes the cluster's members and nothing else: the stored graph is in source form, a referrer's triples do not change when an identity changes, and there is one delta per member and none per referrer ([0082](../decisions/0082-source-form-and-the-shared-view.md) §1). A **read ACL** or an **erasure** produces a delta whose deleted side is what left the public form and whose inserted side is empty, or the tombstone form where one exists.

**Why from stored state.** Under the `latest` history policy a mirror's earlier state is compacted away ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2), and [0012](../decisions/0012-api-requirements.md) §7 concedes that the resolved view at a past time cannot be rebuilt in general. The delta is therefore computed from the state the write path kept and **stored** ([0032](../decisions/0032-sparql-update-stream.md) §3), not recomputed on request; a rebuild truncates `rdf_delta` and bumps the epoch (§6.4), and consumers reload from the post-rebuild dump.

**One sequencer.** `rdf_delta.seq` is assigned by the delta consumer alone, so insert order is commit order and a consumer reading `seq > cursor` never skips a row that committed late ([0083](../decisions/0083-write-path-in-three-tiers.md) §7). Because the consumer is single, the stream can be served from a replica.

**Bootstrap and bulk modes** ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.6, [0013](../decisions/0013-postgres-storage.md) §9) produce no deltas: tier 3 is off, and a consumer of a bootstrapped partition loads the dump taken at the end of the load. A `snapshot` job's tombstone sweep ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4) produces one delta per removed entity, as any tombstone does.

### 6.3 Erasure and hiding reach the consumer, and the store

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §7.*

**The consumer.** An `erase` record or a read ACL produces a delta whose deleted side names every triple that left the public form ([0015](../decisions/0015-record-format-and-partition-registry.md) §1, [0023](../decisions/0023-moderation.md) §5): the statement, its qualifiers, references and values, the truthy triple, the revision node's hidden fields in the metadata graph. A consumer that applies the stream is therefore as compliant as this instance is, which is the property [0020](../decisions/0020-change-feeds.md) §4 gives the activity stream and [0006](../decisions/0006-log-integrity-and-erasure.md) §7 asks of copies elsewhere. It cannot be enforced beyond that.

**The store.** Delta rows hold triples, and an erased literal may sit in the `inserted` side of an earlier row within the retention window. The erasure path of [0014](../decisions/0014-caches-and-search.md) §5 gains a **sixth step**: delete or rewrite every `rdf_delta` row that contains an erased triple, found through `rdf_delta_entity` for an entity's rows and through the index on `(partition, "offset")` for the rows an erased record produced on its own, a metadata-only row with no entity among them ([0083](../decisions/0083-write-path-in-three-tiers.md) §7). A consumer that fetches that range afterwards receives the rows as rewritten; one that fetched them before is in the position [0006](../decisions/0006-log-integrity-and-erasure.md) §7 describes, and the deleting delta reaches it next. The retention window bounds how long an erased triple can persist in the table, as the TTL ceilings of 0014 bound the caches.

**Hidden and suppressed content never enters a delta**, because the delta is computed from the public form.

### 6.4 Delivery: pull, with a sync client

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §7.*

The instance **serves**; it never pushes, and it holds no consumer's credentials.

| Route | Meaning |
|---|---|
| `GET /updates/stream?graphs=&metadata=&from=` | Server-sent events. Each event is one transaction ([0032](../decisions/0032-sparql-update-stream.md) §3): its `id` is the cursor `{epoch}:{seq}` of the transaction's last delta, its `event` is `update`, its `data` is one SPARQL Update request, `DELETE DATA { … } ; INSERT DATA { … }`, over every delta in the transaction, and a leading `: ` comment line carries the event's provenance: the tenant, the record `partition` and `offset` that caused it and its `revid`, or the job for a sync batch, so a consumer can fetch `/record/{partition}/{offset}` and its proof on demand. Nothing of that enters the triples. `Last-Event-ID` resumes. It is the mechanism of `GET /activity/stream` ([0020](../decisions/0020-change-feeds.md) §4) with a different body |
| `GET /updates?graphs=&metadata=&from=&limit=` | The same events as one `application/sparql-update` document, one request per event separated by `;`, each preceded by a `#` comment line with its cursor and provenance, with the next cursor in a `Link: rel="next"` header, for consumers that poll |
| `{base}/dumps/updates/{epoch}/{from}-{to}.ru` | Batch files, written every `updates.batch_interval` (default one hour), each holding the events of its range; `manifest.json` beside them lists the ranges and the cursor of every dump under `{base}/dumps/` |
| `GET /updates/cursor` | The current epoch and head `seq`, and the retention horizon |

**Every dump is stamped** with the cursor it was taken at, in its manifest and in a comment on its first line, so a consumer knows where to start. A `from` older than the horizon, or from another epoch, is answered with **`410 Gone`** and the URL of the newest dump that continues into the window; the client reloads.

**The epoch is per tenant** (`ops.tenant_epoch`, [03](03-storage-caches-and-search.md)), and a cursor is meaningful only against the tenant whose stream issued it. A tenant's epoch is bumped by a rebuild of its deltas (§6.2), by a change to its provider list, its query isolation or a base alias, so that a tenant that opts in to a provider tells only its own consumers to reload; the farm base's provider streams have their own. **The epoch carries a rendering version**, incremented by a change to `scatter-wikibase-rdf`, `triplespace-rdf` or a value normalizer, because `DELETE DATA` matches only byte-identical triples and a renderer that writes a literal differently would leave the old form behind; the lexical forms of every value type are pinned as a fixture in [wikibase-compat.md](../api/wikibase-compat.md) so that the version changes when they do and not otherwise.

**Idempotency.** `DELETE DATA` and `INSERT DATA` are set operations, so applying an event twice is harmless and a consumer may commit its cursor after each event or after a batch. Applying events out of order is not harmless; the stream is ordered and the client applies it in order.

**`triplespace-cli sparql-sync`** is the reference consumer, run by the operator of the triplestore:

```
triplespace-cli sparql-sync --source https://librarybase.org --graphs resolved \
    --endpoint https://qlever.example/api/update --token-file ./qlever.token \
    --state ./librarybase.cursor
```

It loads the dump on first run when asked, then follows the stream, POSTs each event (or a batch) to the endpoint as `application/sparql-update` with the endpoint's own authorization, and commits the cursor to its state file after the endpoint acknowledges. On `410` it stops and names the dump to load. It speaks only standard SPARQL 1.1 Update over HTTP, so it works against QLever, Oxigraph, Fuseki and any store that does; endpoint-specific options (QLever's access token, a graph-store URL) are flags, not code paths.

**Rate limits** are the `stream` class of [0024](../decisions/0024-subsidiary-accounts.md) §5; a consumer that needs more opens a subsidiary and asks for it. **Permissions:** `read`. A tenant whose partitions are `private` has no stream, as it has no dump.

### 6.5 The local quad store and QLever as consumers

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §6; [0013](../decisions/0013-postgres-storage.md) §8; [0033](../decisions/0033-backend-stack.md) §8, §11.*

**The local quad store is a consumer.** An instance that runs Oxigraph or QLever for its own SPARQL endpoint feeds it from `view.rdf_delta`, the same rows the stream serializes, in process through `scatter-quadstore`'s `apply(delta)`, or from its own stream with `triplespace-cli sparql-sync`; there is no separate quad projection. `scatter-quadstore` has `apply(deleted, inserted)` beside `lookup`, which Scatterbase's drivers may implement or ignore. The local store has one reader, the query service (§7), which answers compiled queries and raw SPARQL for the instance from it; it remains a consumer of these rows and nothing else writes to it.

**QLever** is an export destination, and optionally the remote backend of the query service (§7.2). It is never a required dependency: the service's default backend is Oxigraph embedded in the binary. It reached full SPARQL 1.1 compliance, including Update and the Graph Store Protocol, in June 2025, and its own tooling keeps a Wikidata index current from a change stream (`qlever update-wikidata`). This stream matches that model.

- `triplespace-cli sparql-sync` is the reference consumer and is tested against QLever in CI ([0033](../decisions/0033-backend-stack.md) §15).
- A `qlever update-triplespace` command in qlever-control, pointed at `/updates/stream`, is to be offered upstream.
- The `resolved` subscription is the default for a QLever that serves outside consumers only. A QLever that is the query service's remote backend loads `full`, with its named graphs, since the service isolates tenants by dataset ([0059](../decisions/0059-query-service.md) §4); the `full` benchmark is part of 0059's test plan.

### 6.6 What the stream is not

*Sources: [0032](../decisions/0032-sparql-update-stream.md) §8.*

- **Not the verified sync of [0022](../decisions/0022-federation.md).** Another Triplespace instance reads a provider's `local` graph through the activity stream and verifies every record against the provider's checkpoints. This stream is derived from the resolved view, includes mirrors, and carries no proofs. An instance that consumed it would hold what the provider *shows*, unverifiably; 0022 gives it what the provider *asserts*, provably. The two are kept apart.
- **Not the activity stream of [0020](../decisions/0020-change-feeds.md) §4.** That stream carries activity rows redacted per viewer, for bots, tools and other instances; this one carries triples, for triplestores. They share the transport and the `Last-Event-ID` discipline and nothing else.
- **Not a change feed of the log.** A delta is a projection's output. Records, offsets and proofs are the log's; a consumer that wants them reads `/record/` and `/.well-known/tlog/` ([0022](../decisions/0022-federation.md) §1).

## 7. The query service

*Sources: [0059](../decisions/0059-query-service.md) §1, §2, §3, §5; [0033](../decisions/0033-backend-stack.md) §11.*

The query service is the one reader of the quad store. Tenant isolation by dataset or by store ([0059](../decisions/0059-query-service.md) §4) is in [08](08-tenants-and-instances.md); raw SPARQL for people ([0059](../decisions/0059-query-service.md) §6) is in [18](18-api.md) and [19](19-site-ui.md); operations and crates ([0059](../decisions/0059-query-service.md) §7, §8) are in [22](22-crates-and-stack.md).

### 7.1 One reader of the quad store

*Sources: [0059](../decisions/0059-query-service.md) §1.*

**The quad store has exactly one reader: the query service.** The service is a trait in the crate `triplespace-query` ([0059](../decisions/0059-query-service.md) §8):

```rust
trait QueryService {
    /// Evaluate a read-only SPARQL query against the dataset of one tenant.
    fn query(&self, tenant: TenantId, q: &CompiledQuery, limits: &Limits) -> Result<Solutions, QueryError>;
    /// The last `view.rdf_delta` row the store has applied, as the stream's cursor.
    fn cursor(&self) -> Cursor; // {epoch, seq}
}
```

Everything else §4.1 says stands: `view` is the serving model; the store is a consumer of `view.rdf_delta` and never written to by anything else; it holds the public form only (§6.1); it is never the authority for anything, and a rebuild of `view` rebuilds it (§7.3). **Nothing reads from it but the query service.**

The service is **optional**: `query.enabled` (`site` configuration, default `false`) turns it on. Every feature that uses it says so, and either degrades to its last result or is absent while the service is off (§7.3). The one-binary, one-required-service rule of [0033](../decisions/0033-backend-stack.md) §1 is kept by the embedded backend (§7.2).

### 7.2 Two backends

*Sources: [0059](../decisions/0059-query-service.md) §2; [0033](../decisions/0033-backend-stack.md) §11; [0082](../decisions/0082-source-form-and-the-shared-view.md) §2.*

| `query.backend` | Store | Fed by | Isolation ([0059](../decisions/0059-query-service.md) §4) |
|---|---|---|---|
| `embedded` (default) | Oxigraph, in process, behind `scatter-quadstore`'s existing feature ([0005](../decisions/0005-crate-organization.md) §4.4), on a RocksDB directory under `query.path` | `scatter-quadstore::apply(deleted, inserted)` from `view.rdf_delta`, in the projection worker, after each delta batch commits | `dataset` or `store` |
| `remote` | Any SPARQL 1.1 Protocol endpoint; QLever first | `triplespace-cli sparql-sync` (§6.4) run by the operator against the instance's own `full` stream, or QLever's own `update` tooling (§6.5) | `dataset` only |

**Embedded is the default** so that turning the service on changes nothing about the deployment: the `triplespace` binary and Postgres. The embedded store is a second copy of the public graphs on the server's disk, grown by `rdf_delta`, and is sized like the `full` dump. **It is the small profile's backend** ([0013](../decisions/0013-postgres-storage.md) §11; [03](03-storage-caches-and-search.md)): the service refuses to start it above a configured triple count, and the Wikidata-scale profile requires `query.backend = remote`. A larger instance moves to `remote` when the embedded store's query times or disk no longer suit it; nothing above the trait notices.

**Both backends sit behind one rewriting layer.** Where a query constant names an entity that is a member of a cluster, the planner expands the constant to the member set, `lb:Q9` becoming `VALUES ?v { lb:Q9 wd:Q5 … }`, one indexed lookup against `view.cluster_member` per constant with L0 in front of it, before the query reaches the store; any member of a cluster is therefore accepted wherever a query names an entity ([04](04-entities-and-identifiers.md); [0082](../decisions/0082-source-form-and-the-shared-view.md) §2). **Property paths are not expanded.** `wdt:P279*` follows stored triples, neither backend does `owl:sameAs` reasoning, and a query that needs `owl:sameAs` closure writes it; the fused body under every member (§2.1) makes the common cases work without it. Result bindings are rewritten to the request's preferred form through the concept-IRI template on the way out ([18](18-api.md)).

**The remote backend is one store.** It is loaded once from the `full` dump and followed from the `full` stream, so that it holds every tenant's graphs under their names ([0059](../decisions/0059-query-service.md) §4). It is never loaded from `view` or `log` directly, which [0056](../decisions/0056-security-model.md) §10 line 7 forbids. The endpoint is `query.endpoint`; its credential, if any, is read as every secret is ([0033](../decisions/0033-backend-stack.md) §12). The service checks the endpoint's cursor by reading a marker triple the stream's consumer writes, `<{base}/.well-known/query> scatter:cursor "{epoch}:{seq}"`, in the store's default graph; `sparql-sync` writes it after each batch.

The `full` subscription is the only form a shared store loads, with `metadata=local` (§6.1) by default for both backends; the query-service setting `query.metadata = all` makes the embedded worker apply every delta and the remote loader take the `metadata=all` dump and stream.

### 7.3 What the store holds, and how fresh it is

*Sources: [0059](../decisions/0059-query-service.md) §3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §7.*

**The `full` form** (§6.1): every public source graph, **one shared resolved graph**, a small overlay graph per tenant, and every tenant's metadata graph, each in its own named graph. The shared resolved graph is the instance's, computed under the instance's policy record (`reconcile:default`, [08](08-tenants-and-instances.md) §2.5) from the shared rows of `view` ([03](03-storage-caches-and-search.md) §7); a tenant's overlay graph holds only the entities that tenant has overlaid, and a tenant's dataset is the shared graph with its overlay shadowing it ([0082](../decisions/0082-source-form-and-the-shared-view.md) §5; the deltas that feed both are in [08](08-tenants-and-instances.md) §10.5). A shared provider's source graph (Wikidata's, OpenAlex's) is likewise stored once, since its deltas are stored once ([0032](../decisions/0032-sparql-update-stream.md) §4), and is part of the dataset of every tenant that opted in to the provider ([0018](../decisions/0018-tenants.md) §5). This is why one store scales where one per tenant would not: forty tenants mirroring Wikidata share one copy of Wikidata's source triples and one copy of its resolved triples, and a query written for Wikidata runs unchanged on a tenant with no overlay.

**Public form only.** The store holds what the stream carries: read ACLs applied, erased content purged (§6.3), export policy respected. A query therefore never leaks, whoever asks, and a viewer who may read more than the public sees fewer results than `view` could show them. Accepted: the store is for finding things, and what is found is then read through `view` with the viewer's rights.

**Freshness is a cursor.** `cursor()` is the last `rdf_delta` row applied, in the stream's `{epoch}:{seq}` form. `siprop=triplespace` reports `query: {available, backend, cursor, lag_seconds}`, where lag is the age of the newest delta row not yet applied. Every result the service returns carries the cursor it was computed at, and every stored result (a scope's members, [0060](../decisions/0060-scopes.md) §5) records it, so that a page can say "as of {time}; {n} changes since". A `view` rebuild truncates `rdf_delta` and increments the epoch (§6.2, §6.4; [0083](../decisions/0083-write-path-in-three-tiers.md) §7) and empties the embedded store, which is then reloaded from a fresh `full` dump by the worker before it resumes applying deltas; a remote store is reloaded by the operator, as any stream consumer is, and the service reports `available: false` while its epoch is behind. Because epochs are per tenant, a tenant's provider change reloads that tenant's graphs and no other's.

**When the service is unavailable** (off, reloading, the endpoint down, or lag above `query.max_lag`, default 1 h), a compiled query (§7.4) returns `QueryError::Unavailable` and its caller keeps the last materialized result with its cursor; raw SPARQL ([0059](../decisions/0059-query-service.md) §6) answers `503` with `Retry-After`.

### 7.4 Compiled queries

*Sources: [0059](../decisions/0059-query-service.md) §5.*

**A compiled query is SPARQL the instance wrote.** Its source is a structured definition: a scope's `statement`, `category`, `column`, `query` or set-algebra kinds ([0060](../decisions/0060-scopes.md) §4), and whatever later ADRs add. The pure crate that owns the definition generates the query body (for scopes, `scatter-scope`, [0060](../decisions/0060-scopes.md) §11); `triplespace-query::compile` wraps it into a `CompiledQuery`: a `SELECT DISTINCT ?item` with the dataset of [0059](../decisions/0059-query-service.md) §4, a `LIMIT` of the caller's bound plus one (so that truncation is detected), and `ORDER BY ?item` where the caller needs a stable prefix. The result is a set of subject IRIs and nothing else; the service maps each back to an entity or page ID through the IRI rules of [0015](../decisions/0015-record-format-and-partition-registry.md) §4–6 and drops any it cannot.

A `query`-kind definition carries user-written SPARQL. The compiler **wraps** it rather than trusting it: `SELECT DISTINCT ?item WHERE { { <user query> } } LIMIT n`, after checking that the user query is a `SELECT` that projects `?item`, has no `FROM`, no `SERVICE` and no update operation, and after the same prefix handling as [0059](../decisions/0059-query-service.md) §6. A definition that fails these checks is refused at save ([0060](../decisions/0060-scopes.md) §4).

Compiled queries run in the projection worker ([0013](../decisions/0013-postgres-storage.md) §7), never in a request, under `query.timeout` (default 30 s) like any query. They are what the scope projection of [0060](../decisions/0060-scopes.md) §5 calls.
