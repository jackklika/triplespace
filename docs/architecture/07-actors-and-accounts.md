# 07. Actors and accounts

This chapter describes who acts in Triplespace: what an actor is and how it is named, how a local user logs in, how accounts are linked and vanished, the subsidiary accounts that automated edits run under, their API keys and OAuth tokens, the rate limits every class of request is counted against, the instance's OAuth server, and the preferences and private state that belong to an account. It assumes the record header and erasure of [01](01-log-and-records.md), the metadata graph of [02](02-graphs-rdf-and-query.md), and the `view.actor` table, the `private` schema, the sessions and the rate-limit counters of [03](03-storage-caches-and-search.md). Tenants and the farm are in [08](08-tenants-and-instances.md); groups, blocks and the permission catalogue in [09](09-security-and-moderation.md); the account routes in [18](18-api.md); the account pages in [19](19-site-ui.md).

## 1. What an actor is

*Sources: [0007](../decisions/0007-actor-identity.md) §1, §2, §4, §5, §6, §8.*

### 1.1 An actor is an issuer and a subject

*Sources: [0007](../decisions/0007-actor-identity.md) §1; [0079](../decisions/0079-derived-issuer-codes.md) §1, §2, §4; [0088](../decisions/0088-external-identity-providers.md) §1.*

Actors are namespaced by **issuer**, as entities are namespaced by provider ([04](04-entities-and-identifiers.md)). The issuer is the authority that assigns the account. The subject is the ID it assigned.

**The issuer is not the provider.** One provider may involve several issuers, and some issuers provide no content:

| Issuer | Code | Actor model | Actors come from |
|---|---|---|---|
| Each tenant | Its issuer code, derived from its founding record ([08](08-tenants-and-instances.md) §1.2) and written by its slug in examples, such as `librarybase`; `local` names the current tenant's ([0018](../decisions/0018-tenants.md) §4) | Numeric | The tenant's users (§2) |
| Wikidata | `wikidatawiki` | Numeric | Wikidata revisions and logs, in the Wikidata mirror |
| Wikimedia central accounts | `wikimedia-central` | Numeric, never published | Wikimedia OAuth only (§2) |
| OpenAlex | `openalex` | Provider only | None. Changes are attributed to OpenAlex as a whole (§1.5). |
| Password on this instance (§2.2) | `password` | Numeric, the tenant's own user IDs | None. A login provider only |
| The farm, where the tenancy policy gives it identity | The farm code ([08](08-tenants-and-instances.md) §1.2) | Numeric | Farm accounts, which edit nothing ([0028](../decisions/0028-tenancy-policy.md) §2) |
| The instance as operator | `instance` | Provider only | Instance acts written into a tenant ([0040](../decisions/0040-instance-prerogatives.md) §2) |
| Surrogates on a tenant (§1.4) | `actor`; like `local`, never registered and read as the current tenant's | Numeric, minted by the tenant | IP edits, imported edits under an unregistered prefix, fediverse actors |

Another Wikibase or Miraheze wiki gets an issuer of its own, keyed by its wiki ID.

**The issuer registry is configuration.** Like the provider registry ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4), it is data passed in by the caller and recorded in the log ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §9). Each entry records:

- the issuer code;
- the actor model: `numeric` or `provider-only`;
- the actor IRI template (§1.2);
- the API used to resolve names and central IDs, where there is one;
- which providers' adapters attribute actors to it;
- whether it may serve as an identity provider for local login;
- for an issuer that serves login through OAuth, its `oauth` table: the authorization, token and profile endpoints, whether its subjects are CentralAuth or local user IDs, where a consumer is registered, and what a login button names it (§2.7).

**A registered issuer code is at most 25 characters.** Tenant issuer codes and the farm code are derived, always 26 characters of base32 ([08](08-tenants-and-instances.md) §1.2), so no registered code can equal one, and the registry refuses a longer code at build time.

**For Wikibase issuers, the subject is the numeric user ID.** A name is used only where no ID exists (§1.4).

**The `instance` issuer** has one actor, the operator: actor key `instance:{farm code}`, IRI `{farm base}/instance/operator` ([0046](../decisions/0046-primary-tenant.md) §7). It is the actor of every instance act written into a tenant; the person who carried the act out is recorded only on the act's authority record ([0040](../decisions/0040-instance-prerogatives.md) §2).

**An actor key** is the compact form `{issuer}:{id}`, such as `wikidatawiki:12345` or `local:42`. It is what the attestation part of a record carries ([01](01-log-and-records.md) §2.3); a record's header never names the actor responsible for it, and carries an actor key only as the key of an actor record, whose subject the actor is. [0006](../decisions/0006-log-integrity-and-erasure.md) §3 requires a header key to be an identifier, never content, and an actor key is an identifier that never contains a name. `view.activity`'s issuer index filters on the tenant's issuer code ([03](03-storage-caches-and-search.md) §4.5).

### 1.2 Actor IRIs

*Sources: [0007](../decisions/0007-actor-identity.md) §2.*

| Actor | IRI | Example |
|---|---|---|
| Local user | `{base}/user/{id}`, under the tenant's base | `{base}/user/42` |
| Farm account | `{farm base}/instance/user/{id}` ([0046](../decisions/0046-primary-tenant.md) §7) | |
| The instance as operator | `{farm base}/instance/operator` ([0046](../decisions/0046-primary-tenant.md) §7) | |
| User of a MediaWiki issuer | `{article path}Special:Redirect/user/{id}` | `https://www.wikidata.org/wiki/Special:Redirect/user/12345` |
| Actor with no stable ID (§1.4) | `{base}/actor/{n}`, a surrogate minted by the instance | `{base}/actor/7` |
| Provider as a whole (§1.5) | The agent IRI in the provider registry | The provider's Wikidata item, for example |
| Wikimedia central account | None | Central IDs are never published (§2.1) |

**The upstream IRI is canonical for foreign actors**, as it is for foreign entities ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §4). `Special:Redirect/user/{id}` is minted in the issuer's own URL space, and MediaWiki supports it on every wiki. It survives renames, and it dereferences to the account's current user page. An issuer whose wiki has no such page names another template in the registry.

**An IRI never contains a name or an IP address.** Both can change and both can be erased. The IRI must still be valid afterwards.

**Local user IRIs are instance data**, under the instance's base URI ([0001](../decisions/0001-revision-metadata-rdf.md) §5, [0005](../decisions/0005-crate-organization.md) §5).

### 1.3 Names are attributes, kept in actor records

*Sources: [0007](../decisions/0007-actor-identity.md) §4; [0024](../decisions/0024-subsidiary-accounts.md) §1, §2; [0025](../decisions/0025-oauth-server.md) §3.*

**Change sets and revisions carry actor keys, never names.** Each actor's name, kind and status live in **actor records**: log records whose header key is the actor key. The payload type is `scatter:v0/actor`, and the body holds:

- the actor's kind: `registered`, `temporary`, `bot`, `anonymous` (§1.4), `imported` (§1.4), `provider` (§1.5) or `federated` (§1.4, [0022](../decisions/0022-federation.md) §8);
- its current name, if it has one;
- its status: `active`, `renamed`, `vanished` or `hidden`, and, for a subsidiary account, `retired` ([0024](../decisions/0024-subsidiary-accounts.md) §2) and `pending` ([0025](../decisions/0025-oauth-server.md) §3);
- for surrogates only, the raw value the surrogate stands for (§1.4);
- for a `bot`, its **operator**: the actor key of the primary account that owns it (§4.1).

The `hidden` status is set by an `actor` ACL restricted to `suppress` ([0023](../decisions/0023-moderation.md) §5; [09](09-security-and-moderation.md)).

This puts every name and IP address in one place per actor, so each can be erased in one place.

**Local actors** are recorded in the `actors` graph (§1.6) with full history. A rename appends a new record.

**Foreign actors** are recorded in one `actors/{provider}` graph per provider, with history policy `latest`. Adapters split what upstream sends. Wikidata's XML dumps and API give a name and an ID for each revision. The ID goes into the revision, and the name goes into an actor record. When upstream renames or vanishes an account, the next sync appends an actor record with the new name. Compaction then removes the old name. The upstream `renameuser` event itself is kept in the provider log without names ([0011](../decisions/0011-logs.md) §6.2; [16](16-logs-feeds-and-notifications.md)).

**RDF carries only the current name.** In the metadata graph, an actor node gets `sioc:name` from its latest actor record, and no past names. The name is not emitted if the actor is hidden, as [0001](../decisions/0001-revision-metadata-rdf.md) §4 already requires.

Actor records are projected to `view.actor`, whose columns, including `operator` and the status values, are in [03](03-storage-caches-and-search.md) §4.5. Two of its columns are not from actor records: `created_at`, the time of the actor's first record, and `editcount`, maintained by the activity projection from the actor's local `edit` rows, erased ones included and job rows excluded; the implicit `autoconfirmed` group is computed from them ([09](09-security-and-moderation.md) §3.1).

### 1.4 Actors with no stable numeric ID

*Sources: [0007](../decisions/0007-actor-identity.md) §5.*

| Case | Treatment |
|---|---|
| **Upstream temporary accounts** | Temporary accounts are ordinary user rows with numeric IDs, so they get normal issuer IRIs with kind `temporary`. The IP behind a temporary account is never requested or stored. |
| **Upstream IP edits,** made before temporary accounts | The IP gets a surrogate, `actor:{n}` with the IRI `{base}/actor/{n}`, of kind `anonymous`. The IP is kept only in the surrogate's actor record. The same IP maps to the same surrogate through an index projected from actor records. The IP is shown in history views as upstream shows it, and is never projected to RDF. |
| **Imported edits** (`imported>Name`, or another interwiki prefix) | At import time, the adapter resolves the name to the source wiki's numeric ID if the source is a registered issuer, and records the actor under that issuer. Otherwise the actor gets a surrogate of kind `imported`, whose record holds the prefix and the name. |
| **Upstream revisions whose user is hidden** (`userhidden`) | The revision carries a `hidden` marker and no actor key. MediaWiki hides the user ID as well as the name, so there is nothing more to record. |
| **States with no revision metadata** | Observed states from JSON dumps carry no actor ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3). Their actor is known only after a backfill from the API or the XML history dumps. |
| **Fediverse actors** replying to a federated talk page | A surrogate `{base}/actor/{n}` of kind `federated`, whose actor record holds the remote actor IRI and its last-seen name; the IRI is content, never a key ([0022](../decisions/0022-federation.md) §8). |

**Surrogates are minted by the tenant, in sequence,** from `log."{tenant}.actor_surrogate"` ([03](03-storage-caches-and-search.md) §2.3). Their actor key is `actor:{n}`: like `local`, the code `actor` is never registered, reads as the current tenant's, and is reserved in the shared namespace of slugs and issuer codes; outside the tenant a surrogate is named by its IRI. Its actor record is in the tenant's `actors` partition, keyed by that key. Surrogates carry no information about what they stand for. If a surrogate's actor record is erased, the surrogate stays behind as an opaque anonymous actor, and its revisions stay grouped. An IP seen after that erasure gets a new surrogate.

### 1.5 Providers without individual actors

*Sources: [0007](../decisions/0007-actor-identity.md) §6; [0024](../decisions/0024-subsidiary-accounts.md) §1, §6.*

OpenAlex publishes no per-change attribution. Its changes are attributed to:

- the provider as a whole, an agent of type `prov:Organization` whose IRI comes from the provider registry;
- the import job that brought the change in ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3; [05](05-providers-and-ingest.md)), which is a `prov:Activity`.

Such a provider has one actor record, of kind `provider` (§1.3), in its `actors/{provider}` graph (§1.6), written when the provider is registered; its agent type is `prov:Organization`. Every attestation, OpenAlex's mirror records included, names an actor key.

**The job's own actor is separate.** A job records who ran it on this instance. That is a local actor, a bot account. It is not the upstream actor. A local bot account is a `prov:SoftwareAgent` with `prov:actedOnBehalfOf` pointing at its operator's local account. Every bot account is a **subsidiary** of a primary account (§4.1), the operator is an attribute of its actor record, and a job's actor is always such a subsidiary, so "the account that ran a job" ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3, [0011](../decisions/0011-logs.md) §6.3) is always a subsidiary, and its operator is always known (§4.6).

