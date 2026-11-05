# 09. Security and moderation

This chapter describes who may do what in Triplespace and what each principal may see: the permission vocabulary and the one catalogue of every permission the ADRs name, with its default groups; groups, memberships and blocks, and the global groups and blocks a farm adds; ACLs, the targets they attach to and how they compose; the security model, with its invariant, the visibility of a target, absence, flow, writes and the privacy test; protection, deletion, hiding, suppression and patrolling as ACLs and log events, and the visibility of moderation and erasure records; and edit filters, with their contexts, rule language, actions, records and farm-wide form. It assumes the log, records, parts and erasure of [01](01-log-and-records.md), the `view` schema, caches and search of [03](03-storage-caches-and-search.md), the actors, subsidiaries, grants and rate limits of [07](07-actors-and-accounts.md), and the tenants, tenancy policy and instance prerogatives of [08](08-tenants-and-instances.md). The log events that moderation projects are catalogued in [16](16-logs-feeds-and-notifications.md); the special pages that expose it are in [21](21-special-pages.md); the deployment boundary that the invariant stops at is in [20](20-web-tier.md). `docs/registry/groups.toml` is the registry of record for permissions and groups; the catalogue in §2.2 is assembled from the ADRs and the registry is authoritative where they differ.

## 1. Vocabulary, the invariant and principals

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §1; [0056](../decisions/0056-security-model.md) §1; [0034](../decisions/0034-frontend-stack.md) §11.*

### 1.1 Vocabulary

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §1.*

| Term | Meaning | MediaWiki clients see |
|---|---|---|
| **Permission** | A named capability, such as `edit` or `ts-erase`. A string. | A right |
| **Group** | A named set of permissions | A user group |
| **Membership** | An actor's membership in a group, with an optional expiry | Group membership |
| **`universe`** | The group that contains everyone, including anonymous readers and temporary accounts. It cannot be joined or left. | `*` |
| **`owner`** | The group that holds every permission, including any added later. It always has at least one member. | A group named `owner` whose rights are the full list |
| **ACL** | An access-control entry attached to a target by identifier, restricting named permissions to a group until an expiry | Page protection, where the target is a page |
| **Block** | A record that removes named permissions from one actor until an expiry | A block |

The MediaWiki names are the permission names. The Scatterbase names map onto them and are historical:

| Scatterbase (historical) | MediaWiki (the permission name) | Meaning |
|---|---|---|
| `view` | `read` | Read public data |
| `edit` | `edit` | Write to the local graph and to document pages |
| `delete` | `delete` | Delete document pages |
| `view_deleted` | `deletedhistory`, `deletedtext` | See hidden revisions and deleted pages |
| `block_user` | `block` | Block accounts |

### 1.2 The invariant

*Sources: [0056](../decisions/0056-security-model.md) §1.*

**Nothing leaves the instance that the principal receiving it may not read.** A response, a stream event, a search hit, a feed row, a log row, a notification, a rendered include, a cache entry, a dump, a federation activity and a file byte are each the result of evaluating `read` (§4.4) for a **principal** against the **target** the data comes from. There is no route, job, cache, index or export that reads `view` or `log` on a principal's behalf without that evaluation, and there is no whitelist: MediaWiki's `$wgWhitelistRead` has no counterpart, because a restriction that admits exceptions by title is a restriction that content can escape.

**Default is deny for everything a restriction covers; default is public for everything else.** A tenant with no `read` restriction is a public wiki and nothing in the security model costs it anything. A `read` restriction applies to its target and everything the target encloses, and nothing loosens it (§4.4). An instance of public tenants has no restrictions, and a hosting instance needs none either: tenants never see each other's data except through what the tenancy policy lets cross ([08](08-tenants-and-instances.md)), mirrored entities, page repositories and shared files, which are public at their source and cross as public form only (§5.4), and identity, sessions and partitions are per tenant whatever the policy. A `tenant` restriction (§4.7) is for a tenant that is private to the world, not for keeping tenants apart.

### 1.3 Principals

*Sources: [0056](../decisions/0056-security-model.md) §1.*

A principal is what a request acts as, after authentication:

| Principal | Groups | Capabilities |
|---|---|---|
| Anonymous, or a request before a temporary account exists | `universe` | `universe`'s permissions |
| A session of a tenant account | Its memberships, plus the global groups of its linked farm account where the policy has them (§3.7) | Effective permissions (§3.4) |
| A subsidiary acting through an API key or an OAuth token | The subsidiary's memberships | Effective permissions ∩ the credential's grants ([07](07-actors-and-accounts.md)). A grant can withhold a capability; it cannot add a group, so it never widens what an ACL admits |
| Another tenant, or another instance, reading this tenant as a provider, page repository, template repository or file repository | `universe` | Public form only (§5.4) |
| A federation peer | `universe` for outbound; the `federated` surrogate's groups for inbound ([17](17-federation-and-publication.md)) | As anonymous |
| The renderer, the indexer, the feed projection, the notifier | None of its own: a **derived-output** principal that carries the visibility of what it produces (§5.4) | Writes outputs tagged with their visibility; serves nothing itself |
| The instance operator | None in any tenant's UI | Instance acts ([08](08-tenants-and-instances.md)) and out-of-band access inside the deployment boundary ([20](20-web-tier.md)) |

### 1.4 Sessions, requests and the browser

*Sources: [0056](../decisions/0056-security-model.md) §1; [0034](../decisions/0034-frontend-stack.md) §11; [0086](../decisions/0086-theming.md) §5; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5, §9.*

A session is per host: a cookie for `librarybase.org` is nothing on `example.wiki`. What joins them is the auth host ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5, §9; [07](07-actors-and-accounts.md) §2.8): its own session, the central session, holds an instance login, and a tenant opens a session of its own from it only when the person presses **Log in** there, through a redirect to the auth host and a one-time login code; no tenant page probes the auth host. Logging out on a tenant ends that tenant's session; logging out everywhere, on the auth host, ends the central session and every tenant session it opened. Session cookies are `Secure`, `HttpOnly` and `SameSite=Lax`; every write carries a CSRF token as MediaWiki's do; bearer credentials are never accepted in a URL. Where session state lives is in [03](03-storage-caches-and-search.md) and [07](07-actors-and-accounts.md).

The site UI runs under a strict Content Security Policy: scripts only from the instance's own origin, no inline scripts, no `eval`. **Stylesheets come only from the origin too, and `<style>` elements are refused, but `style` attributes are allowed** (`style-src-attr 'unsafe-inline'`): wikitext writes them, as infoboxes, navboxes and coloured table cells on mirrored Wikipedia pages do, and the renderer passes only what MediaWiki's Sanitizer accepts, its `style` check included ([10](10-pages-and-content-models.md) §5.1; [0034](../decisions/0034-frontend-stack.md) A13). The sheets that `<templatestyles>` emits and that mirrored Wikipedia HTML carries reach the browser as same-origin stylesheets under `/w/styles/`, linked where the `<style>` element stood, so the policy needs no exception for them ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) A5; [11](11-rendering-templates-and-modules.md) §6.3). Fonts and images may also come from the origin of `media.base_url`, where site assets such as a tenant's typefaces and logo are served ([0086](../decisions/0086-theming.md) §5). Initial data is passed in `application/json` blocks, which CSP does not execute. Components never insert HTML they built from user input. HTML they insert comes only from `action=render`, which is built from `askama`'s escaped templates and from HTML the API has sanitized ([11](11-rendering-templates-and-modules.md)).

## 2. Permissions

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §2, §5; [0012](../decisions/0012-api-requirements.md) §8; [0019](../decisions/0019-discussions.md) §12; [0020](../decisions/0020-change-feeds.md) §7; [0021](../decisions/0021-notifications.md) §9; [0022](../decisions/0022-federation.md) §12; [0023](../decisions/0023-moderation.md) §11; [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0028](../decisions/0028-tenancy-policy.md) §13; [0030](../decisions/0030-edit-filters.md) §5, §12; [0039](../decisions/0039-files-and-media.md) §21; [0040](../decisions/0040-instance-prerogatives.md) §9; [0046](../decisions/0046-primary-tenant.md) §8; [0047](../decisions/0047-special-pages.md) §12; [0051](../decisions/0051-page-redirects.md) §8; [0087](../decisions/0087-account-registration-and-challenges.md) §6, §7.*

### 2.1 The permission set and the ownership rules

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §2; [0012](../decisions/0012-api-requirements.md) §8.*

The permission set is the union of MediaWiki's rights that Triplespace implements, Wikibase's, and the `ts-*` rights of [0012](../decisions/0012-api-requirements.md) §8, with the additions later ADRs made. MediaWiki's own rights are reused where they exist (`deletedhistory`, `suppressrevision`, `deletelogentry`, and the rest of §2.2); the `ts-*` rights are new. `meta=userinfo&uiprop=rights` reports them. Which groups hold which rights is §2.2's, and `docs/registry/groups.toml` is the registry of record: each ADR that adds permissions lists them with their defaults and updates the registry.

**Ownership rules** are built in and are not records. The holder of an account may rename it, vanish it, and add or remove its own bindings and links ([07](07-actors-and-accounts.md)). The owner of a user page may edit its `json` and `yaml` subpages ([10](10-pages-and-content-models.md)). The actor who ran a job may revert it and read its rejects. The operator of a subsidiary may edit its user pages, issue and revoke its keys, retire, reactivate and rename it ([07](07-actors-and-accounts.md)). No permission grants access to another user's bindings: the `accounts` graph is outside this model entirely. Nobody vanishes another's account. The ownership rules do not extend to moderation: a user may not delete or hide their own contributions without the right ([0023](../decisions/0023-moderation.md) §11).

### 2.2 The permission catalogue

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §2; [0012](../decisions/0012-api-requirements.md) §8; [0019](../decisions/0019-discussions.md) §12; [0020](../decisions/0020-change-feeds.md) §7; [0021](../decisions/0021-notifications.md) §9; [0022](../decisions/0022-federation.md) §12; [0023](../decisions/0023-moderation.md) §11; [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0028](../decisions/0028-tenancy-policy.md) §13; [0030](../decisions/0030-edit-filters.md) §5, §12; [0039](../decisions/0039-files-and-media.md) §21; [0040](../decisions/0040-instance-prerogatives.md) §9; [0046](../decisions/0046-primary-tenant.md) §8; [0047](../decisions/0047-special-pages.md) §12; [0051](../decisions/0051-page-redirects.md) §8; [0085](../decisions/0085-settings-and-preferences.md) §3; [0087](../decisions/0087-account-registration-and-challenges.md) §6, §7; [0095](../decisions/0095-the-interface-namespace.md) §4.*

Every permission the ADRs name, with the group that holds it by default and what it allows. The catalogue is [`../registry/permissions.toml`](../registry/permissions.toml), and *Granted by default to* is derived from [`../registry/groups.toml`](../registry/groups.toml) (every group but `owner`, which holds every permission); the table is generated from the two by `catalogues.py --write` and never edited by hand ([24](24-documentation.md) §11). The generator fails if a group grants a permission the catalogue lacks, or the catalogue lists one no group grants without saying who holds it. A permission marked *(new)* is Triplespace's own; the rest are MediaWiki's or Wikibase's, kept under MediaWiki's names so that a Wikimedia administrator and the bots that check rights recognise them. `steward`, `global-sysop` and `platform-admin` are the global groups of §3.7, which exist only on a farm whose tenancy policy ships them. Where a permission is also an **instance right** (§2.3), the default named here is the group on the primary tenant.

