# 22. Crates and stack

This chapter covers how Triplespace is built: the four layers of the Cargo workspace and the rules that hold between them, the crate map that CI checks, which seams Scatterbase shares, the build order, naming, licensing and repository, the backend technology stack from toolchain to packaging, the frontend build and its tests, the two deployment profiles, and the component manifest that `Special:Version` reads. It assumes the log, records and `LogStore` of [01](01-log-and-records.md), the graph registry of [02](02-graphs-rdf-and-query.md) and the Postgres schemas, blob store, caches and search of [03](03-storage-caches-and-search.md); what each crate implements is described by the chapter of its subject, and this chapter says only which crate holds it. The web tier the site's crates make up is [20](20-web-tier.md), the wikitext subset and the renderers are [11](11-rendering-templates-and-modules.md), the registry files the crates embed are [23](23-configuration-and-registry.md), and `Special:Version` itself is [21](21-special-pages.md).

## 1. The four layers and the rules

*Sources: [0005](../decisions/0005-crate-organization.md) §1, §3, §4, §4.1, §4.2, §4.3, §4.4; [0007](../decisions/0007-actor-identity.md) §10; [0016](../decisions/0016-permissions-and-access-control.md) §8; [0023](../decisions/0023-moderation.md) §12; [0024](../decisions/0024-subsidiary-accounts.md) §12; [0025](../decisions/0025-oauth-server.md) §12; [0039](../decisions/0039-files-and-media.md) §22; [0040](../decisions/0040-instance-prerogatives.md) §10; [0041](../decisions/0041-content-models.md) §11; [0043](../decisions/0043-lua-modules.md) §14; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §9; [0057](../decisions/0057-web-tier.md) §12.*

### 1.1 Four layers

*Sources: [0005](../decisions/0005-crate-organization.md) §1.*

The workspace is organized in four layers. Dependencies point only downward.

1. **Substrate.** The log and its integrity, projections, providers, identity clusters, normalizers, actors, document pages, threads, and the Activity Streams profile. Knows nothing about Wikibase.
2. **Wikibase domain.** The data model, its RDF and JSON forms, reconciliation, change sets, shape detection, edit filters, property constraints, and MediaWiki's log model. Knows nothing about Triplespace's APIs or deployment.
3. **Ingest.** Jobs, and one adapter per provider, including the verified adapter for another Triplespace instance. The only crates that name a specific provider.
4. **Triplespace surfaces.** The Postgres schemas and projections, the compatibility APIs, accounts, notifications, federation, the OAuth server, caches, search and the binaries. The only layer specific to Triplespace.

