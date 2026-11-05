# 23. Configuration and registry

This chapter covers where Triplespace's configuration lives and what it consists of: the `config` partition and its record shape, every config kind and the scope each belongs to, every named setting the ADRs define, the registry files under `docs/registry/` that an instance's configuration starts from, and the deployment configuration and command-line tools that sit outside the log. It assumes the log, partitions and record format of [01](01-log-and-records.md), the graph registry and the registry of record of [02](02-graphs-rdf-and-query.md), the `view.registry` projection of [03](03-storage-caches-and-search.md), and the tenant model of [08](08-tenants-and-instances.md), which holds the tenancy policy and its presets. Permissions on configuration (`ts-config`, `ts-locked`) are in [09](09-security-and-moderation.md); what `meta=siteinfo` and `Special:Version` report of it is in [18](18-api.md) and [21](21-special-pages.md).

## 1. The `config` partition

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3.*

### 1.1 One source partition for instance configuration

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3.*

A source partition is registered for instance configuration. It corresponds to Scatterbase's `server` graph, and the two share one record shape so that Scatterbase can adopt it.

| Graph | IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Config** | `{base}/graph/config` | Source | Administrators, through the configuration API ([0016](../decisions/0016-permissions-and-access-control.md)) | Full | `logged` | Public |

**Records.** The payload type is `scatter:v0/config`. The header key is `{kind}:{code}`, which is the primary key of `view.registry` ([03](03-storage-caches-and-search.md)). The content part holds the entry and a per-kind `schema` integer naming the shape the entry was written in; the comment part holds the reason for the change; the attestation names who made it. A later record for the same key replaces the entry, and a record whose content is null retires it. `view.registry` is the projection of the latest record per key, storing the entry opaquely with its schema number: a record of a kind or schema the running build does not know is projected opaquely and never fails replay, so a newer registry or an older binary can meet in one log.

[0004](../decisions/0004-identity-clusters-and-equivalence.md) §9's "configuration is recorded in the log" names this partition. [0006](../decisions/0006-log-integrity-and-erasure.md) §4's `logged` policy for "the configuration partition" and §6's key registration have their home here, and 0006 §9's export bundle carries "the configuration records that register and rotate keys" because the partition is public.

### 1.2 Order of the first records

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3.*

The instance `config` partition, found by its registry name, has a random ID like every other partition; its record at offset 0 is the instance's first `key:` record, as [0006](../decisions/0006-log-integrity-and-erasure.md) §6 requires. That record carries the **founding attestation**, the third attestation form beside the actor's and the instance's ([01](01-log-and-records.md) §2.4): it names no actor, it is signed by the key it registers, so that it certifies itself, and the issuer code the instance acts under afterwards, the farm code, is derived from its leaf hash ([08](08-tenants-and-instances.md) §3.5). The records at offsets 1 and 2 of the instance `config` are attested by `instance:{farm code}`, the actor that derivation yields. A tenant's `config` begins the same way: its offset 0 is a `key:` record under a founding attestation whose authority is `(instance config, 0)`, and the tenant's issuer code is derived from that leaf; so a single-tenant instance needs no authority record and no `ts-prerogative` path to exist before its first entity.

Every other partition is created by a `graph:` record in `config`, appended before that partition's first record, and has no genesis record of its own: the partition's name, hash function, segment exponent `k` and policies are that `graph:` record, which the export bundle carries. The `config` partition's own `graph:config` record follows its key record at offset 1.

### 1.3 Scope: instance and tenant

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §1.*

The kinds split by scope ([0018](../decisions/0018-tenants.md) §3, in [08](08-tenants-and-instances.md)). The instance's `config` holds `key`, `graph`, `provider`, `issuer`, `keyed-type`, `tenant`, `alias`, `primary`, `tenancy`, `template`, `setting`, `consumer`, `forwarder` and `reports`, the instance lists of `sitelink-policy` and `federation-policy`, global `group`s, `page-repo`, and the instance records of `reconcile` and `role` that shared rows are computed under (§2.3); `file-repo` is tenant or instance; each tenant's `config` holds the rest, `provider-readers` among them. This paragraph is the one list of instance kinds, and [0013](../decisions/0013-postgres-storage.md) §5.5's `view.registry` refers to it. A tenant's `config` begins with a `key:` record, the current instance key, and every `key:` record of the instance is appended to it as well, so a tenant's partitions verify from the tenant's bundle alone ([0018](../decisions/0018-tenants.md) §2).

`view.registry`'s key is `(tenant, kind, code)`; instance-level kinds have the empty-string tenant ([03](03-storage-caches-and-search.md)).

### 1.4 Templates, locks and the ceiling

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0085](../decisions/0085-settings-and-preferences.md) §2.1.*

Under `config.template = defaults` or `locks` ([0028](../decisions/0028-tenancy-policy.md) §8, in [08](08-tenants-and-instances.md)), the instance `config` may hold records of kind `template`, keyed `template:{kind}:{code}`, each a tenant-config record new tenants start from; `tenancy:locked` lists the locked keys. Any `site` setting can be given an instance default or be locked this way, the `wikitext.*` settings among them. A `site` template is not written into a new tenant: it is the instance's **default** for the setting, read whenever a tenant without a record of its own resolves it (§3.1; [0085](../decisions/0085-settings-and-preferences.md) §2.1). Templates of every other kind are written at creation. A tenancy switch of instance scope may also cap what tenants may turn on: `wikitext.ceiling` (`expansion` or `lua`; `lua` in every preset) bounds `wikitext.lua`, and the operator lowers it to bound CPU on a crowded farm. Expansion itself is part of wikitext and is not capped ([0042](../decisions/0042-template-expansion-and-parsoid.md) A19); a `subset` ceiling written before then caps Lua as `expansion` does.

## 2. The config kinds

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0029](../decisions/0029-resolver-namespaces.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5, §5.2.*

### 2.1 Kinds defined by `scatter-log`

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0081](../decisions/0081-recovery-keys-and-continuations.md) §9.*

Five kinds are defined by `scatter-log`: two because the log cannot start without them, and three that carry a tenant's chain of custody, which Scatterbase's server identity needs too ([0081](../decisions/0081-recovery-keys-and-continuations.md) §9):

| Kind | Code | Entry |
|---|---|---|
| `key` | The signed-note key ID ([0006](../decisions/0006-log-integrity-and-erasure.md) §6) | The instance's public key. Rotation is a new `key:` record whose attestation is signed by the previous key |
| `graph` | The graph name ([02](02-graphs-rdf-and-query.md)) | The registry entry of [0005](../decisions/0005-crate-organization.md) §4.1: IRI, kind, writers, history, integrity and export policies, and, for a partition, its number, segment exponent k and hash function |
| `recovery-key` | `recovery-key` | A tenant's recovery keys, threshold and delay, with the recovery signatures that authorize the record ([08](08-tenants-and-instances.md) §6.8) |
| `continuation` | `continuation:{n}` | A move the old instance did not sign: the old checkpoints continued from, tail ranges, the new key and origin host, mode and recovery signatures ([08](08-tenants-and-instances.md) §6.8) |
| `continuation-cancel` | `continuation-cancel:{n}` | Voids continuation `n` within its delay, signed by the authorizing set ([08](08-tenants-and-instances.md) §6.8) |