### 1.6 The actor graphs and private state

*Sources: [0007](../decisions/0007-actor-identity.md) §8.*

Three kinds of graph are in Triplespace's registry for actors; the first two fill the "destructible data" row of [0005](../decisions/0005-crate-organization.md) §4.1. Each tenant has its own `accounts` and `actors` partitions ([0018](../decisions/0018-tenants.md) §4); under farm identity, the farm's are instance partitions ([0028](../decisions/0028-tenancy-policy.md) §2; [08](08-tenants-and-instances.md)). The graph catalogue is in [02](02-graphs-rdf-and-query.md).

| Graph | Illustrative IRI | Kind | Written by | History | Integrity | Export |
|---|---|---|---|---|---|---|
| **Accounts** | `{base}/graph/accounts` | Source | Login and account management only | Full | `logged` | **Private** |
| **Actors** | `{base}/graph/actors` | Source | Account management, renames, links | Full | `logged` | Internal only |
| **Foreign actors**, one per provider | `{base}/graph/actors/wikidata` | Source | That provider's sync jobs | `latest` | `hashed` | Internal only |

**`private` is an export policy.** A private graph is never projected into any graph, never placed on a feed, and never included in an export bundle ([0006](../decisions/0006-log-integrity-and-erasure.md) §9), including one made for verification. Its integrity can be checked only by the instance itself.

The metadata graph ([0001](../decisions/0001-revision-metadata-rdf.md) §2) projects from the actor graphs: actor nodes, current names, kinds and links. It never reads the accounts graph.

**Private state** belongs to an account but is not a record at all: secrets, API keys (§5), the watch set ([0020](../decisions/0020-change-feeds.md) §3), and the inbox, contact details and notifier key ([0021](../decisions/0021-notifications.md) §3, §5). It lives in the `private` schema ([03](03-storage-caches-and-search.md) §1.2, §4.16) under the rules of the `private` export policy. §8.3 says which of it a person can carry to another instance.

## 2. Local users and delegated authentication

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0035](../decisions/0035-adopting-a-wikibase.md) §5; [0024](../decisions/0024-subsidiary-accounts.md) §4; [0025](../decisions/0025-oauth-server.md) §5; [0057](../decisions/0057-web-tier.md) §13; [0087](../decisions/0087-account-registration-and-challenges.md) §1–§7; [0088](../decisions/0088-external-identity-providers.md) §1, §2, §3, §4, §5, §6, §7, §8, §9; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1–§16.*

### 2.1 User IDs and bindings

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0079](../decisions/0079-derived-issuer-codes.md) §6; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2, §3, §4.*

**The instance mints its own user IDs.** They are sequential, start at 1 and are never reused. A local user's identity is `local:{id}`, whatever the user logged in with. On a tenant that adopts an existing Wikibase, the sequence starts past the source's highest user ID, and the source's accounts are written as `{code}:{id}` actor records, `{code}` being the tenant's issuer code ([08](08-tenants-and-instances.md) §1.2), under their own numbers, without bindings, reclaimable as [0018](../decisions/0018-tenants.md) §10 describes (§2.5).

**Logging in uses bindings, in two steps** ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §2–§4). A person authenticates to the instance as a **login** (§2.8), through the login's bindings, which map an identity-provider subject to it, for example `(wikimedia-central, 7654321) → login:17`; the login then reaches a tenant account through the account's own binding, `(login, 17) → local:42`. The rules are:

- A login may hold several bindings of providers, a password among them, and may add or remove them. A tenant account holds at most one binding, to a login, and a login reaches at most one account on each tenant.
- A provider subject is bound to at most one login.
- Tokens and other secrets are never written to the log. They live in an operational store.
- The `sub` claim is normalized. The legacy form `7654321` and the newer form `mw:CentralAuth:7654321` are the same subject.
- A login's bindings live in the instance's private `logins` partition, and a tenant account's binding to its login in that tenant's `accounts` partition. One login may be bound to accounts on several tenants; that makes them one person's accounts, not one actor ([0018](../decisions/0018-tenants.md) §4).

**Keying local users on OAuth's `sub` is rejected.** An instance could then never change or add identity providers. The central ID is also not what Wikidata attributes edits to.

**API keys are credentials of subsidiary accounts only** (§5.1). A primary account authenticates through its login and never holds a key.

**Farm identity.** Under a tenancy policy with farm identity, a person signs up to the farm once. Tenant accounts are created from the farm account, under its name, and publicly linked to it ([0028](../decisions/0028-tenancy-policy.md) §2; [08](08-tenants-and-instances.md)).

**Bindings are private.** They go to the `accounts` graph (§1.6) and the instance's `logins` graph, which are never projected, exported or placed on a feed. The instance may use a binding for its own decisions, such as requiring a Wikimedia account in good standing to edit. It never shows which provider a user logs in with.

**Logging in does not link identities.** A binding proves that the user controls a Wikimedia account. It does not mean the user consents to that account being linked to their local account in the graph. Linking is a separate act that only the user can perform (§3.1).

### 2.2 The built-in password issuer

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §3.*

One issuer is operated by the instance itself, `password`, registered in `issuers.toml` with `builtin = true`. Its subject is a login's ID, so a password is the binding `(password, 17) → login:17` ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §3), and the hash lives in `private.password`, keyed by login ([0013](../decisions/0013-postgres-storage.md) §4), never in the log; one password serves every tenant account bound to the login. It is an identity provider and nothing else: no actor is ever attributed to it, and it is a form on the auth host's `Special:UserLogin` (§2.8; [19](19-site-ui.md)). Whether the auth host offers it is the instance default of the `site` setting `login.password`, and whether a tenant admits a login authenticated by it is the tenant's value; `triplespace-cli instance create` turns it on and sets the first administrator's password on the first login ([0016](../decisions/0016-permissions-and-access-control.md) §3), and an instance that registers an external provider may turn it off or leave it as a fallback. This is the single-user fallback Scatterbase asks for, and it is how an instance with no external identity provider is administered. It reverses the rejection of `sub` as a key only to this extent: the instance consumes providers, and is one, for its own users, never for other sites. OpenID Connect is left open by [0025](../decisions/0025-oauth-server.md).

### 2.3 Temporary accounts

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0087](../decisions/0087-account-registration-and-challenges.md) §2.*

**Anonymous local editing uses temporary accounts.** If an instance allows editing without logging in, each such editor gets a local temporary account with a numeric ID. Local change sets never carry an IP address as their actor. IP addresses collected for abuse handling stay in an operational store and are never written to the log; seeing one there requires `ts-viewip` *(new)*, default `sysop` ([09](09-security-and-moderation.md)). Where a tenant lists the `tempaccount` trigger, the save that would create a temporary account needs a solved challenge (§2.6); the temporary account then holds the session, so the editor is asked once.

### 2.4 Sessions

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4; [0025](../decisions/0025-oauth-server.md) §5; [0057](../decisions/0057-web-tier.md) §13; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5, §9.*

A person's edits are attributed by session, a program's by credential (§5.4). Session state lives in Valkey (`s:{session}`) when a shared cache is configured and in `private` through `triplespace-accounts` otherwise ([0025](../decisions/0025-oauth-server.md) §8; [03](03-storage-caches-and-search.md) §9.2, §12.3). A session opened with an API key records the key ID, and one opened with an OAuth token records the token ID, so that revoking either ends every session it opened (§5.5, §7.6). Request handles, authorization codes and device codes of the OAuth flows follow sessions: without a shared cache they are kept in `private` through `triplespace-accounts`, not in process, so that any replica of an API pool can finish a flow another began (§7.4).

**The central session** (§2.8). On the auth host a session holds a login and how it was authenticated, not an account. A tenant session opened through a login code records the login and the central session it came from, so that logging out everywhere can end it: `s:{session}` carries both, a Valkey set `sc:{central session}` lists the tenant sessions each central session opened, and `private.session` has two columns for them. Changing or removing a sign-in method ends every other session of the login and the tenant sessions they opened.

### 2.5 Adopted accounts

*Sources: [0035](../decisions/0035-adopting-a-wikibase.md) §5; [0079](../decisions/0079-derived-issuer-codes.md) §6, §9; [0088](../decisions/0088-external-identity-providers.md) §8; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §14.*

**Source accounts become tenant accounts by number.** The tenant is the source's issuer ([0018](../decisions/0018-tenants.md) §4), so the source's user 42 is `{code}:42`, under the tenant's issuer code. Where the registry already knows the source wiki as an issuer (`librarybase` in `issuers.toml`), its entry gains `tenant`, the adopting tenant's issuer code, and `{registered code}:{n}` becomes an input form of `{tenant code}:{n}`: canonicalized wherever an actor key is accepted, never stored or output. A reader that meets `librarybase:42` in a mirror of the source wiki and the tenant's own key reads one account ([0079](../decisions/0079-derived-issuer-codes.md) §9). Adoption writes an actor record (§1.3) for every account in the source's user list: kind `registered`, the current name, status `active`, and no binding. A source account that was vanished is written with status `vanished` and no name; for a source account whose name is hidden, adoption writes the `actor` ACL restricted to `suppress` that sets `hidden` for any local account (§1.3), and no name. Source group memberships become membership records ([0016](../decisions/0016-permissions-and-access-control.md) §3; [09](09-security-and-moderation.md)), with one exception: a source bot account has no structural operator (§4.1), so it is adopted as an ordinary account without the `bot` group, keeps its history, and its operator, once reclaimed, creates a subsidiary in its place.

**Adopted accounts cannot log in until reclaimed.** The source has no binding digest, so reclaiming is the manual path of [0018](../decisions/0018-tenants.md) §10: a bureaucrat records a `reclaim/reclaim` log event, or, while the source wiki is still up, the source vouches by acting as an identity provider for a grace period. An account never reclaimed keeps its name and its history, exactly as after a move.

**Vouching** ([0088](../decisions/0088-external-identity-providers.md) §8). With the tenant setting `reclaim.vouching` naming an issuer that serves login (§2.7), a login with no account on the tenant whose central session was authenticated through that issuer (§2.8) asks the source who the person is, from the subject and username the login code carries: the subject itself where the issuer's `subject` is `local`, or, where it is `central`, the `userid` that `list=users&usprop=centralids` on the source's users API (the `users_api` of the registry entry whose `tenant` is this tenant) returns for the provider's username, when that user's `CentralAuth` ID is the subject. If `{code}:{n}` is an adopted account with no binding, one change set appends its binding to the login and a public `scatter:v0/reclaim` record with `method: vouched` in the tenant's `log` partition, projected as `reclaim/reclaim` ([16](16-logs-feeds-and-notifications.md) §2.3), and the person is logged in with `ts-reclaimed`. An account already held answers `ts-reclaim-taken`; a source that cannot be reached, `ts-reclaim-unavailable`, and nothing is created; a source that names no user leaves the login unbound. The grace period is the time the setting is on.

**The owner.** `triplespace-cli instance create` creates `local:1` as the sole member of `owner` ([0016](../decisions/0016-permissions-and-access-control.md) §3). On an adopting tenant, `instance create --adopt {source base} --owner {user_id}` creates the owner under the source's user ID instead, with the same password binding, sets the user-ID floor from the source, and writes `adopted_from` ([0035](../decisions/0035-adopting-a-wikibase.md) §1; [05](05-providers-and-ingest.md)); the adoption job then finds the owner's actor record already present and leaves it. "`local:1`" in 0016 §3 reads as "the owner account" on such a tenant. The owner must therefore be the source's account under its own user ID and name: the adoption's survey refuses the job, before writing anything, when a source account's name is held by an account under another ID or a source ID is held under another name, since account names are unique on a tenant ([0035](../decisions/0035-adopting-a-wikibase.md) A16).

### 2.6 Registration, challenges and approval

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0087](../decisions/0087-account-registration-and-challenges.md) §1, §2, §3, §5, §6, §7; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §6, §8.*