Layers 1 and 2 are the shared crates. Layer 3 is shared in principle (Scatterbase's Phase 4 needs the Wikidata adapter) but is where I/O begins. Triplespace is the first product assembled from the lower three, and Scatterbase is expected to be the second.

### 1.2 Rules

*Sources: [0005](../decisions/0005-crate-organization.md) §3; [0043](../decisions/0043-lua-modules.md) §14; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §9; [0057](../decisions/0057-web-tier.md) §12.*

1. **Dependencies point only downward.** No `scatter-*` crate depends on a `triplespace-*` crate. No crate below the surfaces layer names Triplespace or a specific graph IRI, and only the adapters name a specific provider. CI checks the dependency graph against the table in §2.
2. **The core is pure.** The crates `crates.toml` marks `pure` (the *Rules* column of §2.1: the shared vocabulary, model, normalization, page, wikitext, thread, talk, filter, log-event, CSS, scope, task, ShEx, merge, file, extraction and title crates, every `scatter-wikibase-*` crate, and `triplespace-scribunto`) perform no I/O, use no async runtime, and depend on no tokio. `scatter-activitypub` signs and verifies; it never sends. Async appears only in `scatter-log-postgres`, `scatter-ingest` and the surfaces. `LogStore`'s methods return futures ([01](01-log-and-records.md)), which is a type signature, not a runtime: they are written as `impl Future + Send` in the trait (return-position `impl Trait`), `scatter-log` depends on `std::future` alone, and its file backend does synchronous I/O inside those futures and nothing else, so a runtime caller wraps a file-backend call in `spawn_blocking` and a synchronous caller drives it with any executor. `LogStore` carries an associated **unit-of-work type**, the transaction or handle within which an append and the rows written beside it commit together; `Backend::apply_in` in `scatter-projection` and `IngestStore` in `scatter-ingest` take that type rather than naming a database, which is what lets the write path append and project in one Postgres transaction while the in-memory and file backends stay runtime-free. `triplespace-scribunto` is pure (no I/O, no async runtime), but through `mlua` it links Lua's C code, so it cannot build for wasm and is not on rule 7's list ([0043](../decisions/0043-lua-modules.md) §14).
3. **Policy is data.** The graph and partition registry, the provider registry, the issuer registry, the namespace registry, the keyed-type registry, the role map, the provider order, reconciliation rules and normalizer overrides are values passed in by the caller. They are not enums, constants or globals. [0004](../decisions/0004-identity-clusters-and-equivalence.md) §9 records them in the log as configuration, in the `config` partition of [0015](../decisions/0015-record-format-and-partition-registry.md) §3; their defaults come from `docs/registry/` ([0015](../decisions/0015-record-format-and-partition-registry.md) §5), and [0013](../decisions/0013-postgres-storage.md) §5.5 projects their current state.
4. **Each hashed type has exactly one canonical encoding,** produced by one function in the crate that owns the type. No other code path computes a hash or signature preimage for it. The encodings are specified in [0006](../decisions/0006-log-integrity-and-erasure.md) §2.
5. **RDF terms and I/O use the `oxrdf` family** (`oxrdf`, `oxttl`, `oxrdfio`). Shared crates do not define their own term types.
6. **Cargo features select optional heavy dependencies only,** such as Oxigraph, a SIMD JSON parser, or the CommonMark renderer in `scatter-pages`. Features never switch product behavior.
7. **The crates `crates.toml` marks `wasm` must build for `wasm32-unknown-unknown`,** so that the statement UI ([0003](../decisions/0003-statement-ui.md)), the page editor's preview ([0008](../decisions/0008-namespaces-and-document-pages.md) §11), title normalization in the browser ([0009](../decisions/0009-keyed-entity-types-and-domain.md) §12) and the classifier audit over a Wikidata dump run the same code as the server. The map is the one wasm list; [0034](../decisions/0034-frontend-stack.md) §6 and `cargo xtask wasm` read it. **The rule closes over dependencies:** every transitive dependency of a listed crate builds for the target too, so a listed crate takes no crate, workspace or third-party, that does not, and adding one is refused by the CI build rather than discovered in the browser. CI builds that target, and builds `scatter-wasm` ([0034](../decisions/0034-frontend-stack.md) §6), the one crate that binds them for the browser ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §9), against a budget of 2 MB gzipped; the editor bundle is two lazily loaded modules, the statement editor's and the page editor's, so that neither editor pays for the other's code.
8. **Contracts are tested where they are implemented.** `scatter-wikibase-model` and `scatter-wikibase-rdf` use the files in `docs/samples/wikibase-1.43` and `docs/samples/wikidata` as golden fixtures. `scatter-log` and `scatter-log-postgres` share one conformance suite for `LogStore`.
9. **Schemas are contracts.** A change to what the `log` schema's records are needs a new payload or header version and a migration that keeps every existing header verifiable; a change to how they are stored that keeps every record's logical bytes ([0058](../decisions/0058-packed-record-storage.md) §1) needs only a migration; a change to `view` needs a migration or a rebuild. Migrations live with the crate that owns the schema.
10. **The site's crates reach no store.** `triplespace-ui`, `triplespace-client` and `triplespace-web` depend on no crate that opens a connection to Postgres, Valkey or OpenSearch on the instance's behalf, and on no API crate: they reach the instance only through its public API ([0057](../decisions/0057-web-tier.md) §1, §12). The response cache's Valkey client in `triplespace-client` holds public API responses only. CI checks the rule with the dependency graph; `cargo xtask deps` checks it ([0057](../decisions/0057-web-tier.md) §12).
11. **A payload type or config kind lives in the lowest crate whose product needs it.** `scatter-log` defines the `config` payload type and the kinds the log cannot start without, `scatter-pages` the page records; a `scatter:v0/*` payload type or a config kind that only Triplespace writes is declared by the Triplespace crate that writes it, never by `scatter-log` or `scatter-pages`, since a payload type is a value the log stores and the log need not know its shape. So `scatter:v0/mirrored-page` and the `page-repo` kind are `triplespace-repos`'s, `scatter:v0/derivation` is `triplespace-extraction`'s, and the `tenancy`, `template` and `provider-readers` kinds are `triplespace-accounts`'s; the `acl`, `membership`, `block`, `key`, `graph`, `recovery-key` and continuation kinds, which Scatterbase shares, stay in the substrate.

### 1.3 What Scatterbase shares

*Sources: [0005](../decisions/0005-crate-organization.md) §4, §4.1, §4.2, §4.3, §4.4; [0007](../decisions/0007-actor-identity.md) §10; [0016](../decisions/0016-permissions-and-access-control.md) §8; [0023](../decisions/0023-moderation.md) §12; [0024](../decisions/0024-subsidiary-accounts.md) §12; [0025](../decisions/0025-oauth-server.md) §12; [0039](../decisions/0039-files-and-media.md) §22; [0040](../decisions/0040-instance-prerogatives.md) §10; [0041](../decisions/0041-content-models.md) §11.*

Four seams in the substrate are shared with Scatterbase; each is described in full by the chapter of its subject, and this section says what of it Scatterbase takes.

- **Named graphs are registry entries.** `scatter-log` does not enumerate graphs; each deployment registers its own, and a registry entry records the graph IRI and its tenant, its kind, its ACL, its history policy, its integrity policy and its export policy. The two products register different sets, in the correspondence table of [02](02-graphs-rdf-and-query.md): `local`, `mirror/{provider}`, `metadata`, `log`, `pages`, `accounts`, `actors`, `config` and `resolved` on Triplespace's side against `claim`, `foreign`, `meta`, `mutable` and `server` on Scatterbase's. The table shows a correspondence, not an identity: Triplespace's `metadata` graph is derived and Scatterbase's `meta` graph is asserted, so they are not interchangeable ([0005](../decisions/0005-crate-organization.md) §4.1).
- **Log partitions carry their own policy.** Each partition has a history policy (`full` or `latest`), an integrity policy (`logged` or `hashed`), a segment size 2^k and a hash function fixed at creation ([01](01-log-and-records.md)). Scatterbase's claim partition keeps everything and is `logged`; Triplespace's mirror partitions compact. `scatter-log` supports both from its first release, since adding either one later would change the on-disk format. A partition may be backed by files or by Postgres; both implement `LogStore`, whose methods return `Send` futures and whose associated unit-of-work type is what a projection backend and an ingester commit within (§1.2 rule 2), and export bundles use the file format whichever backend is live ([0005](../decisions/0005-crate-organization.md) §4.2).
- **A record is a header plus a payload.** The header is what the Merkle tree commits to and is never erased; the body is a list of salted parts, each erasable on its own ([01](01-log-and-records.md)). In Triplespace the parts are content, comment and attestation, and a payload type may declare more; in Scatterbase they are the claim, its attribution, the client and server signatures and, later, prior-use acknowledgments. Who attests a change is kept apart from what the change is: everything above `scatter-log` reads payloads and never depends on the attestation format ([0005](../decisions/0005-crate-organization.md) §4.3). The instance key is the only signer of checkpoints and manifests, and an actor may additionally sign its own record with the client `signature` of [0015](../decisions/0015-record-format-and-partition-registry.md) §1.
- **Storage contracts.** `scatter-quadstore` (`lookup(s, p, o, g)` with wildcards, `apply(deleted, inserted)`) is the storage contract from Scatterbase's plan: Scatterbase's drivers implement it and it is Scatterbase's serving store, while in Triplespace it is a consumer of `view.rdf_delta` that nothing reads but the query service ([03](03-storage-caches-and-search.md)). Triplespace's serving store, the `view` schema, is not a shared seam: Scatterbase has no need of it, and its tables are Wikibase-shaped by design. `LogStore` in `scatter-log` is the third contract, and the one both products share fully. Extra access paths, such as claim retrieval or chain traversal, are separate traits in the crates that need them; they are not added to `scatter-quadstore` or `LogStore` ([0005](../decisions/0005-crate-organization.md) §4.4).

Later ADRs say what else is Scatterbase's:

- **Actors.** Adapters turn upstream actor fields into actor keys and actor records; `scatter-actors` is pure and shared, and `triplespace-accounts` is the only crate that reads the accounts partition ([0007](../decisions/0007-actor-identity.md) §10).
- **Permissions.** Scatterbase inherits [0016](../decisions/0016-permissions-and-access-control.md) §1, §3 and §4 whole: the vocabulary, groups and memberships as records, blocks as the only negative rule, ACLs by target with conjunctive evaluation over enclosures, and the single-user fallback ([09](09-security-and-moderation.md)). Its five open points are answered as follows. Matching is exact on the target plus its enclosures. Inheritance is enclosure only. Deny has no precedence because there is no deny; a block is the only subtraction and it applies before ACLs are consulted. A property-specific rule is the `property` target kind ([0023](../decisions/0023-moderation.md) §2). ACL changes are records, in the `config` partition for graphs and in `log` for moderation ([0023](../decisions/0023-moderation.md) §3), with checkpoints and manifests signed by the instance key like every record's; Scatterbase may make them client-signed claims in its `server` graph without changing their shape ([0016](../decisions/0016-permissions-and-access-control.md) §8).
- **Moderation.** Scatterbase inherits [0023](../decisions/0023-moderation.md) §1–3 and §5 whole. Read ACLs suit it better than deletion would: a signed claim's content stays in the log and verifiable, and only who may read it changes, which is the answer its decision record wanted for `view_deleted` and for restricting "individual claims". The `record` target with `parts` is the claim-level restriction it reserved. Patrolling is Triplespace's and is not shared ([0023](../decisions/0023-moderation.md) §12).
- **Subsidiaries and keys.** Scatterbase's decision record wants client signing keys bound to actors. A subsidiary is the actor such a key binds to: `private.api_key` is the *authentication* half, and the signing keys of [0024](../decisions/0024-subsidiary-accounts.md) §4, registered as `key` records whose signature travels in the record ([0015](../decisions/0015-record-format-and-partition-registry.md) §1), are the *attestation* half; Scatterbase's client signature is the same field shape in its own part layout. The grants table and the operator attribute are Scatterbase's to reuse; rate limits are Triplespace's ([0024](../decisions/0024-subsidiary-accounts.md) §12).
- **OAuth.** Nothing of the OAuth server is Scatterbase's. A crawler has no users to delegate for. Consumer records and grants are ordinary `config` and registry data and need no Scatterbase reader ([0025](../decisions/0025-oauth-server.md) §12).
- **Blobs.** Scatterbase gets `scatter-blob` whole: its blobs and Triplespace's files are the same objects under the same layout, so a Scatterbase instance can use a directory or a bucket exactly as Triplespace does. The removal vocabulary of [0039](../decisions/0039-files-and-media.md) §8 is available to it; whether a signed claim's blob may be expunged is its policy, as [0006](../decisions/0006-log-integrity-and-erasure.md) §7 left erasure of claims. File pages, thumbnails and repositories are Triplespace's ([0039](../decisions/0039-files-and-media.md) §22).
- **Instance attestation.** Scatterbase has no tenants, so it has no guests. It inherits the instance attestation as a shape: a Scatterbase server that writes a claim on its own authority (a server-side correction, a takedown of a claim) can attest it the same way, with its server key in place of the instance key. Whether it does is Scatterbase policy ([0040](../decisions/0040-instance-prerogatives.md) §10).
- **Content models.** In Scatterbase's terms, the stored bytes are the blob and the content model is the view ([0008](../decisions/0008-namespaces-and-document-pages.md) §5). Scatterbase has no namespaces, but the registry and the trait are in a substrate crate, and the source, serialization and diff parts apply to any claim store that serves pages. Whether Scatterbase uses them is its own decision ([0041](../decisions/0041-content-models.md) §11).

## 2. The crate map

*Sources: [0005](../decisions/0005-crate-organization.md) §2; [0006](../decisions/0006-log-integrity-and-erasure.md) §10; [0007](../decisions/0007-actor-identity.md) §10; [0008](../decisions/0008-namespaces-and-document-pages.md) §11; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §12; [0011](../decisions/0011-logs.md) §10; [0012](../decisions/0012-api-requirements.md) §9; [0013](../decisions/0013-postgres-storage.md) §10; [0014](../decisions/0014-caches-and-search.md) §9; [0015](../decisions/0015-record-format-and-partition-registry.md) §7; [0016](../decisions/0016-permissions-and-access-control.md) §9; [0017](../decisions/0017-entity-id-grammar.md) §7; [0018](../decisions/0018-tenants.md) §12; [0019](../decisions/0019-discussions.md) §13; [0020](../decisions/0020-change-feeds.md) §8; [0021](../decisions/0021-notifications.md) §10; [0022](../decisions/0022-federation.md) §13; [0023](../decisions/0023-moderation.md) §13; [0024](../decisions/0024-subsidiary-accounts.md) §13; [0025](../decisions/0025-oauth-server.md) §11; [0026](../decisions/0026-sitelinks.md) §10; [0027](../decisions/0027-preferences-and-portability.md) §9; [0028](../decisions/0028-tenancy-policy.md) §14; [0029](../decisions/0029-resolver-namespaces.md) §9; [0030](../decisions/0030-edit-filters.md) §13; [0031](../decisions/0031-property-constraints.md) §9; [0032](../decisions/0032-sparql-update-stream.md) §9; [0035](../decisions/0035-adopting-a-wikibase.md) §8; [0036](../decisions/0036-openstreetmap-providers.md) §7; [0037](../decisions/0037-gdelt-provider.md) §8; [0038](../decisions/0038-page-metadata-and-categories.md) §15; [0039](../decisions/0039-files-and-media.md) §23; [0040](../decisions/0040-instance-prerogatives.md) §11; [0041](../decisions/0041-content-models.md) §12; [0042](../decisions/0042-template-expansion-and-parsoid.md) §19; [0043](../decisions/0043-lua-modules.md) §17; [0044](../decisions/0044-tenant-relative-ids.md) §6; [0045](../decisions/0045-table-content-model.md) §13; [0046](../decisions/0046-primary-tenant.md) §10; [0047](../decisions/0047-special-pages.md) §14; [0048](../decisions/0048-notation.md) §8; [0049](../decisions/0049-boards.md) §15; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §10; [0053](../decisions/0053-mirrored-pages.md) §12; [0054](../decisions/0054-forking-a-mirrored-page.md) §12; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §9; [0056](../decisions/0056-security-model.md) §16; [0057](../decisions/0057-web-tier.md) §12; [0058](../decisions/0058-packed-record-storage.md) §12; [0059](../decisions/0059-query-service.md) §8; [0060](../decisions/0060-scopes.md) §11; [0061](../decisions/0061-sprints-and-tasks.md) §12; [0062](../decisions/0062-workspaces.md) §10; [0063](../decisions/0063-query-namespace.md) §8; [0064](../decisions/0064-entityschema-and-validation.md) §9; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §5; [0066](../decisions/0066-lexemes.md) §10; [0067](../decisions/0067-proposals.md) §9; [0068](../decisions/0068-merging-with-upstream.md) §7; [0069](../decisions/0069-synchronized-talk-pages.md) §12; [0070](../decisions/0070-shallow-entity-mirroring.md) §11; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §14; [0072](../decisions/0072-template-mappings.md) §7; [0073](../decisions/0073-lines-links-and-url-patterns.md) §7; [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §9; [0075](../decisions/0075-mcp-server.md) §7; [0076](../decisions/0076-dataset-publication.md) §7; [0078](../decisions/0078-entity-sources.md) §12; [0082](../decisions/0082-source-form-and-the-shared-view.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §8; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §6; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §12; [0091](../decisions/0091-one-wikibase-adapter.md) §5.*

The table of [0005](../decisions/0005-crate-organization.md) §2 is the crate table that CI checks (`cargo xtask deps`, [0057](../decisions/0057-web-tier.md) §12). Every later ADR that adds a crate or changes what one holds records the change in its own Crates section, and the table carries it; §2.2 lists those sections and the crates each introduced, and the provenance line above names them all.

### 2.1 The table

*Sources: [0005](../decisions/0005-crate-organization.md) §2; [0006](../decisions/0006-log-integrity-and-erasure.md) §10; [0007](../decisions/0007-actor-identity.md) §10; [0008](../decisions/0008-namespaces-and-document-pages.md) §11; [0009](../decisions/0009-keyed-entity-types-and-domain.md) §12; [0011](../decisions/0011-logs.md) §10; [0012](../decisions/0012-api-requirements.md) §9; [0013](../decisions/0013-postgres-storage.md) §10; [0014](../decisions/0014-caches-and-search.md) §9; [0015](../decisions/0015-record-format-and-partition-registry.md) §7; [0016](../decisions/0016-permissions-and-access-control.md) §9; [0017](../decisions/0017-entity-id-grammar.md) §7; [0018](../decisions/0018-tenants.md) §12; [0019](../decisions/0019-discussions.md) §13; [0020](../decisions/0020-change-feeds.md) §8; [0021](../decisions/0021-notifications.md) §10; [0022](../decisions/0022-federation.md) §13; [0023](../decisions/0023-moderation.md) §13; [0024](../decisions/0024-subsidiary-accounts.md) §13; [0025](../decisions/0025-oauth-server.md) §11; [0026](../decisions/0026-sitelinks.md) §10; [0027](../decisions/0027-preferences-and-portability.md) §9; [0028](../decisions/0028-tenancy-policy.md) §14; [0029](../decisions/0029-resolver-namespaces.md) §9; [0030](../decisions/0030-edit-filters.md) §13; [0031](../decisions/0031-property-constraints.md) §9; [0032](../decisions/0032-sparql-update-stream.md) §9; [0035](../decisions/0035-adopting-a-wikibase.md) §8; [0036](../decisions/0036-openstreetmap-providers.md) §7; [0037](../decisions/0037-gdelt-provider.md) §8; [0038](../decisions/0038-page-metadata-and-categories.md) §15; [0039](../decisions/0039-files-and-media.md) §23; [0040](../decisions/0040-instance-prerogatives.md) §11; [0041](../decisions/0041-content-models.md) §12; [0042](../decisions/0042-template-expansion-and-parsoid.md) §19; [0043](../decisions/0043-lua-modules.md) §17; [0044](../decisions/0044-tenant-relative-ids.md) §6; [0045](../decisions/0045-table-content-model.md) §13; [0046](../decisions/0046-primary-tenant.md) §10; [0047](../decisions/0047-special-pages.md) §14; [0048](../decisions/0048-notation.md) §8; [0049](../decisions/0049-boards.md) §15; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §10; [0053](../decisions/0053-mirrored-pages.md) §12; [0054](../decisions/0054-forking-a-mirrored-page.md) §12; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §9; [0056](../decisions/0056-security-model.md) §16; [0057](../decisions/0057-web-tier.md) §12; [0058](../decisions/0058-packed-record-storage.md) §12; [0059](../decisions/0059-query-service.md) §8; [0060](../decisions/0060-scopes.md) §11; [0061](../decisions/0061-sprints-and-tasks.md) §12; [0062](../decisions/0062-workspaces.md) §10; [0063](../decisions/0063-query-namespace.md) §8; [0064](../decisions/0064-entityschema-and-validation.md) §9; [0065](../decisions/0065-mediainfo-captions-and-commons.md) §5; [0066](../decisions/0066-lexemes.md) §10; [0067](../decisions/0067-proposals.md) §9; [0068](../decisions/0068-merging-with-upstream.md) §7; [0069](../decisions/0069-synchronized-talk-pages.md) §12; [0070](../decisions/0070-shallow-entity-mirroring.md) §11; [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §14; [0072](../decisions/0072-template-mappings.md) §7; [0073](../decisions/0073-lines-links-and-url-patterns.md) §7; [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §9; [0075](../decisions/0075-mcp-server.md) §7; [0076](../decisions/0076-dataset-publication.md) §7; [0078](../decisions/0078-entity-sources.md) §12; [0079](../decisions/0079-derived-issuer-codes.md) §8; [0080](../decisions/0080-tenants-as-entity-sources.md) §9; [0081](../decisions/0081-recovery-keys-and-continuations.md) §10; [0082](../decisions/0082-source-form-and-the-shared-view.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §8; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §6; [0085](../decisions/0085-settings-and-preferences.md) §9; [0086](../decisions/0086-theming.md) §7; [0087](../decisions/0087-account-registration-and-challenges.md) §10; [0088](../decisions/0088-external-identity-providers.md) §10; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §17; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §12; [0091](../decisions/0091-one-wikibase-adapter.md) §5; [0092](../decisions/0092-following-recent-changes.md) §8; [0093](../decisions/0093-fallback-typefaces.md) §4; [0094](../decisions/0094-accessibility-alternates.md) §7; [0095](../decisions/0095-the-interface-namespace.md) §6; [0096](../decisions/0096-adopting-any-mediawiki.md) §6; [0097](../decisions/0097-versioning-and-skins.md) §17.*
The crate map is [`../registry/crates.toml`](../registry/crates.toml): one entry per crate with its layer, what it holds, what it may depend on, and whether it is pure (§1.2 rule 2) or builds for `wasm32` (§1.2 rule 7). `cargo xtask deps` checks the workspace against that file. The table below is generated from it by `crates.py --write` (one line per crate); [`CRATES.md`](CRATES.md), generated likewise, holds every entry in full, with the citations. Neither is edited by hand ([24](24-documentation.md) §11).

<!-- crates.py: table -->
| Layer | Crate | What it holds | Depends on | Rules |
|---|---|---|---|---|
| Substrate | [`scatter-vocab`](CRATES.md#scatter-vocab) | IRI constants for the scatter: vocabulary and the external vocabularies it reuses | `oxrdf` | pure |
|  | [`scatter-log`](CRATES.md#scatter-log) | An append-only log split into partitions and generic over the record type | `oxrdf` |  |
|  | [`scatter-merge`](CRATES.md#scatter-merge) | Line-based three-way merge of text with hunk classification, and the unified diff writer | — | pure, wasm |
|  | [`scatter-challenge`](CRATES.md#scatter-challenge) | The proof-of-work challenge of 0087 §3: the challenge string, its MAC given a key and a binding, the SHA-256 work… | — |  |
|  | [`scatter-log-postgres`](CRATES.md#scatter-log-postgres) | `LogStore` over the `log` schema of 0013 §2, and that schema's migrations | `scatter-log`, `scatter-integrity`, `tokio-postgres`, `zstd`, `hmac` |  |
|  | [`scatter-blob`](CRATES.md#scatter-blob) | The blob store over `object_store`: the `fs` and `s3` backends, storage scopes and the content-addressed key layout,… | `object_store` |  |
|  | [`scatter-integrity`](CRATES.md#scatter-integrity) | Signed checkpoints, inclusion and consistency proofs, erasure bookkeeping, export bundles and `verify`, checked per part | `scatter-log`, `scatter-blob` |  |
|  | [`scatter-quadstore`](CRATES.md#scatter-quadstore) | The quad-store trait: lookup with wildcards, deltas, and its two backends | `oxrdf` |  |
|  | [`scatter-projection`](CRATES.md#scatter-projection) | The projection trait, including the transactional variant `Backend::apply_in` that applies inside the appending… | `scatter-log` |  |
|  | [`scatter-providers`](CRATES.md#scatter-providers) | Provider codes, slugs and numbers, type codes, canonical IRI templates, and ID rewriting such as `Q123` ↔ `WDQ123` | `oxrdf` | pure |
|  | [`scatter-identity`](CRATES.md#scatter-identity) | Identity clusters: one member per namespace, link tiers, `different-from`, held conflicts | — | pure |
|  | [`scatter-normalize`](CRATES.md#scatter-normalize) | Normalized value keys by data type and by property, and the URL normalizer | `oxrdf`, `idna` | pure, wasm |
|  | [`scatter-actors`](CRATES.md#scatter-actors) | The issuer registry, actor keys, IRI templates, the surrogate allocator trait and the account-link rules | `oxrdf`, `ed25519-dalek` | pure |
|  | [`scatter-pages`](CRATES.md#scatter-pages) | Page records and their operations, page IDs, the content-model trait, and validation and rendering for `json`,… | `scatter-css` | pure, wasm |
|  | [`scatter-titles`](CRATES.md#scatter-titles) | The namespace registry, embedding `docs/registry/namespaces.toml`, and the special-page registry, embedding… | `scatter-pages`, `scatter-normalize` | pure, wasm |
|  | [`scatter-wikitext`](CRATES.md#scatter-wikitext) | Parsing and rendering the wikitext subset of 0008 §8 | `scatter-css` | pure, wasm |
|  | [`scatter-wikitext-expand`](CRATES.md#scatter-wikitext-expand) | MediaWiki's preprocessor and frame expansion: the preprocessor tree, frames, strip markers, pre-save `subst:`, core… | ICU4X crates ([0033](../decisions/0033-backend-stack.md) §9.3) | pure |
|  | [`scatter-css`](CRATES.md#scatter-css) | CSS Syntax Level 3 parsing, the TemplateStyles-policy sanitizer (`validate` and `sanitize`), selector scoping under… | `cssparser`, ([0033](../decisions/0033-backend-stack.md) §16) | pure, wasm |
|  | [`scatter-threads`](CRATES.md#scatter-threads) | The `scatter:v0/thread` payload type and its four parts | `scatter-pages` | pure |
|  | [`scatter-talk`](CRATES.md#scatter-talk) | The wikitext talk-page splitter: sections, comments, signatures, depth and parents, heading, closing and collapsing… | `scatter-wikitext` | pure |
|  | [`scatter-talk-import`](CRATES.md#scatter-talk-import) | Planning the thread records of a converted talk page, for every import job: `scatter-talk`'s conversion mapped onto… | `scatter-talk`, `scatter-threads`, `sha2` | pure |
|  | [`scatter-files`](CRATES.md#scatter-files) | The upload and expunge payload types | `image`, `resvg` | pure |
|  | [`scatter-activitypub`](CRATES.md#scatter-activitypub) | The Activity Streams 2.0 profile of 0019 §10 and 0021: object building, actor, WebFinger and NodeInfo documents,… | `serde_json`, a signature crate (§6) | pure |
| Wikibase | [`scatter-wikibase-model`](CRATES.md#scatter-wikibase-model) | Entities, terms, sitelinks, statements, snaks, data values and data types | `scatter-providers`, `scatter-normalize`, `scatter-pages` | pure |
|  | [`scatter-wikibase-rdf`](CRATES.md#scatter-wikibase-rdf) | RDF format 1.0.0, the truthy projection, both dump flavors, and EDTF | `scatter-wikibase-model`, `scatter-vocab`, `scatter-providers` | pure |
|  | [`scatter-wikibase-resolve`](CRATES.md#scatter-wikibase-resolve) | Reconciliation across source graphs and statement fusion | `scatter-wikibase-model`, `scatter-identity`, `scatter-normalize` | pure |
|  | [`scatter-wikibase-changeset`](CRATES.md#scatter-wikibase-changeset) | The change-set operations and the NDJSON wire format | `scatter-wikibase-model`, `scatter-providers` | pure |
|  | [`scatter-wikibase-constraints`](CRATES.md#scatter-wikibase-constraints) | Parsing constraints from a property's statements by role | `scatter-wikibase-model`, `scatter-normalize` | pure |
|  | [`scatter-wikibase-shape`](CRATES.md#scatter-wikibase-shape) | Shape detection and the role map | `scatter-wikibase-model`, `scatter-pages`, `scatter-scope` | pure, wasm |
|  | [`scatter-extract`](CRATES.md#scatter-extract) | Template mappings with conditions, parsers, value maps, split and trim | `scatter-wikitext`, `scatter-wikibase-model`, `scatter-normalize`, `regex` | pure, wasm |
|  | [`scatter-scope`](CRATES.md#scatter-scope) | The `triplespace-scope` content model: definition types, schema validation, canonicalization, cycle detection given… | `scatter-wikibase-model`, `scatter-pages` | pure, wasm |
|  | [`scatter-shex`](CRATES.md#scatter-shex) | ShExC parsing, the supported ShEx 2.1 subset and its refusals, validation over a `TripleSource` with depth and… | `scatter-vocab`, `oxrdf` | pure |
|  | [`scatter-tasks`](CRATES.md#scatter-tasks) | The `triplespace-sprint` definition: schema, validation, canonicalization | `scatter-wikibase-model`, `scatter-pages`, `scatter-scope` | pure, wasm |
|  | [`scatter-filter`](CRATES.md#scatter-filter) | The `filter` and `filter-hit` payload types | `scatter-wikibase-changeset`, `scatter-pages`, `scatter-threads`, `scatter-normalize`, `cel`, ([0033](../decisions/0033-backend-stack.md) §9.4) | pure |
|  | [`scatter-mwlog`](CRATES.md#scatter-mwlog) | The `logevent` payload type, MediaWiki log types and actions, typed parameters, visibility bits, both parameter… | `scatter-vocab` | pure |
|  | [`scatter-wasm`](CRATES.md#scatter-wasm) | `wasm-bindgen` bindings for the browser over the pure crates editors need: the shape classifier,… | `scatter-wikibase-shape`, `scatter-normalize`, `scatter-titles`, `scatter-wikitext`, `scatter-pages`, `scatter-css`, `wasm-bindgen` | wasm |
| Ingest | [`scatter-ingest`](CRATES.md#scatter-ingest) | Jobs, version cursors, `upsert` and `snapshot` modes with the tombstone threshold, temporary refs, match keys, ID… | `scatter-log`, `scatter-projection`, `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-threads`, `scatter-talk` |  |
|  | [`scatter-adapter-wikibase`](CRATES.md#scatter-adapter-wikibase) | The adapter for every MediaWiki+Wikibase provider, parameterised by its registry entry: Wikidata's entity dumps and… | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize`, `scatter-actors`, `scatter-mwlog`, `scatter-adapter-mediawiki` |  |
|  | [`scatter-adapter-openalex`](CRATES.md#scatter-adapter-openalex) | The OpenAlex adapter, with its field mapping onto Wikidata properties where one fits and otherwise onto OpenAlex's… | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` |  |
|  | [`scatter-adapter-musicbrainz`](CRATES.md#scatter-adapter-musicbrainz) | The MusicBrainz adapter: MBIDs under the `uuid` grammar, URL entities as statements | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers` |  |
|  | [`scatter-adapter-triplespace`](CRATES.md#scatter-adapter-triplespace) | Reading a Triplespace provider tenant on another instance: bootstrap from its local-graph source dump, then its… | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-integrity` |  |
|  | [`scatter-adapter-openstreetmap`](CRATES.md#scatter-adapter-openstreetmap) | The OpenStreetMap map-data adapter: nodes, ways, relations, tags as statements | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize` |  |
|  | [`scatter-adapter-gdelt`](CRATES.md#scatter-adapter-gdelt) | The GDELT 2.0 adapter: events, mentions and GKG files by 15-minute slot, with row filters | `scatter-wikibase-changeset`, `scatter-wikibase-model`, `scatter-providers`, `scatter-normalize` |  |
|  | [`scatter-adapter-mediawiki`](CRATES.md#scatter-adapter-mediawiki) | Reading a MediaWiki wiki's pages: page bundles from the REST and Action APIs and the Wikimedia Enterprise API, the… | `scatter-providers`, `scatter-mwlog`, `scatter-pages`, `scatter-threads` |  |
| Triplespace | [`triplespace-db`](CRATES.md#triplespace-db) | The `view`, `private` and `ops` schemas, their migrations, roles and grants, connection pools, and replica routing… | `scatter-log-postgres` |  |
|  | [`triplespace-projections`](CRATES.md#triplespace-projections) | Every `view` projection, each registered with its class and its place in the partial order of §3.1, in the order of… | `triplespace-db`, `scatter-projection`, `scatter-filter`, `scatter-wikibase-constraints`, any substrate or Wikibase crate, any layer-1 or layer-2 crate (including  and ) |  |
|  | [`triplespace-rdf`](CRATES.md#triplespace-rdf) | The streaming RDF projection: resolved view (the main graph), source graphs and metadata graph as N-Quads, for the… | `triplespace-db`, `scatter-wikibase-rdf`, `scatter-vocab`, `scatter-mwlog`, `scatter-quadstore` |  |
|  | [`triplespace-titles`](CRATES.md#triplespace-titles) | The DB-backed title resolver over `scatter-titles`'s registries and normalization | `triplespace-db`, `scatter-titles`, `scatter-pages`, `scatter-wikibase-model`, `scatter-identity`, `scatter-normalize` |  |
|  | [`triplespace-accounts`](CRATES.md#triplespace-accounts) | Bindings, OAuth login, the linking flow, and the operational store for secrets | `triplespace-db`, `scatter-actors`, `scatter-log`, `scatter-challenge` |  |
|  | [`triplespace-notify`](CRATES.md#triplespace-notify) | The addressing projection, inbox storage and read state, bundling, the web, email and fediverse channels and the… | `triplespace-db`, `triplespace-projections`, `triplespace-accounts`, `scatter-projection`, `scatter-log`, `scatter-activitypub` |  |
|  | [`triplespace-federation`](CRATES.md#triplespace-federation) | Per-actor ActivityPub keys and followers in `private` (`ap_key`, `ap_follower`), Person and Group actor endpoints,… | `triplespace-db`, `triplespace-projections`, `triplespace-accounts`, `scatter-activitypub`, `scatter-actors` |  |
|  | [`triplespace-oauth`](CRATES.md#triplespace-oauth) | The OAuth 2.0 authorization server: metadata, authorize, token, device, revoke and identify endpoints at native and… | `triplespace-accounts`, `triplespace-cache`, `scatter-actors` |  |
|  | [`triplespace-files`](CRATES.md#triplespace-files) | The upload pipeline and stash | `triplespace-db`, `triplespace-projections`, `triplespace-cache`, `triplespace-upstream`, `scatter-blob`, `scatter-files`, `scatter-actors` |  |
|  | [`triplespace-upstream`](CRATES.md#triplespace-upstream) | The live upstream client: cache, rate limits and the error mapping of 0012 §6 | `scatter-adapter-wikibase`, `scatter-mwlog`, `triplespace-cache`, `triplespace-db`, `triplespace-accounts` |  |
|  | [`triplespace-publish`](CRATES.md#triplespace-publish) | Publications: rows from scope members and property paths, transitive paths, escaping and deterministic text,… | `triplespace-upstream`, `triplespace-query`, `triplespace-db` |  |
|  | [`triplespace-render`](CRATES.md#triplespace-render) | The render pipeline: `ExpandHost` and the Lua host over the serving model, renderer choice, the Parsoid client and… | `triplespace-db`, `triplespace-projections`, `triplespace-titles`, `triplespace-cache`, `triplespace-upstream`, `scatter-wikitext`, `scatter-wikitext-expand`, `scatter-pages`, `triplespace-scribunto` |  |
|  | [`triplespace-repos`](CRATES.md#triplespace-repos) | Page repositories: the `page-repo` config kind, one per provider at instance scope, and the five-part… | `scatter-talk`, `scatter-adapter-mediawiki`, `triplespace-upstream`, `triplespace-files`, `triplespace-render`, `triplespace-cache`, `triplespace-search`, `triplespace-projections` |  |
|  | [`triplespace-extraction`](CRATES.md#triplespace-extraction) | The derivation payload type and the extraction jobs | `scatter-extract`, `scatter-wikibase-changeset`, `triplespace-db`, `triplespace-repos` |  |
|  | [`triplespace-query`](CRATES.md#triplespace-query) | The `QueryService` trait | `triplespace-db`, `scatter-quadstore`, `oxigraph`, `scatter-wikibase-rdf`, `scatter-vocab`, `spargebra`, `reqwest`, (feature ) |  |
|  | [`triplespace-mcp`](CRATES.md#triplespace-mcp) | The MCP server: the Streamable HTTP transport, initialization with instructions, the read-only tools with their… | `triplespace-query`, `triplespace-search`, `triplespace-db` |  |
|  | [`triplespace-scribunto`](CRATES.md#triplespace-scribunto) | Lua 5.1 through `mlua` in Scribunto's sandbox | `scatter-wikitext-expand`, `scatter-wikibase-model`, `scatter-providers`, `mlua` |  |
|  | [`triplespace-cache`](CRATES.md#triplespace-cache) | Key construction, the in-process and Valkey layers, single flight, generations, HTTP cache headers and tag purges | `triplespace-db` |  |
|  | [`triplespace-search`](CRATES.md#triplespace-search) | Index mappings and analysis settings, the indexer projection, query builders, and the Postgres fallback | `triplespace-db`, `triplespace-projections`, `scatter-wikibase-model` |  |
|  | [`triplespace-ui`](CRATES.md#triplespace-ui) | Server rendering through skins | `triplespace-client`, `scatter-wikibase-shape`, `scatter-wikibase-model`, `scatter-normalize`, `scatter-providers`, `scatter-titles`, `scatter-pages`, `minijinja`, `semver`, `html5ever` |  |
|  | [`triplespace-client`](CRATES.md#triplespace-client) | Typed requests and responses for the Action API modules and REST routes the site uses | `reqwest`, `tower`, `moka`, `redis`, `scatter-challenge` |  |
|  | [`triplespace-web`](CRATES.md#triplespace-web) | The stateless web tier: a binary that serves the site through `triplespace-ui` over the HTTP transport, with the… | `triplespace-ui`, `triplespace-client`, `rust-embed` |  |
|  | [`triplespace-api-action`, `triplespace-api-rest`, `triplespace-api-ingest`](CRATES.md#triplespace-api-action) | The Action API, the REST API with its OpenAPI contract, and the bulk-job ingest API | any crate in the same or a lower layer, Any lower crate |  |
|  | [`triplespace-server`, `triplespace-cli`](CRATES.md#triplespace-server) | The server and the CLI binaries | any crate in the same or a lower layer, Any crate |  |

Crates named by earlier ADRs that no longer exist as separate crates: `scatter-graphs` (in `scatter-log`), `scatter-markdown` (in `scatter-pages`), `scatter-keyed` (in `scatter-normalize`), `triplespace-activity` (in `triplespace-projections`), `triplespace-revmeta` (in `triplespace-rdf`), `scatter-adapter-wikidata` (renamed `scatter-adapter-wikibase`, [0091](../decisions/0091-one-wikibase-adapter.md) §5), `scatter-adapter-internetdomains` (in `scatter-adapter-wikibase`, [0091](../decisions/0091-one-wikibase-adapter.md) §5). A reference to one of them in an ADR reads as a reference to its new home.
<!-- /crates.py -->

Adapters do not depend on `scatter-log` directly. An adapter turns upstream records into change sets; where those change sets go is the ingester's concern. The one exception is transitive: `scatter-adapter-triplespace` depends on `scatter-integrity`, and so on `scatter-log`'s header and proof types, because it verifies a remote provider's proofs ([0022](../decisions/0022-federation.md) §2). It still never appends.

Two rows say the old thing. `scatter-wikibase-rdf` emits no schema entity node: entity schemas have no RDF, and only the `TripleSource` over a rendered entity is the crate's part of validation ([02](02-graphs-rdf-and-query.md)).

Captions on `mediainfo` entities are emitted on the page node `{base}/page/{page ID}`; there is no `M` node.

### 2.2 How later ADRs amended the map

*Sources: [0005](../decisions/0005-crate-organization.md) §2.*

The map carries its own history: every entry in `crates.toml` cites the ADR sections that added or changed it, [`CRATES.md`](CRATES.md) shows them beside the text, and each of those ADRs' Amendment logs holds what it replaced. The narrative that stood here, listing which ADR introduced which crate, is [0005](../decisions/0005-crate-organization.md) A112's Replaced text.

### 2.3 The scanner and the extractor are separate crates

*Sources: [0072](../decisions/0072-template-mappings.md) §7; [0073](../decisions/0073-lines-links-and-url-patterns.md) §7.*

Recognizing wikitext structure and mapping it to statements are separate, and neither depends on Triplespace. This is what lets the Wikipedia Citations Database use them later:

- **`scatter-wikitext`** gains a **scan** API over its parse tree: template calls with normalized names, parameters (raw and plain), `in_ref` and source positions; and, for [0073](../decisions/0073-lines-links-and-url-patterns.md), lines (list items with depth and parent, table rows with header cells and column names, headings and pseudo-headings), external links and bare URLs. It needs no repository, no network and no log; a redirect map and the namespace case rule are passed in. It builds for `wasm32`, as it does already.
- **`scatter-extract`** (new, in layer 2, since it produces `scatter-wikibase-model`'s types) evaluates mappings, parsers and value maps over scanned calls, and produces subjects with match keys and statements in `scatter-wikibase-model`'s types; it holds the line rules, the compiled URL match-pattern set with PCRE translation and the unsupported-pattern report, identifiers from URLs, and heading and column facets. It is pure, builds for `wasm32`, and takes its configuration as data, so a caller outside Triplespace (a WCD pipeline mapping `{{cite web|doi=…}}` to a DOI) can use it with its own mappings and no Triplespace types beyond the Wikibase model. `scatter-extract` takes patterns as data, so a caller outside Triplespace can load Wikidata's patterns for DOI, ISBN, PubMed and the rest and map citation URLs to identifiers with the same code.

`triplespace-extraction` holds the `templates` and `lines` extractors over `scatter-extract`, the `template-mapping` and `value-map` config kinds, unmapped-string collection, loading patterns from properties under the two roles and the unmatched-host report; `scatter-normalize` removes configured query parameters, for matching only; `triplespace-cli` has `extraction draft` ([13](13-mirrored-pages.md)).

## 3. Build order, naming, licensing and repository

*Sources: [0005](../decisions/0005-crate-organization.md) §6, §7; [0033](../decisions/0033-backend-stack.md) §1, §16; [0035](../decisions/0035-adopting-a-wikibase.md) §8; [0043](../decisions/0043-lua-modules.md) §14; [0057](../decisions/0057-web-tier.md) §12.*

### 3.1 Build order

*Sources: [0005](../decisions/0005-crate-organization.md) §7; [0035](../decisions/0035-adopting-a-wikibase.md) §8; [0057](../decisions/0057-web-tier.md) §12; [0083](../decisions/0083-write-path-in-three-tiers.md) §8; [0091](../decisions/0091-one-wikibase-adapter.md) §5.*

The order is a dependency order for the workspace, and a build need not take every crate or every projection. **A milestone names its subset.** The projection order is a partial order over the projections a build registers: a build that registers none of the page projections has no `ops.projection_state` row for them and nothing waits on them, and a later projection is added by registering it with its class and the projections it reads after ([0083](../decisions/0083-write-path-in-three-tiers.md) §8; [03](03-storage-caches-and-search.md)). The first milestone, the API-only single-tenant instance that the three Wikibase client libraries are tested against (§4.2), is steps 1 to 6 narrowed to: `scatter-vocab`, `scatter-providers`, `scatter-normalize`, `scatter-wikibase-model`; `scatter-log`, `scatter-log-postgres`, `scatter-integrity`; `scatter-projection`, `triplespace-db`, `scatter-actors`; `scatter-wikibase-changeset`, `scatter-ingest`, `triplespace-api-ingest`, `scatter-adapter-wikidata`, `scatter-adapter-internetdomains`; `scatter-wikibase-resolve`; `triplespace-projections` with the projections `registry`, `keyed_surrogate`, `actor` (with groups and memberships), `entity_source`, `keyed_map`, `job`, `entity`, `term`, `identifier` and `activity`; `triplespace-accounts`, `triplespace-api-action`, `triplespace-server` and `triplespace-cli`.

1. **Pure crates with golden fixtures:** `scatter-vocab`, `scatter-providers`, `scatter-normalize`, `scatter-actors`, `scatter-wikibase-model`, `scatter-wikibase-rdf`, `scatter-wikibase-shape`, `scatter-mwlog`, `scatter-pages`, `scatter-titles`, `scatter-wikitext`. They need no log or storage, are testable now against `docs/samples` and the UI fixtures, and are what Scatterbase's Phase 4 needs.
2. **The log:** `scatter-log` (the formats, the tree and the `LogStore` trait with its in-memory backend), `scatter-integrity`, then `scatter-log-postgres` and the `LogStore` conformance suite, and the `segments` file backend last, against the same suite: Postgres is the backend the milestone serves from, and the file backend is what export bundles and tools need.
3. **Projection plumbing:** `scatter-projection`, `triplespace-db`, and `scatter-quadstore` behind its feature.
4. **Ingest:** `scatter-wikibase-changeset`, `scatter-ingest` and `scatter-adapter-wikibase`, enough to bootstrap a mirror ([0013](../decisions/0013-postgres-storage.md) §9), and the adoption job of [0035](../decisions/0035-adopting-a-wikibase.md) §2–4, which a tenant continuing an existing Wikibase needs before step 5.
5. **Resolution:** `scatter-identity`, `scatter-wikibase-resolve`, and `triplespace-projections` through the resolution and term projections.
6. **Reading:** `triplespace-titles` over `scatter-titles`, `triplespace-cache`, `triplespace-api-action` for `wbgetentities`, and `triplespace-server`. This is the first point at which the statement UI ([0003](../decisions/0003-statement-ui.md) §10) can be pointed at Triplespace instead of Wikidata.
7. **The rest:** the remaining projections and adapters, `triplespace-rdf`, `triplespace-search`, `triplespace-accounts`, `triplespace-upstream`, `triplespace-api-rest`, `triplespace-api-ingest`, `triplespace-cli`; with the write path, `scatter-filter` and its projections ([0030](../decisions/0030-edit-filters.md)), and after resolution `scatter-wikibase-constraints` and the constraint projection ([0031](../decisions/0031-property-constraints.md)). Moderation ([0023](../decisions/0023-moderation.md)), subsidiaries ([0024](../decisions/0024-subsidiary-accounts.md)), sitelinks ([0026](../decisions/0026-sitelinks.md)), preferences ([0027](../decisions/0027-preferences-and-portability.md)), tenancy policy ([0028](../decisions/0028-tenancy-policy.md)) and resolvers ([0029](../decisions/0029-resolver-namespaces.md)) add no crate and land in the crates they extend.
8. **Discussion and notification:** `scatter-threads` and the thread projections and routes ([0019](../decisions/0019-discussions.md)); the feed queries, watchlist and stream ([0020](../decisions/0020-change-feeds.md)); then `scatter-activitypub` and `triplespace-notify` ([0021](../decisions/0021-notifications.md)). Each depends on the activity projection of step 7 and on nothing later.
9. **Federation and delegated tools:** `triplespace-federation` ([0022](../decisions/0022-federation.md)) and `triplespace-oauth` ([0025](../decisions/0025-oauth-server.md)), which depend on step 8 and on `triplespace-accounts`; the verified Triplespace adapter follows once `scatter-integrity` can verify remote proofs.
10. **Files:** `scatter-blob` and `scatter-files`, then `triplespace-files` with the file projections and routes ([0039](../decisions/0039-files-and-media.md)). They need step 6 for serving; the blob store itself has no dependency on the log and can be built and tested first.
11. **Expansion and Lua:** `scatter-wikitext-expand`, which is pure and can be built with step 1's crates, then `triplespace-scribunto` and `triplespace-render` with the refresh job and the Parsoid service ([0042](../decisions/0042-template-expansion-and-parsoid.md), [0043](../decisions/0043-lua-modules.md)). They need step 6 for reading, and step 7's search and upstream client.
12. **The web tier:** `triplespace-client`, `triplespace-ui` and `triplespace-web` ([0057](../decisions/0057-web-tier.md)). They need step 6 for reading and `triplespace-accounts` for logging in, and grow with step 7's REST routes; `scatter-wikibase-shape` from step 1 comes first.

### 3.2 Names

*Sources: [0005](../decisions/0005-crate-organization.md) §6.*

Shared crates carry the `scatter-` prefix. Crates that only Triplespace uses carry `triplespace-`. Shared Wikibase crates are named `scatter-wikibase-*`, so no crate name leads with "wikibase", which is Wikimedia Deutschland's project name. `scatter-mwlog` and `scatter-wikitext` model MediaWiki conventions without naming MediaWiki.

### 3.3 Licensing

*Sources: [0005](../decisions/0005-crate-organization.md) §6; [0033](../decisions/0033-backend-stack.md) §1, §16, A22; [0043](../decisions/0043-lua-modules.md) §14.*

- **Triplespace is GPL-3.0-or-later.** That covers every crate in this workspace and the `ui/` package. Workspace crates and `ui/package.json` declare `GPL-3.0-or-later`, set once in `[workspace.package]` ([0033](../decisions/0033-backend-stack.md) §16).
- **The shared `scatter-*` crates are dual-licensed.** Like the Scatterbase kernel, they are also available under a commercial license from the copyright holder. Dual licensing requires Scatter to hold copyright in every shared crate, so outside contributions need a contributor license agreement.
- **Third-party dependencies must be compatible with GPLv3.**
  - **Accepted.** Every third-party crate linked into `triplespace` carries one of these licences: MIT, MIT-0 (the vendored `parse-wiki-text-2`), Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, BlueOak-1.0.0 (`minicbor`), CDLA-Permissive-2.0 (root-certificate data in `webpki-roots`), LGPL, GPL-2.0-or-later or GPL-3.0.
  - **Denied.** Licences that cannot be combined with GPLv3: the pre-3.0 OpenSSL/SSLeay licence, GPL-2.0-only and non-commercial terms. AGPL is also denied, as a policy choice, so that its network clause never reaches the combined work.
  - **Enforcement.** `cargo-deny` enforces both lists in CI (§3.4).
- **Lua.** Lua 5.1 is linked through `mlua` (MIT) in `triplespace-scribunto`, which vendors Scribunto's and WikibaseClient's Lua ([0043](../decisions/0043-lua-modules.md) §4). The vendored Lua is GPL code: Scribunto is "GPL-2.0-or-later AND MIT", WikibaseClient GPL-2.0-or-later. Both are on the accepted list, so the binary may include them. They cannot go into a `scatter-*` crate, because the shared crates are dual-licensed and Scatter holds copyright in every line of them. So the Lua runtime is a Triplespace crate, `triplespace-scribunto`, under GPL-3.0-or-later only and never dual-licensed. Scatterbase can use it on GPL terms, not on commercial ones ([0043](../decisions/0043-lua-modules.md) §14).
- **`docs/` is CC0-1.0.** That includes the ADRs, the registry, the API contracts and the test vectors, so independent implementations can embed them. Third-party files in `docs/samples/` keep their own licences, which [its README](../samples/README.md) lists set by set.
- **Bindings in other languages are Apache-2.0.** Python comes first. They implement the API contract and the specifications in `docs/`. They do not link to Triplespace code, are not compiled against it, and do not port it.
- **RSA for HTTP Signatures.** `scatter-activitypub` needs RSA because Mastodon accepts only RSA keys there; §4.4 provides it through `aws-lc-rs`. The instance's Ed25519 key ([0006](../decisions/0006-log-integrity-and-erasure.md) §6) cannot be used for this, which is one more reason the notifier key of [0021](../decisions/0021-notifications.md) §5 is separate.

### 3.4 Repository and supply chain

*Sources: [0005](../decisions/0005-crate-organization.md) §6; [0033](../decisions/0033-backend-stack.md) §16; [0097](../decisions/0097-versioning-and-skins.md) §3.*

Every crate lives in one Cargo workspace in the Triplespace repository, under `crates/`. Scatterbase takes Git dependencies pinned to a commit. The `scatter-*` crates move to their own repository when Scatterbase first depends on them. **Each `scatter-*` crate has its own version**, whose contract is its public Rust API under the Cargo SemVer rules, checked by `cargo-semver-checks` against its last published version once it has one; the other crates are `publish = false` and carry the product's version from `[workspace.package]` (§3.5; [0097](../decisions/0097-versioning-and-skins.md) §3).

- `cargo-deny` checks licences (the allowlist of §3.3), bans (no duplicate crypto providers, no `openssl-sys`), advisories and sources.
- `parse-wiki-text-2`'s manifest names only a licence file; a clarify entry records it as MIT-0 ([0033](../decisions/0033-backend-stack.md) A22).
- `aws-lc-sys` is required at 0.39 or later. Earlier versions included code under the OpenSSL licence, which is incompatible with GPLv3. For the same reason, if `ring` replaces it (§4.4), `ring` must be 0.17.9 or later.
- The repository root carries the GPLv3 text in `LICENSE`; `docs/LICENSE` carries CC0-1.0.
- `cargo xtask manifest` writes the component manifest of §7.1: the build, the crates linked into each binary, the frontend packages that reach the browser and vendored code, each with its licence, copyright and notice files. It uses the allowlist above, `triplespace-server` and `triplespace-web` embed it, and release builds fail without it.

### 3.5 Versions and releases

*Sources: [0097](../decisions/0097-versioning-and-skins.md) §1, §2, §6, §7, §18.*

**Every contract Triplespace defines has a semantic version**, `MAJOR.MINOR.PATCH`: a patch fixes what exists, a minor adds without breaking, a major breaks, and the leftmost non-zero number is the compatibility boundary, as Cargo reads it, so under 1.0 a minor may break and a patch may not. What breaking means is defined per contract, from the side of whoever depends on it:

| Contract | Depended on by | A major is |
|---|---|---|
| Each published `scatter-*` crate (§3.4) | Scatterbase and other Rust code | A public API break under Cargo's rules |
| Record formats ([01](01-log-and-records.md) §2.2) | Every reader of the log | A record older readers cannot interpret |
| The REST API and `siprop=triplespace` ([20](20-web-tier.md) §4) | The web tier, tools | A field, route or behaviour removed or changed |
| Registry and configuration schemas ([23](23-configuration-and-registry.md)) | Operators, the crates that embed them | A key renamed or removed, or its meaning changed |
| The URL scheme | Links, bookmarks, tools | A route or special-page name changed or removed |
| The template contract ([19](19-site-ui.md) §5.10) | Skins | A template or view-model field removed or renamed, or a new obligation |
| Each skin, each message bundle ([19](19-site-ui.md) §5.6, §5.10) | Operators, user CSS, Interface overrides | A markup hook or message key changed or removed |

**A contract Triplespace implements keeps its owner's version**: the MediaWiki release the Action API matches, in `generator`; the Wikibase release of the JSON and RDF snapshots; export schema 0.11; the protocol versions of MCP, ActivityPub, HTTP Signatures and OAuth; and Codex's design tokens, whose version is the `@wikimedia/codex-design-tokens` release the binary pins ([19](19-site-ui.md) §5.9).

**The product's version is the operator's upgrade contract.** A patch is a drop-in replacement that can be rolled back; a minor adds features and settings, needs nothing but a restart, keeps existing configuration and installed skins working, and can be rolled back to the minor before it; a major needs the operator to act or takes away something they relied on: a setting removed or renamed, a record major written by default, a REST, template-contract or token major, a dropped implemented contract, or a new required service. **A release's bump is derived**: the largest among the contracts it ships. `Special:Version` and `GET /version` report the product version and each contract's beside it ([21](21-special-pages.md) §5.6).

**When a number moves.** A version is **published** when it is built as a downloadable bundle (a release binary or image), shipped to crates.io, or released by any equivalent act that marks the code as stable, and each publication is tagged: `vX.Y.Z` for the product, `{crate}-vX.Y.Z` for a crate published on its own, `skin-{name}-vX.Y.Z` and `i18n-{bundle}-vX.Y.Z` for a skin or message bundle released apart from the product.

- **Once something is published, its patch goes up in the repository** at once, so the repository always carries an unpublished version, and a site running from git reports at least one patch above the last release; the build's commit and `modified` flag on `Special:Version` say exactly what it runs.
- **A feature raises the minor, and a breaking change the major, when the change is made**, setting the numbers after it to 0. Under major 0 a breaking change raises the minor, and so does a feature.
- **Bumps since the last publication do not stack**: a second feature leaves a raised minor where it is, and a breaking change after it raises the major.
- **A contract published inside the product** follows the same rules, except that its patch goes up only the first time it changes after a publication.
- **Features and breaking changes are developed on branches**, with their bump, so that `main` stays fit to publish; a fix to a published line that `main` has moved past is made on a `release/X.Y` branch cut from that line's tag.

The product starts at `0.1.0`, which the first publication carries.

## 4. The backend stack

*Sources: [0033](../decisions/0033-backend-stack.md) §1, §2, §3, §6, §7, §9, §9.1, §9.2, §9.3, §9.4, §13, §14, §15, §17; [0042](../decisions/0042-template-expansion-and-parsoid.md) §18; [0043](../decisions/0043-lua-modules.md) §15; [0057](../decisions/0057-web-tier.md) §11; [0059](../decisions/0059-query-service.md) §7.*

### 4.1 Principles

*Sources: [0033](../decisions/0033-backend-stack.md) §1.*

1. **One binary, one required service.** A small instance runs with the `triplespace-server` binary and Postgres, and the binary serves the site itself. Valkey, OpenSearch, QLever and a Parsoid service are optional services a larger instance adds (§6, [03](03-storage-caches-and-search.md), [11](11-rendering-templates-and-modules.md)), and the query service of [0059](../decisions/0059-query-service.md) §2 runs embedded in the binary by default, moving to QLever when an instance chooses and necessarily at Wikidata scale, where the embedded backend is refused above a configured triple count (§6); Parsoid is a separate PHP program never linked into the binary, and the only one that needs a PHP runtime. A larger instance may also run `triplespace-web`, a second binary from the same workspace that renders the site, holds no state and reaches the instance only through its API, so that page rendering scales apart from the API ([0057](../decisions/0057-web-tier.md) §2). Nothing needs a message broker, a JVM or a Node runtime.
2. **Pure crates stay pure.** Crates on rule 2's pure list (§1.2) do no I/O and pull in no async runtime. The ones on rule 7's wasm list must build for `wasm32-unknown-unknown`, so they avoid C dependencies.
3. **GPLv3-compatible licences inside the binary.** Triplespace is GPL-3.0-or-later; the accepted and denied lists and their enforcement are §3.3.
4. **Prefer a small, well-understood dependency to a framework.** Where a protocol surface is small (OAuth server, HTTP Signatures, OpenSearch's REST API), Triplespace implements it over general-purpose crates rather than adopting a framework that would dictate structure.

### 4.2 Language and toolchain

*Sources: [0033](../decisions/0033-backend-stack.md) §2; [0056](../decisions/0056-security-model.md) A25.*

- Stable Rust, edition 2024, pinned in `rust-toolchain.toml`. The minimum supported Rust version moves deliberately, in its own commit.
- **The binaries allocate with mimalloc** ([0033](../decisions/0033-backend-stack.md) §2, A24): `triplespace`, `triplespace-server` and `triplespace-web` set the `mimalloc` crate as the global allocator behind a default `mimalloc` feature, and built without it use the system allocator, which heap profilers and valgrind's memcheck read best. Library crates set no allocator, so the pure and wasm crates (§1.2 rules 2 and 7) are unaffected. On a Wikidata sync glibc's `malloc` and `free` were a third of the client's instructions; mimalloc cut its CPU by about a quarter for about 35 MB more peak memory (`docs/testing/performance.md` §8).
- One Cargo workspace for all the crates of §2.
- CI runs `cargo nextest`, `cargo clippy -D warnings`, `cargo deny check`, `cargo hack --feature-powerset` on crates with features, a `wasm32-unknown-unknown` build of every rule-7 crate and so of every crate they depend on, and a build of `scatter-wasm` that fails above 2 MB gzipped (§1.2 rule 7).
- CI also runs the Wikibase client libraries — WikibaseIntegrator, Pywikibot and WikidataIntegrator, at pinned versions — against a server loaded from a sample of the Librarybase dump (`tools/client_compat.sh`, `docs/clients.md`). The compatibility [0012](../decisions/0012-api-requirements.md) promises is a measured property, not a reading of the reference contract, and a client's probes (`siteinfo`, `paraminfo`, `meta=wikibase`) are part of the API surface the tests hold fixed ([18](18-api.md)).
- CI also runs the permission model's attack battery (`tools/acl_check.sh`) against a second such server: every read route for a confidential page, entity and thread, the write-side oracles, privilege escalation, a hidden name, tokens and a private tenant on the negative side, and what every legitimate role must be able to do on the positive side ([09](09-security-and-moderation.md) §4–§5; [0056](../decisions/0056-security-model.md) A25). A finding fails the job.

### 4.3 Async runtime and HTTP server

*Sources: [0033](../decisions/0033-backend-stack.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

| Concern | Choice |
|---|---|
| Runtime | `tokio` (multi-threaded) |
| HTTP server | `axum` on `hyper`, with `tower` middleware |
| Middleware | `tower-http` (compression, request IDs, timeouts, CORS for the documented public routes) |
| Server-sent events | `axum::response::sse` for `/activity/stream` ([0020](../decisions/0020-change-feeds.md)), `/updates/stream` ([0032](../decisions/0032-sparql-update-stream.md)), job progress ([0012](../decisions/0012-api-requirements.md)) |

The appending transaction, tier 1 of the write path ([0083](../decisions/0083-write-path-in-three-tiers.md) §1; [03](03-storage-caches-and-search.md)), is built as tower layers in its order (auth → grants → rate limit → ACLs → filters → normalize and diff → base check → ID allocation → append → graph state → activity row → commit), so the order is visible in one place and testable on its own; composition of the written entity follows commit as a step of the handler, and nothing else of tiers 2 and 3 runs in a request. Redaction for the viewer ([0012](../decisions/0012-api-requirements.md) §1) is a response layer shared by every route.

### 4.4 Encoding, hashing and cryptography

*Sources: [0033](../decisions/0033-backend-stack.md) §6; [0088](../decisions/0088-external-identity-providers.md) §3.*

| Concern | Choice | Notes |
|---|---|---|
| Canonical CBOR ([0006](../decisions/0006-log-integrity-and-erasure.md) §2, [0015](../decisions/0015-record-format-and-partition-registry.md) §1) | `minicbor` | Encoding is written by hand to the canonical rules, with no derive magic in the hashed path. Round-trip tests over Wikidata snapshots. `no_std`-capable and wasm-friendly. |
| Hashing | `sha2` | Merkle leaves and nodes ([0006](../decisions/0006-log-integrity-and-erasure.md)). |
| Instance and client signatures | `ed25519-dalek` | The key chain of [0006](../decisions/0006-log-integrity-and-erasure.md); subsidiary signing keys ([0024](../decisions/0024-subsidiary-accounts.md) §4). |
| HMAC | `hmac` | Binding digests ([0018](../decisions/0018-tenants.md) §10), Atom tokens. |
| RSA for ActivityPub HTTP Signatures ([0021](../decisions/0021-notifications.md) §5, [0022](../decisions/0022-federation.md)) | `aws-lc-rs` | Constant-time RSA signing. Not the `rsa` crate. |
| TLS | `rustls` with the `aws-lc-rs` provider | One crypto provider for TLS and RSA signing. `ring` is the fallback if musl builds (§4.10) become a requirement. |
| Password issuer ([0007](../decisions/0007-actor-identity.md) §1) | `argon2` | Argon2id, parameters in site config. |
| Sealing secrets at rest: OAuth client secrets, publication credentials, upstream grants ([0088](../decisions/0088-external-identity-providers.md) §3) | `chacha20poly1305` (XChaCha20-Poly1305) | Under the instance sealing key, a subkey of the signing seed (`ts-seal-v1`); a 24-byte random nonce per value; the table, tenant and issuer or owner as associated data. |
| Sealing private extracts ([0027](../decisions/0027-preferences-and-portability.md) Q1) | `age` (X25519 recipients) | Proposed; 0027 Q1 stays open until James agrees. Never the Ed25519 instance key. |
| Randomness | `getrandom`, `rand_core` | |
| Token and ID encoding | `base64` (URL-safe, no padding), `ulid` only where [0015](../decisions/0015-record-format-and-partition-registry.md) names ULIDs | |

### 4.5 HTTP client and upstream access

*Sources: [0033](../decisions/0033-backend-stack.md) §7.*

- `reqwest` with rustls for the Wikidata Action API, WebFinger, ActivityPub delivery and cross-instance reads ([0022](../decisions/0022-federation.md)).
- `eventsource-stream` over `reqwest` for Wikimedia EventStreams (`recentchange`, `revision-visibility-change`; [0011](../decisions/0011-logs.md) §4).
- Every upstream request carries a policy-compliant `User-Agent` and `maxlag`. The upstream fetch budget ([0010](../decisions/0010-site-ui.md), [0012](../decisions/0012-api-requirements.md) §9) is enforced with `governor` token buckets per provider.

### 4.6 Text and markup

*Sources: [0033](../decisions/0033-backend-stack.md) §9, §9.1, §9.2, §9.3, §9.4.*

**Wikitext, in `scatter-wikitext`.** The parser is a vendored `parse-wiki-text-2` (a maintained fork of Fredrik Portström's `parse_wiki_text`, MIT-style licence without a notice clause). It is pure Rust, gives every node start and end positions, recognises templates, parameters, tags, tables, lists, links, images, categories and comments, reports warnings for malformed markup, and bounds its own running time. Its `Configuration` (namespaces, extension tags, URL protocols) is generated from `namespaces.toml`. Vendoring, rather than a crates.io dependency, is because the fork's last release is over a year old and the crate becomes part of Triplespace's content model. Changes are offered upstream where they are general.

What the parser must do: parse any wikitext without failing, because imported history predates the template-flattening revision ([0008](../decisions/0008-namespaces-and-document-pages.md) §8); recognise what it will not render; keep positions for line-level conflict marking and link autocomplete ([0010](../decisions/0010-site-ui.md)); extract `page_link` rows and categories; build for wasm so the preview ([0034](../decisions/0034-frontend-stack.md)) runs the same code as the server.

The renderer is written in `scatter-wikitext`. It shares with the markdown path (`scatter-pages`, `markdown` feature) the `ammonia` sanitizer, wiki-link resolution (entity links rendered with labels, red links) and `page_link` extraction, so both content models render and link-index the same way. The rendered subset, what renders as a visible chip showing its source, and the behaviour switches and magic links that are off are in [11](11-rendering-templates-and-modules.md); files in wikitext follow [12](12-files-and-media.md).

The parser renders and extracts from **expanded** text, produced by `scatter-wikitext-expand`; a tenant may instead render through a Parsoid service that calls back into Triplespace for expansion ([0042](../decisions/0042-template-expansion-and-parsoid.md) §1, §8). Parsoid stays rejected as an in-process parser.

Conformance has two test sources: a subset of MediaWiki's `tests/parser/parserTests.txt` covering only the rendered subset, run in CI as a fixture (the file is GPL-2.0-or-later, which is compatible with Triplespace's licence; it is test data and is not compiled into the binary), and a differential test against Parsoid on the MediaWiki 1.43 reference install ([0000](../decisions/0000-init.md)), with sampled pages rendered by both and DOMs normalized and compared for the supported constructs (§4.9).

Not chosen as the core parser: `tree-sitter-wikitext` (Wikimedia, MIT). It produces a concrete syntax tree that would need lowering, and its C core complicates `wasm32-unknown-unknown` builds. It remains available for editor highlighting in the browser ([0034](../decisions/0034-frontend-stack.md) §7). `wikitext-parser` (approximate, unmaintained) and Parsoid or mwparserfromhell (not in-process) were also rejected.

**Markdown, in `scatter-pages`.** `comrak`, with its wikilinks extension for `[[wiki links]]`, and `ammonia` for sanitization. Both build for wasm.

**Identifiers, URLs and case.**

| Concern | Choice |
|---|---|
| URLs ([0026](../decisions/0026-sitelinks.md) §1) | `url` (WHATWG parsing) |
| IDNA / UTS 46 ([0009](../decisions/0009-keyed-entity-types-and-domain.md)) | `idna` |
| Case folding (DOI resolver, [0029](../decisions/0029-resolver-namespaces.md)) and plural rules | `icu_casemap`, `icu_plurals` (ICU4X; wasm-friendly) |
| Unicode normalization | `unicode-normalization` |

**Edit filter language.** The `cel` crate, the crate formerly published as `cel-interpreter` ([09](09-security-and-moderation.md)).

### 4.7 Observability, health and operational commands

*Sources: [0033](../decisions/0033-backend-stack.md) §13; [0057](../decisions/0057-web-tier.md) §11; [0059](../decisions/0059-query-service.md) §7.*

- `tracing` and `tracing-subscriber` for structured logs.
- `opentelemetry` with the OTLP exporter for traces.
- `metrics` with the Prometheus exporter. Required metrics: projection lag per projection, append latency, sync lag per provider, upstream fetch budget used, delivery queue depth, cache hit rates per layer, update-stream consumer lag.

The query service adds, on `server.admin_listen` ([0056](../decisions/0056-security-model.md) §10 line 9): queries started, finished, timed out, truncated; lag; store size. Its operator commands are `triplespace-cli query status` (backend, cursor, lag, store size, and for `remote` the endpoint's marker cursor), `triplespace-cli query rebuild [--tenant]` (drops the embedded store, or one tenant's, and reloads it from a fresh `full` dump taken at the current cursor, then resumes deltas; run by the `view` rebuild automatically) and `triplespace-cli query run --tenant {slug} {file}` (runs a query as the service would, for operators and tests) ([02](02-graphs-rdf-and-query.md)).

The web tier ([20](20-web-tier.md)): `/healthz` reports liveness and `/readyz` that the API is reachable and recent enough, checked at most every few seconds; both on `web.admin_listen`. On `SIGTERM` the web tier stops accepting, finishes what is in flight, and exits. `X-Request-Id` is passed to the API, each API call is a `tracing` span, and the metrics are page latency by page kind, API calls per page, the response cache's revalidation and hit rates, and the API's error rate.

### 4.8 Error handling

*Sources: [0033](../decisions/0033-backend-stack.md) §14.*

`thiserror` in library crates, `anyhow` only in `triplespace-cli` and test code. API errors map to MediaWiki-shaped error codes ([0012](../decisions/0012-api-requirements.md)) in one place in the API crates.

### 4.9 Testing and conformance

*Sources: [0033](../decisions/0033-backend-stack.md) §15; [0042](../decisions/0042-template-expansion-and-parsoid.md) §18; [0043](../decisions/0043-lua-modules.md) §15; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §13.*

| Tool | Use |
|---|---|
| `cargo-nextest` | test runner |
| `proptest` | canonical CBOR, Merkle proofs, normalizers, filter evaluation |
| `insta` | snapshots of JSON, RDF and rendered HTML |
| `testcontainers` | Postgres, Valkey, OpenSearch and QLever in integration tests |
| `cargo-fuzz` | wikitext and markdown parsers, CBOR decoding, HTTP Signature parsing, CEL compilation, inbound ActivityPub bodies; the preprocessor, whose random input must terminate within the limits and never panic ([0042](../decisions/0042-template-expansion-and-parsoid.md) §18); the Lua sandbox, whose generated Lua must end within the limits, never panic the host, and never reach a global outside the enumerated set ([0043](../decisions/0043-lua-modules.md) §15) |
| MediaWiki 1.43 reference install, with WikibaseClient, ParserFunctions and Scribunto | API compatibility ([0012](../decisions/0012-api-requirements.md)), Parsoid differential tests (§4.6), differential `action=expandtemplates` tests and Lua conformance ([0042](../decisions/0042-template-expansion-and-parsoid.md) §18, [0043](../decisions/0043-lua-modules.md) §15), Pywikibot acceptance ([0008](../decisions/0008-namespaces-and-document-pages.md) §12) |

The `LogStore` conformance suite (rule 8 of §1.2) runs against both the file and Postgres implementations.

Expansion adds, beside the core parser fixtures of §4.6 ([0042](../decisions/0042-template-expansion-and-parsoid.md) §18):

- **The preprocessor sections of MediaWiki's `parserTests.txt`**, and ParserFunctions' parser tests, run in CI as fixtures. They are GPL test data and are not compiled in.
- **A differential `action=expandtemplates` test** against the MediaWiki 1.43 reference install over a corpus of imported pages, templates and modules. The reference install gains ParserFunctions and Scribunto, and keeps WikibaseClient, which Lua needs as well.
- **The Parsoid differential test covers templated pages:** the same pages rendered by the reference install's Parsoid and by Triplespace with the Parsoid service, with DOMs normalized and compared.

Lua adds ([0043](../decisions/0043-lua-modules.md) §15; [11](11-rendering-templates-and-modules.md)):

- **Scribunto's own Lua-side test modules**, the `*Tests.lua` files beside its PHPUnit suite, run against the Rust host, library by library.
- **WikibaseClient's Lua tests** run the same way, against a fixture tenant whose entities match theirs.
- **The global environment test** of [0043](../decisions/0043-lua-modules.md) §4.
- **A differential `action=expandtemplates` test** over pages that call widely used modules from English Wikipedia (citations, infoboxes, coordinates, `Module:Wd`), on the reference install with Scribunto and WikibaseClient and on Triplespace, with the same entities on both.

Talk-page conversion adds ([0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §13; [13](13-mirrored-pages.md) §5.12):

- **Golden tests for `scatter-talk` against DiscussionTools,** on committed fixtures in `docs/samples/enwiki-talk/`: ten English Wikipedia pages as of 2026-10-21, one with its whole history, each beside `discussiontoolspageinfo`'s parse of the same revision. `tools/talk_dumps.py` fetches samples and `tools/talk_fixtures.py` compresses them and strips the parse's HTML, about 3.3 MB; their README carries the licence (CC BY-SA 4.0) and the attribution. `TRIPLESPACE_TALK_SAMPLES` points the tests at other samples.
- **Agreement thresholds:** comments compared by DiscussionTools name, and for each its parent and ID. The splitter must find 99.9% of comments and agree on 97.5% of parents and 97% of IDs, with no more than 1% of comments in fallback sections: the agreement of `tools/talk_split.py`, the Python prototype, compared by `tools/talk_compare.py`.
- **Attribution** on the page with its full history, replayed revision by revision: no delivery bot is a signed post's author, a former name resolves to the account, and history agrees with most signatures.
- **A test per splitting rule**, on short pages written for it.

### 4.10 Packaging

*Sources: [0033](../decisions/0033-backend-stack.md) §17.*

- Three binaries for each target, built for Linux (x86-64, arm64, glibc) and macOS for development: the server binary `triplespace-server`, the web binary `triplespace-web`, and the command line, whose binary is `triplespace` (crate `triplespace-cli`).
- An OCI image with the three binaries, the frontend assets embedded in each (§5.1), and nothing else; the web tier runs from the same image with another entry point ([0057](../decisions/0057-web-tier.md) §2).
- A `compose.yaml` for development with Postgres, and optional Valkey, OpenSearch and QLever services.
- musl static builds are not a goal; if they become one, the crypto provider moves from `aws-lc-rs` to `ring` (§4.4).

## 5. The frontend build and testing

*Sources: [0034](../decisions/0034-frontend-stack.md) §8, §12.*

### 5.1 Build

*Sources: [0034](../decisions/0034-frontend-stack.md) §8.*

- `ui/` is an npm workspace: TypeScript, Vue 3, Codex packages pinned to exact versions, CodeMirror 6. Codex is pinned at its latest release when work begins (2.7.0 on 2026-10-03) and moves to a newer release deliberately, in a commit of its own with the snapshot and accessibility tests as the check, not with the version a MediaWiki release bundles.
- **Vite** builds the components into hashed ES modules and a manifest.
- The release build embeds `ui/dist` into both binaries that can serve the site, `triplespace-web` and `triplespace-server` (`rust-embed`); templates read the manifest to emit `<script type="module">` tags. **Node is a build-time dependency only**; a small instance stays one binary (§4.1), and an instance that runs the web tier separately runs two ([0057](../decisions/0057-web-tier.md) §2).
- In development, whichever binary serves the site proxies asset requests to the Vite dev server for hot reload.
- Lint: ESLint with `eslint-config-wikimedia`, Stylelint with `stylelint-config-wikimedia`. Styles are plain CSS using Codex's CSS custom-property tokens; no Less.

The frontend stack itself (Codex, the server-rendered frame, `scatter-wasm` in the browser) is in [19](19-site-ui.md).

### 5.2 Testing

*Sources: [0034](../decisions/0034-frontend-stack.md) §12.*

| Tool | Use |
|---|---|
| Vitest + `@vue/test-utils` | component tests |
| Playwright | end-to-end flows, including a JavaScript-disabled run of every read page |
| `@axe-core/playwright` | accessibility checks on every page kind |
| `insta` | snapshots of rendered templates and fragments |

## 6. Deployment profiles

*Sources: [0013](../decisions/0013-postgres-storage.md) §11; [0033](../decisions/0033-backend-stack.md) §1; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6.*

The schema is the same everywhere. Two profiles differ in what is optional and in which of two identical schemas' objects are used:

| | Small instance | Wikidata scale |
|---|---|---|
| Postgres | One server | Primary plus replicas; mirror child tables on their own tablespace; dumps taken from a replica under one exported snapshot shared by parallel workers |
| `term_prefix` index | Present as a partial index over local entities; serves suggest and `wbsearchentities` | Kept as schema and never created or read: an instance holding a full mirror never builds it, and OpenSearch serves suggest and `wbsearchentities` ([0014](../decisions/0014-caches-and-search.md) §7) |
| `view.term` | Local entities, overlaid entities, and mirrored entities in the instance's term languages | The same rule; its size is mirrored entities × the instance's term languages × term kinds, which is the budget the instance's `entities.term_languages` sets ([23](23-configuration-and-registry.md) §3.2) |
| `entity_ref` | Present | Present and, with `view.term`, the largest `view` table; keyed on integer page IDs with a role bitmask; may be omitted, with "Links here" and the backlink count served from the query service |
| Shared cache | Optional | Required ([0014](../decisions/0014-caches-and-search.md) §2) |
| Quad store | Optional; `query.backend = embedded`, refused once the store passes a configured triple count | `query.backend = remote` is required; QLever from dumps |

This is [0000](../decisions/0000-init.md) §4's rule that smaller deployments follow as corollaries: they drop services, not tables, and the one index the large profile does without is left in the schema unbuilt. A small instance is the `triplespace-server` binary and Postgres; Valkey, OpenSearch, QLever, a Parsoid service and `triplespace-web` are what a larger instance adds (§4.1).

## 7. The component manifest and the checker

*Sources: [0077](../decisions/0077-special-version.md) §6, §12, §15; [0033](../decisions/0033-backend-stack.md) §16.*

### 7.1 `cargo xtask manifest`

*Sources: [0077](../decisions/0077-special-version.md) §15; [0033](../decisions/0033-backend-stack.md) §16.*

**`cargo xtask manifest`** writes the component manifest that `Special:Version` and its licence subpages read ([21](21-special-pages.md)):

- the build: version, commit, `modified` (a dirty tree), the commit date, rustc, target and features;
- the workspace crates and the third-party crates linked into each binary, from `cargo metadata` for the binary's target with normal dependencies only;
- the frontend packages reachable from `ui/package.json`'s `dependencies`, from `ui/package-lock.json`;
- vendored code, from a `vendor.toml` beside each vendored tree (name, upstream, revision, licence);
- for each component, its licence expression and the licence, copyright and notice files it ships (`LICENSE*`, `COPYING*`, `NOTICE*`, `AUTHORS*`), deduplicated by content.

**It shares `cargo-deny`'s allowlist** (§3.3): a component whose licence is not on it fails the manifest as it fails `cargo deny`.

**`triplespace-server` and `triplespace-web` embed it** at build time. A build without one, such as an ordinary development build, embeds a stub, and the page says "development build: components not listed". Release builds and the OCI image fail without a manifest.

### 7.2 Crates and third-party components on `Special:Version`

*Sources: [0077](../decisions/0077-special-version.md) §6.*

**Triplespace crates:** each workspace crate linked into the binary, with its version and licence. A `scatter-*` crate is marked as also available under a commercial licence (§3.3).

**Third-party components,** grouped and sorted by name, each with its version, licence expression, upstream URL and a link to its licence page:

- **Crates:** the binary's dependencies for its target, as linked: normal dependencies only, not build or development dependencies.
- **Frontend packages:** the packages in `ui/package-lock.json` reachable from `dependencies` (Codex, Vue and the typefaces), not `devDependencies`, since only those reach the browser.
- **Vendored code:** source copied into the tree, such as Scribunto's and WikibaseClient's Lua in `triplespace-scribunto` ([0043](../decisions/0043-lua-modules.md) §4) and `parse-wiki-text-2` in `scatter-wikitext` (§4.6), each with its upstream and revision.

Native code that a `-sys` crate bundles (Lua 5.1 through `mlua`, RocksDB through Oxigraph, `aws-lc`) is covered by its crate's licence files, which the manifest collects whole.

### 7.3 The ADR checker

*Sources: [0077](../decisions/0077-special-version.md) §12.*

`check_adrs.py` has two checks for `version.toml` ([23](23-configuration-and-registry.md)):

- **Check 9.** Every agent an ADR's Author line names ("James Hare / Claude Opus", and "revision by James Hare / Claude Fable") is an `[[agent]]` in `version.toml`, and every `[[agent]]` is named by at least one ADR. An Author line naming only "Claude" is accepted for the ADRs in `[agent_attribution].unattributed` and reported for any other.
- **Check 10.** Every `origin` in the three registries is `mediawiki`, `triplespace`, `generic` or the `name` of an `[[extension]]`; no two names differ only in case; and every `[[extension]]` is named by an `origin` or carries `inspired_by`.