### 2.2 The catalogue of Triplespace's kinds

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0029](../decisions/0029-resolver-namespaces.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5, §5.2; [0085](../decisions/0085-settings-and-preferences.md) §1, §7; [0086](../decisions/0086-theming.md) §1.*

The remaining kinds are Triplespace's, and each product declares its own. The scope column is [0015](../decisions/0015-record-format-and-partition-registry.md) §3's: where its table says nothing, the scope is the one its scope paragraph implies (§1.3).

| Kind | Code | Scope | Entry | Defined in |
|---|---|---|---|---|
| `provider` | The provider code | Instance | The provider registry entry | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4, [0011](../decisions/0011-logs.md) §4 |
| `issuer` | The issuer code | Instance | The issuer registry entry | [0007](../decisions/0007-actor-identity.md) §1 |
| `namespace` | The namespace number | Tenant | The namespace entry | [0008](../decisions/0008-namespaces-and-document-pages.md) §1 |
| `keyed-type` | The type name | Instance | The keyed-type entry | [0009](../decisions/0009-keyed-entity-types-and-domain.md) §1 |
| `role` | The role name | Tenant, and an instance-scope record beside the tenant records: the bindings the constraint checker uses for shared rows and the only bindings an adapter reads ([0082](../decisions/0082-source-form-and-the-shared-view.md) §5.2) | The properties bound to a role | [0003](../decisions/0003-statement-ui.md) §7, [0004](../decisions/0004-identity-clusters-and-equivalence.md) §6 |
| `reconcile` | `default` or a provider code | Instance: `reconcile:default` is the policy record shared rows are computed under; a tenant's record narrows it (§2.3) | Instance: the provider order among registry providers, an optional `order_by_type` override, the tier-2 link properties, the identifier properties for inference, normalizer overrides, the role bindings for shared rows, the languages that populate shared `view.term`, reconciliation rules. Tenant: a hidden provider, a narrower display-language list, link properties and ordering for the tenant's own entity sources, inference off for itself | [0004](../decisions/0004-identity-clusters-and-equivalence.md) §9; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5 |
| `site` | A setting name | Tenant, and instance for the settings §3.3 lists | A setting's value: the site's name, language and logo, the recent-changes window, checkpoint cadence and the other settings of the catalogue in §3 | [0006](../decisions/0006-log-integrity-and-erasure.md) §6, [0010](../decisions/0010-site-ui.md) §7, [0085](../decisions/0085-settings-and-preferences.md) §7 |
| `setting` | The setting name | Instance | A registered `site` setting: scope, bound, group, type, choices and default, copied from `docs/registry/settings.toml` and synced, or the instance's own (§3.1) | [0085](../decisions/0085-settings-and-preferences.md) §1 |
| `group` | The group name | Tenant; a global group, which carries `scope`, is instance ([0028](../decisions/0028-tenancy-policy.md) §8) | A permission group | [0016](../decisions/0016-permissions-and-access-control.md) §3 |
| `tenant`, `alias` | — | Instance | A tenant; a base-URI change | [0018](../decisions/0018-tenants.md) §3, §9 |
| `primary` | `primary` | Instance | The primary tenant's slug and, for a transfer, the offer it accepts; one current record | [0046](../decisions/0046-primary-tenant.md) §2 |
| `providers` | `list` | Tenant | The providers whose graphs contribute to this tenant's view and search, a selection from the instance's registry providers in the instance's order: the list narrows what the tenant reads and never reorders it; the default is Wikidata | [0018](../decisions/0018-tenants.md) §3, §5; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5 |
| `thread-status` | The status name | Tenant | A thread status with its category and order | [0019](../decisions/0019-discussions.md) §6 |
| `site-alias` | A MediaWiki site ID | Tenant | The host, article path and language of an aliased site | [0026](../decisions/0026-sitelinks.md) §2 |
| `sitelink-policy` | `deny` (instance), `mode` or `list` (tenant) | Both | Sitelink allow and deny lists | [0026](../decisions/0026-sitelinks.md) §3 |
| `view-pin` | `{property}` or `{entity}:{property}` | Tenant | A tenant-wide shape pin | [0027](../decisions/0027-preferences-and-portability.md) §5 |
| `tenancy` | A switch name | Instance | The tenancy policy ([08](08-tenants-and-instances.md)) | [0028](../decisions/0028-tenancy-policy.md) §1 |
| `template` | `{kind}:{code}` | Instance | A tenant-config record new tenants start from; `tenancy:locked` lists the locked keys | [0028](../decisions/0028-tenancy-policy.md) §8 |
| `palette` | The palette's name | Tenant or instance | A palette: label, shape tokens, the palette it extends, whether it is for accessibility, and its `light`, `dark`, `light-more` and `dark-more` colour variants, each contrast-checked | [0086](../decisions/0086-theming.md) §1 |
| `typeset` | The type set's name | Tenant or instance | A type set: label, the three `font-family-*` roles, per-language stacks, its typefaces (shipped, or font files named as site assets with their licences), and whether it is for accessibility | [0086](../decisions/0086-theming.md) §1, §3 |
| `provider-readers` | `mode` or `list` | Tenant | Which tenants may read this provider tenant | [0028](../decisions/0028-tenancy-policy.md) §5 |
| `resolver` | The resolver name | Tenant | A resolver namespace: binding, grammar, normalizer with case rule, external IRI. Adding a resolver is a `config` record with `ts-config`; binding its role is the ordinary `role` record | [0029](../decisions/0029-resolver-namespaces.md) §1, §8 |
| `federation-policy` | `deny` (instance), `mode` or `list` (tenant) | Both | Fediverse domain allow and deny lists | [0022](../decisions/0022-federation.md) §8 |
| `consumer` | The consumer slug | Instance | A registered OAuth consumer, its owner, redirect URIs, requested grants and status | [0025](../decisions/0025-oauth-server.md) §2 |
| `forwarder` | The forwarder key ID | Instance | A forwarder key's label and dates, never its secret or hash; a null record revokes it | [0057](../decisions/0057-web-tier.md) §10 |
| `consumer-policy` | `list` | Tenant | Which approved consumers may be authorized, and which are auto-approved | [0025](../decisions/0025-oauth-server.md) §2 |
| `tag` | The tag name | Tenant | A user-defined change tag, its description, whether it is active, and the group that may apply it | [0030](../decisions/0030-edit-filters.md) §5 |
| `category-mapping` | The mapping name | Tenant | A category name or pattern, and the page statement its members get | [0038](../decisions/0038-page-metadata-and-categories.md) §5 |
| `page-repo` | The repository name | Instance, one per provider | A page repository, its kind (`tenant` or `mediawiki`), provider, served namespaces, mode, `titles`, events, licence and display name. A tenant selects repositories with `pages.repos` (§3.2), whose entries carry the tenant's own `shadowed`, `talk` (default `link`, [0069](../decisions/0069-synchronized-talk-pages.md) §1) and `cache_ttl` per repository (§3.5). Replaces `template-repo` | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1 |
| `reports` | `default` or a tenant slug | Instance | Which reports run as batch, their mirror-graph widenings, the batch schedule and the row limit; written with `ts-config` at the farm base | [0047](../decisions/0047-special-pages.md) §4.3 |
| `extraction` | The source name | Tenant | An extraction source: its repository, pages, extractors, mappings, line settings, page subject and reference properties | [0071](../decisions/0071-derived-statements-from-mirrored-pages.md) §2 |
| `template-mapping` | The mapping name | Tenant | A template, conditions, subject, match keys, label and statements | [0072](../decisions/0072-template-mappings.md) §2 |
| `value-map` | The map name | Tenant | Strings to values, for parsers and heading facets | [0072](../decisions/0072-template-mappings.md) §4 |
| `publication` | The publication name | Tenant | A destination wiki, account, scope, rows, data pages and kits | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §1 |
| `jsonld-context` | The profile name | Tenant | The schema.org types, property terms, intervals and identifiers of a JSON-LD profile | [0076](../decisions/0076-dataset-publication.md) §2 |
| `entity-source` | The source's name | Tenant | An entity source: label, entity types with upstream prefixes, ID grammars and IRI templates, namespace, `api`, `entity_data`, `revision_ids`, `events`, `role`, licence, its assigned provider number and `promoted_to` | [0078](../decisions/0078-entity-sources.md) §1 |

A **file repository** is also a `config` record, of kind `file-repo`, keyed `file-repo:{name}`, either in a tenant's `config` or in the instance `config`, where a tenant refers to it by name; it can be supplied by a tenancy template ([0039](../decisions/0039-files-and-media.md) §11, in [12](12-files-and-media.md)).

### 2.3 The `reconcile` record is the instance's policy record

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0082](../decisions/0082-source-form-and-the-shared-view.md) §5, §5.2.*

The instance `reconcile:default` record is the **policy record** under which the shared rows (`tenant = ''`) of every `view` table are computed ([03](03-storage-caches-and-search.md)). It holds, for shared rows, the provider order among registry providers, an `order_by_type` override, the tier-2 link properties, the identifier properties that drive tier-3 inference, the normalizer overrides, the reconciliation rules, the role bindings the constraint checker and the adapters read (the instance-scope `role` record of §2.2), and the languages that populate shared `view.term` (`entities.term_languages`, §3.2). Each shared row names the policy record it was computed under, so that resolution profiles, one shared view per policy record, can be added later by adding records and rebuilding a projection, with no change to the log; only `default` exists.

**A tenant's `reconcile` record narrows; it does not reorder.** A tenant may hide a provider in its own view and API, restrict its own display languages, switch inference off for itself, and add link properties and ordering for its own entity sources, whose graphs are read per tenant in any case ([05](05-providers-and-ingest.md)). It may not reorder shared providers, change a shared normalizer, add inference properties or rebind a shared role; a tenant-scope write that tries is refused with `ts-instance-policy`, and the tenancy presets say so ([08](08-tenants-and-instances.md)). The operations the record governs are in [04](04-entities-and-identifiers.md).

### 2.4 ACLs are not config kinds

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3.*

ACLs were first listed as a config kind. Under [0023](../decisions/0023-moderation.md) §3 they are records of their own payload type, `scatter:v0/acl`: graph ACLs are appended to `config`, and page, entity, record and actor ACLs to the tenant `log` partition ([09](09-security-and-moderation.md)).

## 3. The settings catalogue

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0075](../decisions/0075-mcp-server.md) §6; [0077](../decisions/0077-special-version.md) §13; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

### 3.1 Four places a setting can live

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0033](../decisions/0033-backend-stack.md) §12; [0077](../decisions/0077-special-version.md) §13; [0085](../decisions/0085-settings-and-preferences.md) §1, §2.*

A named setting of the form `something.something` is one of four things, and the tables below are split that way:

- A **`site` setting**: a `config` record of kind `site` whose code is the setting name (§2.2). Most are a tenant's (§3.2); a few are the instance's, written with `ts-config` at the farm base (§3.3). A record that breaks a requirement is refused when it is written, as any invalid `site` record is.
- A **tenancy switch**: a `config` record of kind `tenancy` in the instance `config`, with preset values (§3.4; the presets are in [08](08-tenants-and-instances.md)).
- A **field of another config record** (§3.5): `mirror.set` on a `page-repo`, `lines.identifiers` on an `extraction`, and the like.
- **Deployment configuration** (§3.6): read by `figment` from file and environment, never a log record, as the Postgres and OpenSearch addresses are. Pool sizes, concurrency caps and intervals are deployment configuration, never `site` records: how many thumbnails render at once or how often a batch file is written is a property of the machine, not of a wiki.

A dotted setting key an ADR names without saying which `config` holds it is a tenant `site` setting; the catalogue marks the keys this rule settles as "`site` (by rule, §3.1)".

**Every tenant setting has a bound**, the "Bound" column of §3.2, which says who has the last word:

- **`tenant`**: the tenant's record is the value, and the instance can only provision or lock it through a template (§1.4).
- **`tenant ≤ instance ceiling`**: the tenant's record is the value up to a ceiling the instance sets as a tenancy switch (§3.4), and a record above the ceiling is refused. `wikitext.ceiling` is the switch for the `wikitext.*` settings; a budget or limit (`projections.sync_budget`, `projections.sync_time`, `scopes.max_members`, `sprints.max_tasks`, `query.max_results`, `query.timeout`, `schemas.max_bound`) is capped by a switch of the setting's own name, whose preset value is the largest a tenant may set; and a list a tenant may only narrow (`entities.term_languages`) has the instance's list as its ceiling.
- **`instance`**: the setting exists once, in the instance `config` (§3.3), and a tenant-scope record of that name is refused with `ts-instance-policy` (§2.3).

Preference keys, which an account owns, are listed apart in §3.7 because they share the dotted form and are not settings.

**The catalogue is a registry.** `docs/registry/settings.toml` registers every `site` setting with its scope, bound, group, type, choices and default, and the tables of §3.2 and §3.3 are generated from it by `docs/architecture/settings.py`, which CI runs; a table edited by hand fails the check ([0085](../decisions/0085-settings-and-preferences.md) §1). The same entries drive the one validator every writer calls (§5.2), the settings pages and `GET /settings/schema` ([18](18-api.md) §3.2; [19](19-site-ui.md) §2.8).

**A setting resolves, first match wins** ([0085](../decisions/0085-settings-and-preferences.md) §2.1). A tenant setting resolves to the tenant's own record; then the instance's default, its `template:site:{key}` record under `config.template = defaults` or `locks` (§1.4), read when the value is needed; then the registry default. A locked key resolves to the instance's value, and a value a lowered ceiling has left above it resolves to the ceiling. An instance setting resolves to the instance's record, then the registry default. Each resolved value carries its origin, `registry`, `instance`, `tenant` or `locked`, and `capped` where a ceiling applied. A `site` record in a tenant's `config` that carries an instance attestation (a locked template's copy, or a provision written before 0085) is not the tenant's value, and resolution passes over it. An instance default is validated against the ceiling as a tenant value is, and for a list capping is the intersection; a reference-typed default is resolved in each tenant when read, falling back to the registry default where it names nothing the tenant has. Changing an instance default runs, for every inheriting tenant, what changing the tenant's own record runs (render-epoch bumps, refresh queues, jobs, `siteinfo`), fanned out by an instance job that writes nothing into the tenants' logs. A tenant's bundle carries only the records it wrote, so a tenant that moves keeps what it set and takes the new instance's defaults for the rest.

