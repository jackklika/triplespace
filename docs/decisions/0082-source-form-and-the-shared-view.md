# 0082. Source form, identifier normalization and the shared view

- **Status:** Proposed
- **Date:** 2026-10-09
- **Updated:** 2026-11-04 (A2)
- **Author:** James Hare / Claude Fable
- **Changes:** [0001](0001-revision-metadata-rdf.md), [0002](0002-source-graphs-and-mass-ingest.md), [0003](0003-statement-ui.md), [0004](0004-identity-clusters-and-equivalence.md), [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0013](0013-postgres-storage.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0018](0018-tenants.md), [0031](0031-property-constraints.md), [0032](0032-sparql-update-stream.md), [0044](0044-tenant-relative-ids.md), [0059](0059-query-service.md), [0060](0060-scopes.md), [0070](0070-shallow-entity-mirroring.md), [0078](0078-entity-sources.md)
- **Uses:** [0029](0029-resolver-namespaces.md), [0080](0080-tenants-as-entity-sources.md), [0083](0083-write-path-in-three-tiers.md)

## Context

The architecture chapters ([docs/architecture/](../architecture/README.md)) put the resolved view, the identity clusters and the tenant overlay side by side for the first time, and the review of 2026-10-09 ([docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md)) found that three decisions made in different ADRs could not hold together at the scale the core use case names: a Wikidata `full` mirror, local data and OpenAlex on one instance, read by more than one tenant.

[0004](0004-identity-clusters-and-equivalence.md) §4 had the resolved view *rewrite* subjects and entity values to a cluster's canonical ID, and [0013](0013-postgres-storage.md) §5 materialized the rewritten JSON wherever it differed from the source. Together these made the set of materialized entities not "entities in a cluster" but "entities in a cluster plus every entity that references a non-canonical member". With OpenAlex as a second provider most works reference authors, institutions and sources clustered with Wikidata items, so most of the largest table would be materialized, which is the doubling the `source` kind exists to avoid. Tier-1 links are per tenant, so one `same-as` from a local item to `WDQ5` (human) would make the local item canonical for that tenant and require an overlay row, a rewritten body and an RDF delta for every one of the ten million items with `P31 = WDQ5`: one edit, ten million rows per tenant, with the synchronous budget of [0013](0013-postgres-storage.md) §7 bounding the editor's latency but not the work (F-01-1, F-04-1, F-02-7).

[0013](0013-postgres-storage.md) §5 keyed tenant overlays on records in the tenant's own partitions. But provider order, link properties and normalizer overrides ([0004](0004-identity-clusters-and-equivalence.md) §9), the provider list ([0018](0018-tenants.md) §5), the constraint roles ([0031](0031-property-constraints.md) §1) and the term languages ([0070](0070-shallow-entity-mirroring.md) §6) were tenant configuration, and each changes the resolved result of every multi-source entity without a record in any tenant partition. Two tenants with different policy could not share a row, and the only implementations were a full copy of the view per tenant or re-resolution on every read (F-19-14, F-06-3). The cluster tables and `view.entity_source` had no tenant column where the overlay rule needed one (F-01-4, F-04-4), and the shared quad store held one resolved graph per tenant, so forty tenants mirroring Wikidata would hold forty copies (F-02-1). `view.term` under a full mirror was an open question ([0070](0070-shallow-entity-mirroring.md) §6) bounded by a tenant setting that shared rows cannot read (F-04-6), and `entity_ref` at 10^9 rows was keyed on text IDs and statement GUIDs (F-01-10).

The identifier decision here follows the rule [0029](0029-resolver-namespaces.md) and [0004](0004-identity-clusters-and-equivalence.md) §7 already apply to identifier values: normalize at the input, store the canonical form, never rewrite data because a mapping changed. Applied to entity IDs it is three things that this ADR decides together: the stored graph is in source form; every entry point normalizes what it is given; and a layer on the way out rewrites identifiers to the form the consumer asked for. The read-side rewrite is not new either: [0018](0018-tenants.md) §5 and [0078](0078-entity-sources.md) §8 already rewrite IDs on read between tenants, and this ADR generalizes that rewrite from "provider form per tenant" to "the form this consumer prefers", with the cluster map as one more rewrite table.

### Direction

James's direction, from the design review of 2026-10-09:

- On shared rows under one policy: **"I would go with Option A. It does reduce customization per-tenant, but I think performance of the graph as a whole is more important here. I think we could incorporate Option B a little by allowing the addition of configuration profiles later on, but we only implement the one for the whole instance."**
- On the stored form: **"I wonder if we want consistent internal form for the internal graph for lookup purposes (basically the original IRI), and then aliases and alternate forms are rewritten as part of query planning. But the idea is that user inputs are normalized according to the internal representation before the query executes."**
- On responses: **"What if, once the query results were returned, a middle layer rewrote the results to provide the entity identifiers expected by the consumer? This would be done with known exact matches only."**

## Decision

### 1. The internal graph is in source form (amends 0002 §3 and §6, 0004 §4, 0013 §5, 0014 §7, 0018 §7 and 0032 §2; extends 0017 §1 and 0032 §3)

*Current text: [00](../architecture/00-overview.md) §2.1, §2.2, §5; [02](../architecture/02-graphs-rdf-and-query.md) §5, §6.2; [03](../architecture/03-storage-caches-and-search.md) §4.1, §4.2, §5, §11.1; [04](../architecture/04-entities-and-identifiers.md) §4.4, §4.9, §6; [05](../architecture/05-providers-and-ingest.md) §1.4, §2.3.*

### 2. Normalization at every entry point (extends 0012 §5, 0014 §8, 0017 §1, 0044 §1 and §3 and 0059 §2; amends 0010 §3, 0012 §4 and 0014 §7)

*Current text: [00](../architecture/00-overview.md) §2.2, §2.6; [02](../architecture/02-graphs-rdf-and-query.md) §7.2; [03](../architecture/03-storage-caches-and-search.md) §11.1, §11.6; [04](../architecture/04-entities-and-identifiers.md) §2.1; [18](../architecture/18-api.md) §1.4, §2.3, §3.1; [19](../architecture/19-site-ui.md) §2.1.*

### 3. Rewriting in responses, with exact matches only (extends 0003 §2, 0012 §4, 0014 §3 and §4, 0032 §1, 0044 §1 and 0078 §8; amends 0004 §4, §5 and §6 and 0018 §5 and §7)

*Current text: [00](../architecture/00-overview.md) §2.2, §2.6, §5; [02](../architecture/02-graphs-rdf-and-query.md) §4.1, §6.1; [03](../architecture/03-storage-caches-and-search.md) §9.3, §12.1; [04](../architecture/04-entities-and-identifiers.md) §4.9, §4.12, §6; [05](../architecture/05-providers-and-ingest.md) §2.3, §6.7; [08](../architecture/08-tenants-and-instances.md) §4.1; [18](../architecture/18-api.md) §1.6, §2.3, §3.1; [19](../architecture/19-site-ui.md) §2.1, §4.2.*

### 4. RDF: the fused body under every member (amends 0004 §4; extends 0001 §1 and 0032 §2)

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §2.1, §5; [04](../architecture/04-entities-and-identifiers.md) §4.4.*

### 5. Shared rows are computed under a named instance policy (amends 0004 §9, 0013 §5, 0018 §3, §5 and §6, 0031 §4 and 0070 §6; extends 0031 §1, 0032 §4 and 0059 §3 and §4)

*Current text: [00](../architecture/00-overview.md) §2.1, §2.3, §5; [02](../architecture/02-graphs-rdf-and-query.md) §7.3; [03](../architecture/03-storage-caches-and-search.md) §4.1, §4.2, §5, §7; [04](../architecture/04-entities-and-identifiers.md) §4.4, §4.10; [05](../architecture/05-providers-and-ingest.md) §1.4; [08](../architecture/08-tenants-and-instances.md) §2.5, §4.1, §5.1, §9.1, §10.5; [23](../architecture/23-configuration-and-registry.md) §2.2, §2.3.*

#### 5.1 Tenant columns and the overlay-exclusion rule (amends 0004 §3, 0013 §5, 0014 §7 and §10 and 0018 §6)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.1, §4.2, §4.3, §4.4, §5, §7, §11.1, §11.4, §12.5; [04](../architecture/04-entities-and-identifiers.md) §4.3.*

#### 5.2 Roles at instance scope (extends 0031 §1; amends 0003 §7 and 0031 §4)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §7; [06](../architecture/06-statements-and-properties.md) §1, §2.4; [08](../architecture/08-tenants-and-instances.md) §2.5; [23](../architecture/23-configuration-and-registry.md) §2.2, §2.3.*

### 6. `view.term` and `view.entity_ref` at scale (amends 0013 §5 and §11, 0014 §8, 0018 §6 and 0070 §4 and §6)

*Changed by A2.*