<!-- catalogues.py: permissions -->
| Permission | Granted by default to | What it allows | Source |
|---|---|---|---|
| `read` | `universe` | Reading anything public. Every public feed, including the stream; the discovery endpoints of notifications; reading a foreign page, whose repository's content is public by definition; export from the report forms; exporting a proposal | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0020](../decisions/0020-change-feeds.md) §7; [0021](../decisions/0021-notifications.md) §9; [0047](../decisions/0047-special-pages.md) §12; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9; [0053](../decisions/0053-mirrored-pages.md) §11; [0067](../decisions/0067-proposals.md) §8 |
| `edit` | `temp`, `user` | Local-graph `add`, `remove` and `override` on any subject; page `edit`. On threads: `create`, `post` (including a status), and `edit` of one's own posts; a board's definition; `create` with `also` needs `edit` on the home and on each listed target. Asserting, changing and removing a page's statements, including `page-alternates`. Editing a `sanitized-css` page and the site-styles page. Editing a scope. Opening a proposal on the subject's talk page; replying upstream from a synchronized talk page; `follow` on a fork. Forking, with `createpage` | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0019](../decisions/0019-discussions.md) §12; [0049](../decisions/0049-boards.md) §13; [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §8; [0060](../decisions/0060-scopes.md) §9; [0067](../decisions/0067-proposals.md) §8; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `createpage` | `temp`, `user` | Page `create`; creating a board (namespace 310) or a scope (namespace 312); forking a mirrored page, with `edit` | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0049](../decisions/0049-boards.md) §13; [0060](../decisions/0060-scopes.md) §9; [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `item-term`, `property-term` | `user` | Labels, descriptions and aliases of items and properties | [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `item-redirect`, `item-merge` | `user` | Local `redirect` between local items ([04](04-entities-and-identifiers.md)) | [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `property-create` | `user`, `propertycreator`, `sysop` | Creating a local property. Held by every registered account by default, as in a stock Wikibase; a tenant that wants Wikidata's arrangement removes it from `user` in its configuration, leaving `propertycreator` and `sysop` | [0016](../decisions/0016-permissions-and-access-control.md) §2, A32 |
| `ts-linkproperty` *(new)* | `propertycreator`, `sysop` | `equivalent-property` links and a local property's `redirect` across namespaces ([04](04-entities-and-identifiers.md) §4.10), because they change predicates for every consumer | [0016](../decisions/0016-permissions-and-access-control.md) A32 |
| `ts-link` *(new)* | `autoconfirmed` | `same-as` and `different-from`; resolving held conflicts ([04](04-entities-and-identifiers.md)); in bulk, with `ts-runjob` | [0016](../decisions/0016-permissions-and-access-control.md) §2, §5 |
| `move` | `autoconfirmed` | Page `move`. On threads: `rename`, `move`, `attach` and `detach` (with `edit` on the target), `pin` and `unpin` (with `edit` on the page) | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0019](../decisions/0019-discussions.md) §12; [0049](../decisions/0049-boards.md) §13; [0069](../decisions/0069-synchronized-talk-pages.md) §11 |
| `editcontentmodel` | `user` | `action=changecontentmodel`, including to or from `sanitized-css` | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §8 |
| `delete`, `undelete` | `sysop` | Page `delete` and `undelete`: `read` ACLs on pages, threads and local entities naming the deletion group, and their retirement; applied to file pages and versions. `nuke` is part of the `delete` grant | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0019](../decisions/0019-discussions.md) §12; [0023](../decisions/0023-moderation.md) §11; [0039](../decisions/0039-files-and-media.md) §21; [0047](../decisions/0047-special-pages.md) §12 |
| `deletedhistory`, `deletedtext` | `sysop` | Reported to MediaWiki clients by members of the deletion group, which is what decides who sees hidden revisions, hidden usernames and deleted pages; they gate no evaluation of their own (§5.2) | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0023](../decisions/0023-moderation.md) §11 |
| `browsearchive` | `sysop` | As `deletedhistory`: reported by members of the deletion group, gating nothing | [0023](../decisions/0023-moderation.md) §11 |
| `deleterevision`, `deletelogentry` | `sysop` | Hiding parts of revisions and log events: `record` ACLs on `local`, `pages` and `actors` records, and on `log` records, naming `sysop`; hiding a post's text, comment or actor; `read` ACLs on statements and properties; the hiding Nuke does; applied to file versions | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0019](../decisions/0019-discussions.md) §12; [0023](../decisions/0023-moderation.md) §2, §11; [0039](../decisions/0039-files-and-media.md) §21; [0047](../decisions/0047-special-pages.md) §12 |
| `suppressrevision` | `suppress` | Suppression: any `read` ACL naming the suppression group; applied to file versions | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0023](../decisions/0023-moderation.md) §11; [0039](../decisions/0039-files-and-media.md) §21 |
| `viewsuppressed` | `suppress` | Seeing what the suppression group may see, and the `suppress/*` events | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0023](../decisions/0023-moderation.md) §11 |
| `hideuser` | `suppress` | `actor` ACLs: hiding a username; global actor ACLs when held through a global group | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0023](../decisions/0023-moderation.md) §11; [0028](../decisions/0028-tenancy-policy.md) §13 |
| `protect` | `sysop` | Setting ACLs on pages, entities, namespaces, threads, talk pages, boards and sets, including reserved titles: `edit` and `move` restrictions, and `read` as a confidential restriction (§4.5); changing a set's membership; enabling inbound federated replies on a talk namespace by ACL | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0023](../decisions/0023-moderation.md) §11; [0056](../decisions/0056-security-model.md) §3, §4; [0022](../decisions/0022-federation.md) §12 |
| `block` | `sysop` | Blocking accounts (§3.3); global blocks when held through a global group | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0028](../decisions/0028-tenancy-policy.md) §13 |
| `userrights` | `bureaucrat` | Changing memberships in any group except `owner`; approving a pending subsidiary; global memberships when held through a global group | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0028](../decisions/0028-tenancy-policy.md) §13 |
| `renameuser` | `bureaucrat` | Renaming any account | [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `import`, `importupload` | `sysop` | Page imports. Forking never needs `import` | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `bot` | `bot` | The bot flag on edits; also selects the `bot` row of the rate-limit table | [0016](../decisions/0016-permissions-and-access-control.md) §2; [0024](../decisions/0024-subsidiary-accounts.md) §11 |
| `ts-retain`, `ts-convert` *(new)* | `sysop` | `retain` and `convert` on entities ([05](05-providers-and-ingest.md)); in bulk, with `ts-runjob` | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2, §5 |
| `ts-runjob` *(new)* | `bot`, `sysop` | Submitting bulk jobs; a bulk `retain` or `convert` also needs the right above. Jobs run under subsidiaries: an administrator runs a job by creating or approving a subsidiary they operate. For instance jobs (a mirror sync or another job that writes only instance partitions) an instance right, default `bot` | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2; [0046](../decisions/0046-primary-tenant.md) §8 |
| `ts-revertjob` *(new)* | `sysop`, and the job's own actor | Reverting a job | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `ts-viewrejects` *(new)* | `sysop`, and the job's own actor | Downloading a job's rejects file | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `ts-erase` *(new)* | `suppress` | Appending `erase` records ([01](01-log-and-records.md) §5); erasing a post's text; applied to file versions | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2; [0019](../decisions/0019-discussions.md) §12; [0039](../decisions/0039-files-and-media.md) §21 |
| `ts-viewerasures` *(new)* | `sysop` | Seeing an erasure's reason class and authority reference (§6.6) | [0012](../decisions/0012-api-requirements.md) §8; [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `ts-unlink` *(new)* | `sysop` | Removing another user's account link | [0016](../decisions/0016-permissions-and-access-control.md) §2 |
| `ts-config` *(new)* | `bureaucrat` on a tenant; the farm's `steward` or `platform-admin` at instance scope | Writing `config` records: providers, issuers, namespaces, keyed types, roles, reconciliation, site settings, graph ACLs, the default retention policy; the tenant's own `tenant` ACL; `trust` mode per provider, `federation.*` settings and `federation-policy` lists; every `tenancy` switch except identity, `template`, locks, `provider-readers`; `page-repo` records, `pages.repos`, `pages.share`, a repository's mirror set, mode and events, and `pages index`; `fork.*` settings; `wikitext.site_styles` and `templatestyles.max_bytes`; category mappings; report modes and widening at the farm base; `site` settings, instance defaults and locks through `PATCH /settings` and the settings pages, unless a setting's registry entry names a stronger right ([0085](../decisions/0085-settings-and-preferences.md) §3). At the farm base an instance right | [0016](../decisions/0016-permissions-and-access-control.md) §2, §4; [0022](../decisions/0022-federation.md) §12; [0028](../decisions/0028-tenancy-policy.md) §13; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9; [0053](../decisions/0053-mirrored-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §8; [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0047](../decisions/0047-special-pages.md) §12; [0056](../decisions/0056-security-model.md) §3; [0085](../decisions/0085-settings-and-preferences.md) §3 |
| `ts-keys` *(new)* | `owner` only | Registering and rotating the instance key; `key:` records in `config`; the identity switches of the tenancy policy, since they are as consequential as the key. An instance right | [0016](../decisions/0016-permissions-and-access-control.md) §2, §4; [0028](../decisions/0028-tenancy-policy.md) §13 |
| `ts-editpost` *(new)* | `sysop` | `edit` of another actor's post | [0019](../decisions/0019-discussions.md) §12 |
| `viewmywatchlist`, `editmywatchlist` | `user` | Reading and changing one's own watch set, token and `seen` marks | [0020](../decisions/0020-change-feeds.md) §7 |
| `editmyoptions` | `user` | The preference matrix, addresses and handles; making one's own account a federation actor | [0021](../decisions/0021-notifications.md) §9; [0022](../decisions/0022-federation.md) §12 |
| `viewmyprivateinfo` | `user` | Reading the preference matrix, addresses and handles | [0021](../decisions/0021-notifications.md) §9 |
| `patrol` | `autoconfirmed` | Writing patrol events; seeing the unpatrolled mark (§6.4) | [0023](../decisions/0023-moderation.md) §11 |
| `autopatrol` *(new)* | `autopatrolled`, `bot`, `sysop` | Changes by this actor are patrolled from the start (§6.4) | [0023](../decisions/0023-moderation.md) §11 |
| `nuke` | `sysop` | Running `Special:Nuke`; each write it makes also needs its own right, `delete`, and `deleterevision` for hiding. Part of the `delete` grant in `grants.toml` | [0023](../decisions/0023-moderation.md) §11; [0047](../decisions/0047-special-pages.md) §12 |
| `editinterface` | `sysop` | Creating, editing or moving a page into or out of the Interface namespace, besides the rights any edit or move takes | [0095](../decisions/0095-the-interface-namespace.md) §4 |
| `createaccount` | `universe` | Self-registration, and creating an account for another, as in MediaWiki: creating a subsidiary, and creating one in the OAuth consent page. A subsidiary a new user creates is pending until approved. A private tenant removes it from `universe` (§4.7), and so does any tenant that closes registration; with `account.approval` on, self-registration files a request instead ([07](07-actors-and-accounts.md) §2.6) | [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0056](../decisions/0056-security-model.md) §3; [0087](../decisions/0087-account-registration-and-challenges.md) §1, §6 |
| `confirmaccount` | `bureaucrat` | ConfirmAccount's right: reviewing account requests at `Special:ConfirmAccounts`, approving or rejecting each; with `account.approval` on, creating an account for another at once. ConfirmAccount's `requestips` is `ts-viewip` here, and its `lookupcredentials` has no counterpart, since nothing of a request is kept once it is decided ([07](07-actors-and-accounts.md) §2.6) | [0087](../decisions/0087-account-registration-and-challenges.md) §6 |
| `skipcaptcha` | `bot`, `sysop` | ConfirmEdit's right, with its defaults: exemption from every challenge trigger ([07](07-actors-and-accounts.md) §2.6) | [0087](../decisions/0087-account-registration-and-challenges.md) §7 |
| `ts-transferaccount` *(new)* | `bureaucrat` | Transferring a subsidiary to another operator | [0024](../decisions/0024-subsidiary-accounts.md) §11 |
| `noratelimit` | `sysop` | Exemption from every rate-limit class but `job` | [0024](../decisions/0024-subsidiary-accounts.md) §11 |
| `mwoauthproposeconsumer` | `autoconfirmed` | Registering an OAuth consumer | [0025](../decisions/0025-oauth-server.md) §10 |
| `mwoauthupdateownconsumer` | `autoconfirmed` | Updating and rotating the secret of a consumer one owns | [0025](../decisions/0025-oauth-server.md) §10 |
| `mwoauthmanageconsumer` | `bureaucrat`, so the primary tenant's bureaucrats; the farm's `steward` and `platform-admin` | Approving, rejecting and disabling consumers, which are instance configuration. An instance right: held to no effect by a bureaucrat of any other tenant | [0025](../decisions/0025-oauth-server.md) §10; [0046](../decisions/0046-primary-tenant.md) §8 |
| `mwoauthmanagemygrants` | `user` | Viewing and revoking authorizations of one's own subsidiaries | [0025](../decisions/0025-oauth-server.md) §10 |
| `mwoauthviewprivate` | `sysop` | Viewing any account's authorizations and token metadata, for abuse investigation | [0025](../decisions/0025-oauth-server.md) §10 |
| `abusefilter-view`, `abusefilter-log` | `universe` | Seeing public filters and public hits | [0030](../decisions/0030-edit-filters.md) §12 |
| `abusefilter-view-private`, `abusefilter-log-private` | `sysop` | Seeing private filters' rules and hit details | [0030](../decisions/0030-edit-filters.md) §12 |
| `abusefilter-log-detail` | `sysop` | Seeing the full context and attempted content of a hit | [0030](../decisions/0030-edit-filters.md) §12 |
| `abusefilter-modify` | `sysop` | Creating and changing filters and their `log`, `tag`, `warn`, `disallow`, `throttle`, `unpatrol` and `notify` actions. For `scope = global` filters an instance right, held through a global group | [0030](../decisions/0030-edit-filters.md) §8, §12 |
| `abusefilter-modify-restricted` | `bureaucrat` | The `block` and `degroup` actions | [0030](../decisions/0030-edit-filters.md) §12 |
| `abusefilter-revert` | `sysop` | Reverting every change a filter matched in a period, as one job | [0030](../decisions/0030-edit-filters.md) §12 |
| `abusefilter-bypass` | none | Exemption from every filter | [0030](../decisions/0030-edit-filters.md) §12 |
| `managechangetags` | `sysop` | The tag registry: registering user-defined tags (§7.6) | [0030](../decisions/0030-edit-filters.md) §5 |
| `changetags` | `user` | Applying a tag by hand with `action=tag`, subject to the tag's own group | [0030](../decisions/0030-edit-filters.md) §5 |
| `upload` | `autoconfirmed` | Uploading a new file; stashing; copying a fork's files, with the same default | [0039](../decisions/0039-files-and-media.md) §21; [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `reupload` | `autoconfirmed` | Overwriting an existing file; `filerevert` | [0039](../decisions/0039-files-and-media.md) §21 |
| `reupload-own` | `autoconfirmed` | Overwriting a file one uploaded oneself | [0039](../decisions/0039-files-and-media.md) §21 |
| `reupload-shared` | `sysop` | Shadowing a foreign repository's file with a local one; copying a fork's files that shadow one | [0039](../decisions/0039-files-and-media.md) §21; [0054](../decisions/0054-forking-a-mirrored-page.md) §8 |
| `upload_by_url` | none | Uploading from a URL, where enabled | [0039](../decisions/0039-files-and-media.md) §21 |
| `movefile` | `autoconfirmed` | Moving a file page | [0039](../decisions/0039-files-and-media.md) §21 |
| `ts-takedown` *(new)* | `owner`; the farm's `steward` and `platform-admin` | Taking down and reinstating a file by hash. An instance right, evaluated on the primary tenant | [0039](../decisions/0039-files-and-media.md) §21 |
| `ts-expunge` *(new)* | `owner`; the farm's `steward` and `platform-admin` | Expunging a hash. An instance right, evaluated on the primary tenant | [0039](../decisions/0039-files-and-media.md) §21 |
| `ts-viewtakedown` *(new)* | `owner`; the farm's `steward` and `platform-admin` | Seeing taken-down bytes, confidential notices and `takedown/expunge` events | [0039](../decisions/0039-files-and-media.md) §21 |
| `ts-viewoperator` *(new)* | `owner` on the primary tenant; the farm's `steward` and `platform-admin` | Seeing which operator carried out an instance act: the attestation of authority records, and the operator column of the farm-base instance-action list | [0040](../decisions/0040-instance-prerogatives.md) §9 |
| `ts-primary` *(new)* | `owner` only | Offering and withdrawing the primary role. An instance right | [0046](../decisions/0046-primary-tenant.md) §8 |
| `suppressredirect` | `bot`, `sysop` | `noredirect` on a move | [0051](../decisions/0051-page-redirects.md) §8 |
| `delete-redirect` | `user` | Moving over a one-revision redirect that points at the moved page | [0051](../decisions/0051-page-redirects.md) §8 |
| `move-subpages` | `user` | `movesubpages` | [0051](../decisions/0051-page-redirects.md) §8 |
| `move-rootuserpages`, `move-categorypages` | `user` | Moving a user's root page, and moving category pages, as MediaWiki gates them | [0051](../decisions/0051-page-redirects.md) §8 |
| `ts-viewip` *(new)* | `sysop` | Seeing an IP in the abuse-handling store: the IP shown on a filter hit page | [0007](../decisions/0007-actor-identity.md) §3; [0030](../decisions/0030-edit-filters.md) §2, §5, §12 |
<!-- /catalogues.py -->

### 2.3 Instance rights

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §2; [0040](../decisions/0040-instance-prerogatives.md) §9; [0046](../decisions/0046-primary-tenant.md) §8; [0028](../decisions/0028-tenancy-policy.md) §13; [0039](../decisions/0039-files-and-media.md) §21; [0025](../decisions/0025-oauth-server.md) §10.*

An **instance right** is a permission that authorizes an instance act ([08](08-tenants-and-instances.md)). It is evaluated on the tenant that is primary at the time of the act, or through a global group at the farm base (§3.7); held on any other tenant it grants nothing at instance scope. The instance rights are `ts-keys`, `ts-primary` (offering the primary role; `owner` only), `ts-config` at the farm base, `abusefilter-modify` for `scope = global` filters (§7.7), `userrights`, `block` and `renameuser` on farm accounts (§3.7), `ts-takedown` and `ts-expunge`, `mwoauthmanageconsumer` (approving OAuth consumers, which are instance configuration), `ts-runjob` for instance jobs, that is, a mirror sync or another job that writes only instance partitions, and `ts-viewoperator`. The three file operator rights, `ts-takedown`, `ts-expunge` and `ts-viewtakedown`, are held only through the primary tenant or through a global group; `tenancy.toml` adds them to `steward` and `platform-admin`. `mwoauthmanageconsumer` is held to no effect by a bureaucrat of any other tenant. `ts-config` is `bureaucrat` on a tenant and the farm's `steward` or `platform-admin` at instance scope; `ts-keys` also covers the identity switches of the tenancy policy. Accepting an offer of the primary role needs no permission beyond membership of `owner` on the offered tenant.

### 2.4 Which permission each action needs

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §5.*

| Action | Permission |
|---|---|
| Bulk `retain` or `convert`; the default retention policy | `ts-retain` or `ts-convert` with `ts-runjob`; the default is a `config` record, so `ts-config` |
| `same-as`, `different-from`, bulk links, conflict resolution | `ts-link`; in bulk, with `ts-runjob`; `equivalent-property` needs `ts-linkproperty` |
| Rename, hide, vanish, remove a link | Own account: the holder. Others: `renameuser`, `hideuser`, `ts-unlink`. Nobody vanishes another's account |
| Create, move, delete, protect pages | `createpage`, `move`, `delete`, `protect`, plus the user-page ownership rule |
| Revert a job, see hidden usernames, erase, retain from the history page | `ts-revertjob`, membership in the deletion group, `ts-erase`, `ts-retain` |
| Protection, blocks, user rights | §3 and §4; each projects as the log events [16](16-logs-feeds-and-notifications.md) reserves |

## 3. Groups, memberships and blocks

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3; [0022](../decisions/0022-federation.md) §12; [0023](../decisions/0023-moderation.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0028](../decisions/0028-tenancy-policy.md) §3, §4, §13; [0056](../decisions/0056-security-model.md) §1.*

### 3.1 Default groups

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3; [0022](../decisions/0022-federation.md) §12; [0023](../decisions/0023-moderation.md) §11.*

The instance ships with `universe`, `temp`, `user`, `autoconfirmed`, `bot`, `propertycreator`, `sysop`, `bureaucrat`, `suppress`, `owner`, `autopatrolled`, `federated` and `read-all`. `read-all` holds no permissions and `includes` every group but `owner` and the suppression group (§3.2), so its members read every confidential target and may do nothing more with it; it is empty until someone is added to it, as any group is, and joining it notifies nobody (§3.3; [0016](../decisions/0016-permissions-and-access-control.md) A37, A38). `federated` is the group of fediverse surrogates, remote actors' stand-ins, and holds no permissions until a tenant's ACL names it. `autopatrolled` is a group administrators assign with `userrights`, as on Wikimedia projects; it holds `autopatrol`. MediaWiki's `interface-admin` is not created, because there are no user scripts or styles. `temp` is the group of temporary accounts ([07](07-actors-and-accounts.md)); its default permissions are `read`, `edit` and `createpage`, and an instance may empty it to require login. `user` is every registered local account, except a subsidiary with status `pending`, a status only an OAuth authorization sets ([07](07-actors-and-accounts.md) §4.3), which receives `user` only by an explicit membership. `autoconfirmed` is an **implicit group**, like `user`: no membership record is written for it, and an account is a member once `view.actor.created_at` and `view.actor.editcount` meet the age and edit-count thresholds set in `site` configuration. `editcount` is maintained by the activity projection from the account's local `edit` rows, erased ones included and job rows excluded, so a bot's job writes do not confirm its operator ([07](07-actors-and-accounts.md) §1.3; [03](03-storage-caches-and-search.md) §4.5).

### 3.2 Groups are configuration

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3.*

A group is a `config` record of kind `group` ([01](01-log-and-records.md), [23](23-configuration-and-registry.md)) listing its permissions. The defaults above ship in `docs/registry/groups.toml` and are written to `config` when an instance is created. Changing a group's permissions needs `ts-config`.

**A group may include others.** Its record may carry `includes`, a list of group names, or `"*"` for every group but `owner` and the tenant's suppression group (`moderation.suppress_group`), which a tenant adds by name beside `"*"` if it wants it. Its members count as members of each included group when a `read` restriction is evaluated, and for nothing else: not for an `edit`, `move` or `createpage` restriction, not for the lock-out of §4.5, and not for permissions, which come from their own groups alone (§3.4). An including group reads what the included groups read and changes nothing more. Inclusion is one level deep: a group named in `includes` passes on nothing of its own `includes`, so there are no chains and no cycles. Since `includes` changes who may read, changing it needs `userrights` as well as `ts-config`, and it is logged as the group's configuration is; it notifies the members of a newly included group only where that group notifies of joins (§3.3; [0016](../decisions/0016-permissions-and-access-control.md) A35, A37, A38).

**A group may delegate membership.** Its record may carry `adds` and `removes`, lists of group names: its members may add any account but their own to each group in `adds`, and remove any account but their own from each group in `removes`, without holding `userrights`. `adds_self` and `removes_self` list the groups they may also join or leave themselves, and are empty unless set, as MediaWiki's `$wgGroupsAddToSelf` is. None of the four may name `owner` or a group that holds `userrights` or `ts-config`, so delegation never reaches the rights that reach everything. A department's administrators can so run its membership (`hr-admins` with `adds = ["hr"]`) while a holder of `userrights` stays the one who can enter any group (§4.5; [0016](../decisions/0016-permissions-and-access-control.md) A36).

**A group may be scoped.** Its record may carry `within`, a list of namespace numbers and set IDs. Its permissions then hold only on targets those enclose along the containment axis (§4.4): a page and its subpages in a listed namespace, a member of a listed set, and the threads either is home to. Outside them the group grants nothing, so a scoped group fails closed: a new namespace or set is beyond its members until a `within` names it, where restricting an unscoped group space by space fails open. A scoped group may hold only permissions that act on a page, entity or thread; one with no such target (`userrights`, `ts-config`, `block`, `createaccount`, the instance rights of §2.3) is refused when the record is written. Membership in a scoped group counts everywhere in ACL tests, `read` included: the scope limits what its members may do, not which groups they are in. Changing `adds`, `removes`, `adds_self`, `removes_self` or `within` needs `userrights` as well as `ts-config`, as changing `includes` does ([0016](../decisions/0016-permissions-and-access-control.md) A36).

### 3.3 Memberships and blocks are actor records

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3.*

A membership change is a record of payload type `scatter:v0/membership` in the `actors` partition, keyed by the actor, whose content names the group, whether the actor is added or removed, and an expiry. Its comment part holds the reason. It projects as a `rights/rights` log event. Vanishing an account erases its memberships with everything else keyed to it.

A membership record is written by a holder of `userrights`, for any account and any group but `owner`, or by a member of a group whose `adds` or `removes` names the group, for any account but their own, and for their own where `adds_self` or `removes_self` names it (§3.2). Either way it projects as a public `rights/rights` log event, which is what makes a membership accountable. **Whether a join notifies is the group's to say:** a group record may carry `notify_joins = true`, and then each new member, by any route, is announced to the group's members (`group-join`, [16](16-logs-feeds-and-notifications.md) §5.2), as a channel announces who joined it. It is off unless set, and every shipped group leaves it off, `read-all` among them; a confidential group may want it on (§4.5; [0016](../decisions/0016-permissions-and-access-control.md) A36, A38).

A block is a record of payload type `scatter:v0/block` in the `actors` partition, keyed by the blocked actor, naming the permissions removed (by default `edit` and `createpage`, optionally everything but `read`) and an expiry. It projects as `block/block`, `block/reblock` or `block/unblock`. MediaWiki's practice of hiding a username on a block is kept as a checkbox on `Special:Block` that writes an `actor` ACL beside the `block` record (§6.3).

### 3.4 Evaluation

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3; [0025](../decisions/0025-oauth-server.md) §10; [0028](../decisions/0028-tenancy-policy.md) §3; [0056](../decisions/0056-security-model.md) §1.*

A block is the only negative rule in the model: an actor's effective permissions are the union of their groups' permissions, minus what their active blocks remove. A subsidiary's permissions are the union over its own groups, minus its own blocks and its operator's, so blocking a person blocks their bots; a block on a subsidiary does not reach its operator. A subsidiary acting through an API key or an OAuth token has its effective permissions intersected with the credential's grants: a grant can withhold a capability; it cannot add a group, so it never widens what an ACL admits ([07](07-actors-and-accounts.md)). An actor with status `pending` does not receive the implicit `user` membership. On a farm with global groups, a tenant account's permissions also include the global groups of the farm account it is publicly linked to, and subtract that farm account's global blocks (§3.7). There is no revocation at the group level. Membership in a group, as a `read` restriction tests it, also counts the groups the actor's groups include (§3.2); every other ACL test, the lock-out of §4.5 and permissions count direct membership only ([0016](../decisions/0016-permissions-and-access-control.md) A37). A **scoped** group's permissions count only on targets its `within` encloses (§3.2): an actor holds a permission on target *T* when one of its groups grants it and is either unscoped or scoped to something enclosing *T*. A move needs the permission at its source and at its destination, so a scoped group cannot carry a page out of its scope. `meta=userinfo` reports a scoped group's permissions apart, under `ts-scopedrights` with their scopes, and leaves them out of MediaWiki's `rights`, which clients read as holding everywhere ([0016](../decisions/0016-permissions-and-access-control.md) A36).

### 3.5 Blocks on IP addresses

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3; [0028](../decisions/0028-tenancy-policy.md) §4.*

Blocks on IP addresses apply to anonymous requests before a temporary account exists. IP addresses are never written to the log, so IP blocks live in the `private` schema ([03](03-storage-caches-and-search.md) §1.2) and appear in `list=blocks` without appearing in the log. Under `blocks.global = ip`, farm-wide blocking is by IP address only, in `private` likewise, applied to every tenant's anonymous requests and account creations. Every tenancy preset has that much, because a hosting operator needs it too.

### 3.6 `owner`

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §3.*

`owner` is created with the instance, on the primary tenant, and that group is the instance's `owner`: the one that holds `ts-keys` and `ts-primary`. A tenant created later has an `owner` of its own, which is that wiki's owner and exercises no instance right there ([08](08-tenants-and-instances.md)). `instance create` creates the first local user, `local:1`, as the sole member of `owner`, and gives it a binding to the built-in `password` issuer with a password the CLI sets; a primary account holds no token. On a tenant that adopts an existing Wikibase, `instance create --adopt {source} --owner {user_id}` creates the owner under the source's user ID instead, with the same password binding, and sets the user-ID floor ([05](05-providers-and-ingest.md)). That is the single-user fallback Scatterbase asks for: an instance with no external issuer configured has exactly this one user, who logs in with a password and registers issuers, groups and the sync subsidiaries from there. Membership in `owner` is changed only by a member of `owner`. **`owner` reads everything by definition:** it ships with `includes = ["*"]`, so its members read every confidential target, though not what the suppression group alone may read (§3.2), and change or lift a confidential restriction only for a group they have joined (§4.5). **`owner` is a bootstrap role.** It sets a tenant up; a mature tenant whose own groups cover every right it uses may retire it. The last member may leave only when nothing needs `owner` any more: every permission the tenant's groups use, `userrights` among them, is held by a group other than `owner` that has a current member, Rights that no group but `owner` holds do not count: on the primary tenant, `ts-keys` and `ts-primary` may be granted to a group, which will usually be a very exclusive one, or to none, in which case they are exercised from the command line alone; a mature deployment may prefer that. With no member, `owner` is restored, new owners are appointed and the instance rights are granted again only from the command line (`triplespace-cli`), each an instance act and recorded as one (08) ([0016](../decisions/0016-permissions-and-access-control.md) A37, A38).

### 3.7 Global groups and global blocks

*Sources: [0028](../decisions/0028-tenancy-policy.md) §3, §4, §13.*

With `groups.global = inherited`, the instance `config` may hold `group` records with `scope = global`. Their memberships are `membership` records in `actors/{farm}` keyed by the **farm account**, written with `userrights` by members of a global group that holds it, and projected as `rights/rights` in `log/{farm}`.

Evaluation on tenant *T* for a session gains one term: the effective permissions of a tenant account are the union over its own groups **and over the global groups of the farm account it is publicly linked to**, minus its own blocks, its operator's and the farm account's. The join runs over `view.account_link` ([03](03-storage-caches-and-search.md) §4.5), which is public, so no private data enters evaluation. **A tenant cannot exclude a global group**, as a Wikimedia wiki cannot exclude stewards; the presets that enable them are the ones where that is wanted, and an instance that wants otherwise sets `none`. Global groups are subject to ACLs like anyone: an ACL restricting `edit` to a tenant's `sysop` is not satisfied by a global group unless the ACL names it.

The `community` preset ships two: `steward` (`userrights`, `block`, `hideuser`, `ts-config`, `renameuser`, `deletedhistory`, `viewsuppressed`) and `global-sysop` (the tenant `sysop` set). The `enterprise` preset ships `platform-admin` with `steward`'s set. All are in `tenancy.toml`, which also adds `ts-takedown`, `ts-expunge` and `ts-viewtakedown` to `steward` and `platform-admin` (§2.3). `userrights`, `block` and `hideuser` govern global memberships, blocks and actor ACLs when held through a global group, with the defaults of §2.2 in these groups.

**Global blocks.** With `blocks.global = farm-actor`, a `block` record in `actors/{farm}` keyed by a farm account removes permissions from **every tenant account linked to it**, through the same join, and prevents the farm account from creating accounts on further tenants. It projects as `block/block` in `log/{farm}` and is listed on `Special:GlobalBlockList` at the farm base. A tenant's own blocks are unaffected and still apply to its accounts alone.

**Farm-wide actor ACLs.** An `actor` ACL (§6.3) on a farm account hides its name on every tenant, since every linked account carries the same name by construction. An actor ACL written by a tenant on its own account hides that account only. Farm-wide hiding is a property of farm identity, not of the primary tenant.

## 4. ACLs and their targets

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4; [0019](../decisions/0019-discussions.md) §12; [0023](../decisions/0023-moderation.md) §1, §2, §3; [0056](../decisions/0056-security-model.md) §3, §4.*

### 4.1 An ACL is a restriction on a target

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4; [0023](../decisions/0023-moderation.md) §1, §3.*

An ACL attaches a restriction to a **target**, named by identifier so that the record's key is never content ([01](01-log-and-records.md)). Its content lists, for each permission it restricts, the group whose members may still perform it, and an expiry. ACLs restrict `read` as well as write permissions, which is what deletion, revision-hiding, suppression and username-hiding are: every administrative action of §6 except patrolling is an ACL record, a restriction of named permissions on one target to one group, until an expiry, and undoing the action retires the record. A `read` ACL is of one of two kinds (§5.2): a **moderation** restriction, deletion, hiding, suppression or username-hiding, and a **confidential** restriction, which keeps the target live for its group and absent for everyone else.

**`scatter:v0/acl` is a payload type, not a `config` kind.** A graph ACL is configuration and is appended to the tenant `config` partition. Every other target is **moderation**, and its records go to the tenant `log` partition, which is where protection was always expected to be written: it is "an action with no other record". The same payload type is used in both partitions; `docs/registry/graphs.toml` lists it for both. Moderation records are frequent, they are not configuration, and the `config` partition is exported publicly; suppression records must not be. The `log` partition is `logged`, `full` and **internal**: served by the API under the visibility rule of §6.5, never in a public dump. Edit filter and hit records share the `log` partition for the same reason (§7.5). The one exception among non-graph targets is the `tenant` ACL, which lives in `config` because it is not secret (§4.7).

### 4.2 Target kinds

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4; [0023](../decisions/0023-moderation.md) §2; [0056](../decisions/0056-security-model.md) §3; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

| Target kind | Key | Partition | Restricts |
|---|---|---|---|
| `graph` | `acl:graph:{name}` | Tenant `config` | Writing records to the graph. This replaces the *writers* field of the crate layout, which was already an ACL under another name |
| `namespace` | `acl:namespace:{number}` | Tenant `log` | Actions on every page or entity in the namespace; a `read` ACL on a namespace makes it private |
| `page` | `acl:page:{page id}` | Tenant `log` | Actions on one document page and, if the namespace allows subpages, its subpages; `read` (deletion) and, for a **reserved** page ID, `createpage` (§4.8) |
| `entity` | `acl:entity:{id}` | Tenant `log` | Local assertions about one entity, and its `retain` and `convert`; `read` (deletion of a local entity) |
| `statement` | `acl:statement:{guid}`, the GUID as stored in the log; the UUID part identifies the statement across the canonical-ID rewrite ([04](04-entities-and-identifiers.md) §4.9) | Tenant `log` | `edit`: changing, overriding or removing that one statement, its qualifiers and references. `read`: the statement is left out of the resolved view, RDF, search and diffs for viewers outside the group |
| `property` | `acl:property:{id}` | Tenant `log` | `edit`: writing any snak whose property is *P*, as main snak, qualifier or reference, on any entity. `read`: every snak with predicate *P* is left out for viewers outside the group. Enclosure by predicate |
| `record` | `acl:record:{partition}:{offset}` | Tenant `log` | `read` of the named **parts** of one record. The ACL's content carries `parts`, any subset of the payload type's parts (`content`, `comment`, `attestation`, and `text` for threads); an empty list means the whole record. Revision-hiding and suppression |
| `actor` | `acl:actor:{key}` | Tenant `log` | `read` of the actor's name and other attributes, everywhere they appear; the actor key and IRI, which never contain a name, stay visible. Username-hiding |
| `blob` | `acl:blob:{takedown id}`, at instance scope | Instance | `read` and `upload`, to no group, on every file version in every tenant whose SHA-256 or SHA-1 equals the hash in the record. Takedowns ([12](12-files-and-media.md)) |
| `tenant` | `acl:tenant:{slug}` | The tenant's `config` | `read`: the tenant's visibility; a tenant with a `read` restriction is private. Encloses every partition of the tenant, and so every page, entity, thread, file, record and log event in it |
| `set` | `acl:set:{id}`, the ID from the page-ID sequence as a reserved title takes one | Tenant `log`, as every moderation-shaped ACL is | `read`, `edit`, `move` on its **members**: pages (each with its subpages, when the namespace has them), entities and threads, listed by ID in the record's content with the set's name; membership changes need `protect` |

A `read` ACL on a mirrored statement is not an `override`. The `override` of [05](05-providers-and-ingest.md) is an editorial act, a local record public in history, and the resolved view says the statement was suppressed. A `read` ACL is moderation: the statement is hidden for privacy or legal reasons, only the group sees that it exists, and the source graph is untouched. Both are available; they answer different questions. Setting either the `statement` or the `property` target kind needs `protect` for `edit` and `deleterevision` for `read`, as for a page; they project as `protect/*` and as a `delete/statement` event.

The current ACL per target is projected to `view.acl`, whose shape is `(target, restrictions jsonb, read_kind, extra jsonb, "offset")` with primary key `(target)`, `read_kind` being `confidential` or `moderation`, derived from the right that set the record ([03](03-storage-caches-and-search.md) §4.5, §4.14); the evaluation path reads it for the target and its enclosures, cached in L0, and the common case, no row, is one indexed miss. For the `statement` and `property` kinds the common case costs less than that: `view.tenant` carries a counter of the tenant's live `statement` and `property` read ACLs, maintained by the ACL projection, and composition skips the lookup altogether while it is zero (§4.4).

### 4.3 The ACL record and its log events

*Sources: [0023](../decisions/0023-moderation.md) §3; [0016](../decisions/0016-permissions-and-access-control.md) §4.*

| | |
|---|---|
| Header key | The target key of §4.2, so the latest record per key is the current ACL, and erasing a target's history by key ([01](01-log-and-records.md) §5) takes its ACL history with it |
| Content part | The target; for each restricted permission, the group and an optional expiry; `parts` for a `record` target; the reserved title for create-protection; for a whole-page or whole-entity deletion, the flag `talk: true` when the paired talk page is deleted by the same action, and for a page's confidential restriction when the talk page is under it (§4.4), kept as `confidential_talk` while such a page is deleted; for the deletion of a confidential target, `confidential`, its confidential `read` restriction, kept beside the deletion group so that the deleted target is seen by members of both and stays absent for everyone else, and restored by undeleting (§5.2; [0023](../decisions/0023-moderation.md) A15). A content of `null` retires the ACL |
| Comment part | The reason. This is MediaWiki's deletion, protection or suppression reason, and a `record` ACL on this record's `comment` part hides it if the reason is itself abusive |
| Attestation | Who acted, as for every record |

**Undelete, unhide and unprotect are retirements.** A record with null content, same key. **Partial undeletion**, MediaWiki's restoring of some revisions but not others, is a retirement of the page ACL followed by `record` ACLs on the revisions that stay hidden; both are written in one request. **Expiry** applies to protection as MediaWiki's does; a deletion is written without one.

**Log events**, projected from these records ([16](16-logs-feeds-and-notifications.md)):

| ACL | Log event | Parameters |
|---|---|---|
| Protect: written, changed, retired | `protect/protect`, `protect/modify`, `protect/unprotect` | Each restriction with its group and expiry |
| Delete: written, retired | `delete/delete`, `delete/restore` | Whether the talk page went with it; on restore, the record ACLs that stay |
| Hide a record in `local`, `pages` or `actors` | `delete/revision` | The revision ID, the parts hidden and the parts revealed |
| Hide a record in `log` | `delete/event` | The log ID, likewise |
| Suppress (any of the above with the suppression group) | `suppress/delete`, `suppress/revision`, `suppress/event` | As above |
| Hide a username | `suppress/hide-user` (MediaWiki writes this as `suppress/block` with `hideuser`) | The actor key |

A record whose target is itself hidden appears with its target hidden, as MediaWiki shows `(username removed)`. Page ACLs project as `protect/protect`, `protect/modify` and `protect/unprotect` with the parameters the log-event catalogue already lists.

### 4.4 Evaluation and enclosure

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4; [0019](../decisions/0019-discussions.md) §12; [0023](../decisions/0023-moderation.md) §1, §2; [0056](../decisions/0056-security-model.md) §3, A23; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

**Evaluation is conjunctive.** To perform action *A* on target *T*, an actor must hold the permission for *A* after blocks (§3.4), **and** satisfy every ACL that restricts *A* on *T* or on a target enclosing *T*. To read *T*, an actor must hold `read`, which `universe` does, **and** satisfy every ACL restricting `read` on *T* or on a target enclosing *T*; a deletion ACL naming `sysop` is satisfied only by members of `sysop`. There is no other inheritance, no cascading protection, and no rule under which a more specific ACL loosens a broader one. This is how MediaWiki protection composes with group rights: matching is by target and enclosures, inheritance is enclosure only, and nothing denies except a block.

**Enclosure has three axes.** The **containment** axis is fixed: tenant ⊃ graph ⊃ namespace ⊃ page ⊃ subpage; tenant ⊃ set ⊃ member; a talk page or board ⊃ the threads whose **home** it is, not those only listed on it, and the page's `edit` ACL governs what may be listed there ([14](14-discussions.md)); entity ⊃ statement. So a `read` ACL on a namespace makes it private, protecting `Item talk:Q42` with `acl:page:{talk page ID}` restricts every thread homed on it while a thread moved elsewhere leaves that protection behind, and deleting a talk page deletes the threads homed there. Namespace ACLs on talk namespaces apply as they do to any namespace. **A subject page does not enclose its talk page**, in either direction: protecting an item leaves its talk page open, which is what protection is for, and deleting an item leaves its talk page unless the administrator deletes that too. The delete form offers to, as MediaWiki's does, and writes two records. **A confidential restriction may take the talk page with it** in the same way: the subject's ACL carries `talk: true`, and the talk page and the threads homed there are then under the restriction, read, listed and posted to by the group alone, as a deletion with `talk` deletes them. `action=protect` takes `tstalk`, `/acl` takes `talk`, and the Protect dialog offers it, on by default when a `read` restriction is set, since people who hide a page usually mean its discussion too; the enclosure rule is unchanged, since it is the one record that names both ([0056](../decisions/0056-security-model.md) A24). The **predicate** axis: a property ACL applies to every snak using that property, wherever it sits. To edit statement *S* on entity *E*, an actor satisfies the ACLs on *S*, on *E*, on *E*'s namespace, on the graph, and on *S*'s property. The cost is one indexed lookup per property a write touches, cached in L0; on the read side, composition ([03](03-storage-caches-and-search.md) §6) asks `view.acl` for the entity's GUIDs and properties in one query, and the public form of the resolved view simply lacks what a read ACL hides. That query is skipped while the tenant's counter of live `statement` and `property` read ACLs in `view.tenant` is zero (§4.2), which on most tenants it always is, and shared-row composition never makes it, since a read ACL is a tenant's and a shared row belongs to none; the ACL and visibility re-projections consume composition events rather than running inside any write ([0083](../decisions/0083-write-path-in-three-tiers.md) §3). The **content** axis: a `blob` target encloses every file version whose bytes hash to the record's value, in every tenant.

Evaluation stays conjunctive and nothing loosens: a page in a public namespace of a private tenant is private; a public page added to a restricted set becomes restricted; a page in two sets is read by members of both groups.

**A page encloses its subpages with all of its ACL but its deletion.** In a namespace with subpages, `Foo/bar` is under `Foo`'s protections and confidential restriction, but not under `Foo`'s deletion, which is of the page alone, as in MediaWiki: deleting `Foo` leaves `Foo/bar` readable. A confidential restriction that a deletion kept beside its own group (§5.2) still encloses. **A set encloses what its members enclose**: a member page's subpages and the threads it is home to are restricted with it, as a namespace's pages are ([0056](../decisions/0056-security-model.md) A23).

### 4.5 Who may restrict what

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4; [0056](../decisions/0056-security-model.md) §4; [0023](../decisions/0023-moderation.md) §2; [0085](../decisions/0085-settings-and-preferences.md) §4.*

Setting an ACL on a page, entity, namespace or set needs `protect`; on a graph or the tenant, `ts-config`, and the instance may lock the tenant's under `config.template = locks`, which makes it a prerogative ([08](08-tenants-and-instances.md)). A moderation `read` restriction needs `delete`, `deleterevision`, `suppressrevision` or `hideuser` by target (§6). The `blob` target is the operator's ([12](12-files-and-media.md)).

- **`protect` may restrict `read`.** `action=protect` and the Protect dialog accept `read` beside `edit` and `move`, with a group and an expiry, on a page, entity, namespace, thread, talk page, board or set. This is the confidential restriction of §5.2.
- **The actor must be in the group they restrict to, and in the group of any confidential restriction they change or lift.** Setting a `read` restriction that names a group the actor is not a member of is refused with `ts-locked-out`; so is replacing or retiring a confidential `read` restriction whose group the actor is outside, whatever rights the actor holds ([0056](../decisions/0056-security-model.md) A18). This is the check MediaWiki makes for protection levels and it keeps an administrator from hiding a page from themselves, or unhiding one they were never shown; it also binds `owner`, which holds every permission and reads every confidential target through its default `includes` (§3.6), but sets, changes or lifts a confidential restriction only for a group it has joined, since inclusion counts for reading alone (§3.2). A restriction naming the suppression group hides the target from `owner` as a suppression does, unless the tenant includes that group for `owner` ([0056](../decisions/0056-security-model.md) A21).
- **Groups are the only route to a confidential target.** No permission reads past a confidential restriction: not `protect`, `delete` or the other moderation rights, not `ts-config`, not the permissions of `owner`. A principal reads the target because it is a member of the group, directly or through a group that `includes` it (§3.2); `owner` and `read-all` read everything that way by default (§3.1, §3.6; [0056](../decisions/0056-security-model.md) A21, A22). A tenant that wants a class of accounts to read every restricted target, such as auditors over siloed departments, gives their group `includes`, or puts them in each group ([0056](../decisions/0056-security-model.md) A18).
- **`userrights` is the accountable way in.** A holder of `userrights` (`bureaucrat` by default, §2.2) is in no group by virtue of the right, but may add any account, itself included, to any group but `owner`, and so reach any confidential target. The membership is a record like any other (§3.3): it projects as a public `rights/rights` log event, which is the accountability, and the group's members are notified where the group notifies of joins (`notify_joins`, §3.3; [0056](../decisions/0056-security-model.md) A22). `Special:UserRights` proposes an expiry when an actor adds itself, since a look at one page needs no standing membership. It follows that no tenant can have a group that `userrights` cannot enter; that is a deliberate limit, the price of the next rule ([0056](../decisions/0056-security-model.md) A18).
- **Nothing is unrecoverable.** Every confidential target stays reachable by an accountable act: a membership written under `userrights`, or an instance act ([08](08-tenants-and-instances.md); §5.6). So that an orphaned group is found before anyone needs it, `Special:ProtectedPages` shows holders of `userrights`, beside the restrictions they may read, each group that a confidential restriction in force names and that has no current member, with the number of restrictions naming it and no titles ([0056](../decisions/0056-security-model.md) A18).
- **A tenant's visibility needs `ts-config`.** Making a public tenant private purges its public forms ([03](03-storage-caches-and-search.md) §9.6) and withdraws it from every reader it had (§5.4); `Special:WikiSettings` says so before it saves.
- **The tenancy policy decides whether tenants may restrict at all** (`security.restrictions`, [08](08-tenants-and-instances.md)). A community farm in the Wikimedia tradition sets `none`, and `protect` with `read=` is refused there with `ts-policy`.
- **A restriction names a group the tenant has**: a registry default, a group its `config` defines (§3.2), or a group with a current member. A group that only ACLs name needs no record, and exists once someone is in it; a misspelt name is refused ([0056](../decisions/0056-security-model.md) A23).
- **Nothing else changes.** Deletion, hiding and suppression keep their rights and their public notices; graph ACLs stay `ts-config`.

### 4.6 Default graph ACLs

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §4.*

Written with the instance:

| Graph | `edit` restricted to |
|---|---|
| `local`, `pages` | `universe` (no restriction beyond the permission itself) |
| `mirror/{provider}`, `actors/{provider}`, `log/{provider}` | `bot`, so only sync jobs write there |
| `config` | `bureaucrat`; `key:` records additionally need `ts-keys` |
| `actors` | Written only by account management; `membership` and `block` records need `userrights` and `block` respectively |
| `accounts` | The login system only; no group |

### 4.7 The tenant and the set

*Sources: [0056](../decisions/0056-security-model.md) §3, §13; [0023](../decisions/0023-moderation.md) §2; [0085](../decisions/0085-settings-and-preferences.md) §4.*

**A tenant's visibility is its `tenant` ACL.** A tenant with none is **public**. A tenant whose `tenant` ACL restricts `read` is **private**: readable by the group the ACL names, `user` by default, which is every account of the tenant, or any group the tenant chooses for a narrower membership. The ACL lives in `config` because it is not secret: that a tenant is private is the first thing a visitor learns (§5.3). A private tenant usually also removes `createaccount` from `universe`, so that accounts are made by its bureaucrats or by invitation; the Visibility tab of `Special:WikiSettings` offers both switches together. `temp` is irrelevant on a private tenant, since a temporary account is created by an edit and nobody outside `user` can read what they would edit. The tenant's `read` ACL records on a namespace or tenant target project to `view.tenant (tenant, visibility_epoch)` ([03](03-storage-caches-and-search.md) §4.6), the purge epoch of [03](03-storage-caches-and-search.md) §9.6 (§5.1); the tenant's configuration stays in `view.registry`.

What a private tenant does not have, because each of these is a public form of its data: a public dump or bundle (its partitions are exported as `private` for every purpose but the operator's own backups and a tenant move), an update stream, a public SPARQL endpoint, graphs in the shared query store (under `query.isolation = store` it keeps a query service of its own for principals that satisfy its `tenant` ACL, [08](08-tenants-and-instances.md) §9.1), a provider code, `pages.share` or a file or template repository that other tenants read, outbound federation, a sitemap, or indexable pages: every response carries `noindex`. It may still *read* everything the tenancy policy lets it: Wikidata, a provider tenant, Wikipedia as a page repository. Public data flows in; nothing flows out.

**What a private tenant shows an outsider.** Every page, entity, thread and search result is absent, exactly as a confidential target is (§5.3, §5.8 case 1): `missing` in the Action API, 404 from REST, nothing in lists or counts. A page edit is refused as at any title the editor may not read (`ts-restricted-title`); an entity write is refused with `permissiondenied`, since a Wikibase client cannot usefully be told that the item it is creating is missing. Login, account creation where the tenant allows it, and the account routes work as on a public tenant. `meta=siteinfo` alone says the tenant is private, in a limited form: `general` keeps `sitename`, `lang`, `logo`, `favicon`, `server` and `mainpage` and adds `private: true`; `triplespace` keeps the tenant, the API version, the theme, `visibility` and the login, account and challenge fields; every other `siprop` is left out ([0056](../decisions/0056-security-model.md) A23).

**A set is a grouping that only `protect` can change.** Its record carries a `name`, shown where the restriction is shown, and `members`. Members are IDs, never titles, because a key is never content and a move must not change what is restricted. A page ID that names an entity's page is read as that entity, as `pageids=` reads it, and the set records the entity's ID ([0056](../decisions/0056-security-model.md) A28). A set is flat: it does not contain sets or namespaces, which have ACLs of their own, and a target may be in any number of sets. Adding or removing a member is a new version of the record, needs `protect`, and projects as `protect/modify` with the member named; the set's `read` restriction is in the same record as its membership, so there is no moment at which a member is listed and unrestricted. The ID is taken from the page-ID sequence as a create-protection reserves one (§4.8); a set is not a page and has no title. A category, a title prefix or a tag would not do: all three are content. A category link is written by whoever may edit the page; a prefix changes with a move; a tag on a revision is a claim by its author. A grouping that restricts reading has to be changed only by the right that sets restrictions, and only by ID.

**A scope ([15](15-structured-pages.md)) is not a set.** It is content: a page that anyone with `edit` may change, naming subjects by title and by query. It restricts nothing, is never a target, and shares no table with sets.

### 4.8 Create-protection reserves a page ID

*Sources: [0023](../decisions/0023-moderation.md) §2.*

A title that does not exist has no page ID, and a key must never be content, so the protecting record takes a page ID from the sequence and carries the namespace and title in its content part. The title index maps the title to the reserved ID; a `create` by an actor the ACL admits takes that ID, and the page then exists. This is the pattern talk-page IDs use ([14](14-discussions.md)). A `create` at a create-protected title by an actor the ACL does not admit is refused with `ts-restricted-title`, the same refusal a confidential title gives (§5.3).

**What a reservation takes.** A title with no page takes `createpage` (MediaWiki's `create`) and `read`; `edit` and `move` wait for the page, and `createpage` is refused on a page that exists. A `read` restriction on a title with no page is a **confidential reservation**: the page is confidential from its first revision, so nothing about it is ever public. A deleted page takes no new `read` restriction, as nothing that does not exist does; its title can be reserved instead, and a reservation, like a new page, keeps the deleted page from being undeleted into the title (§6.2). Lifting every restriction before the page exists retires the ACL and frees the title. The reserved ID is held in `view.page` as a row with no revision (`reserved`, `latest_offset` null), which the title index counts, so no other page can be created there; every reader that asks whether a page exists skips it.

## 5. Visibility, absence and flow

*Sources: [0056](../decisions/0056-security-model.md) §2, §5, §6, §9, §12, §15; [0012](../decisions/0012-api-requirements.md) §8; [0023](../decisions/0023-moderation.md) §1.*

### 5.1 The visibility of a target

*Sources: [0056](../decisions/0056-security-model.md) §2.*

The **visibility** of a target *T* is the set of `read` ACLs on *T* and on every target that encloses *T*, along the three axes of §4.4. A principal **may read** *T* when it holds `read` and satisfies every ACL in that set. Writing `groups(T)` for the groups those ACLs name, the test is membership in each. The visibility of a public target is empty.

The set is what everything downstream keys on: a cache entry is good for every principal that satisfies the set it was computed under; a search document is returned only to principals that satisfy its set; a derived output is readable only where every input's set is satisfied (§5.4). Every cache key whose value depends on what the viewer may read carries a `{vis}` segment, a hash of the visibility set with the tenant's visibility epoch ([03](03-storage-caches-and-search.md) §9.4, where each key is marked). **The empty set is `-{epoch}` for a tenant's key and plain `-` for a shared key**: a public entry under a tenant's namespace lapses when the tenant's epoch moves, and a shared row's entry, which belongs to no tenant and can be enclosed by nothing a tenant restricts, carries no epoch and is never purged by one.

A `read` ACL written, changed or retired on a target, of either kind, bumps the target's **generation**, as hiding does, since both kinds of read restriction hide for privacy and no entry at any version may remain reachable; a version bump is for changes that hide nothing. **Only a `namespace` or `tenant` ACL bumps the tenant's visibility epoch**, so that every entry under the enclosing target lapses at once ([03](03-storage-caches-and-search.md) §9.6); a change to a `set`, its restriction or its membership, bumps the generation of each member it touches, since the members are listed in the record and no entry outside them can be affected.

### 5.2 Two kinds of `read` restriction, one record

*Sources: [0056](../decisions/0056-security-model.md) §2; [0023](../decisions/0023-moderation.md) §1.*

The payload is `scatter:v0/acl` either way (§4.3); the intent, the right that sets it and what outsiders see differ.

| | Moderation | Confidential |
|---|---|---|
| Means | The target has been removed: deleted, hidden, suppressed | The target exists for a group and for nobody else |
| Set with | `delete`, `deleterevision`, `suppressrevision`, `hideuser`, by target | `protect`, with `read=` in the restriction (§4.5) |
| Expiry | None on a deletion; as MediaWiki on a hide | Allowed, as on any protection |
| For the group | The deleted-page notice, the hidden history, `prop=deletedrevisions` | An ordinary live target: edited, watched, searched, fed, notified, rendered |
| For everyone else | MediaWiki's notices: "this page has been deleted", with the public deletion log entry; a revision marked hidden; `(username removed)` | **Absent** (§5.3): indistinguishable from a target that never existed |
| In search, feeds, notifications | Not indexed, not fed, not delivered | Indexed, fed and delivered to the group, with the visibility set attached |
| Log event | Public, naming the target, as MediaWiki's deletion and protection logs are | Readable only by principals that may read the target (§5.3) |

**Restricting `read` to a group hides the target from everyone outside it.** The moderation kind means the target was removed, and outsiders see the notices of §6; the confidential kind hides even that the target exists. Where both apply, nothing new is evaluated: a deleted confidential page is seen by the intersection of its deletion group and its confidential group, which is what conjunction gives. **Suppression is a narrower group, not a different mechanism.** The same record with `suppress` in place of `sysop` hides the target from administrators too; MediaWiki's fourth RevisionDelete bit is this and nothing more. **Groups, not rights, name who may still see.** MediaWiki gates deleted content on the `deletedhistory` and `deletedtext` rights and suppressed content on `viewsuppressed`. Here the ACL names a group, and those rights are what membership in that group reports to `meta=userinfo`. The default deletion group is `site` configuration (`moderation.delete_group`, default `sysop`; `moderation.suppress_group`, default `suppress`), so an instance that wants a separate reviewers group can have one without a new mechanism.

### 5.3 Absence: what a principal outside the group sees

*Sources: [0056](../decisions/0056-security-model.md) §5; [0012](../decisions/0012-api-requirements.md) §8.*

**A target under a confidential restriction the principal does not satisfy is absent.** Every route answers as it would for a target that does not exist: `missing` in the Action API, 404 from REST with the body a missing title gets, a red link in rendered text, `nil` from Lua, an empty stack in title inheritance ([13](13-mirrored-pages.md)). Lists and counts omit it, and a log event on it is omitted with it. The response is the same status, shape and cache class as a genuine miss, so that a probe cannot tell them apart; a file request is already answered this way, 404 and never 403 ([12](12-files-and-media.md)). This differs from moderation deliberately: a deleted page says it was deleted, because MediaWiki users and tools expect that, and because a deletion is a public act; a confidential page says nothing, because its existence is the first fact being protected.

**Everything that lists, counts or links filters by the principal.** Recent changes, watchlists, histories, contributions and the activity stream, the logs, search and suggestions, category members and counts, `Special:WhatLinksHere` and `list=backlinks`, `list=allpages`, `Special:Random`, `prop=categoryinfo`, site statistics, file usage, template usage, the report pages and the farm-wide views show rows whose target the principal may read and no others; counts are counts of those rows. `view.site_stats` counts public targets only, so a public number never moves when a private page is created. Search documents carry the visibility set and a query filters on it ([03](03-storage-caches-and-search.md) §11.5); the Postgres search fallback filters results after the scan with the read-time visibility set, and no `read_groups` column is added to `view.page`, `view.page_text` or `view.term` ([03](03-storage-caches-and-search.md) §11.6).

**A log event whose target is restricted is restricted with it.** The `protect/protect` event that creates a confidential restriction, and every `move`, `delete`, `patrol`, `upload`, `thread` and filter-hit event on the target afterwards, is shown to principals that may read the target and absent for others. MediaWiki's protection log announces every protection by title; here that would announce every secret. An event whose target is a set names the set and its members, and is readable by those who may read the set. `Special:ProtectedPages` lists each restriction to principals that may read its target.

**A readable thing that refers to a restricted thing shows the reference, not the referent.** A wikilink in a readable page renders as a link to a missing page; an entity value whose entity is restricted shows the bare ID as a missing entity does; a sitelink from a public tenant to a private tenant's page is a URL that answers 404. The reference is the readable page's own content and is not hidden; what it points at is.

**What this does not hide**, stated so that nobody relies on it:

- **That a title is taken.** A `create` at a title that exists under a restriction the actor does not satisfy is refused with `ts-restricted-title`, which is also the refusal for a create-protected title (§4.8), so the two cases are indistinguishable. The actor learns that *something* reserves the title and nothing else. The alternative, two pages under one title, is not available; the leak is bounded by the `createpage` rate class and shows in the filter log.
- **Gaps.** Page, revision and log IDs are sequences per tenant, so a restricted edit leaves a gap, as an erasure does (§6.6). Gaps reveal counts, not content.
- **Timing and size.** Nothing is padded or delayed.
- **The operator.** Everything inside the deployment boundary ([20](20-web-tier.md)) is readable by whoever administers it, in the database and the backups. Instance acts are the only *UI* path, and they are recorded ([08](08-tenants-and-instances.md)); the rest is the operator's own controls and the hosting agreement, which the security model cannot enforce and does not pretend to.

### 5.4 Flow: derived output inherits the visibility of its inputs

*Sources: [0056](../decisions/0056-security-model.md) §6.*

Anything computed from more than one target can carry data from one into another: transclusion, `#invoke` and `mw.title.getContent`, `#property` and `mw.wikibase`, table rows, a thread listed on a board, a followed redirect, TemplateData and TemplateStyles, a search document, a feed row, a diff, a notification. Two rules close every such path.

**Rule 1: an output is readable only where every input is.** A search document, feed row, diff, notification or rendered page carries the union of its inputs' visibility sets, and is served to a principal that satisfies all of them. For outputs produced once per request (a diff, an entity's resolved view with its labels) this is evaluation at serving time. For outputs produced once and stored (a rendered page, a search document, a feed row, a notification) the producer runs as a derived-output principal (§1.3) and writes beside the output **a reference to the enclosures it was computed under**, the targets along the three axes of §4.4 whose `read` ACLs make up the set, never the groups themselves; serving resolves those enclosures against `view.acl` through L0 at that moment and filters on the result, so a restriction written or retired after the output was stored takes effect without rewriting the output, and a stored `read_groups` is never stale.

**Rule 2: an include may not widen.** A rendered page is produced once per visibility and served from cache, so a page *A* may include a page or entity *B* only if `groups(B) ⊆ groups(A)`: every reader of *A* could read *B* anyway. Otherwise the include behaves as if *B* did not exist, for every viewer: a red link from `{{:B}}`, `nil` from `getContent`, an empty row in a table, a missing entity from `#property`, no stylesheet from a restricted `sanitized-css` page. The subset test runs in the expander through the host, is cached in L0, and is re-evaluated when either side's restrictions change, which is the refresh trigger the render manifest already has ([11](11-rendering-templates-and-modules.md)). The common cases cost nothing: a public template includes nothing restricted, and a restricted page includes public templates freely, because ∅ is a subset of everything. A thread listed on a board is shown on the board only to principals that may read the thread's home; the listing is absent for others.

**Redirects.** The resolver follows a redirect only when the principal may read the target; otherwise it answers with the redirect page itself, whose text names the target as a red link. The redirect page is content the principal may read; the target is not.

**Cross-tenant reads are anonymous reads.** A tenant reading another as a provider, a page repository, a template repository or a file repository is the `universe` principal of the source: it sees the source's public form and nothing under any restriction. A provider may keep confidential entities, and they are simply not part of what it provides; the rule that reading a provider is all or nothing no longer holds. A private tenant provides nothing (§4.7). Reader lists ([08](08-tenants-and-instances.md)) still decide *which* tenants may read; this decides *what* they read.

**Notifications.** A mention, a `talk` message or a `watch` change on a target the addressed account may not read addresses nobody: no "you were mentioned on a page you cannot see". A notification already delivered whose target becomes restricted is removed from the inbox by the purge of [03](03-storage-caches-and-search.md) §9.6, as a hidden one is. A watch on a target that becomes restricted stays in `private` and produces nothing until the account may read it again.

**Exports.** Public dumps, bundles under the default export policies and the update stream carry the ∅ form only; a confidential target is as absent from them as a suppressed one ([02](02-graphs-rdf-and-query.md) §4.4). The operator's full bundle and a tenant move carry everything. Outbound federation publishes activities for targets with empty visibility only; an inbound reply to a restricted thread is evaluated on the thread's home with the `federated` surrogate's groups, which a tenant's ACL must name for it to pass.

**Edit filters** see the whole change, as they must (§7.2); a hit's row in the filter log is shown to principals that may read the target, and a private filter's details stay with `abusefilter-view-private`. **Jobs** keep the rule for rejects files: the actor and `ts-viewrejects`.

### 5.5 Writes

*Sources: [0056](../decisions/0056-security-model.md) §9.*

The write path ([03](03-storage-caches-and-search.md) §6.2) is authentication, grants, rate limit, ACLs, filters, append. Two rules make it consistent with reading:

- **A principal that may not read a target may not write to it,** whatever `edit` ACL it would satisfy. The refusal is the missing-target refusal (§5.3), not `protectedpage`.
- **A write may not reference what the writer may not read into a wider place.** This is Rule 2 of §5.4 on the way in: a transclusion, `#invoke`, redirect, set membership, board listing or thread attachment that would include a more restricted target into a less restricted one is accepted as text (an editor may type `{{:Board minutes}}`) and simply does not include, so the save never widens. The one place a write *moves* restricted content, forking or copying a page ([13](13-mirrored-pages.md)), copies only what the forking principal may read.

### 5.6 Instance acts on restricted content

*Sources: [0056](../decisions/0056-security-model.md) §12.*

An instance act on restricted content is evaluated or written as instance prerogatives are ([08](08-tenants-and-instances.md)), with its authority record in the instance `log`; takedowns and expunges reach every tenant's files whatever their visibility. The authority record names the target; its attestation is visible to `ts-viewoperator`, and the record itself is readable at the farm base by principals that may read the target on its tenant and, since an operator needs to see what they did, by `ts-viewoperator`. The instance-action list at the farm base filters as every list does (§5.3).

### 5.7 Redaction and caching

*Sources: [0012](../decisions/0012-api-requirements.md) §8.*

**Redaction** is per viewer. Administrators' views include hidden fields, read from the log; everyone else gets the redacted form. Erased bodies are never returned to anyone: they no longer exist. A target under a confidential `read` restriction the viewer does not satisfy is absent, not redacted (§5.3).

**Caching.** Anonymous responses carry `ETag` and may be cached publicly, for a minute at a shared proxy; erasure, and deletion or hiding by `read` ACL (§6.3), purge them. Authenticated responses are `private, no-cache`, with `ETag`, and never enter a shared cache. Routes under `/account` and `/auth` are `no-store`. The cache layers, their keys and what reaches them are in [03](03-storage-caches-and-search.md) §9 and §10.

### 5.8 The privacy test

*Sources: [0012](../decisions/0012-api-requirements.md) §8; [0056](../decisions/0056-security-model.md) §15.*

An automated test checks two things: no route returns data from a `private` graph, except `/account` and `/auth/pending` in the holder's own session; and no internal-only graph appears in any export. Later ADRs added cases:

- No cache entry or search document holds a hidden field or private data.
- Nothing behind a `read` ACL reaches a viewer outside its group, from any route, cache or index.
- No route returns an API key's hash, and only the issuing `POST` returns its secret.
- No route returns the URL, hash or metadata of a file version the viewer may not read.

The security model adds these cases, each run for an anonymous principal, an account outside the group, a subsidiary of an account inside the group whose grants lack `basic`, and another tenant reading through every repository kind:

1. A confidential page, entity, thread, statement and property, a set, a namespace and a whole private tenant: every route that can name the target answers exactly as for a missing target; byte-equal bodies and equal status codes.
2. No row for the target in recent changes, watchlist, logs, contributions, category members, backlinks, `allpages`, reports, site statistics or the activity stream; no search hit or suggestion; no notification delivered; no row in the filter log.
3. No cache entry with `{vis} = -` holds the target's data; no public dump, bundle under default export policy, update-stream event or outbound federation activity contains it.
4. A public page that transcludes, invokes, redirects to or tabulates the target renders it as missing for every viewer, including a member of the group.
5. A `create` at the target's title fails with `ts-restricted-title` and nothing more.
6. `instance check` fails on: a public-role grant on `private`, a superuser role in the server's configuration, a store on a public address without credentials, an unregistered `Host` answered with anything but 421, an empty trust list behind a proxy, a public admin listener, a world-readable key file. These are the misconfigurations of the deployment boundary ([20](20-web-tier.md)).
7. Making a public tenant private, and retiring that, purges and restores its public forms within the TTL ceiling, and no public cache or index entry survives the first.
8. An entry cached under the empty visibility set before a `namespace` restriction is written is not served after it: the entry's `-{epoch}` key no longer matches once the restriction bumps the tenant's epoch (§5.1), and the test asks for the page, its `action=render`, its search hit and its feed row through every cache layer and the web tier.

Where the site is served by a web tier ([20](20-web-tier.md)), every case is also run through it: a confidential target's page, and its `action=render`, are byte-equal to a missing target's, and case 7 covers the web tier's response cache. The test is backed by database grants: no query outside the holder-only services can read `private` ([03](03-storage-caches-and-search.md) §1.2). This makes the Public and Private labels in the UI ([19](19-site-ui.md)) enforceable.

## 6. Protection, deletion, hiding, patrolling and erasure visibility

*Sources: [0023](../decisions/0023-moderation.md) §1, §4, §5, §6, §7; [0016](../decisions/0016-permissions-and-access-control.md) §5, §6.*

### 6.1 The actions

*Sources: [0023](../decisions/0023-moderation.md) §1.*

Every administrative action here except patrolling is an ACL record (§4.1). Undoing the action retires the record.

| Action | Target | Permission restricted | Restricted to | MediaWiki equivalent |
|---|---|---|---|---|
| **Protect** | A page, thread, talk page, board, entity, namespace or set | `edit`, `move`, or both; `createpage` for a title that does not exist yet (§4.8); `read`, as a **confidential** restriction that keeps the target live for its group and absent for everyone else (§5.2) | `autoconfirmed` (semi-protection) or `sysop` (full), or any group; for `read`, a group the actor belongs to | Page protection; `protected_titles`; no MediaWiki equivalent for `read` |
| **Delete** | A page, thread or local entity | `read` | The deletion group, `sysop` by default | Page deletion; the `archive` table |
| **Hide** (revision deletion) | One record, any subset of its parts | `read`, on the named parts | `sysop` | RevisionDelete bits 1, 2 and 4 |
| **Suppress** | As Hide, or a whole page or entity | `read` | `suppress` | RevisionDelete bit 8; suppression |
| **Hide a username** | An actor | `read` | `suppress` | `hideuser` on a block |

**Nothing here touches the log.** Hidden and deleted content stays in its records, keeps its IDs, its place in the Merkle tree and its inclusion proofs ([01](01-log-and-records.md)). Removing content from the log is `erase`, and an `erase` may follow a hide when hiding is not enough.

**Foreign entities are not deleted here.** What upstream holds is upstream's to delete. A local assertion *about* a foreign entity is a local record and can be hidden like any other; the entity `WDQ42` itself cannot be. Keyed entities ([04](04-entities-and-identifiers.md) §3.3) likewise: a Domain exists because its key is valid, so it has no deletion; its local records can be hidden.

**Files.** For files, removal has two axes: who acts (a tenant, or the instance operator across every tenant) and how final it is. Tenants **delete**, **hide** (§6.3) and **erase**; the instance **reclaims** versions that have stayed deleted for a retention period; operators **take down** and **reinstate**, and **expunge**, which erases every reference to a hash and destroys its bytes. The file paths are in [12](12-files-and-media.md).

### 6.2 What deletion does

*Sources: [0023](../decisions/0023-moderation.md) §4.*

**A deleted page or thread** is a page with a `read` ACL naming the deletion group. There are no `delete` and `undelete` operations on page and thread records; the log events (`delete/delete`, `delete/restore`) come from the ACL, and the effect is:

- the page leaves the title index, so its title can be taken by a new page (`view.page.deleted` is derived from the ACL; the partial unique index of [03](03-storage-caches-and-search.md) §4.5 already excludes deleted rows);
- it leaves search, backlinks, `list=allpages`, feeds and the resolved metadata graph for everyone outside the group;
- `action=parse`, `prop=revisions` and the page itself answer as MediaWiki does: the page is reported missing, with the deletion log entry shown in its place, and members of the group see the content behind a banner;
- a deleted thread leaves its talk page's listing and history; a deleted talk page or board takes with it the threads whose home it is, by enclosure. A thread only listed there is untouched: the listing goes dormant, hidden outside the deletion group, and returns on undelete.

**Undeleting into a taken title.** If a new page has taken the title meanwhile, the two have different page IDs and there is nothing to merge. The undelete is refused with the error code `ts-title-taken`, unless the request supplies a new title, in which case a `move` record follows the retirement in the same request. A reservation (§4.8) takes a title as a new page does.

**Several deleted pages under one title.** Each deleted page keeps its own ID, so a title can have had several. An undelete names the one it restores by page ID (`pageid` on `action=undelete`); without it, a title that more than one deleted page had is refused with `ts-ambiguous-undelete`, which lists their IDs. Nothing is chosen for the caller.

**A deleted local entity** is a local entity with a `read` ACL naming the deletion group. While it holds:

- `wbgetentities` returns `missing`, `Special:EntityData/{id}` returns 404, and the entity leaves search, the term store, the resolved dump and every feed, for everyone outside the group. `oldid=N` permalinks to its revisions answer as MediaWiki does for a deleted revision.
- **It leaves its identity cluster** ([04](04-entities-and-identifiers.md) §4), exactly as a member tombstoned under `orphan` does: the next member becomes canonical, and the mirrored data that member holds stays public under its own ID. `same-as` and `different-from` records naming the deleted entity are kept but not applied; on undelete they apply again and any conflict they now cause is held as [04](04-entities-and-identifiers.md) §4.11 describes.
- **Local assertions about it are kept but hidden**, with their `entity_ref` rows. Statements on other entities that point at it are untouched and dangle, as they do on Wikidata after an item is deleted; the UI shows the value with its ID and no label.
- Its `page_id` stays with it. Undeletion restores every row and rejoins the cluster.

Only local entities can be deleted: the ID must be in the local form of [04](04-entities-and-identifiers.md) §2.1. `action=delete` on `Item:WDQ42` or `Domain:…` is refused with `ts-not-deletable`.

**Deletion is not a stale-copy problem.** Nothing is copied to an archive; the records stay where they are, and the projections that already exist read the ACL. MediaWiki's `archive` table, and its history-merge machinery, have no counterpart.

### 6.3 What hiding does

*Sources: [0023](../decisions/0023-moderation.md) §5, A16; [0056](../decisions/0056-security-model.md) A25.*

**Hiding one revision's parts.** A `record` ACL with `parts` sets the visibility bits that the API and `view.activity.visibility` carry:

| Part hidden | Effect outside the group | Action API flag |
|---|---|---|
| `content` (or `text` for a thread record) | The diff, the change set and the page or post text are withheld; the row stays in history and feeds with its time, size and IDs | `texthidden` |
| `comment` | The summary disappears from history, recent changes, diffs, the metadata graph and notifications | `commenthidden` |
| `attestation` | The actor is shown as hidden; the row stays attributed to nobody, and the revision carries `scatter:attributionHidden` in RDF | `userhidden` |

A suppressed part adds MediaWiki's `suppressed` flag for those who may see that much. This is the projection-level flag of the record format ([01](01-log-and-records.md) §2.3); the erasure of a part is its irreversible form and uses the same bits.

**Suppression is marked in the record.** Whether a restriction names the suppression group depends on `moderation.suppress_group`, a setting that projections do not resolve, so the ACL writer decides it and writes `suppress: true` into the record's attestation map; for a retirement, when the ACL it retires was a suppression. The suppressed bit and the `suppress/…` log events are projected from the mark, as patrolled is from `autopatrol` (§6.4). A record's own bits stay under any ACL: an adopted revision's withheld parts ([05](05-providers-and-ingest.md) §7.3) are restricted by the source, and retiring a `record` ACL on that revision leaves them.

**The same ACL on a `log` record** hides a log event's parts (`delete/event`): the comment part is the event's comment, the attestation its performer, the content its parameters and target.

**Hiding a username.** An `actor` ACL restricted to `suppress` is what sets the `hidden` status of an actor ([07](07-actors-and-accounts.md)). While it holds, the name is omitted from every projection and response for everyone outside the group, `sioc:name` is not emitted, and every attribution to the actor reads as hidden. It is set by the `hideuser` right, and MediaWiki's practice of setting it on a block is kept as a checkbox on `Special:Block` that writes the ACL beside the `block` record (§3.3). It is lifted the same way: an unblock, or a reblock without the checkbox, by a holder of `hideuser` retires the ACL (§4.3), while an administrator without `hideuser` lifts the block alone and the name stays hidden ([0056](../decisions/0056-security-model.md) A25). Hiding a name is reversible; removing a past name from the actor's history is `erase`, as for vanishing. **The threads on a hidden user's talk page are hidden with it**: the user encloses a thread whose home is their talk page, so the `actor` ACL is in the thread's chain and the thread reads as removed outside the group, as the talk page is absent; and a post's author shows no name wherever the name is hidden ([0023](../decisions/0023-moderation.md) A16).

**Redaction is per viewer** (§5.7), and hiding reaches every derived copy as erasure does: the projection that applies an ACL bumps the target's generation, purges its keys and tags, deletes or re-sends its search document, and removes delivered notifications whose content is now hidden ([03](03-storage-caches-and-search.md) §9.6). Shared caches hold only the public form, so a hidden field never has to be purged from a cache that a member of the group filled.

### 6.4 Patrolling

*Sources: [0023](../decisions/0023-moderation.md) §6; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

**A patrol mark is a log event**, `patrol/patrol`, in the tenant `log` partition, keyed by the patrolled record's coordinates and carrying its revision ID, as MediaWiki's patrol log does. The projection sets `patrolled` on the activity row. **Unpatrol** is `patrol/unpatrol`, same key, same right; the projection sets `patrolled` false, and the newest of the two events for a record wins.

**What is patrollable:** every activity row of kind `edit` in the tenant's `local` and `pages` partitions, which includes posts and thread operations, and page creations. Mirror syncs are not: upstream's edits are upstream's to review. A **job** is patrolled as one row; marking the job marks every change it made.

**Autopatrol is decided by the write path and written into the attestation part, not a log event.** A change made by an actor who holds `autopatrol` when the record is appended is patrolled from the start: the appending transaction evaluates the actor's effective permissions, which it has already resolved for the write itself, and writes the decision as an `autopatrol` field of the record's attestation map ([01](01-log-and-records.md) §2.4). The activity projection reads the field and sets `patrolled` from it; it never reconstructs the memberships in `actors` as of the record's `appended_at`, since a rebuild replays partitions in an order that need not interleave them as they were written and a vanish may have erased the memberships it would need ([0083](../decisions/0083-write-path-in-three-tiers.md) §6). No record is written per bot edit. MediaWiki writes a `patrol/autopatrol` entry for each; Triplespace does not, and `list=logevents&letype=patrol` returns only manual marks. An edit filter's `unpatrol` action withholds the field as a write lands (§7.4). Whether a viewer sees the unpatrolled mark at all depends on the `patrol` right, as in MediaWiki.

**Where the mark shows:** the `!` marker on rows in recent changes, related changes, the watchlist and histories for viewers with `patrol`; a **Mark as patrolled** action on the diff and on the row, and **Mark as unpatrolled** on a patrolled one; a `patrolled` filter on every feed, which is the `rcshow=patrolled|!patrolled` of `list=recentchanges` and `list=watchlist`. Marks are shown only within the recent-changes window (`site` setting `rc.max_age`), as MediaWiki's are.

**Rights.** `patrol` marks; `autopatrol` exempts. Defaults are in §2.2.

### 6.5 Visibility of the moderation records themselves

*Sources: [0023](../decisions/0023-moderation.md) §7.*

Moderation is public in the way MediaWiki's logs are, with one exception:

- **Protection, deletion, revision-hiding and patrol events are visible to everyone.** That a page was deleted, and by whom and why, is public; the content is not. This is the rule for erasures (§6.6), applied uniformly. A confidential restriction is the exception to this exception: its events are restricted with their target (§5.3).
- **A record whose group holds `viewsuppressed` is visible only to that group.** Its log event is `suppress/*`, and to everyone else the affected fields are simply hidden, with no event saying so. This is MediaWiki's private suppression log.
- **The `log` partition's export policy is internal**, so none of these records enters a public dump; a full dump made by the instance for itself carries them, and it is the instance's to protect.
- **The downstream stream** carries the public events, so a consumer learns that a revision's comment was hidden and can redact its copy, as this instance follows upstream ([01](01-log-and-records.md) §5.4). It never carries a `suppress/*` event or the content behind one.

### 6.6 Erasure visibility

*Sources: [0016](../decisions/0016-permissions-and-access-control.md) §6.*

That a record was erased is public: the gap row in a history and the `erased` flag in the API are shown to everyone, as MediaWiki shows that a revision was deleted. The **reason class** and the **authority reference** of an `erase` record ([01](01-log-and-records.md) §5.1) are shown only to holders of `ts-viewerasures`. The `erase/erase` log event follows the same rule: listed for everyone, with its parameters visible to `ts-viewerasures`. The reason-class vocabulary is `legal`, `privacy`, `upstream` and `operational`; an instance may add classes in `site` configuration. Erasures the instance makes in a tenant's partitions (an operator's expunge, scheduled reclamation of long-deleted file versions) follow the same rule. They are attributed to the instance operator, and their authority is a record in the instance `log`.

## 7. Edit filters

*Sources: [0030](../decisions/0030-edit-filters.md) §1, §2, §3, §4, §5, §8, §12; [0033](../decisions/0033-backend-stack.md) §9.4; [0042](../decisions/0042-template-expansion-and-parsoid.md) §17; [0051](../decisions/0051-page-redirects.md) §9; [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0060](../decisions/0060-scopes.md) §9; [0067](../decisions/0067-proposals.md) §8; [0056](../decisions/0056-security-model.md) §6.*

### 7.1 A filter is a rule with actions, evaluated before append

*Sources: [0030](../decisions/0030-edit-filters.md) §1; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

An **edit filter** is a named rule that is evaluated against every candidate write in the tenant's `local` and `pages` partitions, in the appending transaction, after the permission and ACL checks and before the append; filters are a step of tier 1 ([03](03-storage-caches-and-search.md) §6), since a filter is a function of the written record and the actor and nothing composes while it runs. It has:

- a **rule**: an expression in the language of §7.3 over the context of §7.2, which evaluates to true (the filter **matches**) or false;
- **actions** taken on a match (§7.4);
- **contexts** it applies to: `changeset`, `text`, or both;
- a **scope**: which of the tenant's own writes it sees, by default every interactive write, with exempt groups (§7.4) and a `jobs` flag, **`false` by default**, so that a bulk job's million records do not run every filter unless a filter opts in to seeing them;
- a **privacy** flag: a private filter's rule, notes and hit details are visible only to holders of `abusefilter-view-private` (§7.5);
- an **enabled** flag, a **name**, **notes** for other filter editors, and a **public message** shown to the editor on `warn` and `disallow`.

Filters run in order of ID. Every match is recorded (§7.5); the actions decide what else happens. A write that no filter disallows proceeds to append exactly as it otherwise would.

**What is never filtered:** mirror partitions, since upstream data is upstream's and a mirror is faithful by design; `config`, `actors` and `accounts` records, which are administrative actions with their own permissions; and moderation records (§6), for the same reason. A filter governs what editors and their bots write.

### 7.2 Three contexts and one actor

*Sources: [0030](../decisions/0030-edit-filters.md) §2; [0042](../decisions/0042-template-expansion-and-parsoid.md) §17; [0051](../decisions/0051-page-redirects.md) §9; [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0060](../decisions/0060-scopes.md) §9; [0067](../decisions/0067-proposals.md) §8; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

A rule sees a **context**: a typed set of variables derived from the candidate record and the actor. Variable names follow AbuseFilter's where AbuseFilter has one, so that a filter author can port a rule by hand and the API ([18](18-api.md)) can answer in MediaWiki's shape.

**The change-set context** (`changeset`), for writes to `local`:

| Variable | Meaning |
|---|---|
| `action` | `edit` or `create`; the change-set operations in detail under `ops` |
| `entity_id`, `entity_type`, `entity_new` | The subject, its type, whether this write creates it |
| `ops` | The operations of the change set ([05](05-providers-and-ingest.md)) and the identity operations ([04](04-entities-and-identifiers.md) §4.10) as a list: kind, property, and the snak or term it adds, removes or overrides |
| `properties_added`, `properties_removed` | Property IDs touched, as main snak, qualifier or reference, each with its role |
| `values_added`, `values_removed` | Data values by type, in **normalized form** ([04](04-entities-and-identifiers.md) §4.7, [06](06-statements-and-properties.md)): `external-id` and `string` as text, `wikibase-item` as canonical IDs, `url` as normalized URLs with a `host`, `quantity` and `time` as their parts |
| `labels_added`, `descriptions_added`, `aliases_added`, and `_removed` | Term text by language, so a rule can regex a description in `en` |
| `sitelinks_added`, `sitelinks_removed` | URLs and their hosts |
| `rank_changes` | Statement GUID, old rank, new rank |
| `statement_count`, `statement_delta`, `reference_delta` | Before and after |
| `summary` | The comment part |

A page's change sets (page statements, [10](10-pages-and-content-models.md)) are seen in the change-set context too, with the subject's kind (`entity` or `page`) and namespace exposed, so a filter can treat page metadata differently from item data. Proposals ([14](14-discussions.md)) are seen as `propose` and `submit` records with their content parts, so a tenant can require a reference on every proposed claim.

**The text context** (`text`), for writes to `pages`:

| Variable | Meaning |
|---|---|
| `action` | `edit`, `create`, `move`, `rename`, `post`, `reply`, `thread-create`, or `upstream-post` for a reply or section about to be sent to a repository as the person, which a `disallow` stops ([13](13-mirrored-pages.md)) |
| `upstream_repo` | For `upstream-post`, the repository it would be sent to |
| `page_namespace`, `page_title`, `page_id`, `content_model` | As MediaWiki names them |
| `old_redirect`, `new_redirect` | Whether the text before and after the edit begins with a redirect line, derived from the text, so a filter can catch a page turned into a redirect |
| `fork`, `fork_repo`, `fork_revid` | Whether the edit forks an inherited title, and from which repository and upstream revision |
| `old_text`, `new_text`, `added_lines`, `removed_lines`, `edit_delta` | The full texts and a line diff, as AbuseFilter gives them |
| `added_links`, `removed_links` | Resolved wiki links and mentions, from the expanded text, computed only when an enabled filter reads them, as AbuseFilter computes its variables lazily |
| `added_external_links`, `added_external_hosts` | External URLs and their hosts |
| `thread_id`, `thread_target`, `post_depth`, `in_reply_to` | For posts |
| `summary`, `new_title` | The comment part; the target of a move or rename |

A scope save ([15](15-structured-pages.md)) is seen as a page edit with the definition as text.

**The upload context** (`upload`), for file uploads: AbuseFilter's file variables and `file_sha256`; `action` is `upload` or `stashupload`.

**The actor context**, present in every context:

| Variable | Meaning |
|---|---|
| `user_name`, `user_id`, `user_kind` | The actor's current name, ID and kind; `federated` for a fediverse surrogate |
| `user_groups`, `user_rights` | As of now, including global groups (§3.7) |
| `user_age`, `user_editcount` | Seconds since the first actor record; local edits to date |
| `user_is_subsidiary`, `user_operator` | A rule can treat a bot by its operator's standing |
| `user_blocked_elsewhere` | Whether the operator or the farm account is blocked |
| `rate` | The actor's current counts in each rate-limit class, read from the counters only when an enabled filter's rule names it |
| `ip`, `ip_in_range(cidr)` | The request's address, available to the rule and never written anywhere but the abuse-handling store ([07](07-actors-and-accounts.md)) |
| `timestamp`, `job_id`, `api_key_label` | When; the job if any; the key label if the actor is a subsidiary |

**Rules see the change and the actor, and nothing else.** A rule cannot read the graph: "the value of P31 must be an instance of Q5" is a property constraint, a separate reporting mechanism as on Wikidata ([06](06-statements-and-properties.md)), which reports and never gates. This bounds the cost of a filtered write to the cost of building the context from the record in hand, and the bound is permanent: no `has_statement()` or other lookup will enter the rule language.

### 7.3 The rule language is CEL

*Sources: [0030](../decisions/0030-edit-filters.md) §3; [0033](../decisions/0033-backend-stack.md) §9.4.*

Rules are written in the **Common Expression Language** (CEL): non-Turing-complete, statically typed, evaluated in bounded time, with a Rust implementation and a syntax that reads much as AbuseFilter's does. The implementation is the `cel` crate, formerly published as `cel-interpreter` ([22](22-crates-and-stack.md)). Regular expressions, string functions, list comprehensions and the AbuseFilter conveniences filter authors expect (`contains_any`, `count`, `norm`, `rmwhitespace`, `ccnorm`) are provided as CEL functions. A rule that does not type-check is refused when the filter is saved, with the error; a rule is compiled once and cached until the filter changes.

AbuseFilter's own language is **not** reimplemented. Its Wikidata rules mostly work by matching `new_wikitext`, which has no analogue here, and its text-context rules port to CEL by hand in minutes because the variables keep their names. `Special:EditFilter/import` accepts an AbuseFilter export and produces a draft CEL rule with the variables mapped and the rest marked for the author.

```
// Disallow new accounts adding a description in English that is an external link
user_age < 86400 && !("autoconfirmed" in user_groups)
  && descriptions_added.exists(d, d.language == "en" && d.text.matches("https?://"))
```

```
// Tag any edit that adds a DOI whose prefix is not Crossref-registered-looking
values_added.exists(v, v.property == "doi" && !v.text.matches("^10\\.[0-9]{4,9}/"))
```

### 7.4 Actions

*Sources: [0030](../decisions/0030-edit-filters.md) §4.*

| Action | Effect | Requires |
|---|---|---|
| `log` | Record the hit (§7.5). Every match is logged; this action only says nothing else happens | — |
| `tag` | Add change tags to the record (§7.6) | — |
| `warn` | Refuse the write once with the public message; the same write resubmitted with the returned token within an hour proceeds, and the hit records that it was warned and resubmitted | — |
| `disallow` | Refuse the write with the public message; the hit records the attempted change | — |
| `throttle` | Count matches per (filter, actor or IP) in a `rl:filter:{id}:{key}` counter ([07](07-actors-and-accounts.md)); the filter's other actions fire only once the count exceeds the filter's threshold in its window | — |
| `unpatrol` | Strip autopatrol from this write, so it lands in the review queue whoever made it (§6.4) | — |
| `notify` | Deliver the hit to the inbox of every member of a named group ([16](16-logs-feeds-and-notifications.md)), as a new reason `filter` | — |
| `block` | Append a `block` record for the actor (§3.3) with the filter's duration and message | `abusefilter-modify-restricted` on the filter's author |
| `degroup` | Remove the actor from every group but `user` | `abusefilter-modify-restricted` |

**Exempt groups.** A filter may name groups whose members it does not apply to; `bot` is the usual one, and a filter that names none applies to everyone including administrators, as AbuseFilter does.

**Every enabled filter evaluates every write,** and every match is a hit, as AbuseFilter does; a `disallow` does not stop later filters. The write is refused once, naming the first disallowing filter's message, so hit counts and the test tool stay complete.

`warn`, `disallow`, `block` and `degroup` are **refusals**: the write is not appended, so no record exists for it. The Action API reports them as `abusefilter-warning` and `abusefilter-disallowed`, the REST API as `403` with the same codes, in every case naming the filter's public message and, for `warn`, the resubmission token.

### 7.5 Records: filters and hits

*Sources: [0030](../decisions/0030-edit-filters.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §1.*

**A filter is a record in the tenant `log` partition**, payload type `scatter:v0/filter`, keyed `filter:{id}` where the ID is minted in sequence. Its content part holds the rule, contexts, scope, actions, exempt groups, privacy and enabled flags, name and notes; its comment part the reason for the change; its attestation who changed it. A later record for the same key replaces the filter, so a filter's history is its records, and `Special:EditFilter/history/{id}` is a history page. Filters live in `log`, not `config`, for the reason moderation ACLs do (§4.1): `config` is publicly exported, and a **private filter** must not be. A private filter's records are behind a `read` ACL restricted to holders of `abusefilter-view-private`, written with the filter. Each change projects as `abusefilter/create` or `abusefilter/modify` in the local log.

**A hit is a record**, payload type `scatter:v0/filter-hit`, in the tenant `log` partition, keyed `filter:{id}` as well so that a filter's hits and its history are one key. Its content holds: the filter ID and the version matched; the actions taken; the context's non-personal variables; for a write that was appended, the record's coordinates and revision ID; for a refused write, the **attempted change set or text**, so that reviewers can see what was stopped. Its attestation is the actor. **Hits from a job's writes aggregate**: a filter that has opted in to job writes (§7.1) records one hit per (filter, job) rather than one per record, carrying a count and the coordinates of the first N records matched, as the job's rejects file already summarizes refusals; a `disallow` on a job write is a reject of that record and the job goes on ([05](05-providers-and-ingest.md) §3.6). IP is not in the hit: it is in the abuse-handling store ([07](07-actors-and-accounts.md)) under that store's retention, keyed by the hit's coordinates, and shown on the hit page only to holders of `ts-viewip` (§2.2). A hit of a private filter is visible only to `abusefilter-log-private`; every other hit is public, as AbuseFilter's log is. Hits are activity rows of kind `log` with `log_type = abusefilter`, `log_action = hit`, so the filter log is a feed ([16](16-logs-feeds-and-notifications.md)). Hits are erasable per part like any record; the attempted content of a refused write is the part a suppressor erases when it should never have been kept. A filter record may set **`hit_retention`**: a daily sweep erases (reason class `operational`) its hit records older than that whose actions were only `log` or `tag`, by the filter's key; hits that warned, refused, throttled, blocked or degrouped are never swept, and headers remain, as erasure always leaves them.

### 7.6 Change tags and the tag registry

*Sources: [0030](../decisions/0030-edit-filters.md) §5.*

**Change tags live in the attestation part** of a record, whose meaning widens from "who is responsible" to "who, and how": the actor, the job, and the tags ([01](01-log-and-records.md) §2.4). A tag is a short registered string; `tag` actions add the filter's tags, `job:{id}` and the OAuth consumer tag are tags of the same kind, and MediaWiki's `mw-undo` and `mw-manual-revert` are written by the undo path. Tags are in the erasable body, since a tag can be an accusation.

**The tag registry.** A user-defined tag is a tenant `config` record of kind `tag`: its name is the key, and the content holds a description, an `active` flag and the group whose members may apply it by hand with `action=tag`. The prefixes `job:`, `oauth:`, `filter:` and `mw-` are reserved for the paths that write them and cannot be registered. `managetags/*` events project from the records and `tag/update` from manual application; `list=tags` and `Special:Tags` read the registry; a filter's `tag` action names a registered, active tag. The right is `managechangetags` (`sysop`); applying a tag by hand needs `changetags`, held by `user`, subject to the tag's own group.

### 7.7 Farm-wide filters

*Sources: [0030](../decisions/0030-edit-filters.md) §8.*

The tenancy policy has a switch, `filters.global` (`none` or `inherited`; `none` in the `isolated` preset, `inherited` in `community` and `enterprise`). Under `inherited`, filters in the **instance `log`** partition with `scope = global` run on every tenant before the tenant's own filters, in ID order, and a tenant cannot exclude them, as with global groups (§3.7). A `block` or `degroup` a global filter appends to a tenant's `actors` is an instance prerogative, binding for its duration: a tenant unblock lifts only the tenant's own block. Their hits are recorded in the tenant where the write happened, with the filter's home noted, so each tenant's log shows what stopped its editors and the farm's filter editors see every hit through the all-tenants set ([08](08-tenants-and-instances.md)). Writing a global filter needs `abusefilter-modify` held through a global group.

### 7.8 Filters and visibility

*Sources: [0056](../decisions/0056-security-model.md) §6; [0030](../decisions/0030-edit-filters.md) §12.*

Edit filters see the whole change, as they must; a hit's row in the filter log is shown to principals that may read the target, and a private filter's details stay with `abusefilter-view-private`. MediaWiki's right names are kept for compatibility, though the UI says edit filter; the nine `abusefilter-*` rights and their defaults are in §2.2. The IP shown on a hit page needs `ts-viewip` (§2.2).

## 8. What each feature restricts

*Sources: [0019](../decisions/0019-discussions.md) §12; [0049](../decisions/0049-boards.md) §13; [0069](../decisions/0069-synchronized-talk-pages.md) §11; [0020](../decisions/0020-change-feeds.md) §7; [0021](../decisions/0021-notifications.md) §9; [0022](../decisions/0022-federation.md) §12; [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10; [0039](../decisions/0039-files-and-media.md) §21; [0047](../decisions/0047-special-pages.md) §12; [0051](../decisions/0051-page-redirects.md) §8; [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9; [0053](../decisions/0053-mirrored-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8; [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §8; [0060](../decisions/0060-scopes.md) §9; [0067](../decisions/0067-proposals.md) §8; [0075](../decisions/0075-mcp-server.md) §5.*

The defaults in this section are those of the catalogue (§2.2); what follows is what each feature's actions need and the rules that go with them.

### 8.1 Threads and boards

*Sources: [0019](../decisions/0019-discussions.md) §12; [0049](../decisions/0049-boards.md) §13; [0069](../decisions/0069-synchronized-talk-pages.md) §11.*

The single-permission actions (`edit` for `create`, `post` and one's own posts, `ts-editpost` for another actor's post, `move` for `rename` and `move`, `delete` and `undelete` for the thread's `read` ACL, `deleterevision` for hiding a post's text, comment or actor, `ts-erase` for erasing a post's text) are in §2.2. The compound ones:

| Action | Needs | Default groups |
|---|---|---|
| Create a board | `createpage` in 310 | `user`, `temp` |
| Edit a board's definition | `edit` on the board | `user`, `temp` |
| `create` with `also` | `edit` on the home and on each listed target | `user`, `temp` |
| `attach`, `detach` | `move` on the thread, and `edit` on the target | `autoconfirmed` |
| `pin`, `unpin` | `move` on the thread, and `edit` on the page | `autoconfirmed` |
| Reply upstream, start an upstream thread | `edit` on the local talk page; a grant for the repository; `talk.upstream_post` on | `user` |
| `follow` on a fork | `edit` on the fork | `user`, `temp` |

`edit` on the target means that a board protected to `sysop` decides for itself what is listed on it and what is removed. A tenant that wants fewer boards restricts `createpage` in namespace 310 with an ACL. The enclosure rule for threads is §4.4's: a talk page or board encloses the threads whose home it is. The settings that go with synchronized talk pages (`talk` on a `page-repo`, `talk.archive_window`, `talk.upstream_post`, `talk.new_thread_default`, `talk.upstream_audit_days`, `thread.max_pinned`, `fork.talk`) are in [23](23-configuration-and-registry.md).

### 8.2 Feeds, watchlists, preferences and notifications

*Sources: [0020](../decisions/0020-change-feeds.md) §7; [0021](../decisions/0021-notifications.md) §9.*

`viewmywatchlist` and `editmywatchlist` govern reading and changing one's own watch set, token and `seen` marks; `read` governs every public feed, including the stream. No permission reads another account's watch set; there is no `viewwatchlist`-of-others right and no route for one. Streams and Atom fetches are rate-limited in the `stream` and `atom` classes ([07](07-actors-and-accounts.md)).

MediaWiki's `editmyoptions` governs the preference matrix, addresses and handles; `viewmyprivateinfo` governs reading them. Both default to `user`. The inbox needs no permission beyond being its holder. The discovery endpoints of notifications need `read`.

### 8.3 Federation

*Sources: [0022](../decisions/0022-federation.md) §12.*

`ts-config` governs the `trust` mode per provider, the `federation.*` settings and the `federation-policy` lists; `editmyoptions` making one's own account an actor; the group `federated` holds remote actors' surrogates and no permissions; `protect` enables inbound replies on a talk namespace by ACL, which must name `federated` for a reply to pass (§5.4).

### 8.4 Subsidiaries and OAuth

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §11; [0025](../decisions/0025-oauth-server.md) §10.*

The subsidiary rights (`createaccount`, `ts-transferaccount`, `noratelimit`, `bot`) and the five `mwoauth*` rights are in §2.2; the OAuth names are MediaWiki's so a Wikimedia administrator recognises them. `userrights` covers approval of a pending subsidiary, and creating one in the consent page is `createaccount`; only a subsidiary created in an OAuth authorization is ever pending, and a hand-made one is a member of `user` from its first record ([07](07-actors-and-accounts.md) §4.3). An actor with status `pending` does not receive the implicit `user` membership (§3.4).

**Declined.** `mwoauthsuppress` and `mwoauthviewsuppressed` are not offered: consumer records are `config`, and hiding one is disabling it.

### 8.5 Files

*Sources: [0039](../decisions/0039-files-and-media.md) §21.*

The six upload rights are in §2.2. `delete`, `undelete`, `deleterevision`, `suppressrevision` and `ts-erase` apply to file pages and versions as to anything else. The three operator rights, `ts-takedown`, `ts-expunge` and `ts-viewtakedown`, are held only through the primary tenant or through a global group; held on any other tenant they grant nothing (§2.3). The removal paths themselves are in [12](12-files-and-media.md).

### 8.6 Special pages and Nuke

*Sources: [0047](../decisions/0047-special-pages.md) §12.*

`nuke`, MediaWiki's right from the Nuke extension, runs `Special:Nuke` ([21](21-special-pages.md)); each write it makes also needs its own right: `delete`, and `deleterevision` for hiding. `nuke` joins the `delete` grant in `grants.toml`, so a subsidiary or OAuth consumer granted deletion can nuke. Configuring report modes and widening needs `ts-config` at the farm base, an instance right. Export needs `read`. The forms need the rights their API modules already need. The `job` rate class has `user` and `bot` rows only; an administrator starts a Nuke or a job revert through a subsidiary they operate (§2.2, `ts-runjob`).

### 8.7 Redirects

*Sources: [0051](../decisions/0051-page-redirects.md) §8.*

`suppressredirect`, `delete-redirect`, `move-subpages`, `move-rootuserpages` and `move-categorypages` (§2.2) are MediaWiki's rights with MediaWiki's default groups, so bots that check them behave as they do elsewhere. The redirect mechanism is in [10](10-pages-and-content-models.md).

### 8.8 Page repositories, mirrored pages and forks

*Sources: [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §9; [0053](../decisions/0053-mirrored-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8.*

| Action | Needs | Default |
|---|---|---|
| Writing a `page-repo` record, `pages.repos` or `pages.share` | `ts-config`, as every `config` kind does | |
| Reading a foreign page | `read`; the repository's own content is public by definition | |
| Setting `page-alternates` on a page | `edit` on the page, as any page statement | |
| Writing a repository's mirror set, mode or events; running `pages index` | `ts-config` on the scope that holds the record; the sync job is an instance job | |
| Fork by saving, or `Special:Fork` | `createpage` and `edit` on the title; the title not create-protected (§4.8); `fork.allow` on | `user`, `temp` |
| The history, dependency and talk-page seeding | Nothing beyond the fork: it is part of what saving means here. **`import` is not needed**, which is the point: forking is an edit, not an administrator's import | |
| Copying files | `upload`, `reupload-shared` | `autoconfirmed`; `reupload-shared` as §8.5 has it |
| `fork.*` settings | `ts-config` | |

An instance-scope `page-repo` is shared machinery under `config.template`: provisioned under `defaults`, locked under `locks`, as file repositories are ([08](08-tenants-and-instances.md)). Fetches of foreign pages are server-initiated and are bounded per repository, not per reader; a reader who forces one, with `action=purge`, counts in the `parse` class, and live history fetches count in `upstream`. **`fork.allow`** (`site`, default `on` where `pages.repos` is set) lets a tenant run a pure mirror that nobody forks. The rate class **`fork`** counts forks started: 5 per hour for `user`, 100 for `bot`, because each one is a job that may write thousands of records and fetch thousands of revisions; the seeding itself runs under the repository's fetch budget, not the user's. The fork job, `ops.fork`, is attributed to the forking user and its records are attested to the forking user and the job, as an import's are to the importer ([13](13-mirrored-pages.md)).

### 8.9 Page statements, templates and styles

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §14; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §8.*

Asserting, changing and removing a page's statements needs `edit` on the page, subject to the page's ACLs; for a thread, the right that posting needs. Category mappings are tenant configuration and need the configuration right every `config` kind needs.

| Action | Needs |
|---|---|
| Editing a `sanitized-css` page in Template | `edit`, as any template; the sanitizer is the safeguard, which is TemplateStyles' design |
| Editing the site-styles page | `edit` on the page, under whatever ACL the tenant puts on it; the default tenancy template protects it to `sysop` |
| Setting `wikitext.site_styles` or `templatestyles.max_bytes` | `ts-config` |
| Changing a page's model to or from `sanitized-css` | `editcontentmodel` |

### 8.10 Scopes and proposals

*Sources: [0060](../decisions/0060-scopes.md) §9; [0067](../decisions/0067-proposals.md) §8.*

Creating a scope needs `createpage` in namespace 312; editing one, `edit`. A tenant that wants fewer scopes restricts the namespace with an ACL. There is no right to compute: a scope computes for everyone or no one, and a reader sees the members they may read.

Proposing needs `edit` on the subject's talk page (to open a thread there) and `read` on the subject; exporting needs `read`; pushing needs a grant of one's own. `proposals.max_items` (default 500) bounds a payload; a larger selection is split into several proposals by the form.

### 8.11 The MCP server

*Sources: [0075](../decisions/0075-mcp-server.md) §5.*

**Public tenants** serve the MCP endpoint anonymously, with the visibility of an anonymous reader (§5.1). **Other tenants** require a bearer credential: a subsidiary's API key or an OAuth token, with the `basic` grant, which every key and token carries ([07](07-actors-and-accounts.md) §5.3). Every tool reads as that principal, so restricted data never reaches an agent that could not read it through the API. Every tool call counts against the `read` rate class; `find`, `facets`, `run_query` and `sparql` also against `query`. Results are capped at `mcp.max_results` (default 200) per call, with a cursor; queries run under `query.timeout` ([02](02-graphs-rdf-and-query.md) §7).