### 3.2 Tenant `site` settings

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0075](../decisions/0075-mcp-server.md) §6; [0081](../decisions/0081-recovery-keys-and-continuations.md) §9; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §1, §7; [0087](../decisions/0087-account-registration-and-challenges.md) §8; [0088](../decisions/0088-external-identity-providers.md) §9; [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §11; [0092](../decisions/0092-following-recent-changes.md) §6.*

The table is generated from `settings.toml` (§3.1). Each row cites the ADR section that defines the setting. "Scope" repeats what that section says; "`site` (by rule, §3.1)" means the section names the setting without saying which `config` holds it, and the rule of §3.1 makes it a tenant `site` setting. "Bound" is §3.1's: `tenant`, `tenant ≤ instance ceiling`, or `instance`. A default of "—" means the section gives none.

<!-- settings.py: tenant -->
| Setting | Scope | Bound | Values | Default | Meaning | Source |
|---|---|---|---|---|---|---|
| `account.approval` | `site` | tenant | off, on | off | Whether registering files a request that a holder of `confirmaccount` approves, instead of creating the account at once ([07](07-actors-and-accounts.md) §2.6) | [0087](../decisions/0087-account-registration-and-challenges.md) §6 |
| `account.creation_limit` | `site` | tenant | 1 to 10000 | 6 | How many accounts one address key may create (and requests it may file) in a day before `createaccount` answers `acct_creation_throttle_hit` ([07](07-actors-and-accounts.md) §6.2); `noratelimit` is exempt. MediaWiki's `$wgAccountCreationThrottle` | [0087](../decisions/0087-account-registration-and-challenges.md) §4 |
| `account.request_expiry` | `site` | tenant | duration | 30 days | How long an account request stays open before it is deleted | [0087](../decisions/0087-account-registration-and-challenges.md) §6 |
| `account.request_statement` | `site` | tenant | `off`, `optional`, `required` | `optional` | Whether an account request asks for a statement of up to 1,000 characters | [0087](../decisions/0087-account-registration-and-challenges.md) §6 |
| `blocks.max_per_page` | `site` (by rule, §3.1) | tenant | integer | 20 | Bounds blocks per render | [0062](../decisions/0062-workspaces.md) §3 |
| `blocks.max_rows` | `site` | tenant | integer | 200 | Bounds every list a block renders | [0062](../decisions/0062-workspaces.md) §3 |
| `blocks.refresh_debounce` | `site` (by rule, §3.1) | tenant | duration | 60 s | Coalesces the refresh of pages whose block dependencies changed | [0062](../decisions/0062-workspaces.md) §3 |
| `category.collation` | `site` | tenant | a collation | MediaWiki's `uppercase` | Sort order of a category's members | [0038](../decisions/0038-page-metadata-and-categories.md) §4 |
| `challenge.bits` | `site` | tenant | 8 to 32 | 20 | The base difficulty of a challenge, in leading zero bits of SHA-256: about 2^bits hashes | [0087](../decisions/0087-account-registration-and-challenges.md) §3, §4 |
| `challenge.max_bits` | `site` | tenant | `challenge.bits` to 32 | 24 | The highest difficulty: one bit is added for each account created from the caller's address key in the last 24 hours, up to this | [0087](../decisions/0087-account-registration-and-challenges.md) §4 |
| `challenge.triggers` | `site` | tenant | `createaccount`, `tempaccount`, `badlogin` | all three | The acts a challenge guards ([07](07-actors-and-accounts.md) §2.6); the empty list turns challenges off | [0087](../decisions/0087-account-registration-and-challenges.md) §2 |
| `constraints.scope` | `site` | tenant | `local`, `overlay`, `all` | `local` | Which entities the constraint consumer checks on composition: `local`, the local-graph statements and the entities in the tenant's overlay; `overlay`; or `all`, which is a batch job with progress and never a per-event step ([06](06-statements-and-properties.md)) | [0083](../decisions/0083-write-path-in-three-tiers.md) §3 |
| `content.licence` | `site` | tenant | a licence | unset; a tenant with no licence set is warned, not refused | The tenant's content licence, checked against a repository's when it is added to `pages.repos` and on a push | [0053](../decisions/0053-mirrored-pages.md) §9; [0068](../decisions/0068-merging-with-upstream.md) §3; [0076](../decisions/0076-dataset-publication.md) §4 |
| `discussion.share` | `site` | tenant | off, on | on under `discussion.cross_tenant = read-only` | Whether a provider tenant's threads are shown read-only on reading tenants | [0028](../decisions/0028-tenancy-policy.md) §6 |
| `dumps.keep` | `site` (by rule, §3.1) | tenant | integer | 7 | Previous dumps of each format kept per scope | [0076](../decisions/0076-dataset-publication.md) §1 |
| `entities.closure` | `site`, per provider | tenant | a property list | 0070 §2.3's list for Wikidata; empty for an entity source | Properties whose values are fetched transitively from every mirrored entity | [0070](../decisions/0070-shallow-entity-mirroring.md) §2.3; [0078](../decisions/0078-entity-sources.md) §4 |
| `entities.closure_depth` | `site` | tenant | integer | 6 | Depth of the closure | [0070](../decisions/0070-shallow-entity-mirroring.md) §2.3 |
| `entities.label_ttl` | `site` | tenant | duration | 7 days | Lifetime of the label cache | [0070](../decisions/0070-shallow-entity-mirroring.md) §4 |
| `entities.mirror` | `site`, per provider and entity type | tenant | `off`, `on-demand`, `linked`, `all` | `linked`; for an entity source, `linked` with an `api` and `off` without | What of a provider's entities is mirrored; properties are always at least `on-demand` | [0070](../decisions/0070-shallow-entity-mirroring.md) §2.1; [0078](../decisions/0078-entity-sources.md) §4 |
| `entities.term_languages` | `site`; the instance record is the policy's | tenant ≤ instance ceiling | language list | instance: the instance's content languages (every tenant's `site.language` and the instance's default for it), `mul` and `en`; tenant: the instance's list | The instance record sets the languages in which a mirrored entity's terms populate the shared rows of `view.term` (§2.3); a tenant's record only narrows the languages its own pages and API display, and never adds a language to the shared rows | [0070](../decisions/0070-shallow-entity-mirroring.md) §6; [0082](../decisions/0082-source-form-and-the-shared-view.md) §6, A2 |
| `export.from_namespaces` | `site` | tenant | off, on | — (MediaWiki's `$wgExportFromNamespaces`) | Allows `addns` on `Special:Export` | [0047](../decisions/0047-special-pages.md) §8 |
| `export.history` | `site` (by rule, §3.1) | tenant | off, on | on | Full history on export (`$wgExportAllowHistory`) | [0047](../decisions/0047-special-pages.md) §8 |
| `export.max_bytes` | `site` (by rule, §3.1) | tenant | size | — | Cap on file contents in an export; over it, URLs are written | [0047](../decisions/0047-special-pages.md) §8 |
| `export.max_entity_history` | `site` (by rule, §3.1) | tenant ≤ instance ceiling | integer | 100 | Revisions per entity exported: the per-entity cap, lower than `export.max_history`, under which the export job folds per-revision entity export | [0047](../decisions/0047-special-pages.md) §4, §8 |
| `export.max_history` | `site` (by rule, §3.1) | tenant | integer | 1,000 | Revisions per page exported (`$wgExportMaxHistory`) | [0047](../decisions/0047-special-pages.md) §8 |
| `export.sync_revisions` | `site` (by rule, §3.1) | tenant | integer | 5,000 revisions | An export up to this size streams | [0047](../decisions/0047-special-pages.md) §8 |
| `federation.accept_fep8b32` | `site` | tenant | off, on | off | Accept FEP-8b32 object integrity proofs inbound | [0022](../decisions/0022-federation.md) §8 |
| `federation.accept_rfc9421` | `site` | tenant | off, on | off | Accept RFC 9421 HTTP message signatures inbound | [0022](../decisions/0022-federation.md) §8 |
| `federation.clock_skew` | `site` (by rule, §3.1) | tenant | duration | twelve hours | A request whose `Date` lies outside ± this is rejected | [0022](../decisions/0022-federation.md) §8 |
| `federation.replay_window` | `site` (by rule, §3.1) | tenant | duration | seven days | How long an accepted activity's `id` is kept in `ops.ap_inbox_seen` | [0022](../decisions/0022-federation.md) §8 |
| `federation.talk_pages` | `site`, per namespace | tenant | namespaces | — | Which talk pages (and boards) are `as:Group` actors | [0022](../decisions/0022-federation.md) §6; [0049](../decisions/0049-boards.md) §10 |
| `feeds.related_limit` | `site` | tenant | integer | 5,000 | Members past which a related-changes relation is truncated | [0020](../decisions/0020-change-feeds.md) §2 |
| `files.commons_repo` | tenant `site` | tenant | a repository name | the one named `commons` if configured | The repository that resolves `commonsMedia` values | [0039](../decisions/0039-files-and-media.md) §12 |
| `files.max_size` | `site` (by rule, §3.1) | tenant | size | — | Upload size limit (`file-too-large`) | [0039](../decisions/0039-files-and-media.md) §5 |
| `files.orphan_grace` | `site` (by rule, §3.1) | tenant | duration | 24 hours | Grace before the orphan sweep deletes an unreferenced object | [0039](../decisions/0039-files-and-media.md) §3 |
| `files.repos` | tenant `site` | tenant | repository names | — | The file repositories a tenant uses, in lookup order | [0039](../decisions/0039-files-and-media.md) §11 |
| `files.share` | `site` | tenant | off, on | — | Whether this tenant's files may serve other tenants as a `tenant` repository; under `providers.between_tenants = operator` only the operator can set it | [0039](../decisions/0039-files-and-media.md) §11 |
| `files.stash_ttl` | `site` (by rule, §3.1) | tenant | duration | 48 hours | Expiry of a stash entry | [0039](../decisions/0039-files-and-media.md) §5 |
| `files.thumb_widths` | `site` (by rule, §3.1) | tenant | widths | — | Requested widths are rounded up to the next entry | [0039](../decisions/0039-files-and-media.md) §6 |
| `follow.interval` | `site`, per follower: the instance record for a registry provider or a page repository, the tenant's for its entity sources | tenant | duration | 1 minute | How often a follower without an event stream polls the wiki's recent changes | [0092](../decisions/0092-following-recent-changes.md) §6 |
| `follow.settle` | `site`, per follower: the instance record for a registry provider or a page repository, the tenant's for its entity sources | tenant | duration | 2 minutes | Within this of now, a poll reads by time from ten seconds before its position rather than by continuation | [0092](../decisions/0092-following-recent-changes.md) §6 |
| `follow.tail` | `site`, per follower: the instance record for a registry provider or a page repository, the tenant's for its entity sources | tenant | duration | 15 minutes | How far behind the head the tail pass re-reads, and how long seen rcids are remembered; PT0S turns the tail pass off | [0092](../decisions/0092-following-recent-changes.md) §6 |
| `fork.allow` | `site` | tenant | off, on | `on` where `pages.repos` is set | Whether pages may be forked | [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `fork.content_revisions` | `site` | tenant ≤ instance ceiling | integer, or `all` | 100 | Revisions, counted from the newest, seeded with their text; the rest of the seeded history is metadata only, and dependencies and archives are never seeded with text. `all` seeds every revision's text, under a ceiling of `all` | [0054](../decisions/0054-forking-a-mirrored-page.md) §3, [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §11 |
| `fork.dependencies` | `site` | tenant | `copy`, `inherit` | `copy` | Whether a fork copies its templates or leaves them to the repository | [0054](../decisions/0054-forking-a-mirrored-page.md) §4 |
| `fork.history_source` | `site` | tenant | `chest:{path}`, `api` | `api` | Where a fork's history is seeded from | [0054](../decisions/0054-forking-a-mirrored-page.md) §3 |
| `fork.max_dependencies` | `site` (by rule, §3.1) | tenant | integer | 2,000 | Stops a fork job that would copy more pages | [0054](../decisions/0054-forking-a-mirrored-page.md) §4 |
| `fork.max_revisions` | `site` | tenant | integer, or `all` | 20,000 | The per-job budget: revisions, with or without text, a seeding job reads before it stops, including those a talk-page conversion reads for attribution; `all` reads every revision | [0054](../decisions/0054-forking-a-mirrored-page.md) §3, [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §7, §11 |
| `fork.refollow` | `site` | tenant | `ask`, `auto`, `never` | `ask` | Whether a fork that no longer differs from upstream is offered or automatically re-followed | [0068](../decisions/0068-merging-with-upstream.md) §5 |
| `fork.refollow_after` | `site` (by rule, §3.1) | tenant | duration | 30 days | Under `auto`, how long a fork must equal upstream before a job deletes it | [0068](../decisions/0068-merging-with-upstream.md) §5 |
| `fork.talk` | `site` | tenant | `follow`, `fork` | `follow` | Default choice for a fork's talk page | [0069](../decisions/0069-synchronized-talk-pages.md) §7, §11 |
| `ingest.hash_mismatch` | `site` | tenant | `keep`, `fail` | `keep` | Whether a snak or reference hash that differs from upstream's is kept or aborts the job | [0006](../decisions/0006-log-integrity-and-erasure.md) §2 |
| `integrity.witness_partitions` | `site` | tenant | partition names | `["config"]` | Which partitions' checkpoints are submitted to the witnesses | [0081](../decisions/0081-recovery-keys-and-continuations.md) §6 |
| `integrity.witnesses` | `site` | tenant | list of witnesses, each an endpoint URL and a signed-note verifier key | empty | C2SP tlog-witness endpoints that cosign the tenant's checkpoints ([01](01-log-and-records.md) §4.3) | [0081](../decisions/0081-recovery-keys-and-continuations.md) §6 |
| `login.issuers` | `site` | tenant | issuer codes with `login = true` and an `oauth` table, or `*` for every one | `*`, every such issuer with a client credential | The external issuers whose logins the tenant admits, of those with a client credential; the instance default is what the auth host offers ([07](07-actors-and-accounts.md) §2.7, §2.8). A save leaving a tenant admitting nothing the auth host offers is refused | [0088](../decisions/0088-external-identity-providers.md) §4; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §10, §16 |
| `login.password` | `site` | tenant | off, on | on, set by `instance create` | Whether the tenant admits a login authenticated by its password; the instance default is whether the auth host offers passwords ([07](07-actors-and-accounts.md) §2.2) | [0007](../decisions/0007-actor-identity.md) §3; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §16 |
| `lua.client_site` | `site` | tenant | a site ID with a site alias | the tenant's own alias | Which site Lua's Wikibase client is, and so which item a page is connected to | [0043](../decisions/0043-lua-modules.md) §9 |
| `lua.ids` | `site` | tenant | map from type letter to `local` or a provider code | empty: every letter means `local` | What a bare one-letter ID means in Lua | [0043](../decisions/0043-lua-modules.md) §8 |
| `lua.unheld_ttl` | `site` (by rule, §3.1) | tenant | duration | 10 minutes | Expiry of a recorded miss on an unheld entity | [0043](../decisions/0043-lua-modules.md) §11 |
| `mcp.enabled` | `site` | tenant | off, on | off | Serve MCP at `{base}/mcp` | [0075](../decisions/0075-mcp-server.md) §1, §6 |
| `mcp.facets` | `site` | tenant | properties with descriptions | — | The properties offered as filters and facets | [0075](../decisions/0075-mcp-server.md) §3, §6 |
| `mcp.instructions` | `site` | tenant | text | — | The server's instructions at initialization | [0075](../decisions/0075-mcp-server.md) §3, §6 |
| `mcp.max_results` | `site` | tenant | integer | 200 | Results per call | [0075](../decisions/0075-mcp-server.md) §5, §6 |
| `mcp.scope` | `site` | tenant | a scope title | none: the tools range over the resolved view | The scope the tools search within by default | [0075](../decisions/0075-mcp-server.md) §1, §6 |
| `mcp.scopes` | `site` | tenant | scope titles | — | Other scopes a client may select per session | [0075](../decisions/0075-mcp-server.md) §1, §6 |
| `media.base_url` | tenant `site` | tenant | URL | `{tenant base}/media` | The origin files are served from | [0039](../decisions/0039-files-and-media.md) §7 |
| `mediainfo.mirror` | `site` | tenant | `off`, `on-demand`, `linked`, `all` | `on-demand` | `entities.mirror` for Wikidata's type `M`, under its own name for compatibility | [0065](../decisions/0065-mediainfo-captions-and-commons.md) §2; [0070](../decisions/0070-shallow-entity-mirroring.md) §2.1 |
| `moderation.delete_group` | `site` | tenant | a group | `sysop` | The default deletion group | [0023](../decisions/0023-moderation.md) §1 |
| `moderation.suppress_group` | `site` | tenant | a group | `suppress` | The default suppression group | [0023](../decisions/0023-moderation.md) §1 |
| `notifications.fediverse_domains` | `site` | tenant | allow and deny lists | — | Domains fediverse notifications may go to | [0021](../decisions/0021-notifications.md) §8 |
| `notifications.retention` | `site` | tenant | duration | 90 days; read rows sooner | When inbox rows are deleted | [0021](../decisions/0021-notifications.md) §3 |
| `nuke.confirm_threshold` | `site` | tenant | integer | 500 | Items above which `Special:Nuke` asks for the target's name | [0047](../decisions/0047-special-pages.md) §7 |
| `oauth.access_ttl` | `site` | tenant | duration | four hours | Access token lifetime | [0025](../decisions/0025-oauth-server.md) §1 |
| `oauth.refresh_ttl` | `site` | tenant | duration | a year unused | Refresh token lifetime | [0025](../decisions/0025-oauth-server.md) §1 |
| `pages.repos` | `site` | tenant | repository names | empty | Page repositories, in inheritance order; replaces `wikitext.template_repos` | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1 |
| `pages.share` | `site` | tenant | `off`, `on` | `off` | Whether this tenant's pages may serve other tenants as a `tenant` repository; replaces `wikitext.share` | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1 |
| `preferences.defaults` | `site` | tenant | map | the registry's defaults | Defaults for registered preference keys on this wiki, merged key by key over the instance's default for this setting | [0027](../decisions/0027-preferences-and-portability.md) §1; [0085](../decisions/0085-settings-and-preferences.md) §2.2 |
| `projections.sync_budget` | `site` | tenant ≤ instance ceiling | rows | 1,000 rows | Rows of composition fan-out the write path applies after commit before the rest is queued to the composition worker ([03](03-storage-caches-and-search.md)) | [0013](../decisions/0013-postgres-storage.md) §7 |
| `projections.sync_time` | `site` (by rule, §3.1) | tenant ≤ instance ceiling | duration | 250 ms | Time the write path spends on that fan-out, whichever comes first | [0013](../decisions/0013-postgres-storage.md) §7 |
| `proposals.adoption_grace` | `site` (by rule, §3.1) | tenant | duration | 7 days | After which a partly carried proposal is `partly adopted` | [0067](../decisions/0067-proposals.md) §5 |
| `proposals.direct_edit_lines` | `site` (by rule, §3.1) | tenant | integer | 50 | Lines a clean merge may change and still push to the article itself | [0068](../decisions/0068-merging-with-upstream.md) §3 |
| `proposals.max_items` | `site` (by rule, §3.1) | tenant | integer | 500 | Bounds a proposal payload | [0067](../decisions/0067-proposals.md) §8 |
| `proposals.push` | `site` | tenant | off, on | `off`; refused until the instance implements pushing | Whether proposals may be pushed upstream | [0067](../decisions/0067-proposals.md) §6 |
| `queries.max_params` | `site` (by rule, §3.1) | tenant | integer | 10 | Parameters of a query page | [0063](../decisions/0063-query-namespace.md) §4 |
| `queries.max_scope_values` | `site` (by rule, §3.1) | tenant | integer | 5,000 | Members a scope may bind as `VALUES` | [0063](../decisions/0063-query-namespace.md) §4 |
| `query.cache_ttl` | `site` (by rule, §3.1) | tenant | duration | 60 s | L2 lifetime of a `GET /sparql` response and of a query page's results | [0059](../decisions/0059-query-service.md) §6; [0063](../decisions/0063-query-namespace.md) §5 |
| `query.enabled` | `site` | tenant | `false`, `true` | `false` | Turns the query service on | [0059](../decisions/0059-query-service.md) §1 |
| `query.max_lag` | `site` (by rule, §3.1) | tenant | duration | 1 h | Lag above which the service is unavailable | [0059](../decisions/0059-query-service.md) §3 |
| `query.max_results` | `site` (by rule, §3.1) | tenant ≤ instance ceiling | integer | 100,000 solutions | Bounds every query | [0059](../decisions/0059-query-service.md) §6 |
| `query.timeout` | `site` (by rule, §3.1) | tenant ≤ instance ceiling | duration | 30 s | Bounds every query, raw or compiled | [0059](../decisions/0059-query-service.md) §5, §6 |
| `rc.max_age` | `site` | tenant | duration | 90 days (`$wgRCMaxAge`) | The recent-changes window | [0010](../decisions/0010-site-ui.md) §7; [0023](../decisions/0023-moderation.md) §6 |
| `reclaim.vouching` | `site` | tenant | `off`, or the code of an issuer that serves login | `off` | Whether the wiki this tenant adopted vouches for its old users through that issuer, reclaiming their accounts at login ([07](07-actors-and-accounts.md) §2.5); the grace period is the time it is on | [0088](../decisions/0088-external-identity-providers.md) §8 |
| `repo.debounce` | `site` (by rule, §3.1) | tenant | duration | 30 seconds | Per-page debounce of repository events | [0053](../decisions/0053-mirrored-pages.md) §6 |
| `repo.mirror_grace` | `site` (by rule, §3.1) | tenant | duration | 30 days | A mirrored file unused this long is tombstoned | [0039](../decisions/0039-files-and-media.md) §11 |
| `reports.term_languages` | `site` (by rule, §3.1) | tenant | languages | the content language | Languages the missing-label and missing-description reports cover | [0047](../decisions/0047-special-pages.md) §4.4 |
| `retention.api_max_entities` | `site` | tenant | integer | 10,000 | A bulk `retain` over this is refused for `api` unless forced | [0015](../decisions/0015-record-format-and-partition-registry.md) §4 |
| `retention.auto_properties` | `site` | tenant | off, on | on | Automatic `retain` of mirrored properties a tenant uses | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6 |
| `retention.default` | `site` | tenant | `orphan`, `retain` | `orphan` | The retention the tenant's policy applies to a mirrored entity an upstream tombstone removes, where no `retain` row names it; `cascade` is never a default and is only ever set per entity | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5 |
| `schemas.max_bound` | `site` | tenant ≤ instance ceiling | integer | — | Bound-schema × scope size above which binding a schema to a scope is refused at scope save, unless an operator raises the ceiling | [0083](../decisions/0083-write-path-in-three-tiers.md) §3 |
| `schemas.max_depth` | `site` (by rule, §3.1) | tenant | integer | 2 | Shape references followed in validation | [0064](../decisions/0064-entityschema-and-validation.md) §4 |
| `schemas.max_subjects` | `site` (by rule, §3.1) | tenant | integer | `scopes.max_members` | Bounds a schema's materialized report | [0064](../decisions/0064-entityschema-and-validation.md) §5 |
| `schemas.max_triples` | `site` (by rule, §3.1) | tenant | integer | 10,000 | Triple budget per validation | [0064](../decisions/0064-entityschema-and-validation.md) §4 |
| `schemas.recheck` | `site` (by rule, §3.1) | tenant | schedule | daily | Periodic re-validation job | [0064](../decisions/0064-entityschema-and-validation.md) §5 |
| `schemas.source` | tenant | tenant | `query` | the resolved JSON in `view` | Validate against the query store instead | [0064](../decisions/0064-entityschema-and-validation.md) §4 |
| `scopes.facet_limit` | `site` (by rule, §3.1) | tenant | integer | 1,000,000 | Members below which the builder shows facets | [0062](../decisions/0062-workspaces.md) §5 |
| `scopes.max_depth` | `site` (by rule, §3.1) | tenant | integer | 8 | Nesting of scope operands | [0060](../decisions/0060-scopes.md) §4 |
| `scopes.max_ids` | `site` (by rule, §3.1) | tenant | integer | 5,000 | `ids`, `include` and `exclude` together | [0060](../decisions/0060-scopes.md) §4 |
| `scopes.max_members` | `site` | tenant ≤ instance ceiling | integer | 100,000 | Bound on a scope's materialized members | [0060](../decisions/0060-scopes.md) §4 |
| `scopes.refresh` | `site` (by rule, §3.1) | tenant | duration | 15 min | Refresh interval of `query` scopes | [0060](../decisions/0060-scopes.md) §5 |
| `search.inherited` | `site` | tenant | off, on | `on` where a mirrored repository is listed | Whether `pages-{repo}` indexes are searched beside the tenant's | [0053](../decisions/0053-mirrored-pages.md) §7 |
| `signing.require_for_bot` | `site` | tenant | off, on | off: a signature is never required by default | Require a client signature for the `bot` group | [0024](../decisions/0024-subsidiary-accounts.md) §4 |
| `site.icon` | `site` | tenant | a site asset: PNG, ICO or SVG | none: the browser's default icon | The favicon and touch icon, served to anyone as a site asset, and `general.favicon` in `siteinfo` | [0085](../decisions/0085-settings-and-preferences.md) §7, §8 |
| `site.landing_text` | `site` (by rule, §3.1) | tenant | text | — | Text on a private tenant's landing page | [0056](../decisions/0056-security-model.md) §13 |
| `site.language` | `site` | tenant | a language | `en` | The content language (`$wgLanguageCode`): `general.lang` in `siteinfo`, the default interface language, the language of `Project` page titles and the fallback for labels and reports | [0085](../decisions/0085-settings-and-preferences.md) §7 |
| `site.logo` | `site` | tenant | a site asset: PNG, JPEG, GIF, WebP or SVG | none: the site name is shown as text | The logo in the global header and on a private tenant's landing page, and `general.logo` in `siteinfo`; served to anyone as a site asset | [0085](../decisions/0085-settings-and-preferences.md) §7, §8 |
| `site.name` | `site` | tenant | a name, one line, valid as a namespace name | the tenant's slug | The wiki's name: `general.sitename` in `siteinfo`, the global header, the Project namespace's name, the tenant list's `name`, and the operator's display name when the tenant is primary | [0085](../decisions/0085-settings-and-preferences.md) §7 |
| `sitelinks.schemes` | `site` | tenant | schemes | `https` and `http` | Allowed sitelink URL schemes | [0026](../decisions/0026-sitelinks.md) §1 |
| `skins.available` | `site` | tenant ≤ instance ceiling | installed skins | the instance's default, else every installed skin | The skins readers of this wiki may choose from, among those installed on the instance, up to the instance's ceiling | [0097](../decisions/0097-versioning-and-skins.md) §8 |
| `skins.default` | `site` | tenant | a skin the wiki makes available | `worksheet` | The wiki's skin: the HTML presentation of every page, for readers who have not chosen another | [0097](../decisions/0097-versioning-and-skins.md) §8 |
| `sprints.cache_ttl` | `site` (by rule, §3.1) | tenant | duration | 30 s | L1 lifetime of the `sp:` key | [0061](../decisions/0061-sprints-and-tasks.md) §11 |
| `sprints.claim_ttl` | `site` (by rule, §3.1) | tenant | duration | 7 days | A claim expires after this without a `release` | [0061](../decisions/0061-sprints-and-tasks.md) §5 |
| `sprints.max_claims` | `site` (by rule, §3.1) | tenant | integer | 10 | Live claims an actor may hold on a sprint | [0061](../decisions/0061-sprints-and-tasks.md) §5 |
| `sprints.max_rules` | `site` (by rule, §3.1) | tenant | integer | 20 | Rules per sprint | [0061](../decisions/0061-sprints-and-tasks.md) §4 |
| `sprints.max_tasks` | `site` | tenant ≤ instance ceiling | integer | 100,000 | Tasks materialized per sprint | [0061](../decisions/0061-sprints-and-tasks.md) §4 |
| `subsidiaries.max_per_account` | `site` | tenant | integer | 10 | Subsidiaries per operator | [0024](../decisions/0024-subsidiary-accounts.md) §2 |
| `subsidiaries.name_pattern` | `site` | tenant | pattern | none | Required pattern for subsidiary names | [0024](../decisions/0024-subsidiary-accounts.md) §2 |
| `subsidiaries.oauth_requires_approval` | `site` | tenant | `true`, `false` | `true` | Whether an OAuth-created subsidiary is pending | [0025](../decisions/0025-oauth-server.md) §3 |
| `subsidiaries.pending_ttl` | `site` (by rule, §3.1) | tenant | duration | 90 days | A subsidiary still unapproved after this is retired | [0025](../decisions/0025-oauth-server.md) §3 |
| `subsidiaries.request_page` | `site` | tenant | a page | — | Where "Request bot approval" links | [0024](../decisions/0024-subsidiary-accounts.md) §9 |
| `tables.max_columns` | `site` (by rule, §3.1) | tenant | integer | 50 | Columns of a table | [0045](../decisions/0045-table-content-model.md) §4 |
| `tables.max_rows` | `site` | tenant | integer | 5,000, as `feeds.related_limit` | Rows of a table | [0045](../decisions/0045-table-content-model.md) §4 |
| `talk.new_thread_default` | `site` | tenant | `here`, upstream | `here` | Which destination the new-thread form selects | [0069](../decisions/0069-synchronized-talk-pages.md) §6, §11 |
| `talk.upstream_audit_days` | `site` | tenant | days | 90 | How long a failed upstream send's text is kept | [0069](../decisions/0069-synchronized-talk-pages.md) §5, §11 |
| `talk.upstream_post` | `site` | tenant | off, on | `on` where any repository has `talk = sync` | Whether replies and new sections may be sent upstream at all | [0069](../decisions/0069-synchronized-talk-pages.md) §11 |
| `templatestyles.max_bytes` | `site` | tenant | size | 100 KB (`$wgTemplateStylesMaxStylesheetSize`) | Largest TemplateStyles sheet | [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2 |
| `theme.choice` | `site` | tenant | `any`, `accessibility` | `any` | What readers may choose: `any` palette and type set the wiki offers, or only the accessibility choices (mode, contrast, text size, accessibility palettes and type sets), which are never withheld | [0086](../decisions/0086-theming.md) §4 |
| `theme.palette` | `site` | tenant | a palette the tenant offers | `parchment` | The wiki's palette: the colours of every page, for readers who have not chosen another | [0086](../decisions/0086-theming.md) §4 |
| `theme.typeset` | `site` | tenant | a type set the tenant offers | `default26` | The wiki's type set: the typefaces of every page, for readers who have not chosen another | [0086](../decisions/0086-theming.md) §4 |
| `thread.markup` | `site` | tenant | `markdown`, `wikitext` | `markdown` | The markup of new posts: Markdown, or wikitext as on MediaWiki talk pages. Posts already written keep theirs | [0019](../decisions/0019-discussions.md) §5, A28 |
| `thread.max_attachments` | `site` | tenant | integer | 5 | Home and listings of a thread together | [0049](../decisions/0049-boards.md) §5 |
| `thread.max_pinned` | `site` | tenant | integer | 3 | Pinned threads per page | [0049](../decisions/0049-boards.md) §6; [0069](../decisions/0069-synchronized-talk-pages.md) §4, §11 |
| `ui.count_threshold` | `site` | tenant | integer | 10,000 | Counts above it (`ref_count`, revision counts) render as "10,000+" rather than exactly | [0010](../decisions/0010-site-ui.md) §2 |
| `updates.enabled` | `site` | tenant | off, on | off | Whether the RDF delta consumer runs for this tenant, writing `view.rdf_delta` from composition events; off, no delta is computed and the update stream is unavailable ([02](02-graphs-rdf-and-query.md)) | [0083](../decisions/0083-write-path-in-three-tiers.md) §3 |
| `updates.retention` | `site` | tenant | duration | 30 days | Window of `view.rdf_delta` rows | [0032](../decisions/0032-sparql-update-stream.md) §3 |
| `upstream.retire_adopted` | `site` | tenant | `true`, `false` | `true` | Retire a local assertion when upstream adopts it | [0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; [0067](../decisions/0067-proposals.md) §5 |
| `wikitext.lua` | `site` | tenant ≤ instance ceiling (`wikitext.ceiling`) | `off`, `on` | `off` | Lua modules; capped by `wikitext.ceiling` | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0043](../decisions/0043-lua-modules.md) §1 |
| `wikitext.renderer` | `site` | tenant | `builtin`, `parsoid` | `builtin` | Which stage-2 renderer serves HTML. `parsoid` requires a Parsoid service on the instance | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 |
| `wikitext.site_styles` | `site` | tenant | a Project-namespace title | — (`Project:Site styles.css` by convention) | The tenant's `sanitized-css` site styles page | [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §4 |
<!-- /settings.py -->

Changing any `wikitext.*` setting bumps the render epoch of every `wikitext` page in the tenant and queues them for refresh ([11](11-rendering-templates-and-modules.md)).

`repo.cache_ttl` is not a `site` setting but a per-repository field: of the `file-repo` record, and for a page repository of the tenant's `pages.repos` entry, with a default per kind: 7 days for files, one hour for template source and page bundles (§3.5).

`query.metadata` is a query-service setting: the store holds `full` with `metadata=local` by default for both backends, and `query.metadata = all` makes the embedded worker apply every delta and the QLever loader take the `all` dump ([02](02-graphs-rdf-and-query.md)).

### 3.3 Instance `site` settings

*Sources: [0077](../decisions/0077-special-version.md) §13; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0081](../decisions/0081-recovery-keys-and-continuations.md) §9; [0085](../decisions/0085-settings-and-preferences.md) §1.*

These are `site` records in the instance `config`, written with `ts-config` at the farm base, and the table is generated from `settings.toml` (§3.1); their bound is `instance`, and where a row says a tenant may narrow the value, the tenant record is bounded by the instance's as a ceiling. `version.toml` marks `search.farm_wide` and `files.separation` as the features of scope `instance` that `Special:Version`'s farm form shows ([0077](../decisions/0077-special-version.md) §11; `search.farm_wide` is a tenancy switch, §3.4).

<!-- settings.py: instance -->
| Setting | Values | Default | Meaning | Source |
|---|---|---|---|---|
| `files.reclaim_after` | duration, or `never` | 365 days | After how long unreadable to `universe` a file version is erased; a tenant may set a shorter period, never a longer one | [0039](../decisions/0039-files-and-media.md) §9 |
| `files.separation` | `tenant`, `instance` | `tenant` | Whether each tenant's bytes are stored apart or identical bytes once; changing it is `triplespace-cli files rescope`, needing `owner` | [0039](../decisions/0039-files-and-media.md) §4 |
| `files.types` | file types | the registry's `default = true` entries | The types the instance allows; a tenant `site` setting may narrow the list but not widen it | [0039](../decisions/0039-files-and-media.md) §5 |
| `instance.owner_of_record` | — | unset | The instance's owner of record, shown on `Special:Version` where set | [0077](../decisions/0077-special-version.md) §1 |
| `instance.source_url` | A URL for the corresponding source of this build | Unset: `{developer.source}/tree/{commit}` for an unmodified build | Where a modified build's source is published; a modified build without it shows "modified build; source location not configured", and `instance check` warns | [0077](../decisions/0077-special-version.md) §3, §13 |
| `query.embedded_max_triples` | integer | 10^9 | The triple count above which `query.backend = embedded` is refused: the embedded backend is the small profile's, and the Wikidata profile requires `remote` ([02](02-graphs-rdf-and-query.md)) | [0059](../decisions/0059-query-service.md) §2 |
| `query.max_concurrent` | integer | 8 | Concurrent queries across the instance, beyond which a query waits up to its own timeout; a tenant cannot set it | [0059](../decisions/0059-query-service.md) §6 |
| `storage.dedup_scope` | per tenant, `"instance"` | each tenant is its own domain | Whether fragments are deduplicated per tenant or once for the instance | [0058](../decisions/0058-packed-record-storage.md) §8 |
| `talk.archive_window` | duration | 10 minutes | Window within which a removed thread found on an archive subpage counts as archived | [0069](../decisions/0069-synchronized-talk-pages.md) §2, §11 |
| `thread.max_depth` | integer | — | Depth to which a thread page indents its posts | [0019](../decisions/0019-discussions.md) §8 |
| `version.services` | `full`, `names`, `off` | `full` | How much `Special:Version` discloses of the instance's services: everything, as MediaWiki shows its database version to everyone; names, roles and states without versions; or the build row only. Holders of `ts-config` at the farm base always see `full` | [0077](../decisions/0077-special-version.md) §4, §13 |
| `witness.enabled` | off, on, with the witnessed origins and their keys | off | Whether `triplespace-server` serves the C2SP tlog-witness API at the farm base for the listed origins ([01](01-log-and-records.md) §4.3) | [0081](../decisions/0081-recovery-keys-and-continuations.md) §6 |
<!-- /settings.py -->

### 3.4 Tenancy switches

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0097](../decisions/0097-versioning-and-skins.md) §8.*

A tenancy switch is a `config` record of kind `tenancy` in the instance `config`. The switch table with its `isolated`, `community` and `enterprise` preset values is [0028](../decisions/0028-tenancy-policy.md) §1, in [08](08-tenants-and-instances.md); the switches are `identity.farm_issuer`, `identity.required_issuer`, `identity.shared_names`, `identity.auto_link`, `groups.global`, `blocks.global`, `providers.between_tenants`, `providers.reader_lists`, `discussion.cross_tenant`, `notifications.cross_tenant`, `config.template`, `feeds.farm_wide`, `search.farm_wide`, `filters.global`, `wikitext.ceiling`, `security.restrictions` and `query.isolation`. `wikitext.ceiling` is the one [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 defines:

| Switch | Values | `isolated` | `community` | `enterprise` |
|---|---|---|---|---|
| `wikitext.ceiling` | `expansion`, `lua`: whether tenants may run Lua; expansion itself is part of wikitext (0042 A19) | `lua` | `lua` | `lua` |
| `skins.available` | `all`, or a list of installed skins: the skins a tenant may make available, the ceiling of the setting of the same name ([0097](../decisions/0097-versioning-and-skins.md) §8) | `all` | `all` | `all` |
| `query.isolation` | `dataset`, `store` ([0059](../decisions/0059-query-service.md) §4): one shared store with the dataset fixed per tenant, or an embedded store per tenant. Instance-only: no tenant record overrides it. `store` with `query.backend = remote` is refused only on a farm, since on a single-tenant instance the remote store holds one tenant's graphs and satisfies `store` | `store` | `dataset` | `dataset` |

### 3.5 Keys that are fields of a config record

*Sources: [0069](../decisions/0069-synchronized-talk-pages.md) §11.*

These share the dotted form but are fields of the record named, not `site` settings.

| Key | On | Values | Default | Source |
|---|---|---|---|---|
| `namespaces` | a `pages.repos` entry, per repository | a subset of the namespaces the instance-level `page-repo` record serves | all of them | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1 |
| `shadowed` | a `pages.repos` entry, per repository | `offer`, `hide` | `offer` | [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1 |
| `talk` | a `pages.repos` entry, per repository | `link`, `sync` | `link` | [0069](../decisions/0069-synchronized-talk-pages.md) §1, §11 |
| `cache_ttl` | a `pages.repos` entry, per repository | duration | one hour for page bundles | [0053](../decisions/0053-mirrored-pages.md) §4 |
| `mirror.set` | `page-repo` | `on-demand`, `linked`, `all` | `on-demand` | [0053](../decisions/0053-mirrored-pages.md) §5 |
| `mirror.depth` | `page-repo` | integer | 1 | [0053](../decisions/0053-mirrored-pages.md) §5 |
| `mirror.grace` | `page-repo` | duration | 90 days | [0053](../decisions/0053-mirrored-pages.md) §5 |
| `repo.cache_ttl` | `file-repo`; for a page repository the tenant's `cache_ttl` above | duration | per kind: 7 days for files; one hour for template source and page bundles | [0039](../decisions/0039-files-and-media.md) §11; [0042](../decisions/0042-template-expansion-and-parsoid.md) §11; [0053](../decisions/0053-mirrored-pages.md) §4 |
| `lines.identifiers` | `extraction` | identifier properties | every property designated for `match_key` | [0073](../decisions/0073-lines-links-and-url-patterns.md) §3 |
| `lines.url_property` | `extraction` | a property designated for `match_key` | none | [0073](../decisions/0073-lines-links-and-url-patterns.md) §3 |
| `lines.strip_params` | `extraction` | parameter names | `utm_*`, `fbclid`, `gclid`, `mc_cid`, `mc_eid` | [0073](../decisions/0073-lines-links-and-url-patterns.md) §3 |
| `lines.ignore_hosts` | `extraction` | hosts | — | [0073](../decisions/0073-lines-links-and-url-patterns.md) §4 |
| `lines.first_link_only` | `extraction` | boolean | `false` | [0073](../decisions/0073-lines-links-and-url-patterns.md) §4 |
| `lines.in_ref` | `extraction` | boolean | `false` | [0073](../decisions/0073-lines-links-and-url-patterns.md) §4 |
| `pages.group_by` | `publication` | a path | — | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §3 |
| `pages.title` | `publication` | a pattern such as `"{group}/Semantic data"` | — | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §3 |
| `pages.ungrouped` | `publication` | a page | — | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §3 |
| `pages.max_rows` | `publication` | integer | 500 | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §3 |
| `kits.update` | `publication` | boolean | — | [0074](../decisions/0074-publishing-a-scope-to-an-external-wiki.md) §5 |

### 3.6 Deployment configuration

*Sources: [0033](../decisions/0033-backend-stack.md) §12; [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1; [0097](../decisions/0097-versioning-and-skins.md) §8.*

Deployment configuration is read by `figment` (§5.1), never from a log record, as the Postgres and OpenSearch addresses are; the credentials of an `enterprise` page-repository source are deployment configuration too ([0053](../decisions/0053-mirrored-pages.md) §1).

| Setting | Values | Default | Meaning | Source |
|---|---|---|---|---|
| `server.mode` | `production`, `development` | `instance create` writes `production` | In `production` the server refuses to start while a deployment requirement it can test fails; `development` starts anyway and marks `siprop=triplespace` with `insecure: true` | [0033](../decisions/0033-backend-stack.md) §12; [0056](../decisions/0056-security-model.md) §10 |
| `server.trusted_proxies` | addresses and CIDR ranges | — | Hops whose `X-Forwarded-*` are believed, by address | [0033](../decisions/0033-backend-stack.md) §12; [0057](../decisions/0057-web-tier.md) §10 |
| `server.admin_listen` | a listener | — | Where `/metrics`, health and debug routes are served, never the public one | [0033](../decisions/0033-backend-stack.md) §12; [0056](../decisions/0056-security-model.md) §10 |
| `server.auth_base` | URL | unset: the single tenant's base serves as the auth base | The auth host, where a person authenticates to the instance; required once there is a second tenant, and never a tenant base or the farm base ([07](07-actors-and-accounts.md) §2.8) | [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1, §12 |
| `server.ui` | `embedded`, `off` | `embedded` | Whether the server serves the site | [0033](../decisions/0033-backend-stack.md) §12; [0057](../decisions/0057-web-tier.md) §2 |
| `server.skins` | a directory | unset: the shipped skins only | The skins the operator installs, one directory each, read at start; one named as a shipped skin replaces it ([19](19-site-ui.md) §5.10) | [0097](../decisions/0097-versioning-and-skins.md) §8 |
| `parsoid.url` | URL | unset: no tenant can choose `parsoid` | The Parsoid service | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 |
| `parsoid.timeout` | duration | 30 s | Per render; also the validity of a render's API token | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 |
| `parsoid.concurrency` | integer | — | Concurrent Parsoid renders | [0042](../decisions/0042-template-expansion-and-parsoid.md) §2 |
| `render.pool` | integer | — | Size of the pool that bounds synchronous page renders per tenant; a render the pool cannot take serves the stale `p:` entry and leaves the re-render to the refresh queue ([11](11-rendering-templates-and-modules.md)); deployment configuration, as every pool size is (§3.1) | [0042](../decisions/0042-template-expansion-and-parsoid.md) §10 |
| `query.backend` | `embedded`, `remote` | `embedded` | Oxigraph in process, or one remote store | [0059](../decisions/0059-query-service.md) §2 |
| `query.path` | a directory | — | The embedded store's RocksDB directory; `query.path/{tenant}` under `query.isolation = store` | [0059](../decisions/0059-query-service.md) §2, §4 |
| `query.endpoint` | URL | — | The remote store; its credential, if any, is read as every secret is | [0059](../decisions/0059-query-service.md) §2 |
| `files.thumb_concurrency` | integer | — | Size of the thumbnail rendering pool; deployment configuration, as every pool size and interval is (§3.1) | [0039](../decisions/0039-files-and-media.md) §6 |
| `repo.concurrency` | integer | 4 | Per-repository concurrency cap on the upstream client; deployment configuration, as every pool size and interval is (§3.1) | [0053](../decisions/0053-mirrored-pages.md) §1 |
| `repo.fetches_per_minute` | integer | 600 | Per-repository fetch budget; deployment configuration, as every pool size and interval is (§3.1) | [0053](../decisions/0053-mirrored-pages.md) §1 |
| `updates.batch_interval` | duration | one hour | How often update batch files are written; deployment configuration, as every pool size and interval is (§3.1) | [0032](../decisions/0032-sparql-update-stream.md) §6 |
| `notifications.fediverse_interval` | duration | five minutes | Bundling interval for fediverse delivery; deployment configuration, as every pool size and interval is (§3.1) | [0021](../decisions/0021-notifications.md) §5 |
| `web.*` | see [20](20-web-tier.md) | | `triplespace-web`'s settings: `web.listen`, `web.admin_listen`, `web.api`, `web.mode`, `web.upstream_host` (`forward`, or `fixed` in `development`), `web.assets_from`, `web.cache` (`memory`, `valkey` or `off`) with `web.cache.valkey_url`, `web.cache.valkey_password_file` and `web.cache.max_ttl` (default and ceiling 60 seconds), and `web.forwarder_key_file` | [0057](../decisions/0057-web-tier.md) §2, §4, §5, §10 |

`query.backend`, `query.path` and `query.endpoint` are deployment configuration under [0033](../decisions/0033-backend-stack.md) §12, not `site` records.

### 3.7 Preference keys

*Sources: [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §1, §2.2.*

A preference is a key–value pair owned by an account, registered in `docs/registry/preferences.toml` with a type, a default, a group and a description, and is not a `site` setting. Its value resolves to the account's own value, then `preferences.defaults` (§3.2), the tenant's map merged key by key over the instance's default for it, then the registry default; a key marked `follows = farm` belongs to the farm account under farm identity, and a key marked `browser` may be held in the browser of a reader who is not logged in ([0085](../decisions/0085-settings-and-preferences.md) §1, §2.2; [07](07-actors-and-accounts.md) §8.1). The keys the ADRs name are `autowatch.edit`, `autowatch.create`, `autowatch.post`, `autowatch.thread`, `autowatch.notify`, `feeds.period`, `feeds.syncs`, `shapes.pins`, `threads.visibility`, `threads.destination`, `farm.home_tenant`, the `notifications.{reason}.{channel}` matrix, and `timezone`, `date-format`, `editor.font`, `rc.limit` and the other MediaWiki options Triplespace honours ([0027](../decisions/0027-preferences-and-portability.md) §1; [0021](../decisions/0021-notifications.md) §3; [0028](../decisions/0028-tenancy-policy.md) §7; [0069](../decisions/0069-synchronized-talk-pages.md) §6). They are in [07](07-actors-and-accounts.md).

## 4. The registry files

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §8; [0036](../decisions/0036-openstreetmap-providers.md) §7; [0037](../decisions/0037-gdelt-provider.md) §8; [0048](../decisions/0048-notation.md) §7; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §7; [0077](../decisions/0077-special-version.md) §14.*

### 4.1 The registry of record

*Sources: [0077](../decisions/0077-special-version.md) §14; [0085](../decisions/0085-settings-and-preferences.md) §1.*

The files under `docs/registry/` in the Triplespace repository are the registry of record for the names and codes Triplespace and Scatterbase share ([0015](../decisions/0015-record-format-and-partition-registry.md) §5). The table of files, what each lists and the ADR that defines it is [02](02-graphs-rdf-and-query.md) §1.4, and is not repeated here; this chapter holds what [02](02-graphs-rdf-and-query.md) does not, which is the crate that embeds each file and the changes the registry trailers of later ADRs made to them. `docs/registry/README.md` carries the same table with an "Embedded by" column.

| File | Embedded by |
|---|---|
| `graphs.toml`, `fragments.toml` | `scatter-log` |
| `providers.toml` | `scatter-providers` |
| `issuers.toml`, `groups.toml`, `grants.toml`, `tenancy.toml` | `scatter-actors` |
| `namespaces.toml`, `special-pages.toml` | `scatter-titles` |
| `content-models.toml` | `scatter-pages` |
| `keyed-types.toml`, `notation-schemes.toml`, `resolvers.toml` | `scatter-normalize` |
| `sites.toml` | `scatter-wikibase-model` |
| `thread-statuses.toml` | `scatter-threads` |
| `talk-markup.toml` | `scatter-talk` |
| `preferences.toml`, `settings.toml` | `triplespace-accounts` |
| `file-types.toml` | `scatter-files` |
| `wikitext-functions.toml` | `scatter-wikitext-expand` |
| `css-properties.toml` | `scatter-css` |
| `themes.toml` | `triplespace-ui` |
| `version.toml` | `triplespace-api-rest` |
| `routes.toml`, `action-api.toml`, `permissions.toml`, `log-events.toml` | none: read by `architecture/catalogues.py`, which generates [18](18-api.md) §2.3 and §3.2, [09](09-security-and-moderation.md) §2.2 and [16](16-logs-feeds-and-notifications.md) §2.3 |
| `crates.toml` | none: read by `cargo xtask deps` and `xtask wasm` from the repository, and by `architecture/crates.py` ([22](22-crates-and-stack.md) §2.1) |

The files are `serde` + `toml`, embedded at build time (`include_str!`) by the crates that need them and parsed at build time, so a malformed file fails the build and the defaults an instance starts from are the defaults the code was tested with ([0033](../decisions/0033-backend-stack.md) §12). They are defaults, not state: **an instance's `config` partition starts as a copy of the registry.** `instance create` appends a config record for every registry entry, so the log carries what the instance runs under and a reader of the log needs no binary to know it; `triplespace-cli registry sync` appends records, under the instance attestation, for the entries of the embedded registry that are newer than the instance's, and `instance check` warns when the instance is behind the registry its binary carries. Every config record carries its kind's `schema` integer, and `view.registry` projects an unknown kind or schema opaquely (§1.1). An instance's configuration may diverge from the registry, declaring entity sources, renaming groups or changing permissions, but may not reuse a provider code or number for something else, because IDs and revision IDs computed from them are shared between instances; a provider's code, number, slug and per-type IRI templates are never changed once allocated. A change is a commit; nothing is allocated at runtime, codes and numbers are never reused, and a retired entry stays in the file with `retired = true`. Provider number 0 is the current tenant and is never assigned; registry provider numbers are below 2^12, and numbers 2^12 to 2^13 − 1 are the tenants', assigned at runtime, per tenant, to their entity sources, the one runtime allocation of a provider number, so that every ranged ID `number << 40 | n` a client sees stays below 2^53 ([0078](../decisions/0078-entity-sources.md) §4); the doubled-letter codes `AA` through `ZZ` are never allocated, because `QQQ5` is the tenant-relative form of the local `Q5` ([0044](../decisions/0044-tenant-relative-ids.md) §2); and a tenant's slug, a provider's slug and an issuer's code are one namespace, shared with keyed-type, resolver and notation scheme names, which a tenant's entity source may not take ([0078](../decisions/0078-entity-sources.md) §3).

### 4.2 Providers and issuers

*Sources: [0036](../decisions/0036-openstreetmap-providers.md) §7; [0037](../decisions/0037-gdelt-provider.md) §8; [0048](../decisions/0048-notation.md) §7; [0091](../decisions/0091-one-wikibase-adapter.md) §2.*

`providers.toml` carries `OS` (number 6) and `OW` (number 7), the `OW` key map, two rules with `key_type = "notation"` and `key_scheme = "osm"` and the wiki's `class_property`, beside `XD`'s one rule with `key_type = "domain"` (a key map is always a list of `[[provider.adapter_config.key_map]]` rules, [0091](../decisions/0091-one-wikibase-adapter.md) §2), and `GD` (number 8) with types `E`, `D`, `C`, `T`. `scatter-providers` carries per-type `id_grammar` and the `gdelt-record` and `token` grammars, beside `digits` and `uuid`: the ID grammars of minted types live with the registry field that names them ([0017](../decisions/0017-entity-id-grammar.md) §7), and `scatter-normalize` keeps the key grammars of keyed types. `issuers.toml` carries `openstreetmap`, `osmwiki` and `gdelt`. GDELT adds no keyed type and no namespace. The crates these ADRs add are in [22](22-crates-and-stack.md); the providers themselves are in [05](05-providers-and-ingest.md).

### 4.3 Keyed types, notation schemes, namespaces and content models

*Sources: [0036](../decisions/0036-openstreetmap-providers.md) §7; [0048](../decisions/0048-notation.md) §7.*

- **`keyed-types.toml`:** `osm-tag` is replaced by `notation`. The registry had allocated `osm-tag` but no instance holds data under it, so it is replaced rather than retired.
- **`notation-schemes.toml`:** new, with `osm` and the two candidates ([04](04-entities-and-identifiers.md)).
- **`namespaces.toml`:** 216 and 217 are renamed from `OSM` and `OSM talk` to `Notation` and `Notation talk`, with normalizer `notation`. The numbers serve the same pages they were allocated for, now with the scheme in the title. They had not been filed on mediawiki.org, so the registration is filed under the new name; the registry README lists 216/217 in the mediawiki.org registration still to be filed. `triplespace-titles` gains the `Notation` namespace ([0048](../decisions/0048-notation.md) §8).
- **`content-models.toml`:** `triplespace-osm-tag` becomes `triplespace-notation`, entity type `notation`.
- `scatter-normalize` keeps the `osm-tag` grammar as the `osm` scheme's grammar.

### 4.4 Resolvers

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §8.*

`docs/registry/resolvers.toml` ships `doi` and `url` as defaults, with `orcid`, `isbn` and `handle` as commented candidates whose grammars are written but whose roles a tenant binds when it wants them. Adding a resolver is a `config` record with `ts-config`; binding its role is the ordinary role record (§2.2, kind `role`). `DOI` is namespace 220 (talk 221) and `URL` 222 (talk 223), in the resolver range 220–229 of [0008](../decisions/0008-namespaces-and-document-pages.md) §2; three pairs remain in it, and the range continues at 320–329. Resolver namespaces themselves are in [06](06-statements-and-properties.md).

### 4.5 Wikitext functions, CSS properties and special pages

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §7.*

`docs/registry/wikitext-functions.toml` changes:

| Entry | Was | Now |
|---|---|---|
| `templatestyles` (tag, TemplateStyles) | `chip` | `implemented` |
| `templatedata` (tag, TemplateData) | `chip` | `implemented` |
| `__DISAMBIG__` (switch, Disambiguator) | `ignored` | `implemented` |
| `SHORTDESC` (parser function, Wikibase Client) | — | `implemented` |
| `__EXPECTED_UNCONNECTED_PAGE__` (switch, Wikibase Client) | — | `ignored` |

`docs/registry/css-properties.toml` is new: the CSS at-rules and properties the `sanitized-css` sanitizer allows ([11](11-rendering-templates-and-modules.md)). `content-models.toml` gains `sanitized-css`. `special-pages.toml` marks `PagesWithProp` served ([21](21-special-pages.md)).

### 4.6 `version.toml`

*Sources: [0077](../decisions/0077-special-version.md) §14.*

`docs/registry/version.toml` lists the developer, contributors, funders, AI agents and their attribution, services, feature switches and extensions, as [0077](../decisions/0077-special-version.md) §2, §4, §8, §9 and §11 describe. It is embedded by `triplespace-api-rest`, like the other registries ([0015](../decisions/0015-record-format-and-partition-registry.md) §5), and is CC0 like the rest of `docs/`. Its `[[feature]]` entries carry a scope: the tenant features are `content.licence`, `wikitext.lua`, `wikitext.renderer`, `pages.repos`, `search.inherited`, `query.enabled`, `mcp.enabled` and `proposals.push`, and the instance features `search.farm_wide` and `files.separation` ([0077](../decisions/0077-special-version.md) §11; the page is in [21](21-special-pages.md)).


### 4.7 `talk-markup.toml`

*Sources: [0090](../decisions/0090-converting-talk-pages-comment-by-comment.md) §5.*

`docs/registry/talk-markup.toml` lists the conventions of wikitext talk pages that the talk-page conversion reads ([13](13-mirrored-pages.md) §5.16): timestamp formats as MediaWiki date-format strings with an optional zone, month names and abbreviations per language, the marks that may precede an undated signature, and template names by role (`unsigned`, `outdent`, `wrapper`, `autosign`) and language. It is **universal**: an entry applies on every wiki where it matches, and a wiki configures nothing, since its content language, timezone and namespace and special-page names come from `meta=siteinfo`. It is embedded by `scatter-talk` and starts with English; other languages are added by commits.
## 5. Configuration and the CLI

*Sources: [0033](../decisions/0033-backend-stack.md) §12; [0029](../decisions/0029-resolver-namespaces.md) §8; [0077](../decisions/0077-special-version.md) §13.*

### 5.1 Layered instance configuration

*Sources: [0033](../decisions/0033-backend-stack.md) §12.*

- `clap` for `triplespace-cli` and the server's flags.
- `serde` + `toml` for `docs/registry/` files, embedded at build time by the crates that need them (§4.1).
- `figment` for layered instance configuration: file, then environment. Secrets are read from files (`--token-file`, `--*-file`), never from command-line values.
- `dotenvy` for the instance's `.env` file ([0033](../decisions/0033-backend-stack.md) A23), read into the environment when `triplespace`, `triplespace-server` and `triplespace-web` start, before `clap` parses their flags. Every flag bound to a `TRIPLESPACE_*` variable, and every variable read directly such as `TRIPLESPACE_OWNER_PASSWORD`, can be set there. A flag wins over the environment and the environment over the file, which never overrides a variable already set. The file read is the one `TRIPLESPACE_ENV_FILE` names, which must exist, else `.env` in the working directory if there is one; a file that cannot be read or parsed stops the binary. It is gitignored, and may hold secrets, as any file read for them may.
- `server.mode`, `server.trusted_proxies`, `server.admin_listen` (§3.6) and the registered-host check, and `triplespace-cli instance check` with `--attest` and `--through`, which verify the deployment requirements of [0056](../decisions/0056-security-model.md) §10 ([09](09-security-and-moderation.md)); in `production` the server refuses to start while a requirement it can test fails.
- `server.ui` (`embedded` or `off`) chooses whether the server serves the site; `triplespace-web` takes the `web.*` settings of [0057](../decisions/0057-web-tier.md) §2 by the same rules, its forwarder key and its cache's Valkey password from files. `triplespace-cli instance forwarder create`, `list` and `revoke` manage forwarder keys ([0057](../decisions/0057-web-tier.md) §10, in [20](20-web-tier.md)).

### 5.2 Writing configuration records

*Sources: [0029](../decisions/0029-resolver-namespaces.md) §8; [0077](../decisions/0077-special-version.md) §13; [0015](../decisions/0015-record-format-and-partition-registry.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §3.*

Configuration in the log is written with `ts-config` ([09](09-security-and-moderation.md)): a tenant's records at its own base, and instance records such as the `reports` kind and the instance `site` settings of §3.3 at the farm base. Adding a resolver is one such record; binding its role is a `role` record. The parts of `triplespace-cli` that write configuration for an instance or a tenant (`instance create`, `tenant create`, `tenancy set`, `tenancy check`, `files rescope`, `registry sync`) are in [08](08-tenants-and-instances.md) and [22](22-crates-and-stack.md). `site` records, and with `scope=instance` at the farm base the instance's settings, defaults, locks and tenancy switches, are also written by `PATCH /settings` and the settings pages, `Special:WikiSettings` and `Special:InstanceSettings`, and by `triplespace-cli settings get|set|unset` with `--instance`, `--default` and `--lock`. Every writer calls the one validator in `triplespace-accounts`, which refuses an unknown key (`ts-unknown-setting`), a value outside its type or choices (`ts-invalid-setting`), a value above its ceiling (`ts-above-ceiling`), a locked key (`ts-locked`) and an instance-bound key at a tenant's scope, except a narrowing of a `tenant_may_narrow` setting (`ts-instance-policy`); a save is one change set at its base, all or nothing, whose records carry its summary, with a lock's copies into tenants following as an instance job (§1.4) and which projects as one `settings/change` log event ([0085](../decisions/0085-settings-and-preferences.md) §3; [18](18-api.md) §3.2; [19](19-site-ui.md) §2.8).

## 6. What is reported

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §2; [0075](../decisions/0075-mcp-server.md) §6; [0085](../decisions/0085-settings-and-preferences.md) §1, §7; [0097](../decisions/0097-versioning-and-skins.md) §8.*

`meta=siteinfo&siprop=triplespace` reports a tenant's `wikitext.*` settings, the ceiling, and whether a Parsoid service is available; `thread.markup`, as `thread.markup`, so a client knows which markup its new posts must have ([14](14-discussions.md) §1.6); the MCP endpoint when `mcp.enabled` is on, which the REST root lists too, so a client that knows the wiki can find the server; and, from the sections cited in §3, the tenancy preset and switches, `lua.ids` and `lua.client_site`, the table limits, `subsidiaries.oauth_requires_approval`, the theme's token values and `security.restrictions`; and the wiki's skins as `skins`, its `skins.default` and, where it has narrowed them, its `skins.available`, so the site chooses among what the wiki offers ([0097](../decisions/0097-versioning-and-skins.md) §8). `general.sitename`, `general.lang` and `general.logo` are `site.name`, `site.language` and `site.logo`, and every setting `settings.toml` marks `reported` is reported ([0085](../decisions/0085-settings-and-preferences.md) §1, §7). The rest of the API surface is in [18](18-api.md); what `Special:Version` shows of configuration is in [21](21-special-pages.md).