**`createaccount` decides whether an account may be made at all** ([09](09-security-and-moderation.md) §2.2): `universe` by default, and a private tenant, or any tenant that closes registration, takes it from `universe`; no setting repeats it. A person first creates a login on the auth host (§2.8), with a login name and a password or through an external issuer, and then an account on a tenant by choosing a public name there on the first visit ([19](19-site-ui.md) §2.3). `action=createaccount` serves both, at the auth base and on the tenant ([18](18-api.md) §2.3). How an account then comes to exist is three choices: whether a challenge stands in front of the form and whether the account is made at once or requested, each a setting that resolves through the instance's defaults and locks ([23](23-configuration-and-registry.md) §3.2), and which sign-in methods it admits, which is the issuer registry, `login.issuers` and `login.password` (§2.2, §2.7).

**A challenge is a proof of work** the instance asks of a client before an act that creates an identity or guesses at one. It does not test that the client is a person; it makes each account cost a few seconds of computation, which is nothing to a person and a great deal to a script that wants thousands. The acts are the triggers listed in `challenge.triggers`, named as ConfirmEdit names its own:

| Trigger | The act it guards |
|---|---|
| `createaccount` | Creating a login with a password, on the auth host (§2.8) |
| `tempaccount` | The save that would create a temporary account (§2.3) |
| `badlogin` | A `password` login, on the auth host or through `clientlogin` on a tenant, once the caller's address key (§6.3) has failed 3 times within 5 minutes or the named login 20 times within 10 minutes, counted across every host |

A login created through an external issuer is not challenged, since the issuer has decided who may hold an account there; nor is creating a tenant account from a login, which was challenged or vouched for when it was made; nor is a subsidiary's bot-password login, whose secret cannot be guessed. On the auth host, which is no tenant, the triggers, difficulty and throttle resolve as the instance defaults of their settings. A challenge is issued for one trigger, one tenant (on the auth host, the instance, whose farm code stands for the tenant's) and one session, and a solution is accepted once.

**The work.** A challenge is the string `1.{tenant code}.{trigger}.{bits}.{expires}.{salt}.{mac}`: the format version, the tenant's issuer code, the trigger, the difficulty, the expiry 10 minutes after issue in seconds since the epoch, 16 random bytes, and the first 16 bytes of HMAC-SHA256, base64url, over the rest, a zero byte and the session ID, under a key derived from the instance signing seed as the CSRF tokens' key is, with its own domain separator. A solution is a decimal `n` of at most 20 digits such that SHA-256 of `{challenge}:{n}` begins with at least `bits` zero bits. Checking is the MAC, the expiry, the tenant and trigger, the hash, and that the challenge is unspent; a challenge that passes is spent until it expires, as a Valkey key or a row of `private.challenge_spent` ([03](03-storage-caches-and-search.md) §5, §12.3). `bits` is `challenge.bits` (default 20) plus one for each account created from the caller's address key in the last 24 hours, read from the `createaccount` counter (§6.2), up to `challenge.max_bits` (default 24). The browser solves it in a Web Worker, with a solver written in JavaScript and held to the crate's test vectors, while the person fills in the form; without JavaScript the guarded act is refused with an explanation ([20](20-web-tier.md) §3.1).

**Exemptions.** `skipcaptcha` (`bot`, `sysop`) exempts its holder from every trigger, so an administrator can create an account for someone who cannot run the challenge. The instance's own acts are not challenged: `instance create`, adoption (§2.5), farm auto-creation ([08](08-tenants-and-instances.md) §3.6), and approving a request.

**Approval.** With `account.approval` on, registering files a **request** instead, on ConfirmAccount's model and with its names. The request is a row of `private.account_request`, never a record: the chosen name and its case-folded key, the credential the account will be bound to (a login, or under farm identity the farm subject), an optional statement of up to 1,000 characters (`account.request_statement`), the caller's address key, and, once decided, the decision, reviewer, time and reason. An account that is never approved never enters the log, and a pending request holds its name against the name-taken check. A holder of `confirmaccount` (`bureaucrat`) reviews at `Special:ConfirmAccounts`, seeing the address key only with `ts-viewip`; **approving** creates the account and its binding as one change set, logged as `newusers/create2` with the reviewer as performer ([16](16-logs-feeds-and-notifications.md) §2.3); **rejecting** records a reason in `private`. A login with the credential of a pending request answers `ts-account-pending`, and of a rejected one `ts-account-rejected` with the reason for 7 days, after which the row is deleted; an open request is deleted after `account.request_expiry` (30 days). No email is sent, since the address would be unverified. In approval mode an account created for another person is made at once only by a holder of `confirmaccount`; anyone else's is filed as a request.

### 2.7 External identity providers

*Sources: [0007](../decisions/0007-actor-identity.md) §3; [0088](../decisions/0088-external-identity-providers.md) §1, §2, §3, §4, §5, §6, §7, §9; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §3, §6, §10.*