*Current text: [00](../architecture/00-overview.md) §2.8; [03](../architecture/03-storage-caches-and-search.md) §4.3, §5, §7, §11.6; [05](../architecture/05-providers-and-ingest.md) §5.6, §5.8; [19](../architecture/19-site-ui.md) §4.2; [22](../architecture/22-crates-and-stack.md) §6; [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 7. Crates (extends 0005 §2 and 0060 §11)

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Rewrite on write, as [0004](0004-identity-clusters-and-equivalence.md) §4 had it.** Correct for a handful of local items linked to Wikidata; O(referrers) per tenant per link at the scale the core use case names, and it is what forced `entity_ref` to exist at 10^9 rows.
- **Rewrite on write, with tier-1 links as instance acts.** Halves the problem (one canonical per cluster for everyone) and does nothing about OpenAlex works becoming materialized by referencing clustered institutions.
- **Store the rewritten form as a diff over the source record.** Keeps `source` kind for most entities, but the fan-out is still O(referrers) rows and it is a new storage shape.
- **Read-time rewrite on output only, without input normalization.** Works for display; a consumer could not round-trip what it saw, and a Wikidata-keyed bot would see mixed IDs.
- **Resolution profiles now.** Honest and general, and the shape this ADR leaves room for; it multiplies every shared table by the profile count for a second tenant that does not yet exist.
- **Re-resolve on read under the tenant's policy.** No storage cost; O(contributing graphs) per read, uncached across tenants, and API latency depending on how many providers an entity has.

## Consequences

- **A cluster change is O(members).** Linking a local item to `WDQ5` touches the cluster's rows and no referrer; the overlay holds only what a tenant asserted something about.
- **The internal graph is stable.** Nothing in storage depends on cluster membership, so a cluster can be split or joined without rewriting a row, and the dump and stream never emit a referrer's triples for an identity change.
- **Consumers see their own IDs.** A Wikidata-keyed bot sees `WDQ…` everywhere, a local user sees `Q…` everywhere, both looking at one stored form; the price is that two views of one tenant can differ, and every entry point must normalize.
- **Tenants on a shared instance lose a freedom [0004](0004-identity-clusters-and-equivalence.md) §9 promised**: provider order among shared providers and shared normalizers are the instance's. The tenancy presets must say so, and profiles are the escape hatch.
- **Milestone 1 already behaves this way** for a single tenant with no clusters; the changes land in stage 3 (the full mirror) and with the first cluster work. `scatter-wikibase-resolve` as built fuses one local-plus-mirror case and rewrites nothing, which is the invariant of §1.

## Open questions

- **Q1. The exact-match map for provider-tenant links.** [0080](0080-tenants-as-entity-sources.md) §2 makes a provider tenant's `same-as` records tier-2 links for readers; whether those count as exact matches for rewriting on a reader that has not itself asserted anything about the entities is open. The conservative reading, taken for now, is yes, since they are explicit assertions of identity by a source.
- **Q2. A short form of `prefer` in URLs.** Whether the site UI's local preference should be expressible in a sharable URL, so that a link copied from a Wikidata-form view opens in the same form.

## References

- Wikibase redirects: a redirect target is substituted on read and `owl:sameAs` is emitted in RDF; the precedent for leaving stored values alone
- [docs/audits/architecture-review-2026-10-09.md](../audits/architecture-review-2026-10-09.md): findings F-01-1, F-01-4, F-01-10, F-02-1, F-02-7, F-04-1, F-04-4, F-04-6, F-06-2, F-06-3, F-16-7, F-19-14; decisions G1–G7

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [00](../architecture/00-overview.md), [02](../architecture/02-graphs-rdf-and-query.md), [03](../architecture/03-storage-caches-and-search.md), [04](../architecture/04-entities-and-identifiers.md), [05](../architecture/05-providers-and-ingest.md), [06](../architecture/06-statements-and-properties.md), [08](../architecture/08-tenants-and-instances.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.

### A2. The instance's content languages

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while building the shared term languages, 2026-11-04; unconfirmed
- **Change:** extends §6
- **Summary:** The default of `entities.term_languages` for the shared rows, "the instance's content languages, `mul` and `en`", reads the instance's content languages as every tenant's `site.language` and the instance's default for it (`template:site:site.language`): the shared rows serve every tenant, and each tenant's pages show labels in its own content language, so each of those languages has rows. The instance's own value is the `tenancy:entities.term_languages` ceiling, which replaces the default. Built for the Wikidata benchmark, whose shared `view.term` held 110.8M rows in 629 languages for 2M mirrored entities before the rule was applied (`docs/testing/performance.md` §8).