**An external issuer serves login through OAuth 2.0.** Its registry entry carries an `oauth` table: the authorization, token and profile endpoints, `subject` (`central` for a CentralAuth ID, `local` for the wiki's own user ID) and where a consumer is registered (§1.1). The registry ships one, `wikimedia-central`; Miraheze's is deferred ([0088](../decisions/0088-external-identity-providers.md) Q4). The instance is a confidential client using the authorization code grant with PKCE (`S256`), registered with the provider for identity only. It exchanges the code, calls the profile endpoint once, takes `sub`, normalized from `mw:CentralAuth:{id}` or `mw:local:{id}` to the bare number, and `username`, and discards both tokens: no provider's token is ever stored, and every other field of the profile is ignored. Requests leave through the instance's egress with a 10-second timeout.

**Client credentials** are rows of `private.oauth_client`, one per issuer for the whole instance ([03](03-storage-caches-and-search.md) §4.16): a tenant has no consumer of its own, since a provider returns only to the URL its consumer was registered with and that URL is the auth host's (§2.8). The secret is sealed with XChaCha20-Poly1305 under the **instance sealing key**, a subkey of the signing seed (`ts-seal-v1`), with the table and issuer as associated data; `private.publication_credential` and `private.upstream_grant` are sealed the same way. `triplespace-cli oauth-client set`, `list`, `callback` and `remove` write and show them; no API route does, and none returns a secret. Rotating the signing seed means setting them again.

**The auth host offers an issuer** when it has `login = true` and an `oauth` table, a credential exists, and the instance default of `login.issuers` (default `*`) lists it. A tenant's own `login.issuers` and `login.password` say which methods it **admits** (§2.8). A save, a tenant's or an instance default, that would leave a tenant admitting nothing the auth host offers is refused with `ts-no-login-method`. An issuer no longer offered unbinds no one; the login's page shows its bindings as unavailable.

**A flow** is a ten-minute handle, in Valkey as `oauthc:{state hash}` or in `private.oauth_flow`: the tenant, the session it began in, the issuer, its purpose (`login`, `create` or `link`), the PKCE verifier, the name chosen for `create`, `returnto`, the door it came through, and the outcome. The `state` is 32 random bytes, of which only the hash is kept. The provider returns to one URL for the instance, the auth host's `/auth/{issuer}/callback` (§2.8; [18](18-api.md) §3.2), which is the URL the operator registers; it refuses a flow whose issuer, tenant or expiry does not match, and exchanges the code. The **site's door** is `POST /auth/{issuer}/start` with the session's login token (or CSRF token for `link`), reached without JavaScript; a page carrying an issuer's button widens its `form-action` to the origin of the issuer's authorization endpoint, since a browser applies `form-action` to the start route's redirect ([20](20-web-tier.md) §3.1); the callback applies the outcome to the session the flow began in, refuses any other, and redirects. The **Action API's door** is AuthManager: an issuer request `TriplespaceIssuerAuthenticationRequest:{issuer}` with a button field `tsissuer-{issuer}`, a `REDIRECT` to the provider, and a `*continue` call with the `state` the callback appended to the client's return URL, which must be on the origin the request was made to ([18](18-api.md) §2.3). The provider's code never leaves the API.

**Logging in.** A bound identity authenticates its login on the auth host, as a password does, and updates the binding's `last_used`; neither challenge trigger applies. An identity bound to no login is kept as the auth host's **pending identity** until the flow expires, and creating a login for it asks only for a login name (§2.8); a subject already bound to a login is refused with `ts-oauth-bound`, which says to log in instead. On a tenant, a login with no account there is answered, in order: by an account request whose credential is the login, `ts-account-pending` or `ts-account-rejected` (§2.6); by vouching (§2.5), where `reclaim.vouching` names the issuer that authenticated the login; and otherwise by keeping it as the session's **pending login**, answered `RESTART` (`authmanager-authn-no-local-user-link`) by the API and continued to `Special:CreateAccount` by the site. `action=createaccount&createpreservestate=1` with a name creates the account bound to the login, with no password and no challenge; the `createaccount` throttle counts it (§6.2). Under `account.approval` it files a request whose credential is the login.

**Sign-in methods** are a login's (§2.8). `private.binding (tenant, issuer, subject, actor_key, partition, offset, added, last_used)`, unique on `(tenant, issuer, subject)`, indexes bindings, written in the transaction that appends the binding record and pointing at it so that it can be erased: a login's external identities with the empty tenant and the login as holder, and each tenant account's binding to its login with the tenant and issuer `login`. A password's index is `private.password`, keyed by login. **Adding** one is the flow with purpose `link`, on the auth host's `Special:Account` or `action=linkaccount` at the auth base; a subject bound to another login is `ts-oauth-bound-elsewhere`, and one already bound to this login changes nothing. **Removing** one, `action=unlinkaccount` or **Remove**, erases the binding record with reason class `privacy` ([01](01-log-and-records.md)) and deletes the row; removing the last way to log in, counting a password, is `ts-last-binding`. Neither appears in any log, feed or export.

**Errors** are `ts-oauth-denied` (the person declined at the provider), `ts-oauth-state` (an unknown, expired or mismatched flow, or another session), `ts-oauth-failed` (the provider could not be reached or answered wrongly), `ts-oauth-returnurl`, `ts-oauth-bound`, `ts-oauth-bound-elsewhere`, `ts-last-binding` and `ts-no-login-method`, and for vouching `ts-reclaimed`, `ts-reclaim-taken` and `ts-reclaim-unavailable`.

### 2.8 Instance logins and the auth host

*Sources: [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1, §2, §3, §4, §5, §6, §7, §9, §11, §12, §13, §15, §16.*

**A login is how a person authenticates to the instance.** It has an instance-wide ID and a **login name**, and it is authenticated by its sign-in methods: a password (§2.2), external identities (§2.7), or both. A login is not an actor: nothing is attributed to it, it is in no public record, and only its holder sees that it exists. Its records are in the instance's private `logins` partition, one `scatter:v0/login` record keyed `login:{n}` holding the name, beside the binding records of its external identities; `private.login (id, name, name_key, created, last_used, partition, offset)` indexes it ([03](03-storage-caches-and-search.md) §4.16). Login names follow the username grammar of [19](19-site-ui.md) §2.3 and are unique on the instance, case-insensitively; they are private, may be changed, and leave no tombstone. The built-in issuer `login` has login IDs as its subjects and no IRI: a tenant account reaches its login through the binding `(login, n)` in its tenant's `accounts` partition, at most one per account and at most one account per tenant per login (§2.1). Subsidiaries have none (§5.1).

**The auth host.** An instance with more than one tenant names one with the deployment setting `server.auth_base` ([23](23-configuration-and-registry.md) §3.6), a host that is no tenant's base and not the farm base, which holds no wiki and is the only place a person authenticates. It serves the authentication modules of the Action API, the `/auth` routes, `Special:UserLogin`, `Special:CreateAccount`, `Special:Account` and `Special:UserLogout`, and the site's assets; every other path is 404 ([20](20-web-tier.md) §2.1). A second tenant cannot be created without it (`ts-auth-base-required`). On a single-tenant instance without one, the tenant's base is the auth base: the same code serves the tenant host's login pages, its session is both the central session and the tenant's, the handshake below completes in one request, and providers return to the tenant's base. Setting an auth base later moves the providers' callbacks, which `instance check` lists for the operator to register again.

**Logging in on a tenant** is [0088](../decisions/0088-external-identity-providers.md)'s flow with the issuer `login`, the tenant a client of the auth host. **Log in** on the tenant posts to its `/auth/login/start` with the session's login token; the flow, with its PKCE verifier, sends the browser to the auth host's `/auth/authorize` with the tenant's issuer code, `state`, the PKCE challenge and the purpose. If the auth host's own session holds a login authenticated by a method the tenant admits, it issues a **login code** at once and sends the browser back to the tenant's `/auth/login/callback`; otherwise it keeps the request for ten minutes and shows its `Special:UserLogin`, and issues the code once the person has authenticated. The callback's URL is computed from the tenant's registered base, never taken from the request. The tenant redeems the code in process, checking its tenant, expiry and verifier, and logs the session in. A login code is 32 random bytes, valid for 60 seconds and spent on first use, kept as `authc:{code hash}` in Valkey or a row of `private.auth_code`, holding the login, the method that authenticated it (with the subject and the provider's username for an external issuer), the tenant, the PKCE challenge, the central session and when it authenticated. **Nothing happens without a click**: no page view, frame or script on a tenant probes the auth host, and a reader who never presses **Log in** is never logged in. A client of the Action API that drives a browser has the same flow through `TriplespaceIssuerAuthenticationRequest:login`; one that does not uses `clientlogin` (below).

**What a tenant admits.** The auth host offers the methods the instance defaults of `login.password` and `login.issuers` allow (§2.7). A tenant's own values are what it admits: arriving from that tenant, the person is asked for one of those methods, and a code is issued only for a central session authenticated by one.

**On the first visit**, a login with no account on the tenant is answered as §2.7 says: an account request, then vouching, then the session's pending login, for which the person chooses a public name on `Special:CreateAccount`. The login name is offered as a button, and so is the username of the external identity that authenticated the login, each with the warning that the same name makes accounts easy to connect.

**The auth host's pages.** `Special:UserLogin` takes the login name and password and offers each external issuer; reached from a tenant, it names the tenant, offers only the methods the tenant admits, accepts that tenant's account name in place of the login name, and widens its `form-action` to the tenant's origin for the redirect back. `Special:CreateAccount` creates a login, with a password under the `createaccount` challenge, or from an external identity bound to no login. `Special:Account` is the login's own page: its name with **Rename…**, **Sign-in methods**, the tenant **Accounts** bound to it, seen by the holder alone, and **Sessions** with **Log out everywhere** ([19](19-site-ui.md) §2.3). `Special:UserLogout` ends the central session. The Action API at the auth base offers `query` (`meta=tokens|authmanagerinfo|userinfo|siteinfo`), `clientlogin`, `createaccount`, `linkaccount`, `unlinkaccount`, `changeauthenticationdata`, `removeauthenticationdata` and `logout`, acting on the login ([18](18-api.md) §2.3).

**What stays on tenant hosts.** `action=clientlogin` takes the tenant account's name and the password of its login, under the tenant's `badlogin` challenge, and opens a tenant session only. Bot passwords and OAuth tokens are credentials of a tenant's subsidiaries (§5, §7) and stay there. `action=createaccount` on a tenant creates an account from a pending login or through the `login` request, never from a password; it and the credential-management modules answer `ts-auth-host`, carrying the auth base, otherwise.

**Logging out.** A tenant's `Special:UserLogout` and `action=logout` end that tenant's session and offer **Log out everywhere**, a link to the auth host; there, `action=logout&everywhere=1` ends every central session of the login and every tenant session they opened (§2.4).

**Farm identity** is identity, not authentication. A farm account binds to a login as a tenant account does, `(login, n)` in `accounts/{farm}`, and a login has at most one; tenant accounts under shared names bind to the farm subject, reached from the login through the farm account ([08](08-tenants-and-instances.md) §3.5). Farm signup is on the auth host.

**The migration.** `triplespace-cli logins migrate`, an idempotent instance job the upgrade starts, turns every existing credential into logins: tenant accounts sharing an external identity form one login; a group with several passwords keeps the primary tenant's account's, or else the oldest account's; a login takes the name of the account whose password it kept, or of its oldest account, as `{name} ({tenant slug})` where that is taken; and one change set per account appends its `(login, n)` binding and erases its old password and external binding records with reason class `privacy`, as instance acts. `instance create` and `password set` make and set logins from then on.

**Reporting and errors.** `siprop=triplespace` reports `auth` and `auth_origin`, the auth base and its origin. New errors are `ts-auth-base-required`, `ts-auth-host` and `ts-login-taken`; an unknown, expired, spent or misdirected login code is `ts-oauth-state`.

## 3. Linking accounts and the right to vanish

*Sources: [0007](../decisions/0007-actor-identity.md) §4, §7; [0024](../decisions/0024-subsidiary-accounts.md) §2; [0027](../decisions/0027-preferences-and-portability.md) §4.*

### 3.1 Linking is opt-in

*Sources: [0007](../decisions/0007-actor-identity.md) §7.*

A user can **link** their local account to a foreign account they control, so that their attribution carries across sources. Nothing links accounts except the user's own request. The one link made without a separate request is a tenant account's link to the farm account it was created from, by the consent given once at farm signup ([0028](../decisions/0028-tenancy-policy.md) §2).

**Creating a link:**

1. The user asks to link an account through an identity provider they hold a binding with (§2.1). The instance makes them authenticate with that provider again during the request.
2. The instance resolves the account on the target issuer. For Wikidata, it takes the username from the identify response, queries `list=users&usprop=centralids` on Wikidata, and checks that the returned CentralAuth ID equals the binding's subject. The link target is the Wikidata `user_id` that query returns.
3. It appends a `link-account` record to the local `actors` graph, with the local actor's key as its header key.

**Rules:**

- Only the account holder can create a link. Administrators can remove a link, for example after an account is compromised, but cannot create one.
- A local user may link several foreign accounts, such as a main account and a bot account. Each one requires its own proof of control.
- A foreign account can be linked to at most one local user.
- A link is never inferred: not from a binding, not from a matching username, and not from any statement in the data.

**Removing a link erases it.** An `unlink` request appends an `erase` record ([0006](../decisions/0006-log-integrity-and-erasure.md) §7; [01](01-log-and-records.md)) with reason class `privacy`. That erases the `link-account` record. A strike would leave the link in history, and the history would disclose the same thing. After erasure, what remains is the trace 0006 allows: a record keyed to the local actor existed at time t.

**Projection.** Only a link that exists appears in RDF. Both accounts point at one holder node in the metadata graph:

```turtle
# graph <{base}/graph/metadata>
<{base}/user/42> a sioc:UserAccount ;
    sioc:name "Example" ;
    sioc:account_of <{base}/user/42#holder> .
<https://www.wikidata.org/wiki/Special:Redirect/user/12345> a sioc:UserAccount ;
    sioc:account_of <{base}/user/42#holder> .
<{base}/user/42#holder> a foaf:Agent .
```

The holder node exists only while a link does. An unlinked local account has no `sioc:account_of`.

**Actor links are not identity clusters.** [0004](../decisions/0004-identity-clusters-and-equivalence.md) clusters content entities and rewrites them to a canonical ID ([04](04-entities-and-identifiers.md)). Actor links do neither:

- An attribution always keeps the IRI of the account the change was made under.
- No `owl:sameAs` is emitted between accounts. Two accounts held by one person are still two accounts.
- An actor is never linked to a content entity. That includes an `OAA` author and a Wikidata item about a person. Wikidata's P4174 ("Wikimedia username") and similar properties are ordinary data and are never used to link actors.
- When an account has opted in as a fediverse actor ([0022](../decisions/0022-federation.md) §6; [17](17-federation-and-publication.md)), its public account links are emitted as `rel="me"` links in both directions, so a Mastodon profile and a wiki account can verify each other.

The operator relation of a subsidiary (§4.1) is not an account link in this sense: an operator relation is asserted at creation, public, and about control.

### 3.2 The right to vanish

*Sources: [0007](../decisions/0007-actor-identity.md) §4; [0024](../decisions/0024-subsidiary-accounts.md) §2; [0027](../decisions/0027-preferences-and-portability.md) §4.*

**Right to vanish** is handled like this:

1. An `erase` record ([0006](../decisions/0006-log-integrity-and-erasure.md) §7) erases the local actor's records by key. That removes every past name and every link (§3.1).
2. A new actor record gives the placeholder name, with status `vanished`.
3. Revisions stay attributed to `{base}/user/{id}`, which never contained the name.

This matches Wikimedia's practice: a vanished user's edits stay attributed to the renamed account.

- A primary account with subsidiaries cannot vanish until each has been transferred or **deactivated** (§4.4); the placeholder record then replaces the operator's name wherever a retired subsidiary shows it, as it replaces the name in the vanished account's own history.
- Vanishing a farm account vanishes each tenant account linked to it first ([0028](../decisions/0028-tenancy-policy.md) §2).
- A consumer whose owner vanishes is disabled (§7.3).
- The account page suggests **Download my data** before a vanish (§8.4).

## 4. Subsidiary accounts

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §1, §2, §3, §6; [0025](../decisions/0025-oauth-server.md) §3, §4.*

### 4.1 A subsidiary is a local account with an operator

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §1.*

A **subsidiary account** is a local account of kind `bot` (§1.3) whose actor record carries one more attribute, the **operator**: the actor key of the local account that owns it. Everything else about it is an ordinary account: a sequential numeric ID (§2.1), an actor IRI `{base}/user/{id}`, a name that is an attribute, a user page it owns ([0008](../decisions/0008-namespaces-and-document-pages.md) §6; [10](10-pages-and-content-models.md)), contributions of its own ([0010](../decisions/0010-site-ui.md) §8), and memberships and blocks in the `actors` partition ([0016](../decisions/0016-permissions-and-access-control.md) §3; [09](09-security-and-moderation.md)).

| | Primary account | Subsidiary account |
|---|---|---|
| Kind | `registered` | `bot` |
| Authenticates by | An identity provider, through a binding (§2.1) | An API key (§5) or an OAuth token (§7), and nothing else |
| Bindings | One or more | None. A subsidiary cannot log in interactively |
| API keys | **None** | One or more |
| Operator | — | A primary account of the same tenant |
| May own subsidiaries | Yes | No. One level only |
| Created by | Its holder, at first login ([0010](../decisions/0010-site-ui.md) §10) | Its operator (§4.2), or an OAuth authorization on the operator's behalf (§7.4) |

**The operator is an attribute, not a link.** It is written in the subsidiary's actor record and projected to `view.actor.operator`; the `prov:actedOnBehalfOf` of §1.5 is emitted from it. It is not an account link in the sense of §3.1, which is opt-in and about one person's several accounts on several issuers: an operator relation is asserted at creation, public, and about control. Nor does it enter any identity cluster ([0004](../decisions/0004-identity-clusters-and-equivalence.md)); actors never do.

**The operator relation is public.** A subsidiary's user page identity line reads "Bot operated by Example" ([0010](../decisions/0010-site-ui.md) §2), its contributions header names the operator, and the operator's own contributions page lists their subsidiaries. This is what Wikimedia's bot policy asks operators to write on a user page by hand.

**Bot accounts are subsidiaries, and only subsidiaries.** A local account of kind `bot` always has an operator. The instance's own sync and ingest jobs run as subsidiaries of accounts on the tenant that is primary when the job is submitted ([0018](../decisions/0018-tenants.md) §4, [0046](../decisions/0046-primary-tenant.md) §5; [08](08-tenants-and-instances.md)), created by `triplespace-cli instance create` beside `local:1` and put in the `bot` group at creation, so that the instance's own syncs run flagged from the first ([0016](../decisions/0016-permissions-and-access-control.md) §3); `ts-runjob` is an instance right for such jobs ([0046](../decisions/0046-primary-tenant.md) §8). When the role is transferred, `triplespace-cli primary accept` creates sync subsidiaries on the new primary the same way, and jobs already running finish under theirs. Jobs run under subsidiaries only: `ts-runjob` defaults to `bot` everywhere, and an administrator runs a job through a subsidiary they operate, by creating or approving one.

A subsidiary belongs to the tenant its operator belongs to; there is no cross-tenant bot ([0024](../decisions/0024-subsidiary-accounts.md) §7; [08](08-tenants-and-instances.md)).

### 4.2 Creating a subsidiary

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §2; [0025](../decisions/0025-oauth-server.md) §3.*

A primary account holding `createaccount` (default `universe`; it governs self-registration and creating an account for another, as in MediaWiki, and a private tenant removes it from `universe`; [09](09-security-and-moderation.md)) creates a subsidiary from `Special:Account` ([19](19-site-ui.md)) or `POST /account/subsidiaries` ([18](18-api.md)). The request names the subsidiary; the instance appends its first actor record with kind `bot` and the creator as operator, mints its user ID, and projects `newusers/create2`, MediaWiki's existing action for an account created by another user, with the operator as performer and the subsidiary as target. The name follows the same rules as any account name ([0010](../decisions/0010-site-ui.md) §10); the form suggests `{Operator}Bot`, and an instance may require a pattern in `site` configuration (`subsidiaries.name_pattern`, default none) and cap the number per operator (`subsidiaries.max_per_account`, default 10). Temporary accounts cannot create subsidiaries; nor can subsidiaries. A subsidiary created this way is never pending: it is a member of `user` from its first record, whoever its operator is and however new their account (§4.3).

A subsidiary may also be created on the OAuth consent page (§7.4), under the same name rules, settings and permission, with status `pending` (§4.3). `pending` is an OAuth-only status.

### 4.3 Approval, and the pending status

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §2; [0025](../decisions/0025-oauth-server.md) §3.*

**Approval for bot activity is membership in `bot`.** A new subsidiary created by hand is a member of `user` like any registered account and nothing more. It edits under the operator's supervision at a non-bot rate (§6), its edits are not flagged, and they are patrolled like anyone's ([0023](../decisions/0023-moderation.md) §6). When the community has reviewed it, a bureaucrat adds it to the `bot` group with `userrights`, which is already a `membership` record ([0016](../decisions/0016-permissions-and-access-control.md) §3) projected as `rights/rights`. That is the bot flag: the `bot` right, `ts-runjob`, `autopatrol`, and the bot rate limits. Nothing in the software decides what "reviewed" means; a `Project:Bot requests` page or a thread is the community's, as on Wikimedia projects. An instance that wants every subsidiary flagged at creation puts `bot` in the default groups of new subsidiaries in `site` configuration.

**`pending` is an actor status** (§1.3), and only an OAuth authorization ever sets it: a subsidiary created in one is `pending` until approved, and a hand-made subsidiary never is. A pending subsidiary exists, owns its user page, shows "Bot operated by Example (pending approval)" on it, and can read as `*` reads, but its **implicit membership in `user` is withheld**: its effective permissions are those of `*` and nothing more, whether the request comes with a token or with a key its operator issued. **Approval is `userrights`**: the first explicit `membership` record ends the pending status. The natural groups are `user`, meaning approved to act at a non-bot rate and patrolled like anyone, and `bot`, meaning approved and flagged, which is the approval above in one step. A subsidiary is **approved** when it holds an explicit membership in any group; `user` may be granted explicitly for this purpose, and only for this purpose. A subsidiary created by hand is a member of `user` implicitly and is not pending; it becomes eligible for the consent page's picker (§7.4) when a bureaucrat has approved it in the same way, so the approval requirement cannot be sidestepped by creating the account first and binding the tool to it second.

**While pending**, a write made with the subsidiary's token or key is refused with `oauth-pending` (Action API) or HTTP 403 with that code, and the response names `Special:PendingSubsidiaries`. Reads work. The consumer receives its tokens at authorization regardless, so a tool can complete its login and show the person that approval is awaited, rather than failing in the middle of the flow. `Special:PendingSubsidiaries` lists pending subsidiaries with operator, consumer and date for holders of `userrights`. A bureaucrat who declines **retires** the subsidiary (§4.4), which also revokes its tokens (§7.6); the operator may retire it too. Pending subsidiaries unapproved after `subsidiaries.pending_ttl` (default 90 days) are retired by `triplespace-accounts` on a schedule, with the retirement record's comment saying so.

**A tenant may relax this.** The `site` setting **`subsidiaries.oauth_requires_approval`** (default `true`) makes an OAuth-created subsidiary pending; with `false`, it is created as a hand-made one is, a member of `user`, active at once, and the picker offers every unretired subsidiary. Per consumer, a tenant may set `auto_approve` in its `consumer-policy` (§7.3), which has the same effect for that consumer only, so a tenant can wave through a reference-fixing gadget and still gate QuickStatements. Neither setting touches the bot flag: `bot` is a bureaucrat's decision in every case.

### 4.4 Transfer and retirement

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §2; [0025](../decisions/0025-oauth-server.md) §5.*

**Transfer of ownership** is a new actor record for the subsidiary naming the new operator, written by a bureaucrat holding `ts-transferaccount` ([09](09-security-and-moderation.md)) with the reason in the comment part. It projects as a new log event, `bot/transfer`, whose parameters are the old and new operator keys. History is untouched: every past edit stays attributed to the subsidiary, and the metadata graph emits `prov:actedOnBehalfOf` for the operator as of each revision's time, since actor records are ordered. The new operator must be a primary account of the same tenant and must have accepted, so the request is made by the receiving account and confirmed by the bureaucrat, or the reverse; either order writes one record. A transfer revokes the subsidiary's OAuth tokens, since the new operator did not consent (§7.6).

**Deactivation.** A subsidiary is **deactivated** by revoking every key and writing a new actor record with status `retired`. A retired subsidiary keeps its history and its operator, and can be reactivated by its operator by issuing a key. Retirement also revokes its tokens (§7.6). A primary account with subsidiaries cannot vanish until each has been transferred or deactivated (§3.2).

### 4.5 What a subsidiary inherits

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §3.*

**Permissions: nothing.** A subsidiary's effective permissions are the union over *its own* groups, minus its own blocks ([0016](../decisions/0016-permissions-and-access-control.md) §3; [09](09-security-and-moderation.md)). An administrator's subsidiary is not an administrator; a bot that needs `delete` or `protect` is given those groups by a bureaucrat like any account, which is how Wikimedia handles adminbots.

**Blocks: the operator's, downward.** An actor's effective permissions also subtract what the **operator's** active blocks remove, so blocking a person blocks their bots. This is the one addition to 0016 §3's rule that "a block is the only negative rule", and it adds no second kind of rule: the block record is the same, and evaluation reads one more actor's blocks. A block on a subsidiary does not reach its operator. `hideuser` ([0023](../decisions/0023-moderation.md) §5) is likewise not inherited in either direction.

**Ownership rules** ([0016](../decisions/0016-permissions-and-access-control.md) §2) gain two: the operator may edit the subsidiary's user pages, including its `json` and `yaml` subpages, and may issue and revoke its keys, retire and reactivate it, and rename it. Nobody else may, short of `renameuser`, `userrights` and `ts-transferaccount`.

### 4.6 Jobs, attribution and notifications

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §6; [0025](../decisions/0025-oauth-server.md) §4.*

A job's **actor** is the subsidiary that submitted it; its **operator** is read from the actor record, and `requested_by` ([0012](../decisions/0012-api-requirements.md) §3, [0011](../decisions/0011-logs.md) §6.3) is kept only for the case where an administrator runs a job on someone else's behalf. The activity row's `operator` field ([0012](../decisions/0012-api-requirements.md) §3; [16](16-logs-feeds-and-notifications.md)) is filled for every row whose actor is a subsidiary, not only for jobs. In the metadata graph the subsidiary is a `prov:SoftwareAgent` with `prov:actedOnBehalfOf` its operator as of the revision's time (§1.5). Every record written with an OAuth token is attributed the same way, with the consumer tag beside it (§7.5).

[0021](../decisions/0021-notifications.md) §2's routing stands: a mention of a subsidiary and the `job` reason for its jobs reach the operator's inbox, and a subsidiary has no inbox of its own. A `rights` notification for a subsidiary, its approval or a block, also goes to the operator. A `talk` message on the subsidiary's user talk page reaches the operator, since that is who can answer it.

## 5. API keys and signing keys

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4; [0025](../decisions/0025-oauth-server.md) §3, §4; [0075](../decisions/0075-mcp-server.md) §5.*

### 5.1 A key is a credential of a subsidiary

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4; [0007](../decisions/0007-actor-identity.md) §3.*

**A key is a credential of a subsidiary and of nothing else.** A primary account cannot hold one, so revoking a key never touches a person's login, and every automated edit is attributable to a named agent with a named operator. Pywikibot logs in as a subsidiary, which is the acceptance test of [0008](../decisions/0008-namespaces-and-document-pages.md) §12.

**Keys are private state** (§1.6; [03](03-storage-caches-and-search.md) §4.16). `private.api_key` holds, per key: the subsidiary's actor key; a **key ID**; a **label** chosen by the operator ("toolforge", "laptop"); the **hash** of the secret; its **grants** (§5.3); optional **IP ranges**; created, expires and last-used times; and revoked-at. The secret itself is shown once at issue and stored only as a hash. Nothing about a key is ever a log record: a key ID is not content, but it is not provenance either, and the attestation of a record carries the actor key alone ([0006](../decisions/0006-log-integrity-and-erasure.md) §3, [0015](../decisions/0015-record-format-and-partition-registry.md) §1; [01](01-log-and-records.md)).

### 5.2 Two ways to present a key

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4.*

Both forms authenticate the same subsidiary with the same grants:

| Form | Where | Why |
|---|---|---|
| `Authorization: Bearer {key ID}.{secret}` | Every Action API and REST request | The native form. Stateless; no session |
| `action=login` with `lgname={subsidiary name}@{label}` and `lgpassword={secret}`, then the session cookie | Action API | MediaWiki's **bot password** shape ([mediawiki-compat.md](../api/mediawiki-compat.md) §5.1), so Pywikibot and every tool that speaks it work unchanged. The label selects the key; the secret is checked against its hash; the session records the key ID |

`action=clientlogin` and the OAuth redirect flow ([0012](../decisions/0012-api-requirements.md) §4) are not offered to subsidiaries, and `action=login` is not offered to primary accounts; each form of authentication belongs to one kind of account. Tokens issued by the instance's own OAuth server (§7) are, like keys, credentials of a subsidiary and never of a primary account.

### 5.3 Grants

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4; [0025](../decisions/0025-oauth-server.md) §3; [0075](../decisions/0075-mcp-server.md) §5.*

**Grants** restrict what a key may do below what the subsidiary may do. The effective permissions of a request are the subsidiary's permissions (§4.5) **intersected with** the permissions its key's grants cover; the same rule applies to the grants of an OAuth token (§7.4). Grants use MediaWiki's names where MediaWiki has them, so a Pywikibot user recognises them:

| Grant | Covers |
|---|---|
| `basic` | `read`, and every read route. Always included |
| `highvolume` | Bot rate limits (§6) and the `bot` flag, if the subsidiary holds `bot` |
| `editpage` | `edit` on document pages, threads and posts |
| `createeditmovepage` | `editpage` plus `createpage` and `move`; `property-create`, where Wikibase puts it |
| `editentity` *(Triplespace)* | `edit`, `item-term`, `property-term`, `item-redirect`, `item-merge` and `property-create` on entities; `ts-link` and `ts-linkproperty` |
| `editprotected` | Edits admitted by protection ACLs the subsidiary's groups satisfy ([0023](../decisions/0023-moderation.md) §1) |
| `patrol` | `patrol` |
| `delete`, `protect`, `blockusers` | The administrative permissions of those names |
| `viewdeleted` | `deletedhistory`, `deletedtext` |
| `ts-jobs` *(Triplespace)* | `ts-runjob`, `ts-revertjob`, `ts-viewrejects`, `ts-retain`, `ts-convert` |

A grant covers a permission only if the subsidiary holds it; a grant is never a way to gain one. The set is registry data (`docs/registry/grants.toml`, [0015](../decisions/0015-record-format-and-partition-registry.md) §5; [23](23-configuration-and-registry.md)), so an instance may add a grant for a permission it adds.

The MCP server of a tenant that is not public requires a bearer credential, a subsidiary's API key or an OAuth token, with the `basic` grant, which every key and token carries; every tool reads as that principal ([0075](../decisions/0075-mcp-server.md) §5; [18](18-api.md)).

### 5.4 Signing keys

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4.*

Beside its API keys, a subsidiary may hold one or more **signing keys**: Ed25519 keypairs the operator generates, whose public half is registered as a `scatter:v0/key` record in the `actors` partition ([0015](../decisions/0015-record-format-and-partition-registry.md) §1; [01](01-log-and-records.md)) and whose private half the instance never sees. The operator registers, rotates and revokes them from the subsidiary's keys section and `POST /account/subsidiaries/{name}/signing-keys`; registration and revocation project as `key/register` and `key/revoke` log events ([0011](../decisions/0011-logs.md) §6.1; [16](16-logs-feeds-and-notifications.md)), public like the operator relation. A write submitted with a `signature` (as a field of the change set or page operation, or an `X-Triplespace-Signature` header on a REST write) is verified against the actor's current key before the append, and the record's attestation part carries it. A signature never grants anything and is never required by default; `site` configuration may require one for the `bot` group (`signing.require_for_bot`), which is what an instance that wants third-party-verifiable bot attribution turns on. History rows and "About this edit" ([0010](../decisions/0010-site-ui.md) §5–6) show a *signed* mark with the key ID, and the record and proof routes ([0012](../decisions/0012-api-requirements.md) §5) return the signature so a third party can check it against the export bundle, which carries the `scatter:v0/key` records of every actor whose signatures appear in it ([01](01-log-and-records.md)). `triplespace-cli` and the reference SDK produce the canonical CBOR of the content and comment parts and sign it; Pywikibot does not, and works unsigned. Primary accounts hold no signing key, for the reason they hold no API key: a person's edits are attributed by session, a program's by credential.

### 5.5 Revocation, expiry and IP ranges

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §4.*

**Revocation is immediate.** Revoking a key marks it revoked and invalidates every session opened with it, which is why sessions record the key ID (§2.4). **Expiry** is optional; the account page warns before it. **IP ranges**, when set, are checked on every request, as bot-password restrictions are. **Last used** is written to `private` at most once a minute per key, as [0012](../decisions/0012-api-requirements.md) §5 already treats binding use.

## 6. Rate limits

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §5; [0025](../decisions/0025-oauth-server.md) §6; [0039](../decisions/0039-files-and-media.md) §15; [0042](../decisions/0042-template-expansion-and-parsoid.md) §16; [0053](../decisions/0053-mirrored-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8; [0067](../decisions/0067-proposals.md) §8; [0075](../decisions/0075-mcp-server.md) §5; [0087](../decisions/0087-account-registration-and-challenges.md) §4.*

### 6.1 Site policy, by action class and group

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §5; [0025](../decisions/0025-oauth-server.md) §6.*

**Rate limits are site policy, by action class and group.** They are `site` configuration ([0015](../decisions/0015-record-format-and-partition-registry.md) §3; [23](23-configuration-and-registry.md)) in the shape of MediaWiki's `$wgRateLimits`: for each **action class**, for each group, a count per window. The defaults are starting values, as [0014](../decisions/0014-caches-and-search.md)'s TTLs are. `universe` and `temp` get limits below `user`'s; the `newbie` distinction MediaWiki draws is the `autoconfirmed` group here, which may carry its own row. **The most permissive limit among an actor's groups applies**, as in MediaWiki, and the `noratelimit` right ([09](09-security-and-moderation.md)) exempts an actor from every class but `job`. **Which limit a bot gets is exactly what the `bot` group's row says**, so "how fast bots may edit versus non-bots" is one table an instance edits with `ts-config`, and an unapproved subsidiary edits at `user`'s rate.

A request made with an OAuth token is limited as the **subsidiary** is limited: its groups pick the row of the table, so an approved-but-unflagged tool account edits at `user`'s rate and a community that has flagged one as `bot` gets bot rates for it. There is no per-consumer limit: a consumer is code, and what is limited is the account it acts as.

### 6.2 The class catalogue

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §5; [0025](../decisions/0025-oauth-server.md) §6; [0027](../decisions/0027-preferences-and-portability.md) §3; [0039](../decisions/0039-files-and-media.md) §15; [0042](../decisions/0042-template-expansion-and-parsoid.md) §16; [0053](../decisions/0053-mirrored-pages.md) §11; [0054](../decisions/0054-forking-a-mirrored-page.md) §8; [0075](../decisions/0075-mcp-server.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §1; [0087](../decisions/0087-account-registration-and-challenges.md) §4.*

| Class | Counts | Default (`user` / `bot`) |
|---|---|---|
| `edit` | Local change sets and page and thread records | 90 / 3,000 per minute |
| `create` | New entities, pages and threads | 30 / 1,000 per minute |
| `move` | Page and thread renames and moves | 8 / 100 per minute |
| `link` | `same-as`, `different-from`, `equivalent-property` | 30 / 1,000 per minute |
| `job` | Bulk-job submissions ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.1); an administrator starts a Nuke or a job revert through a subsidiary they operate (§4.1) | 0 / 10 per hour |
| `read` | API requests of any kind, by key or session; token refreshes, against the subsidiary ([0025](../decisions/0025-oauth-server.md) §6); every MCP tool call ([0075](../decisions/0075-mcp-server.md) §5). Counted for authenticated principals only once a per-process count passes a fraction of the limit, and never for a response served from L0 or L1 (§6.3) | 5,000 / 50,000 per minute |
| `stream`, `atom` | Open streams and Atom fetches ([0020](../decisions/0020-change-feeds.md) §4) | 5 / 20 concurrent; 60 / 600 per hour |
| `upstream` | Live upstream fetches ([0012](../decisions/0012-api-requirements.md) §6); live history fetches of a mirrored page ([0053](../decisions/0053-mirrored-pages.md) §11) | 30 / 30 per minute |
| `notify` | Outbound notification requests initiated by the actor: verification messages, handle registrations ([0021](../decisions/0021-notifications.md) §5) | 5 / 5 per hour |
| `account` | Creating subsidiaries, issuing keys, login attempts; OAuth authorization attempts, consent and device-code entry, against the primary account ([0025](../decisions/0025-oauth-server.md) §6); the user data export ([0027](../decisions/0027-preferences-and-portability.md) §3) | 10 / — per hour |
| `createaccount` | Accounts created and account requests filed (§2.6), counted by the caller's address key whoever the caller is, as MediaWiki's `$wgAccountCreationThrottle` counts; over the limit, `action=createaccount` answers `FAIL` with `acct_creation_throttle_hit` ([0087](../decisions/0087-account-registration-and-challenges.md) §4, A4) | `account.creation_limit` per day, for every group: 6 by default |
| `upload`, `renderfile`, `renderfile-nonstandard` | MediaWiki's three classes for files: uploads, per actor and per IP; thumbnails rendered on a miss, per IP; renders whose transform is not in `files.thumb_widths` (a `page=` or time offset) ([0039](../decisions/0039-files-and-media.md) §15). Serving bytes already stored is not rate-limited | per 0039 §15 |
| `parse` | Expansions a client asks for directly: `action=parse` with `text`, `action=expandtemplates`, server preview, and the Lua console if it is ever added ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16); a reader who forces a foreign-page fetch with `action=purge` ([0053](../decisions/0053-mirrored-pages.md) §11). Page views are not counted; they are cached renders | 60 / 600 per minute |
| `export` | Exports ([0047](../decisions/0047-special-pages.md) §8) | 30 / 300 per hour |
| `fork` | Forks started, each of which is a job that may write thousands of records and fetch thousands of revisions ([0054](../decisions/0054-forking-a-mirrored-page.md) §8); the seeding itself runs under the repository's fetch budget ([0053](../decisions/0053-mirrored-pages.md) §1), not the user's | 5 / 100 per hour |
| `query` | SPARQL queries started at `/sparql` or `Special:Query`, and on-demand refreshes of query-backed scopes ([0059](../decisions/0059-query-service.md) §6); the MCP tools `find`, `facets`, `run_query` and `sparql`, beside `read` ([0075](../decisions/0075-mcp-server.md) §5) | 30 / 300 per minute |

Fetches of a mirrored page are server-initiated and are bounded per repository ([0053](../decisions/0053-mirrored-pages.md) §1; [13](13-mirrored-pages.md)), not per reader. The permission half of [0053](../decisions/0053-mirrored-pages.md) §11, [0054](../decisions/0054-forking-a-mirrored-page.md) §8 and [0067](../decisions/0067-proposals.md) §8 is in [09](09-security-and-moderation.md).

### 6.3 Counters and refusals

*Sources: [0024](../decisions/0024-subsidiary-accounts.md) §5; [0083](../decisions/0083-write-path-in-three-tiers.md) §1; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5; [0087](../decisions/0087-account-registration-and-challenges.md) §4.*

**Counters** are the `rl:{class}:{key}` keys of [0014](../decisions/0014-caches-and-search.md) §4 in Valkey ([03](03-storage-caches-and-search.md) §12.3), the key being the actor key or, for anonymous requests, the **address key**: the IPv4 address, or the /64 prefix of an IPv6 address, since a host commonly holds a whole /64 and can rotate through it at no cost ([0087](../decisions/0087-account-registration-and-challenges.md) §4, correcting [0024](../decisions/0024-subsidiary-accounts.md) §5); the window is the TTL. An instance without a shared cache counts in process, approximately, which is what the small profile of [0013](../decisions/0013-postgres-storage.md) §11 accepts, except the `createaccount` counter and the two `badlogin` counters (§2.6), whose windows are a day and ten minutes long: those it keeps in Postgres, one row per counted act in `private.rate_event` `(tenant, class, key, at)`, counted within the window and swept as sessions are, so that a script cannot start again by spreading its attempts over the replicas of a pool or by waiting for a restart ([0087](../decisions/0087-account-registration-and-challenges.md) §4). A request over its limit is refused before anything is appended: HTTP 429 with `Retry-After`, and the Action API's `ratelimited` error. **`read` is counted lazily.** A read by an authenticated principal touches the shared counter only once a per-process count of its reads has passed a fraction of the limit, and a response served from L0 or L1 is never counted at all, so the cost of the `read` class on a cached page is nothing; anonymous reads are counted by address key as before. `maxlag` ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.8) is orthogonal: it answers for lag, the larger of replica lag and the tenant's local-partition composition lag ([0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §5; [18](18-api.md)), not for the actor's rate.

**Bulk jobs are limited at submission, not per record.** A job's throughput is the ingester's ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.6; [05](05-providers-and-ingest.md)); what a subsidiary is limited in is how many jobs it may start, so a runaway script cannot queue a thousand.

### 6.4 Limits that are not rates

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §16; [0067](../decisions/0067-proposals.md) §8; [0075](../decisions/0075-mcp-server.md) §5.*

Expansion **limits** are deployment configuration with MediaWiki's names and MediaWiki's defaults, read from the reference install's `limitreport` output: maximum article size, post-expand include size, template argument size, expansion depth, node count, and expensive function count (shared with Lua, [0043](../decisions/0043-lua-modules.md) §4; [11](11-rendering-templates-and-modules.md)). A tenant may lower them and not raise them. `proposals.max_items` (default 500) bounds a proposal payload; a larger selection is split into several proposals by the form. MCP results are capped at `mcp.max_results` (default 200) per call, with a cursor, and queries run under `query.timeout` ([0059](../decisions/0059-query-service.md) §6).

## 7. The OAuth server

*Sources: [0025](../decisions/0025-oauth-server.md) §1, §2, §3, §4, §5, §6; [0057](../decisions/0057-web-tier.md) §13; [0067](../decisions/0067-proposals.md) §8.*

### 7.1 An OAuth 2.0 authorization server

*Sources: [0025](../decisions/0025-oauth-server.md) §1.*

The instance is an **OAuth 2.0 authorization server** ([RFC 6749](https://www.rfc-editor.org/rfc/rfc6749)) offering:

| Flow | For | Notes |
|---|---|---|
| Authorization code with PKCE ([RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)) | Web tools with a redirect URI | The only flow for public clients; confidential clients may also present a client secret |
| Device authorization ([RFC 8628](https://www.rfc-editor.org/rfc/rfc8628)) | Command-line and desktop tools | The person enters a short code on `Special:OAuth/device` |
| Refresh token | Both | Refresh tokens rotate on use; a reused refresh token revokes the whole authorization |
| Token revocation ([RFC 7009](https://www.rfc-editor.org/rfc/rfc7009)) | Both | Also from the account page (§7.6) |

The client-credentials flow is **not offered**: a tool acting for nobody is a bot with an API key (§5), and MediaWiki's owner-only consumers have no counterpart here. Implicit grant and resource-owner-password are not offered either.

**Tokens** are opaque bearer strings ([RFC 6750](https://www.rfc-editor.org/rfc/rfc6750)) in the shape of API keys, `{token ID}.{secret}`, presented as `Authorization: Bearer` on any Action API or REST request. Token IDs carry a prefix that distinguishes them from key IDs so the authentication layer knows which table to check; from there a token and a key are the same thing, a credential of a subsidiary with grants (§7.4). Access tokens live four hours, refresh tokens until revoked or unused for a year; both are `site` settings (`oauth.access_ttl`, `oauth.refresh_ttl`). Tokens are not JWTs: an opaque token is revocable at once, and nothing about a subsidiary needs to be readable from the token without asking the instance. The tables that hold tokens and consumer secrets are in [03](03-storage-caches-and-search.md) §4.16.

**Not yet.** **OAuth 1.0a is not offered.** Pywikibot and several older tools speak only 1.0a; Pywikibot also speaks bot passwords, which are API keys (§5.2). Whether a 1.0a shim is worth its signing code is left open. OpenID Connect is also left open (§2.2).

### 7.2 Endpoints and `/oauth/identify`

*Sources: [0025](../decisions/0025-oauth-server.md) §1; [0057](../decisions/0057-web-tier.md) §13.*

**Endpoints.** Native paths, and the paths of MediaWiki's OAuth extension so that tools written for it work by changing only the host:

| Native | MediaWiki-compatible | Purpose |
|---|---|---|
| `/.well-known/oauth-authorization-server` | — | Metadata ([RFC 8414](https://www.rfc-editor.org/rfc/rfc8414)): endpoints, PKCE methods, the grant names as `scopes_supported` |
| `/oauth/authorize` | `/w/rest.php/oauth2/authorize` | Validates the request and hands it, under a request handle, to the consent page (§7.4); requires an interactive session of a **primary** account |
| `/oauth/token` | `/w/rest.php/oauth2/access_token` | Code, device and refresh exchanges |
| `/oauth/device` | — | Device authorization request |
| `/oauth/revoke` | — | Revocation by the client |
| `/oauth/identify` | `/w/rest.php/oauth2/resource/profile` | Who the token acts as (below) |

The remaining routes, for consumers, authorizations, pending subsidiaries and request handles, are in [18](18-api.md).

**`/oauth/identify`** returns the **subsidiary** the token acts as, and its operator: `sub` (the subsidiary's user ID, §2.1), `username` (its name), `groups`, `rights` (its effective permissions), `grants` (the token's), `blocked`, `registered`, and an `operator` object with the primary account's `sub` and `username`. The MediaWiki-compatible `profile` route returns the same fields in the extension's shape, so a tool that greets the person by `username` greets the subsidiary; a tool that wants the person's own name reads `operator.username`. There is no `email` or `realname` field: the instance holds neither (§2.1, [0021](../decisions/0021-notifications.md) §3), and a tool that needs to reach the person notifies the subsidiary, which routes to the operator (§4.6).

### 7.3 Consumers are instance configuration

*Sources: [0025](../decisions/0025-oauth-server.md) §2.*

A **consumer** is a registered tool. Its record is a `config` record of kind **`consumer`** in the **instance** `config` partition ([0015](../decisions/0015-record-format-and-partition-registry.md) §3, [0028](../decisions/0028-tenancy-policy.md) §1; [23](23-configuration-and-registry.md)), keyed `consumer:{slug}`, holding: the slug; a display name and description; the developer's actor key (its **owner**); one or more redirect URIs, or `device` for a device-flow-only tool; whether the client is public or confidential; the **grants** it requests, from `docs/registry/grants.toml` (§5.3); a callback-is-prefix flag in the manner of MediaWiki's; and a **status**: `proposed`, `approved`, `disabled` or `rejected`. Config records are append-only, so a consumer's history is its record history and a change of redirect URI or grants is a new record.

**Registered once per instance, because a consumer is one thing.** Its redirect URI, its secret and its developer do not vary by tenant, and a developer should not register QuickStatements forty times on a forty-tenant farm. **Approval is also instance-level**: a holder of `mwoauthmanageconsumer` ([09](09-security-and-moderation.md)) moves a consumer from `proposed` to `approved`, `rejected` or `disabled`, and only an approved consumer may be authorized by anyone. Confidential clients' secrets are hashed into `private.oauth_consumer` and shown once at registration; the owner may rotate them. Who approves under each tenancy preset is in [08](08-tenants-and-instances.md).

**Enablement is per tenant.** A tenant `config` record of kind **`consumer-policy`**, in the shape of [0026](../decisions/0026-sitelinks.md) §3's lists, holds `allow` and `deny` lists of consumer slugs and, per slug, an optional `auto_approve` flag (§4.3). With no record, every approved consumer is available and none is auto-approved. A farm may write a `consumer-policy` template or lock it ([0028](../decisions/0028-tenancy-policy.md) §8), which is how an enterprise limits its tenants to the tools it has vetted.

A consumer whose owner vanishes (§3.2) is disabled; a disabled consumer's tokens stop working at once (§7.6). `Special:OAuthConsumers` lists consumers, their status and their requested grants, and is where registration and approval happen (§7.6).

### 7.4 Authorization and the consent page

*Sources: [0025](../decisions/0025-oauth-server.md) §3; [0057](../decisions/0057-web-tier.md) §13.*

The authorization server keeps its endpoints and their checks; its pages are pages of the site, as MediaWiki's `Special:OAuth/authorize` is a special page: the consent page, `Special:OAuth/device`, `Special:OAuthConsumers` and `Special:PendingSubsidiaries` are rendered by `triplespace-ui` over the OAuth routes ([18](18-api.md), [20](20-web-tier.md)).

1. **`/oauth/authorize`** (at both paths) validates the request: the client, the redirect URI, the PKCE challenge, the requested grants, the consumer's status and the tenant's `consumer-policy`. It stores the validated request under a **request handle** with a ten-minute lifetime, where authorization codes are kept, and answers `303` to `Special:OAuth/authorize?request={handle}`. A request that fails validation is answered by the API and never redirected to an unverified URI.
2. **The consent page** reads `GET /oauth/requests/{handle}`: the consumer, the grants on offer and the subsidiaries the person may choose. Only the primary account in whose session the handle was created may read it; anyone else gets `404`.
3. **The decision** is a form post to `POST /oauth/requests/{handle}/approve`, with the CSRF token, the chosen or new subsidiary and any narrowed grants, or to `POST /oauth/requests/{handle}/deny`. The API checks everything again, issues the code, and returns the consumer's redirect URL, which the web tier answers with `303`.

- **The device flow** joins at step 2: `Special:OAuth/device` posts the user's code to `POST /oauth/requests`, which returns the handle of the pending device authorization.
- **The consent and device pages cannot be framed:** they are sent with `frame-ancestors 'none'`.
- **Request handles, authorization codes and device codes survive a change of replica.** Without a shared cache they are kept in `private` through `triplespace-accounts`, as sessions are (§2.4), not in process, so that any replica of an API pool can finish a flow another began.

The consent page, `Special:OAuth/authorize`, is shown to a **primary account** with an interactive session; a subsidiary cannot reach it, since it cannot log in (§4.1), and a temporary account is refused. It shows the consumer's name and description, the grants requested, intersected with what the consumer was approved for, and **which subsidiary the tool will act as**:

- **An existing subsidiary** of the person, offered if it is **approved** (§4.3) and not retired. The tool then acts as an account the community has already reviewed, at whatever rate its groups allow.
- **A new subsidiary**, created here. The form suggests `{Operator}-{slug}` (`Example-quickstatements`), subject to the same name rules and `subsidiaries.name_pattern` and `subsidiaries.max_per_account` as §4.2, and creation requires `createaccount` as there. The instance appends the actor record with kind `bot`, the person as operator, and status **`pending`** (§4.3), and projects `newusers/create2` as for any subsidiary.

The person may also narrow the grants below what the consumer asked for, as MediaWiki's consent page allows. What results is an **authorization**: a refresh token and its first access token, both **credentials of the chosen subsidiary**, with **grants** = requested ∩ approved ∩ consented, stored in `private.oauth_token` beside the subsidiary's keys. A request made with a token has the effective permissions §5.3 gives a key's.

**One authorization per (subsidiary, consumer).** Authorizing the same consumer again for the same subsidiary replaces the earlier tokens. A subsidiary may be bound to several consumers, each with its own tokens and grants; the tag of §7.5 keeps their edits apart. A token acts in one tenant; the tenant side is in [08](08-tenants-and-instances.md).

### 7.5 Attribution and the consumer tag

*Sources: [0025](../decisions/0025-oauth-server.md) §4.*

Every record written with a token is attributed to the **subsidiary**, with `prov:actedOnBehalfOf` its operator as for any subsidiary (§4.6). The attestation part of the record ([0015](../decisions/0015-record-format-and-partition-registry.md) §1, as [0030](../decisions/0030-edit-filters.md) §5 widened it; [01](01-log-and-records.md)) carries the change tag **`oauth:{slug}`**, so a contributions page can show and filter which tool made an edit and the metadata graph can emit it. This is the tag 0030 §5 reserves for the OAuth server, and it corresponds to MediaWiki's `OAuth CID: n` with the slug in place of the number.

**API keys carry no tag.** A label is private state chosen by the operator for their own bookkeeping, and a tag is public and enters records. A consumer's slug is different in kind, since the consumer is registered, approved and listed publicly. An operator who wants to tell deployments apart uses one subsidiary per deployment, which is what subsidiaries are for.

The tag is written by the request path, not by the client: a client cannot omit it, add it to a request made with a key, or supply another consumer's slug. Edit filters ([0030](../decisions/0030-edit-filters.md) §2; [09](09-security-and-moderation.md)) see the tag in `tags` and `user_kind = bot`, so a filter can throttle or warn on a particular tool. `Special:Contributions` gains a tag filter row for `oauth:*`, and `Special:RecentChanges` already filters by tag ([0010](../decisions/0010-site-ui.md) §7; [19](19-site-ui.md)).

### 7.6 Management, revocation and log events

*Sources: [0025](../decisions/0025-oauth-server.md) §5, §2.*

**Connected applications**, a section of `Special:Account` ([0010](../decisions/0010-site-ui.md) §11; [19](19-site-ui.md)), Private, lists each authorization the person's subsidiaries hold: consumer, subsidiary, grants, authorized and last-used dates, and **Revoke**. A subsidiary's own user page gains no such list; authorizations are private state of the operator. `Special:OAuthConsumers` is the consumer registry: a developer registers and updates their own; a holder of `mwoauthmanageconsumer` approves, rejects and disables; everyone reads it. These pages are listed in [21](21-special-pages.md).

**Revocation is immediate**, as for keys: revoking an authorization marks its tokens revoked and drops every session opened with them, which is why sessions record the token ID (§2.4). Tokens are also revoked when the subsidiary is retired or transferred (§4.4, since the new operator did not consent), when the operator is blocked in a way that removes `edit` (they simply fail, since blocks reach subsidiaries, §4.5), when the consumer is disabled, and when the operator vanishes, which requires the subsidiary to have been retired or transferred first anyway.

**Log events.** The consumer lifecycle is public and is logged: `oauth/propose`, `oauth/update`, `oauth/approve`, `oauth/reject`, `oauth/disable`, with the consumer slug as target and the reason in the comment part, written to the **instance `log`** ([0046](../decisions/0046-primary-tenant.md) §4; [16](16-logs-feeds-and-notifications.md)): consumers are instance configuration, and their record must not move when the primary role does. **Authorizations and revocations are not logged**: an authorization is private state, like the issue of a key (§5.1), and nothing about a token is ever a log record. What is public is what the subsidiary then does, and its creation and approval, which `newusers/create2` and `rights/rights` already cover.

### 7.7 The instance as an OAuth client

*Sources: [0025](../decisions/0025-oauth-server.md) §1; [0067](../decisions/0067-proposals.md) §8; [0088](../decisions/0088-external-identity-providers.md) §2.*

**The instance is also an OAuth client of other wikis, for two purposes.** The first is login: a consumer registered for identity only, whose tokens are discarded once the person is known (§2.7; [0088](../decisions/0088-external-identity-providers.md) §2). The second: [0067](../decisions/0067-proposals.md) §6 ([14](14-discussions.md), [17](17-federation-and-publication.md)) has it obtain, as a separate authorization from login, an edit-scoped grant on the wiki a proposal is destined for, stored in `private.upstream_grant` and used only to push that person's proposals as that person; pushing needs a grant of one's own. The server role here and the client role there never share a credential, and the rule that a tool's edits are a subsidiary's holds on this side: a subsidiary may hold a grant of its own under its operator.

## 8. Preferences, private state and portability

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §1, §2, §3, §4, §5; [0007](../decisions/0007-actor-identity.md) §8.*

### 8.1 One preference store, one key registry

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §1; [0085](../decisions/0085-settings-and-preferences.md) §1, §2.2; [0086](../decisions/0086-theming.md) §4; [0094](../decisions/0094-accessibility-alternates.md) §5.*

**A preference is a key–value pair owned by an account**, stored in `private.preference (actor_key, key, value jsonb, updated)`. The keys are registered in `docs/registry/preferences.toml` ([0015](../decisions/0015-record-format-and-partition-registry.md) §5; [23](23-configuration-and-registry.md)), each with a type, a default, a group and a description; an instance may add keys for features it adds. **A preference resolves, first match wins** ([0085](../decisions/0085-settings-and-preferences.md) §2.2): the account's own value; `preferences.defaults`, the tenant's map merged key by key over the instance's default for it ([23](23-configuration-and-registry.md) §3.1); the registry default. A key marked `follows = farm` belongs to the farm account under farm identity ([08](08-tenants-and-instances.md) §3.5), so every linked tenant account reads one value; a reader's own choice of interface language, label languages, time zone and date format does. A key marked `follows = choice`, as `ui.palette` and `ui.typeset` are, is the tenant account's unless the reader marks it **Use on every wiki**, which keeps it on the farm account; the tenant account's own value is read first ([0086](../decisions/0086-theming.md) §4). **The interface language and the label languages are marked `negotiated`**: without the reader's own choice, they come from the request, `uselang` first and then `Accept-Language` matched against the languages the interface has messages for, for readers who are not logged in too; only a request that names nothing usable falls to the tenant's default (for `language`, `site.language` unless `preferences.defaults` names another) and then the farm's ([0085](../decisions/0085-settings-and-preferences.md) §2.2). A key marked `browser` may be held in the browser of a reader who is not logged in ([19](19-site-ui.md) §2.7). The `group` of a key is its tab of `Special:Preferences` ([19](19-site-ui.md) §2.6). A value that is not registered, or does not fit its type, is refused. This is MediaWiki's `user_properties` table with `$wgDefaultUserOptions`, under a schema.

The keys the earlier ADRs scattered are gathered here:

| Key | Type | From |
|---|---|---|
| `language`, `label-languages` | language code; list of codes | [0010](../decisions/0010-site-ui.md) §11 |
| `ts-upstream-fold`, `ts-rc-syncs` | boolean | [0010](../decisions/0010-site-ui.md) §11, [0012](../decisions/0012-api-requirements.md) §4 |
| `autowatch.edit`, `autowatch.create`, `autowatch.post`, `autowatch.thread` | boolean | [0020](../decisions/0020-change-feeds.md) §3 |
| `autowatch.notify` | boolean: auto-watches carry `notify` | [0021](../decisions/0021-notifications.md) §2 |
| `feeds.period`, `feeds.syncs` | duration; boolean | [0020](../decisions/0020-change-feeds.md) §1–2 |
| `threads.visibility` | the rule of [0019](../decisions/0019-discussions.md) §6, as a map from status category and age to visible, collapsed or hidden | 0019 §6 |
| `notifications.{reason}.{channel}` | boolean, one key per cell of the matrix | [0021](../decisions/0021-notifications.md) §3 |
| `shapes.pins` | a map from property ID to shape (§8.2) | [0003](../decisions/0003-statement-ui.md) §3 |
| `ui.mode`, `ui.contrast`, `ui.text-size`, `ui.vision`; `ui.palette`, `ui.typeset` | colour mode, contrast, text size and the colour-vision alternate, which follow the farm account; a palette and a type set, per wiki unless marked for every wiki | [0086](../decisions/0086-theming.md) §4; [0094](../decisions/0094-accessibility-alternates.md) §5 |
| `timezone`, `date-format`, `editor.font`, `rc.limit`, and the other MediaWiki options Triplespace honours | as in MediaWiki | [mediawiki-compat.md](../api/mediawiki-compat.md) |

**The notification matrix lives here**: `private.notification_pref` is retired, and the addressing projection ([16](16-logs-feeds-and-notifications.md)) reads `notifications.*` keys instead. The four auto-watch flags of [0020](../decisions/0020-change-feeds.md) §3 likewise; `private.watch` is unchanged.

Preferences belong to primary accounts. A subsidiary (§4) has none of its own; where a preference would matter for it, such as the interface language of a notification about it, the operator's applies. Temporary accounts have preferences for the life of the account, as MediaWiki's do.

### 8.2 Shape pins

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §5.*

[0003](../decisions/0003-statement-ui.md) §3 lets an editor pin a shape for a property "for the whole instance or for one entity" ([19](19-site-ui.md)). The pin lives in two places by scope:

- **A viewer's own pin** is the preference `shapes.pins` (§8.1): a map from property ID, or `{entity}:{property}`, to a shape. It affects only that viewer.
- **A tenant-wide pin** is a `config` record of kind `view-pin`, keyed `view-pin:{property}` or `view-pin:{entity}:{property}` ([0015](../decisions/0015-record-format-and-partition-registry.md) §3; [23](23-configuration-and-registry.md)), written with `ts-config`. It is logged, ordered and rebuildable like every registry entry, and the UI's view switch shows it as the default with the viewer's pin, if any, on top.

Neither is a statement, which is what 0003 required.

### 8.3 What private state is, and which of it is portable

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §2; [0007](../decisions/0007-actor-identity.md) §8, §3.*

Everything in `private` ([03](03-storage-caches-and-search.md) §1.2, §4.16) belongs to one account, classified by whether it may leave the instance with that account:

| Class | State | Why |
|---|---|---|
| **Portable** | Preferences (§8.1); the watch set with `seen` and expiries ([0020](../decisions/0020-change-feeds.md) §3); contact details: email address and fediverse handle, with their verification state ([0021](../decisions/0021-notifications.md) §5, §8); subsidiary **metadata**: names, operator, key labels, grants, IP ranges and expiries, never secrets (§5.1); the inbox within its retention, with read state ([0021](../decisions/0021-notifications.md) §3) | Chosen by the person, about the person, useful on any instance |
| **Re-established, not carried** | Bindings (§2.1): the person logs in again and the identity provider proves who they are; API-key secrets and OAuth tokens (§5, §7): reissued; the password hash of the built-in issuer, `private.password` (§2.2): set again; the Atom watch token ([0020](../decisions/0020-change-feeds.md) §4): reissued; sessions; upstream OAuth grants (§7.7): reissued | Secrets are minted by an instance for itself, and a proof of identity is not transferable |
| **Never leaves** | IP addresses held for abuse handling and IP blocks (§2.3, [0016](../decisions/0016-permissions-and-access-control.md) §3); the notifier's private key ([0021](../decisions/0021-notifications.md) §5) | Not the person's, or bound to one instance |

The classification is data: each `private` table is tagged in `triplespace-db`'s schema definition with its class, as a table comment `portability: {class}` that the crate checks against the live schema, and the export of §8.4 is generated from the tags, so a table added later cannot be forgotten. A table whose columns fall in two classes, as `api_key` does (portable metadata, a re-established secret), takes the stricter class, and the export reads only its portable columns. The privacy test of [0012](../decisions/0012-api-requirements.md) §8 ([09](09-security-and-moderation.md)) checks that nothing in the second or third class appears in a bundle.

### 8.4 The user data bundle

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §3.*

**Any account holder may download all of their portable state** as one document, and import one. The bundle is JSON with the payload type `scatter:v0/user-export`, versioned like every payload type ([01](01-log-and-records.md)), and documented in `docs/api/user-export.md`. It holds:

- the account's public identity: name, actor key, the instance's base URI and tenant slug, and the time of export;
- `preferences`: every set key;
- `watchlist`: target kind, target ID (with the entity's current canonical ID and, for pages and threads, the current title, so that a human can read it), `seen`, expiry, `notify`;
- `contacts`: email address and fediverse handle, each with `verified`;
- `subsidiaries`: for each, its name and actor key, and for each key its label, grants, IP ranges and expiry;
- `inbox`, optionally: the rows within retention, each as the activity row it points at plus reason and read state;
- a signature by the instance key ([0006](../decisions/0006-log-integrity-and-erasure.md) §6; [01](01-log-and-records.md)) over the bundle, so a receiving instance can tell an export from a forgery, and the holder can prove where it came from.

**Export** is `GET /account/export` and **Download my data** on `Special:Account` ([18](18-api.md), [19](19-site-ui.md)), rate-limited in the `account` class (§6.2). A bundle that would be large because of the inbox is produced by a job in `ops` and fetched when ready. **Import** is `POST /account/import`, holder only: preferences overwrite, the watch set is a union (an entity ID that does not resolve here is kept and reported), contacts are stored **unverified** and a verification message is sent ([0021](../decisions/0021-notifications.md) §4–5), subsidiaries are recreated as the importing account's with no keys, and inbox rows are stored read-only as history. An import is idempotent: importing the same bundle twice changes nothing.

**MediaWiki's forms stay.** `list=watchlistraw` and `action=watch` ([0020](../decisions/0020-change-feeds.md) §5) and `action=options` keep working; the bundle is the form that covers everything at once.

### 8.5 The private extract in a tenant move

*Sources: [0027](../decisions/0027-preferences-and-portability.md) §4.*

A **cooperative** move ([0018](../decisions/0018-tenants.md) §10; [08](08-tenants-and-instances.md)) carries three things beside the bundle: the retention extract, the binding digest, and a **private extract**: for every account of the tenant, its user data bundle of §8.4 (inbox included), encrypted so that only the receiving instance can read it. The receiving instance supplies, with the public key it already supplies for the key-chain rotation, an **encryption key** for the move; the extract is sealed to it; the old instance keeps no copy once the move completes. The extract travels operator to operator, as the binding digest does, never in the public bundle.

**Applied on reclaim.** A bundle is applied to an account when, and only when, that account is reclaimed by one of the three routes 0018 §10 gives, so nobody's settings appear under an account its holder has not yet proved is theirs. Contacts arrive **verified**, since the old instance had verified them and the move is cooperative; the fediverse handle's *follow* of the notifier has to be renewed, because the notifier is a new actor ([0021](../decisions/0021-notifications.md) §5), and the account page says so. Keys are not in the extract; the operator reissues them, after reclaiming their own account ([0024](../decisions/0024-subsidiary-accounts.md) §7).

**Unreclaimed accounts.** Bundles for accounts never reclaimed are deleted when the reclaim grace period ends, and the receiving instance logs the deletion count. An account can still be reclaimed afterwards; it simply arrives with defaults, and the person may import a bundle they downloaded themselves.

**Non-cooperative moves** have no private extract: `private` is not in backups the new operator can be handed responsibly, and 0018 §10 already limits that case to the public bundle. Self-service export and import (§8.4) are what a person has then, which is why **Download my data** exists at all and why the account page suggests it before a vanish (§3.2).

**Rehosting an instance whole** carries `private` with it ([0018](../decisions/0018-tenants.md) §10) and needs none of this.
