# 19. Site UI

This chapter describes the site as a reader or editor meets it in a browser: the principles every page follows and the one frame every page wears; search, addresses, logging in, account settings and contributions; document pages, histories, diffs, recent changes and jobs; the statement UI, from shape detection through rank, shared qualifiers, provenance and editing to its components; the frontend stack that renders all of it; and what each later feature adds to the pages. It assumes the entity model and identifiers of [04](04-entities-and-identifiers.md), source graphs and mirroring from [05](05-providers-and-ingest.md), statements, roles and constraints from [06](06-statements-and-properties.md), actors, accounts, subsidiaries and preferences from [07](07-actors-and-accounts.md), tenants and bases from [08](08-tenants-and-instances.md), permissions and visibility from [09](09-security-and-moderation.md), pages and content models from [10](10-pages-and-content-models.md), rendering and templates from [11](11-rendering-templates-and-modules.md), files from [12](12-files-and-media.md), mirrored pages from [13](13-mirrored-pages.md), threads and boards from [14](14-discussions.md), structured pages from [15](15-structured-pages.md), and logs, feeds and notifications from [16](16-logs-feeds-and-notifications.md). Everything the UI shows it fetches through the API of [18](18-api.md); the routing and the deployment of the site are [20](20-web-tier.md), the special-page registry is [21](21-special-pages.md), the frontend build and crates are [22](22-crates-and-stack.md), and the settings and preferences the UI reads are catalogued in [23](23-configuration-and-registry.md).

## 1. Principles and the frame

*Sources: [0010](../decisions/0010-site-ui.md) §1, §2; [0003](../decisions/0003-statement-ui.md) §1; [0038](../decisions/0038-page-metadata-and-categories.md) §7; [0034](../decisions/0034-frontend-stack.md) §1.*

### 1.1 Principles

*Sources: [0010](../decisions/0010-site-ui.md) §1; [0003](../decisions/0003-statement-ui.md) §1; [0038](../decisions/0038-page-metadata-and-categories.md) §7.*

These extend the principles of the statement UI (§4.1), which still apply to the whole site: show the exception, not the rule; name the effect, not the mechanism; nothing is lost.

1. **One frame for every page.** An entity, a document page, a user, a job and a special page all get the same header: an identity line, the title, and tabs. A reader learns the frame once. A frame's tabs show data only about what its identity line names; a page's statements are never drawn in its item's frame, or the reverse, and crossing from one to the other is a link in the identity line (§1.4).
2. **Show the exception, applied to activity.** Mirror syncs and bulk jobs are routine. Edits made by people on this instance are the exception, and they are listed one by one. A marker that would be the same on every row is not drawn. Examples:
   - A page in its namespace's default content model shows no model.
   - A local-only item shows no source switch.
3. **Group by cause.** A change that one job or one sync made is shown as part of that job or sync. It is never shown as millions of separate rows.
4. **Say what is missing.** Nothing is silently absent. Each of these is shown in place, as a labelled gap:
   - mirror states that compaction dropped;
   - erased records;
   - hidden usernames;
   - templates that were not rendered;
   - categories from an imported wiki.
5. **Read sources together; compare them apart.** Lists merge all sources by time, and each row carries its source. Comparing two states works only within one source (§3.7).
6. **Label privacy where it is decided.** Any control that shares something about a person carries a **Public** or **Private** label, next to the control. It is never only in a help page.
7. **Every change can be traced and undone.** A row names the account that made the change. A bot also names its operator ([0007](../decisions/0007-actor-identity.md) §6). An edit can be undone, a job reverted, and any record checked against a signed checkpoint.
8. **Fetch upstream history; don't copy it.** Upstream history the instance does not keep is fetched live from the provider when a reader asks, and marked as such. It is never written to the log (§3.6).

### 1.2 The global header

*Sources: [0010](../decisions/0010-site-ui.md) §2; [0021](../decisions/0021-notifications.md) §6; [0034](../decisions/0034-frontend-stack.md) §2; [0085](../decisions/0085-settings-and-preferences.md) §6.*

The global header holds:

- the site name, linking home;
- one search box (§2.1);
- links to Recent changes, Jobs and Special pages;
- a **New** menu for creating items, properties and pages;
- the account menu, or **Log in**;
- the **Appearance** button, for every viewer (§2.7);
- the notifications **bell**, with the unseen count, opening the inbox as a panel (§6.2).

**Special pages** links to `Special:SpecialPages`, the index of every page `docs/registry/special-pages.toml` marks served, grouped, and filtered by the viewer's rights. The **New** menu opens `Special:NewItem` and `Special:NewProperty` for entities, and the edit form for pages ([0047](../decisions/0047-special-pages.md) §5, §9). The frame (global header, identity line, title, tabs) is one template shared by every page kind ([0034](../decisions/0034-frontend-stack.md) §2; §5.2).

### 1.3 The page header: identity line, title and tabs

*Sources: [0010](../decisions/0010-site-ui.md) §2; [0038](../decisions/0038-page-metadata-and-categories.md) §7; [0018](../decisions/0018-tenants.md) §11; [0022](../decisions/0022-federation.md) §11; [0024](../decisions/0024-subsidiary-accounts.md) §9; [0056](../decisions/0056-security-model.md) §13; [0028](../decisions/0028-tenancy-policy.md) §11; [0067](../decisions/0067-proposals.md) §7.*

The page header has three parts:

1. **An identity line.** It shows a chip for the ID or namespace, the kind of page, and provenance where there is any, such as "Foreign item, minted by Wikidata" or "First 14 revisions imported from Librarybase".
2. **The title.**
3. **The page's tabs:**

| Page kind | Tabs |
|---|---|
| Entity view | Statements, Identifiers, Sitelinks, Labels in other languages, History, Links here, and Talk (§6.1); a property adds Constraints (§6.4) |
| Document page | Read, Edit, Page data (§1.4), History, Links here, and Talk |
| Thread, talk page | The posts as a tree, or the list of threads (§6.1), with Page data (§1.4) and History |
| File page | The file, its versions, its usage and metadata (§6.6) |
| User | User page, Contributions, Jobs, Links here |
| Job | Summary, Changes, Rejected |
| Special page | None |

This is the one tabs table; [0038](../decisions/0038-page-metadata-and-categories.md) §7 refers to it.

- **Entities have no Edit tab.** They are edited in place (§4.7).
- **Less common actions** go in the page's overflow menu. For a document page these are Move, Change content model, **Protect…** and **Delete…**, with an option to delete the talk page too; a deleted page shows **Undelete…** to those who may (§6.3). An entity page's overflow menu also carries "Propose to {wiki}" (§6.10).
- **Links here** is served by the links projection ([0008](../decisions/0008-namespaces-and-document-pages.md) §10; [10](10-pages-and-content-models.md)).
- **The Page data tab** shows a document page's or thread's own statements in the statement UI, with projected statements read-only (§1.4).
- **The Sitelinks tab** groups links by host, each host a Domain chip linking to `Domain:{host}` (§6.4).
- **A deleted page or entity** shows, to everyone outside the deleting group, the frame with the deletion log entry in place of the content (§6.3).
- **A file page** shows, for a taken-down version, the operator's notice in place of the file (§6.6).
- **A talk page's header** shows its `Group`'s fediverse handle when its namespace is federated, and a mirrored entity's identity line carries a `verified` chip when its provider is verified ([0022](../decisions/0022-federation.md) §11).
- **A foreign tenant's entity** shows "Item from Librarybase" with the `LB` chip, as a Wikidata item shows `WD` ([0018](../decisions/0018-tenants.md) §11); the identity line of a mirrored provider-tenant entity is the same under every tenancy preset ([0028](../decisions/0028-tenancy-policy.md) §11).
- **A subsidiary's user page** shows "Bot operated by {Name}" in the identity line, with a Bot chip when it holds `bot`, and a **Request bot approval** link that goes wherever `site` configuration points (`subsidiaries.request_page`, a project page or a talk page) ([0024](../decisions/0024-subsidiary-accounts.md) §9).
- **Every page, entity or thread whose visibility is non-empty** shows a **Restricted** pill with a lock icon in its identity line, naming the groups on hover and the sets it is in; readers should know they are reading something not everyone can ([0056](../decisions/0056-security-model.md) §13; [09](09-security-and-moderation.md)).
- **A page served by a page repository** carries the repository's origin chip and an identity line naming the upstream revision and licence; a title with alternates gains **Other versions**, listing each alternate its repositories and the page's `page-alternates` statement allow, reached with `origin={repository}`; its Edit tab is the fork form. A **fork** reads "Forked from English Wikipedia at revision N", and a page followed through a redirect shows "(Redirected from …)" ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §5, [0054](../decisions/0054-forking-a-mirrored-page.md) §6, [0051](../decisions/0051-page-redirects.md) §2; the mirrored-page frame is [13](13-mirrored-pages.md)).

### 1.4 One subject per frame, and the Page data tab

*Sources: [0038](../decisions/0038-page-metadata-and-categories.md) §7; [0010](../decisions/0010-site-ui.md) §2; [0003](../decisions/0003-statement-ui.md) §2.*

**A frame's tabs show data only about what its identity line names.** An item's statements are never drawn in a page's frame, and a page's statements never in an item's, so a reader always knows which thing a statement is about.

**Pages and threads have statements too.** The same statement UI (§4) serves them, with the page as subject, in the Page data tab. **Page data** shows the page's resolved statements in the statement UI, with the page as subject. Projected statements carry a source chip ("From Category:…", "Set in reply …") and no edit controls. Their value menu offers **Remove category**, which opens the editor on the text, or for a status, a link to the thread. The page's categories ([10](10-pages-and-content-models.md)) are listed here too, read-only, with a link to edit the text.

**Crossing between a page and its item is a visible move.** A page paired with an item through a sitelink carries a subject link in its identity line, "About: Douglas Adams (Q42)", which opens the item in its own frame. The item's identity line carries "Article: Douglas Adams", which opens the page. The **About this page** panel (§3.1) keeps its role and gains the page-statement count.

### 1.5 Chips

*Sources: [0010](../decisions/0010-site-ui.md) §2; [0024](../decisions/0024-subsidiary-accounts.md) §9; [0022](../decisions/0022-federation.md) §11; [0056](../decisions/0056-security-model.md) §13; [0030](../decisions/0030-edit-filters.md) §10; [0086](../decisions/0086-theming.md) §2.*

Chips are the shared marks for kinds of thing. The set is kept small.

| Chip | Form | Meaning |
|---|---|---|
| Provider code | Monospace, one colour per provider, taken from the provider registry; in a dark variant the two colours swap, unless the registry gives dark ones ([0086](../decisions/0086-theming.md) §2) | The entity or row comes from that provider's graph |
| Local | Pill | Made on this instance |
| Namespace | Neutral tag | `Project`, `User`, `Special`, and so on |
| Job | Dark tag | A row or page that stands for an import job |
| Log | Outlined tag | A log action, not a content change |
| Corrected here | Outlined pill | A local correction of a mirrored value (§4.4) |
| Best value | As in §4.8 | |
| Public / Private | Pill, with a lock icon on Private | The visibility of an account setting (§1.1, principle 6) |

Later ADRs add marks of the same kind: the Bot chip on a subsidiary's user page ([0024](../decisions/0024-subsidiary-accounts.md) §9), the `verified` chip on a mirrored entity whose provider is verified ([0022](../decisions/0022-federation.md) §11), the **Restricted** pill with a lock icon ([0056](../decisions/0056-security-model.md) §13), the repository's origin chip (§1.3), filter tags as chips on the rows they mark ([0030](../decisions/0030-edit-filters.md) §10), and the source chips of projected page statements (§1.4) and of statements asserted by a non-dominant graph (§4.6).

## 2. Search, addresses and accounts

*Sources: [0010](../decisions/0010-site-ui.md) §3, §8, §10, §11, §12; [0018](../decisions/0018-tenants.md) §8; [0027](../decisions/0027-preferences-and-portability.md) §7; [0024](../decisions/0024-subsidiary-accounts.md) §9.*

### 2.1 Search

*Sources: [0010](../decisions/0010-site-ui.md) §3; [0029](../decisions/0029-resolver-namespaces.md) §7; [0066](../decisions/0066-lexemes.md) §7; [0082](../decisions/0082-source-form-and-the-shared-view.md) §2, §3.*

- **One box covers everything.** It searches entity labels, descriptions and aliases across every provider, domain keys ([04](04-entities-and-identifiers.md)), and page titles.
- **Suggestions are grouped by kind:** items and properties, foreign entities by type (for example "Sources and works"), domains, and pages. Each suggestion shows its label, its description, and its ID chip. The entity suggester and `Special:Search` take lexeme and part IDs ([0066](../decisions/0066-lexemes.md) §7).
- **An ID or domain key jumps straight to its page.** Input that parses as an ID, a keyed ID or a bare key of a keyed type gets a "Go to" option as the first suggestion, and Enter follows it ([0017](../decisions/0017-entity-id-grammar.md) §1, §4). So does a resolver key, such as `DOI:10.1000/xyz`, which lands on the item it resolves to or on a disambiguation page in the site frame. Parsing and resolution use the title resolver ([0008](../decisions/0008-namespaces-and-document-pages.md) §3; [10](10-pages-and-content-models.md)), which accepts any member of an identity cluster ([18](18-api.md) §1.4), so `WDQ42` and the local `Q9` it is clustered with land on the same entity page, shown in the site's local form (§4.2). Search results are one per cluster, the member the local preference selects. When a main-namespace page has the same title as the ID, it is offered as the next suggestion, "Page titled Q42" ([0038](../decisions/0038-page-metadata-and-categories.md) §8).
- **The full results page** is `Special:Search`, with MediaWiki's parameters ([0047](../decisions/0047-special-pages.md) §9). It also searches page text, through a second index queried together with the first ([0014](../decisions/0014-caches-and-search.md) §7; [03](03-storage-caches-and-search.md)).

**The disambiguation page** of a resolver ([06](06-statements-and-properties.md)) uses the site frame with the resolver's label and the key in the identity line, the external link, the candidate list, and the create action. A resolver namespace appears in the namespace selector wherever namespaces are listed, and its pages are excluded from `list=allpages`, since it has none ([0029](../decisions/0029-resolver-namespaces.md) §7).

### 2.2 Addresses

*Sources: [0010](../decisions/0010-site-ui.md) §12.*

MediaWiki's URL forms are kept so that links and tools keep working. **`docs/registry/special-pages.toml` is authoritative** for special pages: it lists each with its aliases, scope and status ([0047](../decisions/0047-special-pages.md) §1; [21](21-special-pages.md)). The table names the addresses the site UI relies on, and its rows are entries there.

| Page | Address |
|---|---|
| Any page | `/wiki/{title}` and `index.php?title={title}` |
| History | `index.php?title={title}&action=history` |
| Edit (document pages) | `index.php?title={title}&action=edit` |
| Content without the frame | `index.php?title={title}&action=render`, with `region` for part of it ([0057](../decisions/0057-web-tier.md) §8) |
| Purge | `index.php?title={title}&action=purge`, a confirmation form ([0057](../decisions/0057-web-tier.md) §14) |
| Diff | `Special:Diff/N` and `index.php?diff=…&oldid=N`, for local and mirror revisions alike ([0013](../decisions/0013-postgres-storage.md) §6, [0015](../decisions/0015-record-format-and-partition-registry.md) §2) |
| Links here | `Special:WhatLinksHere/{title}` |
| Recent changes | `Special:RecentChanges` |
| Contributions | `Special:Contributions/{name}` |
| Jobs | `Special:Jobs`, `Special:Jobs/{id}` (new) |
| Log in, first login | `Special:UserLogin`, `Special:CreateAccount` |
| Account | `Special:Account` (new) |
| Providers, consumers, pending subsidiaries | `Special:Providers` ([0022](../decisions/0022-federation.md) §11), `Special:OAuthConsumers`, `Special:PendingSubsidiaries` ([0025](../decisions/0025-oauth-server.md) §5) |
| Log | `Special:Log` |
| Groups, rights, blocks and protection | `Special:ListGroupRights`, `Special:UserRights`, `Special:Block`, `Special:BlockList`, `Special:ProtectedPages` ([0016](../decisions/0016-permissions-and-access-control.md) §7) |
| Watchlist, notifications | `Special:Watchlist`, `Special:Notifications` ([0020](../decisions/0020-change-feeds.md) §4, [0021](../decisions/0021-notifications.md) §6) |
| Edit filters, constraints | `Special:EditFilter`, `Special:EditFilterLog`, `Special:Tags` ([0030](../decisions/0030-edit-filters.md) §10), `Special:ConstraintReport` ([0031](../decisions/0031-property-constraints.md) §7) |

The pages later ADRs added are named in §6 with the feature that added them, and every one is an entry in the registry.

### 2.3 Logging in

*Sources: [0010](../decisions/0010-site-ui.md) §10; [0087](../decisions/0087-account-registration-and-challenges.md) §5, §6; [0088](../decisions/0088-external-identity-providers.md) §5, §6; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §5, §6, §7, §8, §13.*

**`Special:UserLogin`**

- **On a tenant**, offers one **Log in** button, a form posting to `/auth/login/start`, so it works without JavaScript. It takes the person to the auth host and back ([07](07-actors-and-accounts.md) §2.8), with no prompt when the auth host's session already holds a login the tenant admits.
- **On the auth host**, takes the login name and password, and offers a button for each issuer the auth host offers ([07](07-actors-and-accounts.md) §2.7), such as "Log in with Wikimedia", each a form posting to `/auth/{issuer}/start`. Reached from a tenant, it names the tenant ("Log in to Librarybase"), offers only the methods the tenant admits, and accepts that tenant's account name in place of the login name. It says what the instance learns from a provider: which account the user holds, and nothing else.
- An error the callback returns (`tserror`) is shown above the buttons.
- Explains temporary accounts, and says that IP addresses are never shown or published ([0007](../decisions/0007-actor-identity.md) §3).
- States that nobody else can see which service a user logs in with.

**The first visit** to a tenant continues to its `Special:CreateAccount`, where the user chooses the account's public name. The page asks for no password and shows no challenge, and creates the account with `createpreservestate`; a person who arrives there directly goes to the auth host first. **On the auth host**, `Special:CreateAccount` creates a login: a login name and a password, under the `createaccount` challenge below, or a login name for an identity an issuer has just authenticated. The login name is private, and the page says so.

**Name rules.** A local account name follows MediaWiki's username grammar exactly, so that imported attributions, CentralAuth names and Pywikibot round-trip: Unicode letters, digits and spaces; none of `@ # : / < > [ ] | { }`, control characters or leading, trailing or doubled spaces; at most 255 bytes; normalized to NFC with underscores read as spaces and the first letter uppercased. **Uniqueness is case-insensitive**, as the `actor_local_name` index of [0013](../decisions/0013-postgres-storage.md) §5.4 has it. **A name is never reused.** Every name a local account has ever held, whether renamed away or vanished from, stays reserved as a *name tombstone*, so that `[[User:OldName]]` in old page text or a post can never come to name a different person; the only way to have a name is to have always had it. Because a vanished account's names are erased from the log ([0007](../decisions/0007-actor-identity.md) §4), the tombstone is not a projection: it is a keyed hash of the lower-cased name in `private.name_tombstone` ([0013](../decisions/0013-postgres-storage.md) §5.6), written when a name is released, holding no actor key and no plaintext, and revealing nothing but that a candidate name is reserved, which the "name taken" check reveals anyway. `Special:CreateAccount` and `renameuser` refuse a tombstoned name with `ts-name-reserved`. The subsidiary conventions of [0024](../decisions/0024-subsidiary-accounts.md) §2 and [0025](../decisions/0025-oauth-server.md) §3 apply on top of this grammar, and the farm name registry of [0028](../decisions/0028-tenancy-policy.md) §2 is this rule applied across tenants, tombstones included ([08](08-tenants-and-instances.md)).

- **The name field starts empty.** The login name, and the identity provider's username where one authenticated the login, may be offered as buttons, each labelled with where it came from. Next to it, the page says that using the same name makes the two accounts easy to connect.
- **An optional, unchecked box** offers to link the user's account on the identity provider's wiki, for example Wikidata, once the linking flow of [0007](../decisions/0007-actor-identity.md) §7 is built; until then it is not shown ([0088](../decisions/0088-external-identity-providers.md) §6). It says the link is public. If it is checked, the link is created by the flow in [0007](../decisions/0007-actor-identity.md) §7. The authentication that just took place serves as the fresh proof of control, because it happened in the same request flow.

A farm signup page, on the auth host, states the linking consent of [0028](../decisions/0028-tenancy-policy.md) §2 (§6.11).

**`Special:CreateAccount` when the wiki guards or reviews registration** ([0087](../decisions/0087-account-registration-and-challenges.md) §5, §6; [07](07-actors-and-accounts.md) §2.6):

- **A challenge**, on the auth host's `Special:CreateAccount` where the instance default of `challenge.triggers` lists `createaccount`, is solved in a Web Worker from the moment the page loads, by a solver written in JavaScript and held to `scatter-challenge`'s test vectors, with its progress in a polite live region; the submit button waits only if the person is faster than the solver. `Special:UserLogin` does the same once `badlogin` applies to the caller, and the page editor before a save that would create a temporary account. There is nothing to see or hear, so it asks nothing of a reader using a screen reader, switch access or a magnifier beyond the wait.
- **Without JavaScript**, a `<noscript>` block, and the form's error if it is submitted, say what the wiki asks and why, that nothing is sent anywhere else, and what the person can do instead: turn JavaScript on for the site, ask an administrator to create the account, or, after failed logins, wait out the window (`ts-challenge-nojs`, and `ts-challenge-nojs-tempaccount` for an anonymous save, which adds that logging in avoids it; [20](20-web-tier.md) §3.1).
- **A honeypot field** is on the form in every configuration: an off-screen text input labelled "Leave this field empty", with `tabindex="-1"` and `autocomplete="off"`, outside the accessibility tree. The web tier drops a submission that fills it, answering as it answers a refused creation, without calling the API.
- **Under `account.approval`**, the page says that the wiki reviews new accounts and asks for a statement as `account.request_statement` says; `Special:RequestAccount`, ConfirmAccount's name for the form, is an alias of it. Submitting files a request, and the page says that the outcome is learned by logging in.

**`Special:ConfirmAccounts`**, restricted to `confirmaccount`, lists open requests oldest first with their names, statements and ages, and the address key to holders of `ts-viewip`; each is approved, or rejected with a reason the requester is shown. Holders of `confirmaccount` see the number of open requests in the account menu.

### 2.4 Account settings

*Sources: [0010](../decisions/0010-site-ui.md) §11; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §7; [0027](../decisions/0027-preferences-and-portability.md) §7; [0021](../decisions/0021-notifications.md) §6; [0024](../decisions/0024-subsidiary-accounts.md) §9; [0022](../decisions/0022-federation.md) §11; [0028](../decisions/0028-tenancy-policy.md) §11; [0085](../decisions/0085-settings-and-preferences.md) §5.*

`Special:Account` is visible only to its holder. It has these sections:

| Section | Label | Contents |
|---|---|---|
| Name | Public | The current name and **Rename…**. The page warns that user pages move and that links using the old name stop working ([0008](../decisions/0008-namespaces-and-document-pages.md) §6). |
| Sign-in methods | Private | A link to the auth host's `Special:Account`, where the login's sign-in methods are managed (below) |
| Linked accounts | Public | Each link, with its date and **Unlink…**. **Link another account** requires logging in to the provider again. The page warns that unlinking erases the link from history, and that copies taken earlier may keep it ([0007](../decisions/0007-actor-identity.md) §7). On a farm, the farm account and every linked tenant account are shown here, labelled Public ([0028](../decisions/0028-tenancy-policy.md) §11). |
| Contacts | Private | The email address with its verification state; the fediverse handle with its verification state, a *follow the notifier* hint with the notifier's handle, and *Remove* ([0021](../decisions/0021-notifications.md) §6; [0085](../decisions/0085-settings-and-preferences.md) §5) |
| Watchlist token | Private | The Atom watchlist token, shown and resettable ([0020](../decisions/0020-change-feeds.md) §4) |
| Subsidiaries | Public | The list of one's subsidiaries is Public, since the operator relation is; each subsidiary's **keys** are Private: label, grants, IP ranges, expiry, last used, Revoke, and Issue key, which shows the secret once with a warning. Retire and Rename sit with each subsidiary; Create subsidiary suggests `{Name}Bot` ([0024](../decisions/0024-subsidiary-accounts.md) §9) |
| Connected applications | Private | Each OAuth authorization one's subsidiaries hold, with **Revoke** ([0025](../decisions/0025-oauth-server.md) §5); and each upstream grant the instance holds for the account at a destination wiki, with **Revoke** ([0067](../decisions/0067-proposals.md) §6; [0085](../decisions/0085-settings-and-preferences.md) §5) |
| Fediverse | Public where it concerns actors, Private where it concerns keys | Make this account followable, the follower count, `rel="me"` links ([0022](../decisions/0022-federation.md) §11) |
| Your data | Private | **Download my data** and **Import data…**, with a plain account of what a bundle does and does not hold ([0027](../decisions/0027-preferences-and-portability.md) §7; [07](07-actors-and-accounts.md)) |
| Leave | — | **Vanish this account…**, with a plain account of what vanishing does ([0007](../decisions/0007-actor-identity.md) §4), including that copies of posts delivered to other servers may persist ([0022](../decisions/0022-federation.md) §6), and a pointer to Your data, suggesting a download first |

**The auth host's `Special:Account`** ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §7; [07](07-actors-and-accounts.md) §2.8) is the login's own page, visible only to its holder:

| Section | Contents |
|---|---|
| Login name | The private login name and **Rename…** |
| Sign-in methods | The password, with **Change** and **Remove**, and each external binding, with when it was added and last used and an issuer the auth host no longer offers marked unavailable. **Remove** is disabled for the last way to log in, and says why; removing erases the binding from history. **Add** lists the issuers the auth host offers, each a form to `/auth/{issuer}/start` with `purpose=link` ([07](07-actors-and-accounts.md) §2.7). Changing or removing a method logs out the login's other sessions |
| Accounts | Each tenant account bound to the login, with its tenant and a link to it; seen by the holder alone |
| Sessions | **Log out everywhere**, which ends every session of the login, on the auth host and on every tenant |

Preferences have a page of their own, `Special:Preferences` (§2.6), which the account page links to; the notification matrix is a tab of it ([0085](../decisions/0085-settings-and-preferences.md) §5).

### 2.5 Contributions

*Sources: [0010](../decisions/0010-site-ui.md) §8; [0018](../decisions/0018-tenants.md) §8; [0024](../decisions/0024-subsidiary-accounts.md) §9; [0040](../decisions/0040-instance-prerogatives.md) §7; [0079](../decisions/0079-derived-issuer-codes.md) §4.*

Contributions are per account ([07](07-actors-and-accounts.md)), and an account belongs to one tenant ([08](08-tenants-and-instances.md)). Three views follow:

| View | Shows |
|---|---|
| `Special:Contributions/{name}` on a tenant | That tenant's records attested by that tenant's actor: its `local`, `pages`, `log` and `actors` partitions |
| `Special:GlobalContributions/{name}` on a tenant | The same for each account **linked** to it ([0007](../decisions/0007-actor-identity.md) §7), on every tenant of the instance the viewer's tenant has opted into. Nothing is matched by name. |
| `Special:Contributions/{issuer}:{id}` for a foreign actor | **What `view.activity` holds for that actor:** page revisions imported with that attribution, and the rows of this tenant's log that name the actor. Upstream revision records in `log/{provider}` are not indexed by actor, so they are not listed, and the page says so: "This instance does not index Wikidata's revisions by user." The page says whose user this is, links to their page on the issuer's site, and does not fetch upstream, because per-actor fetching is unbounded. |

For an actor of another tenant on the same instance the third view is complete, since the instance holds every record and that tenant's `view.activity` indexes them. For a Wikidata account it is only what reached this tenant's activity rows, and says so. `/actor/{key}/contributions` serves the same three cases ([18](18-api.md)).

- **The header** shows the account's linked accounts, as links to their upstream pages. Only links the holder created are shown.
- **Jobs run for the user** appear as single rows, and can be filtered out.
- **A bot account** names its operator: a subsidiary's header names and links its operator, and a primary account's header lists its subsidiaries ([0024](../decisions/0024-subsidiary-accounts.md) §9).
- **Pages are addressed by current name.** Old names do not resolve ([0008](../decisions/0008-namespaces-and-document-pages.md) §6).
- **Contributions of `instance:{farm code}`** at a tenant list the instance acts on that tenant; at the farm base, for `ts-viewoperator`, every instance act on every tenant with the operator who carried it out ([0040](../decisions/0040-instance-prerogatives.md) §7; [08](08-tenants-and-instances.md)).

### 2.6 Preferences

*Sources: [0085](../decisions/0085-settings-and-preferences.md) §5; [0027](../decisions/0027-preferences-and-portability.md) §7; [0021](../decisions/0021-notifications.md) §6.*

`Special:Preferences` is the page of an account's preferences, served at each tenant base. It needs `editmyoptions`; a reader who is not logged in is offered **Log in** and pointed to the Appearance menu (§2.7).

- **Tabs** are the groups of `preferences.toml` ([07](07-actors-and-accounts.md) §8.1): Interface, Appearance, History, Watching, Notifications, Discussion and Statements. Interface holds the interface and label languages, the time zone and the date format; History holds the defaults for the upstream-edits fold (§3.6) and the mirror-sync filter (§3.9).
- **Each key** is drawn from its type, with its label and help, the messages `ts-pref-{key}` and `ts-pref-{key}-help`, and **Reset**. **Restore all defaults** resets every key, as `action=options&reset=1` does. A key that follows the farm account says that it applies on every wiki of the farm.
- **The Notifications tab** is the matrix of reasons by channel, read and written in place as the `notifications.*` keys, and the email digest ([0021](../decisions/0021-notifications.md) §6). The email address and fediverse handle stay on the account page, under Contacts (§2.4).
- **Saving** is `PATCH /account/preferences`. Without JavaScript the form posts to the web tier, which saves and answers 303, or re-renders the form with the API's errors ([20](20-web-tier.md) §3.1).

### 2.7 The Appearance menu

*Sources: [0085](../decisions/0085-settings-and-preferences.md) §6; [0086](../decisions/0086-theming.md) §4; [0094](../decisions/0094-accessibility-alternates.md) §5.*

Every page's global header has an **Appearance** button beside the account menu (§1.2), for every viewer. It opens a panel of the preference keys of the `appearance` group, as a disclosure that opens without JavaScript. The keys are colour mode, contrast, text size, colour vision, palette and type set, and what they change is §5.9.

- **Signed in**, the values are the account's preferences, and a change writes them through `action=options`. With JavaScript the change applies at once; without it the panel is a form posted to the web tier, which writes the preference and redirects back ([20](20-web-tier.md) §3.1).
- **Not logged in**, the panel offers the keys marked `browser`, and a change is kept in the browser's `localStorage` under one entry for the tenant's host. A small same-origin script, loaded blocking in `<head>` under an immutable hashed name, applies the stored values before the first paint, which `script-src 'self'` permits ([0034](../decisions/0034-frontend-stack.md) §11). **No cookie is set**, so an anonymous page stays `public` and its cache key does not vary ([20](20-web-tier.md)). Storage that is absent or refused leaves the defaults.
- **Not logged in and without JavaScript**, nothing can be kept, and the panel says that the page follows the device's settings.
- **At login**, when the account holds none of the keys the browser holds, the browser's values are written to the account, so a choice made before signing up survives it; otherwise the account's values win and overwrite the browser's copy. **At logout** the browser keeps the last values, so the page does not change under the reader.
- **While signed in**, the page mirrors the account's values into the browser's storage, and the head script applies stored values only where the server rendered none, so the account's values win while signed in and remain after logout.
- **On a farm**, browser storage is per host, so a reader's choices on one tenant do not reach another; a signed-in reader's keys that follow the farm account do ([07](07-actors-and-accounts.md) §8.1). Colour mode, contrast, text size and colour vision follow the farm account; palette and type set are per wiki unless the reader marks them **Use on every wiki** (§5.9).

### 2.8 Wiki and instance settings

*Sources: [0085](../decisions/0085-settings-and-preferences.md) §4; [0028](../decisions/0028-tenancy-policy.md) §11; [0046](../decisions/0046-primary-tenant.md) §9; [0056](../decisions/0056-security-model.md) §13.*

**`Special:WikiSettings`**, at a tenant base, shows the tenant's settings, and **`Special:InstanceSettings`**, at the farm base, the instance's. Both are built from the registry and `GET /settings` ([23](23-configuration-and-registry.md) §3.1; [18](18-api.md) §3.2), with no code per setting:

- **Tabs** are the registry's groups. Each row shows the label and help (`ts-setting-{key}`), the value with a control drawn from its type, an origin chip (*Default*, *Instance default*, *Set on this wiki*, *Locked by the instance*, *Capped at N*), **Reset** to inherit again, and a link to the setting's history in the log.
- **Anyone who may read the tenant may read them.** A viewer without `ts-config`, or the entry's stronger right, sees the values without controls.
- **Saving** sends one `PATCH /settings` per tab with a summary field. Without JavaScript the tab is a form posted to the web tier, which answers 303 or re-renders the form with the API's errors ([20](20-web-tier.md) §3.1).

`Special:WikiSettings` also has a **Visibility** tab (§6.3), which writes the tenant's `read` ACL through the ACL routes rather than `PATCH /settings` and which the instance may lock under `config.template = locks` ([09](09-security-and-moderation.md) §4.7), a **Public files** list of every site asset the wiki exposes, with the setting that names each ([12](12-files-and-media.md) §4.5), and a read-only **Tenancy** tab: the instance's policy as it applies to this wiki, meaning the preset, the switches, the locked keys and the ceilings, with **Accept** for an `owner` of a tenant offered the primary role ([0046](../decisions/0046-primary-tenant.md) §9). `Special:InstanceSettings` also has a **Tenancy** tab with the tenancy switches, and **Defaults for wikis**: every tenant setting with its registry default, the instance's default and, under `config.template = locks`, a lock; changing a default says how many tenants inherit it and so will change. That tab is absent under `config.template = none`.

`Special:Tenancy` redirects to the Tenancy tab of `Special:WikiSettings` at a tenant base and of `Special:InstanceSettings` at the farm base. On a single-tenant instance, whose farm base is the primary tenant's base ([08](08-tenants-and-instances.md) §1.1), both pages are served on the same host.

## 3. Document pages, history, diffs, activity and jobs

*Sources: [0010](../decisions/0010-site-ui.md) §4, §5, §6, §7, §9; [0023](../decisions/0023-moderation.md) §9.*

### 3.1 Reading a document page

*Sources: [0010](../decisions/0010-site-ui.md) §4; [0042](../decisions/0042-template-expansion-and-parsoid.md) §15; [0038](../decisions/0038-page-metadata-and-categories.md) §7; [0060](../decisions/0060-scopes.md) §8; [0067](../decisions/0067-proposals.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6.*

**Read**

- Renders the page's content model ([0008](../decisions/0008-namespaces-and-document-pages.md) §5, §8; [10](10-pages-and-content-models.md)).
- **Syntax outside the wikitext subset stays visible:**
  - Templates are expanded before rendering; only constructs outside the subset render as chips, and a missing template is a red link ([0042](../decisions/0042-template-expansion-and-parsoid.md) §7, A19; [11](11-rendering-templates-and-modules.md)).
  - Categories are listed at the foot of the page as links to their category pages, with hidden categories collapsed ([0038](../decisions/0038-page-metadata-and-categories.md) §3).
- **Links to entities** render with the entity's label.
- **An "About this page" panel** shows the last edit, the revision count by origin (imported, bot, local), where the page came from, the content model, the backlink count, and the page-statement count ([0038](../decisions/0038-page-metadata-and-categories.md) §7). The panel is its own API response with its own short `max-age`, independent of the page's or entity's `ETag`, because its numbers change when referrers, scopes and proposals change and the subject does not; the counts are counter columns (`ref_count`, the revision counts) maintained in batches by the projections ([03](03-storage-caches-and-search.md) §4.2), never counted on view, and a count above a threshold is shown as "10,000+"; with expansion on, also "Templates used" (`prop=templates`) and the limit report, and on a Template page "Pages that use this template" (`list=embeddedin`) ([0042](../decisions/0042-template-expansion-and-parsoid.md) §15). On a fork it also shows the revisions imported, the dependencies copied, whether files were copied, how many newer revisions upstream has, **Compare with upstream** and **Copy files used by this page** ([0054](../decisions/0054-forking-a-mirrored-page.md) §6–7), and gains the base revision (original or last merged), **Merge from upstream**, **Propose to {repository}**, **Re-follow upstream** under `ask`, and the dependency notice of [0068](../decisions/0068-merging-with-upstream.md) §2; its "{n} newer revisions" line reaches `Special:MergeUpstream/{title}` (§6.10). A page's heading is its `displaytitle` where one is set, and a disambiguation page is marked as one ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6).

**The entity page has the same "About this page" panel**, beside its "Where this comes from" panel (§4.6). On an entity page and a document page alike the panel shows the scopes the subject is in, "In 3 scopes", from `GET /subject/{kind}/{id}/scopes` ([0060](../decisions/0060-scopes.md) §8), and on an entity page the proposal count, "2 proposals: 1 adopted, 1 offered" ([0067](../decisions/0067-proposals.md) §7).

### 3.2 Editing a document page

*Sources: [0010](../decisions/0010-site-ui.md) §4; [0042](../decisions/0042-template-expansion-and-parsoid.md) §15; [0034](../decisions/0034-frontend-stack.md) §7.*

**Edit**

- **The source and a live preview sit side by side.** The preview runs the server's own renderer compiled to WebAssembly (`scatter-wasm`, §5.4). When the source contains template calls, parser functions, variables or tags that the wasm renderer cannot render, the editor previews through `action=parse` with `pst` and `preview`, debounced, under the `parse` rate class ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16). Pages without such constructs keep the in-browser preview.
- **A Format selector** changes the content model (`action=changecontentmodel`). It, and the model shown beside the title, appear only for text models, and the selector only where the namespace allows more than one model ([0041](../decisions/0041-content-models.md) §10; [10](10-pages-and-content-models.md)).
- **Link autocomplete** opens after `[[` and suggests titles and entities in any namespace, through the title resolver. For an entity, it inserts the link and the entity's label.
- **Insert template…** searches Template titles through the stack and builds a parameter form from the template's TemplateData; a parameter popup lists a template's parameters when the cursor is inside a call ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5; [11](11-rendering-templates-and-modules.md)). **The Edit tab of an inherited page** opens the editor on the upstream wikitext with the fork banner, and saving forks ([0054](../decisions/0054-forking-a-mirrored-page.md) §2; [13](13-mirrored-pages.md)).
- **Saving** asks for a summary and a minor-edit flag, and sends the base offset ([0008](../decisions/0008-namespaces-and-document-pages.md) §4). On a conflict, the editor keeps the user's text and marks only the lines that clash.

The editor component itself is §5.5.

### 3.3 What a history covers

*Sources: [0010](../decisions/0010-site-ui.md) §5, §5.1; [0039](../decisions/0039-files-and-media.md) §18.*

**One backing.** The history of an entity is read from `view.entity_history (tenant, canonical_id, time, partition, offset, kind, member)`, which composition writes ([03](03-storage-caches-and-search.md); [16](16-logs-feeds-and-notifications.md) §3.2): one row per record in any source partition keyed to the entity or a member of its cluster, and one per upstream log event keyed to a member, each with its kind and the member it is keyed to. The page is one keyset query over that index, bounded by the entity's own record count, served by `GET /entity/{id}/history` ([18](18-api.md) §3.2); nothing is merged from `log.record` or `view.activity` at request time, and upstream log events are not activity rows.

The history of an entity page lists every record whose key is:

- the entity itself; or
- any member of its identity cluster ([04](04-entities-and-identifiers.md)), with a chip showing which member.

It covers records in every source partition:

- local change sets;
- mirror records;
- log events such as retention changes, `convert`, redirects and `erase`.

A document page's history lists its records in the `pages` partition, from `view.activity`. A file page's interleaves uploads with text and statement revisions, each row labelled by kind ([0039](../decisions/0039-files-and-media.md) §18).

### 3.4 History rows

*Sources: [0010](../decisions/0010-site-ui.md) §5.2; [0023](../decisions/0023-moderation.md) §9; [0040](../decisions/0040-instance-prerogatives.md) §7.*

| Row | Shown as |
|---|---|
| Local edit | One row: time, Local chip, parsed summary, tags, size change, and actor. It links to its diff. |
| Mirror sync | One row per observed state: time, provider chip, "Synced from Wikidata", the upstream revision, the job, and a one-line account of what changed. It links to its diff. |
| Upstream edit | Folded inside the sync that brought it in (§3.6), with the upstream actor linked to their upstream IRI ([0007](../decisions/0007-actor-identity.md) §2). |
| Erased record | A gap row showing only its time and source: "An edit was erased." Its reason class is shown to holders of `ts-viewerasures` ([0016](../decisions/0016-permissions-and-access-control.md) §6). |
| Hidden actor | The row, with "(username hidden)" in place of the name. |
| Log event | A Log chip and a sentence describing the effect. It has no diff. |
| Instance act | The instance operator as its actor, an **Instance action** label, the public reason and a link to `Special:InstanceAction/{partition}/{offset}` at the farm base ([0040](../decisions/0040-instance-prerogatives.md) §7) |

**Rows are identified by time and source,** and may show revision numbers: local and mirror records have global revision IDs ([0013](../decisions/0013-postgres-storage.md) §6, [0015](../decisions/0015-record-format-and-partition-registry.md) §2; [03](03-storage-caches-and-search.md)).

**Hiding and patrol.** History rows carry RevisionDelete checkboxes for holders of `deleterevision`, and a suppress option for holders of `suppressrevision`, opening a dialog with the three parts and a reason; a hidden part is drawn in place and labelled, as a hidden actor is, never silently absent. The patrol marker and **Mark as patrolled** appear for holders of `patrol` ([0023](../decisions/0023-moderation.md) §9; §6.3).

**Linked accounts are marked.** An upstream actor that a local user has linked ([0007](../decisions/0007-actor-identity.md) §7) carries an "also {name} here" badge. Unlinked accounts are never matched by name.

**Instance acts** appear in feeds as in history. `Special:InstanceAction/{partition}/{offset}` shows the authority record's kind, time, public reason and, where the kind allows, its content; the operator's own name only to `ts-viewoperator` ([0040](../decisions/0040-instance-prerogatives.md) §7; [08](08-tenants-and-instances.md)). Notifications that an act triggers on a guest (a rename, a block) name the instance operator. The log events projected from instance acts are [16](16-logs-feeds-and-notifications.md); their RDF is [02](02-graphs-rdf-and-query.md).

### 3.5 The source switch, and mirror states the instance did not keep

*Sources: [0010](../decisions/0010-site-ui.md) §5.3, §5.4.*

- **A switch above the list** offers All sources, Made here, and one entry per mirror that contributes to the entity. The default is All sources.
- **It is hidden when only one source contributes.** A purely local item's history then looks like MediaWiki's.

Under the `latest` history policy ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §2; [05](05-providers-and-ingest.md)), only the newest mirrored state exists. The history says so in its side panel ("Only the latest state is kept here"), and it offers two actions:

- a link to the full history on the provider;
- **Keep full history here…**, which sets `retain` and starts the upstream backfill ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5).

Under `full`, or for a retained entity, every observed state is a row of `view.entity_history` (§3.3), and backfilled upstream revisions are rows of it too.

The foot of the list states where history on this instance begins, for example "first mirrored on {date}; that state was replaced by later syncs".

### 3.6 Upstream edits, fetched live

*Sources: [0010](../decisions/0010-site-ui.md) §5.5, §13.*

- **A checkbox shows or hides upstream edits** inside each sync: "Show Wikidata's own edits inside each sync". A user preference sets its default.
- **Where the instance has not backfilled the upstream revisions a sync covers, the server fetches them from the provider's API.** It uses the endpoint in the provider registry, and the interval runs from the previous observed upstream revision to the current one.
- **The results are cached briefly and rate-limited, and never written to the log.** The fold is labelled "fetched from wikidata.org, not stored here".
- **If the provider cannot be reached,** the fold says so. The rest of the history is unaffected.
- **Providers that publish no revision history,** such as OpenAlex, get no fold.

The upstream history fetch is a server component. It reads the provider registry's API endpoint and reuses the adapter's parsing of upstream revisions. Its cache is operational state, not log data ([0010](../decisions/0010-site-ui.md) §13; [05](05-providers-and-ingest.md)).

### 3.7 Comparing, and narrower histories

*Sources: [0010](../decisions/0010-site-ui.md) §5.6, §5.7.*

- **Compare works within one source.** A reader can compare two local revisions, or two observed states of a mirror that keeps them.
- **The resolved view at a past time cannot be compared.** It depends on mirror states that compaction may have dropped. This is why the compare controls appear only on rows whose source keeps history.
- **A comparison between two local revisions** shows the change in the local graph's own assertions, drawn as in §3.8.

Two narrower histories exist:

- **"History of this value"** in a value's menu (§4.8) opens the same list, filtered to one statement ID.
- **A log view** lists only log events for the entity.

### 3.8 Diffs

*Sources: [0010](../decisions/0010-site-ui.md) §6.*

**The unit** is either one change set or two states of one source (§3.7).

**Diffs are drawn as statements.**

- Changed values are shown in their group's shape (§4.3), with the unchanged values on either side for context, and a "Show all" link.
- Changed cells carry insertion and deletion marks.
- A rank change is drawn as "Normal → Deprecated". An added qualifier is marked as new.
- Terms and sitelinks are diffed as keyed lists.
- A document page gets a line diff of its source, as in MediaWiki.

**Alternative views.** A switch offers JSON (a diff of the canonical JSON) and Change set (the record's operations as stored).

**"What this changes"** states the effect in three places:

- **Readers here:** what the resolved view now shows.
- **Queries:** whether the truthy triples changed.
- **Upstream:** whether anything was written upstream. For a correction, the answer is no, and the page offers "Report to Wikidata…".

**"About this edit"** lists:

- the graph written to;
- the statement ID;
- the size change;
- the log checkpoint that includes the record, with a link to get an inclusion proof ([0006](../decisions/0006-log-integrity-and-erasure.md) §9; [01](01-log-and-records.md)).

**Actions.** Undo appends the inverse change set, with the record as its base. The structured diff the page draws from is the API's ([18](18-api.md)).

### 3.9 Recent changes

*Sources: [0010](../decisions/0010-site-ui.md) §7; [0023](../decisions/0023-moderation.md) §9; [0030](../decisions/0030-edit-filters.md) §10; [0095](../decisions/0095-the-interface-namespace.md) §3.*

**Row kinds:**

| Kind | One row per |
|---|---|
| Item and property edits | Local change set |
| Page edits | Page record |
| Jobs | Local job, however many entities it touched |
| Log actions | Log event: links, erasures, retention, account creation |
| Mirror syncs | Sync job run, never per entity |

- **Filters** toggle each kind, and the `patrolled` filter shows patrol state, with the `!` marker for holders of `patrol` ([0023](../decisions/0023-moderation.md) §9). Mirror syncs are off by default. While they are off, a strip above the list summarises running and recent syncs, and offers to show them.
- **"Group by page"** collapses consecutive edits to the same page into one row, with a count.
- **The time window is configurable.** The default follows MediaWiki's `$wgRCMaxAge`, 90 days.
- **Every feed has an Atom form and a stream** beside the page ([0020](../decisions/0020-change-feeds.md) §4; [16](16-logs-feeds-and-notifications.md)).
- **A refused write** shows the edit filter's public message in place, with the resubmit control for `warn`, and filter tags are chips on the rows they mark ([0030](../decisions/0030-edit-filters.md) §10).

**The Action API stays per revision.** `list=recentchanges` and `list=usercontribs` return local changes one revision at a time, as MediaWiki clients expect ([18](18-api.md)).

- **Accounts and the instance** ([0095](../decisions/0095-the-interface-namespace.md) §3). An account that is a row's target is marked with a person icon, *Account* to a screen reader, before its name, which links to its contributions, so it does not read as its user page. The name is the username, else the wiki's name for its key (`Interface:Name/{key}`), else the key; the key is shown beside it only for an account of another wiki that has no name here. The instance's own key, `instance:{farm code}`, is a **Server Operator** badge wherever it acted. Entities keep their IDs beside their labels.
- **Changes made by a job** carry the bot flag and a `job:{id}` change tag, so `rcshow=!bot` hides them.
- **Mirror records are not in `list=recentchanges`.** Each mirror job run appears in `list=logevents` under the log type `job`.

### 3.10 Jobs

*Sources: [0010](../decisions/0010-site-ui.md) §9; [0018](../decisions/0018-tenants.md) §11; [0034](../decisions/0034-frontend-stack.md) §4.*

`Special:Jobs` lists running and recent jobs. Mirror sync jobs are instance jobs, attested by the primary tenant's actors and listed at the farm base; local bulk jobs are tenant jobs ([0018](../decisions/0018-tenants.md) §11; [08](08-tenants-and-instances.md)). `Special:Jobs/{id}` is a job's page, with these parts:

- **Identity line:** the Job chip, the job ID, the graph it wrote to, and its status.
- **Header:** the actor and the account it ran for, the start and end times, the mode, and the actions.
- **Counts:** created, added to (found by match key), unchanged, and rejected. Each count links to a filtered Changes tab.
- **Rejected records:** the line, the match key and the reason for each, and a link to download the rejects file ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.5; [05](05-providers-and-ingest.md)).
- **Where it came from:** the source and its version, the adapter and its version, the match key, the graph, the checkpoint and inclusion proof, and projection lag ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3).
- **A running job** shows progress, and how far the indexes are behind; the progress bar is an interactive component (§5.3).
- **A `snapshot` job** shows how many entities it removed, next to the safety threshold ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.4).

**Actions:**

- **Revert this job…** (local jobs) appends the inverse change sets as a new job ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3). Where someone has edited an affected entity since, that later edit is kept, and those entities are listed first for review.
- **Sync again** (mirror jobs) is the mirror's form of a revert.
- **Run again** repeats a local job with the same parameters. Match keys make this idempotent.

## 4. The statement UI

*Sources: [0003](../decisions/0003-statement-ui.md) §1, §2, §3, §4, §5, §6, §8, §9; [0034](../decisions/0034-frontend-stack.md) §3; [0031](../decisions/0031-property-constraints.md) §7.*

### 4.1 Principles

*Sources: [0003](../decisions/0003-statement-ui.md) §1.*

1. **Show the exception, not the rule.** Rank, source graph, shared qualifiers and shared references are drawn only where they differ within a statement group. Anything that is the same everywhere is stated once, or not at all.
2. **Name the effect, not the mechanism.** UI text says "Best value", "Deprecated" and "Corrected here". Data-model terms such as rank, `PreferredRank` and graph appear in tooltips, the API and exports.
3. **The data picks the layout.** A group's shape (§4.3) is computed from its statements, so the same property can be a row of chips on one item and a table on another. Editors can override the result, but the override is a view preference, not data.
4. **Deterministic and testable.** Given the same entity JSON and role map ([06](06-statements-and-properties.md)), shape detection always returns the same shape. This means the classifier can be run over a Wikidata dump and its results measured.
5. **Nothing is lost.** Every value that is folded away or hoisted stays one click away. Every shape has a plain list fallback that shows every statement, qualifier and reference.

### 4.2 The UI reads canonical Wikibase JSON

*Sources: [0003](../decisions/0003-statement-ui.md) §2; [0082](../decisions/0082-source-form-and-the-shared-view.md) §3, §6.*

The UI takes three inputs:

- the entity's canonical JSON ([wikibase-compat.md](../api/wikibase-compat.md) §3);
- the labels of the entities and properties it links to;
- the data type of each property.

**The UI asks for the local form.** Every request it makes carries `prefer=local` ([18](18-api.md) §1.6), so a value whose cluster has an exact local match is shown and linked as `Q9` rather than `WDQ5`, and an ID with no local match is shown as stored; the stored graph is never rewritten, and a reader who follows a link in either form reaches the same entity ([18](18-api.md) §1.4). **Labels are fetched by cluster**, through the bulk-labels route of [18](18-api.md) §3.2 in one call per page: a value shown under one member takes its label from whichever member's rows have one, in the order of the instance's policy, so a local item clustered with a Wikidata item is labelled even where the local graph has no label.

A stock Wikibase serves all three, so the same frontend can drive Triplespace or any Wikibase, Wikidata included. Information that only Triplespace has comes from a separate provenance response (§4.6). The canonical JSON stays the same as Wikibase's, as [0001](../decisions/0001-revision-metadata-rdf.md) §3 requires ([06](06-statements-and-properties.md)).

A **statement group** is all the statements for one property on one entity, after reconciliation ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §3; [05](05-providers-and-ingest.md)).

Pages and threads have statements too, and the same UI serves them with the page as subject, in the Page data tab (§1.4); a lexeme's forms and senses are served the same way, with the part as subject (§6.8).

### 4.3 Shape detection

*Sources: [0003](../decisions/0003-statement-ui.md) §3.*

Each statement group gets exactly one shape. The rules are checked in order, and the first one that matches wins.

Terms:

- **V** is the set of statements in the group with normal or preferred rank. Deprecated statements are handled separately (§4.4).
- **n** is the number of statements in V.
- **coverage(q)** is the fraction of statements in V that have at least one qualifier with property q.
- **Roles** such as `time-point` and `time-start` come from the role map ([0003](../decisions/0003-statement-ui.md) §7; [06](06-statements-and-properties.md)).

| Order | Shape | Matches when | Renders as |
|---|---|---|---|
| 1 | Single | n = 1 | The value as a headline, with its qualifiers inline as "key: value" |
| 2 | Timeline | n ≥ 2 and coverage(`time-start`) ≥ 0.8 | Rows on a shared time axis, newest first; the best value leads |
| 3 | Series | n ≥ 4, the main value is a `quantity`, and coverage(`time-point`) ≥ 0.8 | The best value as a headline, a line chart, and a switch to a table |
| 4 | Table | n ≥ 3 and some qualifier q has coverage(q) ≥ 0.8 | One column per qualifier key; a Matrix view where allowed (below) |
| 5 | Chips | No statement in V has qualifiers, and the main value is a `wikibase-item`, `string` or `external-id` | Chips that wrap onto new lines |
| 6 | List | Anything else | One row per statement, with its qualifiers inline beneath |

**Table columns.**

- Every qualifier key with coverage ≥ 0.5 becomes a column, in the order the keys first appear.
- A statement with no value for a column leaves that cell blank.
- Qualifier keys below 0.5 coverage are listed under the row as extra qualifiers.
- A key with the same value on every statement is hoisted instead (§4.5).

**Numbered lists.** A Table whose only column is the `series-ordinal` role, such as an article's authors by their position, is drawn as a list numbered by that column rather than as a one-column table.

**Row order.** Rows are sorted by their column values, from left to right. Numeric strings, such as series ordinals, compare as numbers, so 2 comes before 10. When a column's value repeats down consecutive rows, only the first row of the run shows it. The hidden cells still carry the value as accessible text.

**Matrix.** A Matrix view is offered when all of these hold:

- the table has exactly two columns;
- each column has 12 or fewer distinct values;
- no two statements share the same pair of values.

In the Matrix, a cell with no statement reads "no value".

**Large groups.** When n > 25, the first 10 values are shown inline, followed by "Open full view". The full view is a page for that one property on that one entity, with paging, sorting and filtering. Because an item is a view and not a page, the full view is just another query.

**Overrides.** An editor can pin a different shape for a property, for the whole instance or for one entity. The pin is stored as a view preference, never as a statement. A viewer's own pin is the `shapes.pins` preference, and a tenant-wide pin is a `view-pin` config record ([0027](../decisions/0027-preferences-and-portability.md) §5; [07](07-actors-and-accounts.md), [23](23-configuration-and-registry.md)).

### 4.4 Rank

*Sources: [0003](../decisions/0003-statement-ui.md) §4.*

Rank is shown only when it changes what a reader or a query sees.

| Ranks in the group | What is drawn |
|---|---|
| All normal | Nothing |
| One or more preferred | Preferred values lead the group, with a "Best value" badge. In the Single, Chips and List shapes, the other values fold under "N other values". In the Table and Series shapes they stay in place, and the preferred row carries the badge. |
| Some deprecated | They are taken out of V and folded under "N deprecated". Each is struck through and labeled with its `deprecation-reason` qualifier. |
| Deprecated only | The group reads "No current value", with the deprecated values expanded beneath. |

- The "Best value" badge has a tooltip: "Queries and infoboxes return only this value". This is what truthy triples mean in practice.
- Rank is set from each value's menu, through two items:
  - **"Make this the best value"**, which reads "Also a best value" when a preferred value already exists;
  - **"Mark as deprecated…"**, which asks for a reason and saves it as the `deprecation-reason` qualifier.
- A local rank override on a mirrored statement carries a "Corrected here" chip ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §7; [05](05-providers-and-ingest.md)). Expanding the chip shows:
  - the upstream value and its rank;
  - the ranks as exported;
  - two actions, "Report upstream" and "Undo correction".

  The chip also has a *redundant* state, for when upstream now agrees with the correction, and a *dangling* state, for when upstream has removed the statement the override points at.

### 4.5 Shared qualifiers and references

*Sources: [0003](../decisions/0003-statement-ui.md) §5.*

**Shared qualifiers.** When n ≥ 2 and a qualifier key has coverage 1.0 with the same value on every statement, the key is taken out of the rows. It is shown once, under "Same for all N", in the group footer. For the Series shape, it goes in the side panel instead.

**References** are compared by the `hash` that canonical JSON gives each reference.

- If one reference appears on every statement in V, the group's footer says so once, "Cited by all N values", with its marker, and the rows do not repeat it.
- **A tab's references are its endnotes** ([0003](../decisions/0003-statement-ui.md) A17). Each distinct reference that the tab's groups cite, a deprecated value's included, is numbered once, in the order it first appears down the tab, and listed under **References** at the foot of the tab, with the groups that cite it ("Cited for …", each a link to its group). Each row shows the numbers of its references as markers linking to them, so a reference that many groups cite is written out once.
- A group rendered alone (the `statements/{P}` region) carries the numbers the whole tab gives it, and the editor swaps in the whole tab after a save, since a new reference renumbers the groups after it.
- Statements with no reference carry no marker. The footer counts them, for example "2 values have no source".

### 4.6 Provenance

*Sources: [0003](../decisions/0003-statement-ui.md) §6.*

Canonical JSON does not say which graph asserts a statement. Triplespace serves that separately, as a **provenance response** keyed by statement ID ([18](18-api.md)). For each statement it gives:

- the source graphs that assert it;
- whether it is overridden, and by what;
- its correction state (§4.4);
- for a fused statement, every member statement and the graph it comes from ([0004](../decisions/0004-identity-clusters-and-equivalence.md) §8; [04](04-entities-and-identifiers.md));
- its constraint violations ([0031](../decisions/0031-property-constraints.md) §6; [06](06-statements-and-properties.md)).

Where there is no provenance response, as on a stock Wikibase, the UI draws no provenance.

Where there is one:

- The item header names the **dominant source**, the graph that asserts the most statements. The header also has a toggle between the resolved view, each mirror graph on its own, and the local graph only.
- A statement gets a source chip (`WD`, `OA` or "Local") only when its graph is not the dominant one. A statement asserted by two graphs shows the dominant one, and the chip's hover lists both.
- A "Where this comes from" panel lists, for each graph:
  - how many statements it asserts;
  - when it last synced, and the upstream revision it holds, for a mirror; the revision links to its page upstream where the provider is a MediaWiki that publishes revision IDs ([0003](../decisions/0003-statement-ui.md) A17);
  - the entity's retention policy ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §5);
  - the actions to change retention or convert the entity to a local one ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §6).

### 4.7 Editing

*Sources: [0003](../decisions/0003-statement-ui.md) §8; [0084](../decisions/0084-wikibase-writes-against-the-resolved-view.md) §3, §5.*

Editing happens inside the group, and the group keeps its shape. Every action maps to an existing Wikibase write, so the same UI can edit a stock Wikibase.

| UI action | Behavior | Write |
|---|---|---|
| Add value (Table) | A new row appears with one input per column. The first column carries over the value from the row above. A "Same as the other values" checkbox attaches the shared reference and is checked by default. | New statements in one `wbeditentity` |
| Paste rows | Tab-separated rows map to the columns in order. Each cell is parsed as its column's data type. | `wbparsevalue` for each cell, then one `wbeditentity` |
| Save N values | Every pending row is saved as one revision, with an automatic summary. | One `wbeditentity` |
| Make this the best value | Sets the rank to preferred | `wbsetclaim` |
| Mark as deprecated… | Asks for a reason, then sets the rank to deprecated and adds the reason qualifier | `wbsetclaim` |
| Add a qualifier column | Adds an empty column. The qualifier is written only to statements whose cell is filled in. | `wbsetqualifier` for each filled cell |
| Correct a mirrored value's rank or a mirrored term | Writes a local-graph override; the mirrored statement is untouched | Triplespace `override` ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.2; [05](05-providers-and-ingest.md); [18](18-api.md) §2.5) |
| Replace a mirrored value | A value change on a mirror-owned statement is refused by the API with `ts-foreign-statement` ([18](18-api.md) §2.5). The editor never shows the refusal: editing the value of a statement whose source chip is a mirror's opens as **Replace here**, one action that suppresses the mirrored statement and adds the local one, with the new statement's GUID, and the row keeps its place with a "Corrected here" chip (§4.4). **Propose upstream** sits beside it (§6.10) | `override{suppress}` plus `add` in one change set |

- **Keyboard.** Tab moves between cells, Enter adds another row, and Escape cancels the row being edited. Every input is labeled with its column's property.
- **Conflicts.** Each save carries `baserevid`, which is the entity's newest local revision ([18](18-api.md) §1.3). A mirror advancing since the edit began is never a conflict. If the local graph has changed, the pending rows are applied to the current data as the API patches around a stale base (`wikibase-conflict-patched`, [18](18-api.md) §1.5), and only the cells that clash are marked. The rest of the save goes through.

Statement editing requires JavaScript, as on Wikidata (§5.3).

### 4.8 Components

*Sources: [0003](../decisions/0003-statement-ui.md) §9; [0031](../decisions/0031-property-constraints.md) §7.*

The shapes are arrangements of twelve shared components, not separate widgets.

| Component | Contents |
|---|---|
| Property row | The property label (a link), the value count, and the group card |
| Group card | A header with the sort note, view switch, Add value button and menu; a body holding the shape; a footer with hoisted qualifiers, the reference every value cites, and the count of values without a source |
| View switch | A segmented control listing only the shapes this group allows; hidden when only one applies |
| Value cell | The value, formatted by data type; quantities use tabular numerals |
| Qualifier column | A header with the qualifier property's label, and a cell for each row |
| Footnote marker | A numbered pill linking to its entry in the tab's References (§4.5) |
| Best value badge | A check icon and "Best value", with the tooltip from §4.4 |
| Fold | A disclosure button: "N other values" or "N deprecated" |
| Source chip | The provider code in monospace, or "Local" |
| Correction chip | Collapsed, expanded, redundant and dangling states (§4.4) |
| Constraint marker | An icon by severity beside a violating value. Its popover names the constraint, its source, its clarification text and the parameter that failed. Nothing is drawn where there is no violation ([0031](../decisions/0031-property-constraints.md) §3) |
| Value menu | Edit; Make this the best value; Mark as deprecated…; Copy statement ID; History of this value |

The constraint marker sits in the value cell and follows the rule that nothing is drawn where nothing differs: an entity with no violations looks as it did ([0031](../decisions/0031-property-constraints.md) §7; §6.4). The visual tokens (colour, type, spacing) come from the theme (§5.1).

### 4.9 Statement groups are rendered on the server

*Sources: [0034](../decisions/0034-frontend-stack.md) §3.*

- The layouts of §4.3 (table, matrix, chart, timeline, chips, "Same for all N", footnoted shared references) are rendered **only on the server**.
- Tables use Codex's CSS-only Table.
- Charts, sparklines and timeline strips are **SVG generated in Rust** from the same view model the classifier produces. They cache like any other markup and need no JavaScript. No client charting library is used for these.
- The shape classifier lives in a pure, wasm-capable crate, so an editor can predict the layout a change will produce (§5.4).

## 5. The frontend stack

*Sources: [0034](../decisions/0034-frontend-stack.md) §1, §2, §4, §6, §7, §9, §10, §13; [0010](../decisions/0010-site-ui.md) §13; [0003](../decisions/0003-statement-ui.md) §10.*

### 5.1 Principles

*Sources: [0034](../decisions/0034-frontend-stack.md) §1; [0086](../decisions/0086-theming.md) §1.*

1. **The server renders every page.** Reading never needs JavaScript. Rendered pages are cacheable at the L2 layer ([0014](../decisions/0014-caches-and-search.md) §3; [03](03-storage-caches-and-search.md)).
2. **JavaScript enhances regions, never whole pages.** An interactive component takes over one region of a server-rendered page. There is no client-side router.
3. **One renderer per thing.** Anything the server draws is drawn only by the server. A component that changes it fetches the server's rendering afterwards ([0034](../decisions/0034-frontend-stack.md) §5; [20](20-web-tier.md)) rather than drawing its own copy.
4. **Codex components, themed by tokens.** Codex's components, icons and markup are used without overrides. Their look comes from Codex's design tokens, to which the instance or a tenant gives values of its own through a **palette**, the colours in four variants and the radii, and a **type set**, the typefaces, both config records and both chosen by setting and by the reader (§5.9; [0086](../decisions/0086-theming.md)). Never a component's markup or behaviour. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream. Codex is used for its accessible components, its right-to-left and language support, and CSS-only components that need no JavaScript, more than for its look.
5. **The public API only** ([0012](../decisions/0012-api-requirements.md) §1). The browser calls public routes, and so does the server-side renderer: it calls the public HTTP API with the viewer's own credentials, over the network from `triplespace-web` or through an in-process call into the API's router from `triplespace-server`, and never links an API crate's handlers or reads `view` ([0057](../decisions/0057-web-tier.md) §1; [20](20-web-tier.md)). Every page therefore carries exactly the API's per-viewer redaction. **A page reaches first byte within four API calls.** The budget is met by the page bundle `GET /entity/{id}/page` (entity, provenance and identity-line facts in one response), the bulk-labels route (up to 1,000 IDs, cacheable by language), the About panel's own response (§3.1) and the viewer's session facts (`meta=userinfo` with the inbox count), the four routes of [18](18-api.md) §3.2; inside `triplespace-server` the in-process transport resolves the session once per page and passes the principal to every sub-request, exempt from rate counting ([20](20-web-tier.md) §2.2), so a reading room behind one address is never rate-limited by a budget written for API clients.

### 5.2 Server rendering, in `triplespace-ui`

*Sources: [0034](../decisions/0034-frontend-stack.md) §2; [0010](../decisions/0010-site-ui.md) §13; [0086](../decisions/0086-theming.md) §3; [0093](../decisions/0093-fallback-typefaces.md) §3; [0097](../decisions/0097-versioning-and-skins.md) §8.*

- **Skins:** the HTML is drawn by a skin the operator installs (§5.10; [0097](../decisions/0097-versioning-and-skins.md) §8): MiniJinja templates over view models the binary builds, every page; the built-in rendering the site began with, `askama` templates and string builders, was removed when the last group of pages reached parity (§5.10).
- **Codex markup:** Plain's macros emit Codex CSS-only component markup (buttons, fields, tables, cards, tabs, messages, chips, progress bars), following the approach of WMF's Codex PHP, so a Codex markup change is one edit; until parity, a Rust builder module does the same for the built-in rendering. Another skin may use other components.
- **Frame:** the page frame of §1 (global header, identity line, title, tabs) is one template shared by every page kind.
- **Assets:** Codex CSS, design tokens and icons come from the pinned `@wikimedia/codex`, `@wikimedia/codex-design-tokens` and `@wikimedia/codex-icons` packages at build time ([0034](../decisions/0034-frontend-stack.md) §8; [22](22-crates-and-stack.md)), with hashed file names, served with long-lived cache headers by whichever binary serves the site: `triplespace-web`, or `triplespace-server` with `server.ui = embedded` ([0057](../decisions/0057-web-tier.md) §2). The shipped type sets' typefaces (Source Serif 4, Source Sans 3 and Lilex for `default26`; Atkinson Hyperlegible for `hyperlegible`; 72 Noto families for the fallback layer `noto`; all under the SIL Open Font License) are served the same way, from the site's own origin, as the policy of [0034](../decisions/0034-frontend-stack.md) §11 requires; a tenant's own typefaces are site assets, served from the media origin ([0086](../decisions/0086-theming.md) §3, §5).
- **Mobile:** one responsive site built on Codex's breakpoints. No separate mobile domain or skin.

`triplespace-ui` renders pages on the server, runs in the server or in the stateless web tier, and reaches the instance only through the public API ([0057](../decisions/0057-web-tier.md) §1–2; [20](20-web-tier.md)). Statement diffs reuse `scatter-wikibase-shape`, and the page editor's preview reuses `scatter-wikitext` and the markdown renderer of `scatter-pages`, all built for `wasm32-unknown-unknown` ([0005](../decisions/0005-crate-organization.md) §2; [22](22-crates-and-stack.md)). Recent changes, contributions and page histories are served by the activity projection, `view.activity` in `triplespace-projections`, which grows with local activity and the number of jobs, not with the size of the mirrors; an entity's history is served by `view.entity_history` (§3.3) ([0010](../decisions/0010-site-ui.md) §13; its inputs and tables are [03](03-storage-caches-and-search.md) and [16](16-logs-feeds-and-notifications.md)).

Not yet: detailed mobile layouts stay open ([0034](../decisions/0034-frontend-stack.md) §2).

### 5.3 Interactive components

*Sources: [0034](../decisions/0034-frontend-stack.md) §4.*

Vue 3 with Codex's Vue components, written in TypeScript, mounted into placeholders in the server-rendered HTML. Each component reads its initial data from a `<script type="application/json">` block the server writes beside the placeholder.

| Component | Codex parts | Region |
|---|---|---|
| Statement value editor, rank menu | Lookup, Combobox, Menu, Dialog, TextInput | one value or one statement |
| Term editor (labels, descriptions, aliases) | TextInput, ChipInput | the term box |
| Source editor with live preview (§3.2) | CodeMirror 6 (§5.5), Tabs | the edit view |
| Search box with "Go to" suggestions (§2.1) | TypeaheadSearch | the global header |
| Notifications bell and inbox (§6.2) | Popover, Menu | the account area |
| Live updates switch ([16](16-logs-feeds-and-notifications.md)) | ToggleSwitch | feed pages |
| Recent changes filters | Checkbox, Lookup, MultiselectLookup | the filter panel |
| Job progress (§3.10) | ProgressBar | `Special:Jobs/{id}` |
| User, group and consumer pickers on special pages | Lookup | the form field |

Statement editing requires JavaScript, as on Wikidata. Reading, page source editing through a plain form, and the account pages work without it. A **table grid editor** (Codex Table, Lookup, TextInput; the statement value editor for cells) sits over the server-rendered grid of a `Table` page; reading a table needs no JavaScript (§6.7).

### 5.4 Rust in the browser, in `scatter-wasm`

*Sources: [0034](../decisions/0034-frontend-stack.md) §6; [0042](../decisions/0042-template-expansion-and-parsoid.md) §15.*

One `wasm-bindgen` crate re-exports what editors need from the pure crates:

- the shape classifier of §4.3;
- `scatter-normalize` (value normalization and keyed-ID grammars; [04](04-entities-and-identifiers.md), [06](06-statements-and-properties.md));
- `scatter-wikitext` and the `scatter-pages` markdown renderer, for live preview;
- `scatter-css`, so the editor lints a `sanitized-css` page as it is typed ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2; [11](11-rendering-templates-and-modules.md)).

It is built with `wasm-bindgen-cli` and `wasm-opt`, lazy-loaded only by editors, and never needed for reading. This is one reason [0033](../decisions/0033-backend-stack.md) §9.1 chose a pure-Rust wikitext parser. The browser preview stays WebAssembly where it can: when the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead (§3.2).

### 5.5 Source editor

*Sources: [0034](../decisions/0034-frontend-stack.md) §7.*

CodeMirror 6, which MediaWiki's CodeMirror extension also uses, with its wikitext mode, a markdown mode and a Lua mode for module pages ([0043](../decisions/0043-lua-modules.md) §13; [11](11-rendering-templates-and-modules.md)). Edit conflicts (§3.2) are shown as line decorations. `[[` triggers link autocomplete through the suggest route ([18](18-api.md)). A CSS mode serves `sanitized-css` pages. **Insert template…** builds a form from a template's TemplateData, and a parameter popup lists a call's parameters ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5). The same editor is the Definition tab of a table, a board and a scope (§6.7, §6.1, §6.9).

Not yet: `tree-sitter-wikitext` through `web-tree-sitter` is an optional later enhancement for structural highlighting.

### 5.6 Internationalisation

*Sources: [0034](../decisions/0034-frontend-stack.md) §9; [0085](../decisions/0085-settings-and-preferences.md) §2.2; [0097](../decisions/0097-versioning-and-skins.md) §13.*

- Interface messages are **banana JSON** files in **bundles**, each with its own version ([0097](../decisions/0097-versioning-and-skins.md) §13): the binary's in `i18n/` (errors, notifications, mail, API text) and each skin's in the skin's own `i18n/`, each with `en.json`, `qqq.json` for documentation and one file per language, so that each can be registered with translatewiki.net like any MediaWiki extension. **A skin's keys carry its name as a prefix** (`plain-…`); a skin that extends Plain adds none, as MediaWiki's skins prefix theirs, so a key belongs to one bundle and an Interface override names it unambiguously; switching skins leaves the overrides of the old skin's keys in place and inert. Renaming or removing a key is a major of its bundle; a new translation is a patch.
- The server renders messages with the `banana-i18n` Rust crate; components use the `banana-i18n` JavaScript library. Both read the same files. The Rust crate is at 0.1.0; if it proves incomplete, a small in-house implementation of `PLURAL`, `GENDER`, `GRAMMAR` and `$n` with ICU4X plural rules replaces it.
- `banana-checker` runs in CI.
- Interface language comes from `uselang`, then the reader's `language` preference, then the request's `Accept-Language` matched against the languages the interface has messages for, then the tenant's default and the farm's ([07](07-actors-and-accounts.md) §8.1; [0085](../decisions/0085-settings-and-preferences.md) §2.2); a reader who is not logged in is served in their language without choosing one. Fallback chains follow MediaWiki's. Direction comes from the language and is set on `<html dir>`; Codex handles RTL.

### 5.7 Accessibility and browser support

*Sources: [0034](../decisions/0034-frontend-stack.md) §10.*

- WCAG 2.1 AA, inherited from Codex and checked by axe in end-to-end tests ([0034](../decisions/0034-frontend-stack.md) §12; [22](22-crates-and-stack.md)), and kept under a theme by the contrast check that admits it (§5.1).
- Browser support follows MediaWiki's: modern browsers get the components; older browsers get the server-rendered pages without them.

### 5.8 Prototyping path

*Sources: [0003](../decisions/0003-statement-ui.md) §10; [0034](../decisions/0034-frontend-stack.md) §13; [0010](../decisions/0010-site-ui.md) §13.*

Because the UI reads canonical JSON (§4.2), it is built and tested against live Wikidata before Triplespace serves any data. The prototype is `triplespace-ui` with `scatter-wasm`, server-rendered Rust (§4.9, §5.2, §5.4): `triplespace-ui` renders item pages from Wikidata's canonical JSON (`Special:EntityData`) through the shape classifier, provenance panels use fixtures, and the editing components write through the Action API to test.wikidata.org or the MediaWiki 1.43 reference install.

| Need | Source |
|---|---|
| Statements | Wikidata's canonical JSON (`Special:EntityData`), normalized to one internal form |
| Labels and property data types | `wbgetentities` with `props=labels` or `props=datatype`, up to 50 IDs per call ([mediawiki-compat.md](../api/mediawiki-compat.md) §2.3) |
| Cross-origin reads | `origin=*` on anonymous Action API requests |
| Writes | The MediaWiki 1.43 + Wikibase reference install ([mediawiki-compat.md](../api/mediawiki-compat.md) §1.2), or test.wikidata.org |
| Provenance | Fixture files keyed by statement ID |

Steps:

1. **Normalize.** Fetch an entity, the labels it links to, and its property data types, and turn them into statement groups.
2. **Classify.** Write shape detection (§4.3) as a pure function. Test it with fixtures from the canvas examples:
   - P1082 (population) and P6 (head of government) on Q65, Los Angeles;
   - Pluto;
   - the capital of Kazakhstan;
   - an ionic-radius group.
3. **Render.** Build the six shapes, read-only, against live Wikidata.
4. **Audit.** Run the classifier over a sample of the Wikidata JSON dump. Measure how often each shape occurs, and set the thresholds in §4.3 from what the audit finds.
5. **Edit.** Point the same UI at the reference install and implement §4.7.
6. **Provenance.** Exercise §4.6 with fixtures. Switch to Triplespace's own response once it exists.

The classifier is its own crate (§4.9), so its test fixtures are shared by the server and `scatter-wasm`. Nothing in the UI changes when the source becomes a Triplespace instance, because it only speaks the public API. In development, `triplespace-web` with `web.upstream_host = fixed` is pointed at Wikidata or the reference install directly ([0057](../decisions/0057-web-tier.md) §4; [20](20-web-tier.md)).

### 5.9 Themes: palettes, type sets and modes

*Sources: [0086](../decisions/0086-theming.md) §1, §2, §3, §4, §5; [0093](../decisions/0093-fallback-typefaces.md) §1, §2, §3; [0094](../decisions/0094-accessibility-alternates.md) §1, §2, §3, §4, §5, §6; [0097](../decisions/0097-versioning-and-skins.md) §14.*

**A theme is two choices**, a **palette** and a **type set**, both config records of kinds `palette` and `typeset` at instance or tenant scope, written with `ts-config` and validated when written ([23](23-configuration-and-registry.md) §2.2). `docs/registry/themes.toml` ships the palette `parchment`, the canvases' warm colours with a walnut-brown dark variant; with `parchment` its two accessibility alternates (below), `rg` and `by`; and the type sets `default26` (Source Serif 4 over Source Sans 3 and Lilex) and, for accessibility, `hyperlegible` (Atkinson Hyperlegible) and `opendyslexic` (OpenDyslexic). Two options inspired by Vector 2022 ship beside them, not as defaults: the palette `encyclopedia`, Codex's own colours changed only where they miss the contrast floors, and the type set `basic`, MediaWiki's default stacks for Vector 2022, which serves no typeface. Three more pairs ship as options ([0086](../decisions/0086-theming.md) A10), each palette a popular editor colour scheme with its accents moved in their own hue only where they miss the contrast floors, keying its own `rg` and `by` alternates, beside a type set made for it: `gruvbox` (gruvbox light, and gruvbox dark on its neutral grey) with `retro` (Bitter over IBM Plex Sans and IBM Plex Mono); `nord` (Snow Storm and Polar Night) with `nordic` (Rubik over Inter and JetBrains Mono); and `catppuccin` (the Latte and Mocha flavours) with `cozy` (Fraunces over Nunito and Lilex). Two more are original ([0086](../decisions/0086-theming.md) A11): `greenbar`, fanfold computer paper in light and a green phosphor terminal in dark, with `mainframe` (VT323 headings over Overpass and Overpass Mono), and `cyanotype`, a whiteprint in light and a blueprint on Prussian blue in dark, with `drafting` (Barlow Condensed over Barlow and JetBrains Mono). A tenant offers the registry's, the instance's and its own, and a name means one thing on an instance: no tenant takes a name the registry or the instance uses, and the instance takes none a tenant uses. A name taken later is **keyed**: a tenant's palette or type set whose name the instance or the registry comes to use, when the tenant joins ([08](08-tenants-and-instances.md) §6.3), when the instance writes the name with `key=tenants` or when `registry sync` adds it, is renamed `{name}~{slug}` with the tenant's slug, and an instance's that the registry takes is keyed with the farm slug, with every `theme.*` setting, `extends` and reader's choice that names it in the same act ([08](08-tenants-and-instances.md) §8.6). An instance write of a name a tenant uses is otherwise refused. `~` is reserved for keying. The shipped ones are changed by no reader or operator, written only by `registry sync`, so `parchment`, the default palette, always has all four variants. Palettes and type sets are public, as the logo is: anyone may read them and their stylesheets, a private wiki's outsiders included. They are written through `PUT /palette/{name}` and `/typeset/{name}` ([18](18-api.md) §3.2) and the settings pages' **Palettes and type sets** tab (§2.8).

**Variants.** A palette sets Codex's colour tokens in up to four variants: `light`, required, and `dark`, each overlaying Codex's values for that mode and held to 4.5:1 for text and 3:1 for control boundaries and focus; and `light-more` and `dark-more`, overlaying the palette's own `light` and `dark` and held to 7:1 and 4.5:1. Each is checked as served, merged over Codex's values, with button labels (`color-inverted-fixed` on `background-color-progressive` and `background-color-destructive`) and the error, destructive and success colours among the pairs. A palette without `dark`, `light-more` or `dark-more` takes the default palette's. A palette may `extend` a shipped palette or one at its own scope, laying each variant over that palette's rather than Codex's, so it states only what it changes. A palette marked `accessibility` can always be chosen. The palette it `extends`, and the one its `alternates-from` names, are its **prerequisites**: it cannot be written where one is not offered, and a prerequisite cannot be deleted while a palette names it (0094 §3).

**Accessibility alternates** (0094 §1, §2). For colour-vision deficiencies a reader turns on an **alternate** over whatever palette they wear: a set of colours keyed by one of the registry's keys, `[alternate.*]` in `themes.toml`, which only a release adds: `rg`, *Red–green (protanopia and deuteranopia)*, and `by`, *Blue–yellow (tritanopia)*. A palette keys an alternate under `alternate.{key}` with up to the four variants, each laid over the palette's own as served, its `light-more` and `dark-more` taking its own `light` and `dark` first, and every variant with it on is held to the palette floors. A palette wears its own alternate for a key, else the one of the palette it extends, else of the palette its `alternates-from` names, else the default palette's, so every palette has every alternate, and one that fails its floors with an inherited alternate must key its own. `parchment` keys both, changing only the colours that carry meaning (error, destructive, success, warning, visited, and their subtle backgrounds), chosen by simulating the deficiency each serves; they are the colours of the palettes `parchment-rg` and `parchment-by` of 0086 A3, whose names are retired and read as `parchment` with the alternate, in a setting, a preference or a browser's stored choices (0094 §4). Colour is never the only cue (WCAG 1.4.1): states are told apart by text, icon or mark as well. Everything the site draws takes its colours from tokens, so every variant reaches diffs, charts, the sync strip and chips; provider chips swap their two colours in the dark variants (§1.5).

**Type sets** give `font-family-base`, `font-family-heading-main` and `font-family-monospace`, may map language tags to stacks applied with `:lang()`, and list the typefaces they serve, if any (`basic` serves none and names only families a device may have): shipped ones as build assets, a tenant's as font files it names as site assets, WOFF2 or WOFF under `OFL-1.1`, `Apache-2.0` or `UFL-1.0` ([12](12-files-and-media.md) §4.5). A type set marked `accessibility` can always be chosen; `hyperlegible` and `opendyslexic` ship as such. A type set may name a shipped **fallback layer** (`fallback`), whose typefaces go into each of its stacks, by the stack's generic family, straight after the typefaces it serves, so the order is the type set's own, the layer's, then the device's. The layer `noto`, which `default26`, `hyperlegible` and `opendyslexic` name, gives Noto Sans and Noto Serif, a Noto family for each of the 47 scripts of Wikimedia's language data other than Latin, Greek, Cyrillic, Chinese, Japanese, Korean and SignWriting (its serif where Noto has one, Noto Naskh Arabic for Arabic), Noto Sans Symbols and Symbols 2, and Noto Nastaliq Urdu as the stack for `ur`. Chinese, Japanese and Korean are left to the device's fonts, which pick the regional forms by `lang`.

**Who chooses.** The settings `theme.palette` (default `parchment`), `theme.typeset` (default `default26`) and `theme.choice` (`any`, or `accessibility` to withhold the brand choices and offer only the accessibility palettes and type sets) set the wiki's defaults, with instance defaults and locks ([23](23-configuration-and-registry.md) §3.1). The reader's preferences, all offered in the Appearance menu (§2.7), are `ui.mode` (`auto`, `light`, `dark`), `ui.contrast` (`auto`, `more`), `ui.text-size` (Codex's `small` to `x-large`) and `ui.vision` (`none` or an alternate's key, 0094 §5), which follow the farm account and are never withheld, and `ui.palette` and `ui.typeset`, which are per wiki unless the reader marks them **Use on every wiki** ([07](07-actors-and-accounts.md) §8.1).

**Serving.** One stylesheet per palette and one per type set, named under `/ui/theme/` by the record's offset and hash, immutable and rebuildable from the log for earlier offsets. A palette's holds `light` on `:root`, `dark` under `:root[data-mode=dark]` and, within `@media (prefers-color-scheme: dark)`, `:root:not([data-mode=light])`, and each high-contrast variant only in its own mode under `data-contrast=more` or `prefers-contrast: more`, then each alternate it wears, its rules repeating these with `[data-vision="{key}"]` on `:root`, one attribute more specific than the rule each overlays (0094 §6); a type set's holds the roles, per-language stacks set as `font-family` on `:lang()` elements, the `@font-face` rules, its fallback layer's after its own, and the text-size modes under `data-text-size`. A layer's faces each serve only their script's ranges, as WOFF2, so a page fetches the files for the scripts it holds and no others; `ui/scripts/fallbacks.js` writes them from the registry. The security policy's `font-src` and `img-src` admit the media origin ([09](09-security-and-moderation.md) §1.4). The reader's choices are attributes on `<html>`, written by the server for a signed-in reader and by the head script for one who is not logged in, with MediaWiki's `skin-theme-clientpref-night`, `-os` and `-day` classes beside them; a public page carries `skin-theme-clientpref-os` and no `data-mode`, and the head script swaps the stylesheets for the reader's palette and type set before the first paint, from a JSON block on the page. Without JavaScript the media queries follow the device. In the dark variants the frame honours Wikipedia's content classes: `notheme` keeps an element's light colours, and `skin-invert` and `skin-invert-image` invert it ([11](11-rendering-templates-and-modules.md) §6.2 for night-mode TemplateStyles).

**The tokens are Codex's**, and so is their version: the release of `@wikimedia/codex-design-tokens` the binary pins, which skins name as a requirement; a Codex major that renames or removes tokens is a product major unless the binary maps the old names ([0097](../decisions/0097-versioning-and-skins.md) §14).

### 5.10 Skins

*Sources: [0097](../decisions/0097-versioning-and-skins.md) §8, §9, §10, §11, §12, §15, §16.*

**A skin is the site's HTML presentation**: templates, the skin's own CSS, JavaScript, images and messages, and a manifest. A theme (§5.9) is not a skin: a palette and a type set set Codex's tokens inside whatever HTML the skin draws. The binary answers requests, calls the API, builds each page's view model and hands it to the skin's template.

- **Only the operator installs a skin**, as files in the directory `server.skins` or `web.skins` names, read at start and on reload. A skin is trusted as operator configuration is, and may ship JavaScript, served from the site's origin under the security policy (§1; [09](09-security-and-moderation.md) §1.4).
- **The manifest**, `skin.toml`, gives the name, version, licence and requirements: for a skin that stands alone, the template contract and the Codex token major it is written for; for one that extends Plain, Plain's version. A skin whose requirements the binary does not meet is not loaded, and the reason is logged and shown on `Special:Version`.
- **Two skins ship**, embedded in both binaries from `skins/plain/` and `skins/worksheet/`: **Plain**, which draws the site's HTML and loads no stylesheet of its own, and **Worksheet**, the default, which is Plain with Codex's and the site's stylesheets. An installed skin of the same name replaces a shipped one.
- **A skin either extends Plain or stands alone.** A skin that extends Plain names it as its parent and adds stylesheets and the files they use, and nothing else: no templates, scripts or messages, so its pages have Plain's DOM and everything written against that DOM works on them. A skin that stands alone implements the template contract with its own templates and DOM: complete freedom, at the cost of compatibility with whatever relies on Plain's DOM.
- **Plain's DOM is a contract**, versioned as Plain's version: the element structure, class names, `data-` attributes and landmarks Plain draws. Class names are Codex's where Codex has a component for the element, and `ts-` classes fill the gaps. A skin's stylesheets, a reader's own client-side styles and assistive technology rely on it. Plain links the theme's stylesheets and sets the reader's attributes on `<html>` (§5.9), so CSS written against it has the tokens, dark mode and the accessibility alternates.
- **Which skins a wiki offers** is the tenant setting `skins.available`, capped by the tenancy switch of the same name, `all` in every preset, with the instance's default as every wiki's list until it sets its own: an instance that locks the setting lets no tenant change it, one whose ceiling is its default lets a tenant disable skins, and one whose ceiling is wider lets a tenant add them ([23](23-configuration-and-registry.md) §1.4, §3.1). `skins.default` is the wiki's skin for readers who have not chosen.
- **Readers choose** among the wiki's skins with the preference `ui.skin`, per wiki unless marked **Use on every wiki**, or for one request with `useskin=`, as in MediaWiki. A reader not logged in has no stored choice, since a skin changes the HTML and a public page is cached the same for every reader.
- **The engine is MiniJinja**: templates are parsed once when the skin is loaded, rendered with autoescape on, with no access to files, the network or the environment.

**The template contract** is versioned on its own and written in `docs/api/skin-contract.md`: the templates, one per page kind, by name; the view model each receives; and the skin's obligations, which are that forms work without JavaScript with the actions, field names and CSRF token the view model gives; that parsed content sits in `.mw-parser-output` with the reader's `data-mode`, `data-contrast`, `data-text-size` and `data-vision` and the `skin-theme-clientpref-*` classes on `<html>`, and the theme's stylesheets in the order given (§5.9); that the interactive components and the challenge solver have their mount points (§5.3); that each region `action=render` returns ([20](20-web-tier.md) §3.2) is drawn by one template the page also draws; and that the landmarks, headings and skip link the axe tests check are present.

**A view model carries data, not markup**: the skin draws every element. The one exception is a drawing, which the binary makes and the view model carries as SVG (`*_html`): the Series chart and the Timeline axis ([0097](../decisions/0097-versioning-and-skins.md) A4), since a chart's geometry is computed, not laid out, and no skin would draw it differently. While the skin migration ran, a part of a page whose group had not yet switched was carried the same way until it did ([0097](../decisions/0097-versioning-and-skins.md) A6); none is carried now. A line of text whose words hold links, code or line breaks, a sentence, a table cell, an item of a list, is carried as **parts** (text, a link on or off the site, code, monospace text, a line break, a list that opens on demand) that Plain's `inline.html` draws, and a message whose parameters are links is filled with parts, never with HTML; a page that says one thing (a refusal, a title with no page, a call that failed) is the skin's `message.html` ([0097](../decisions/0097-versioning-and-skins.md) A11). The HTML the API itself serves, a page's rendering or MediaWiki's diff rows, is carried as HTML too (`content_html`, `body_html`): the binary never drew it.

**A template function does no I/O**: each is pure over the view model and what the binary holds in memory, so a page stays within its four API calls ([20](20-web-tier.md)). MiniJinja's built-in filters are available, and Triplespace adds nine, chosen for what gadgets and user scripts keep rebuilding on Wikimedia wikis:

| Function | Returns |
|---|---|
| `msg(key, …params)` | A message in the page's language, with `PLURAL`, `GENDER` and fallback, through the wiki's Interface overrides |
| `url(route, …params)` | A URL from a route's name in the routing table, never from a string |
| `link(target, text?)` | A classified link to a page, entity, user or special page, an entity's label from the view model |
| `user(actor)` | A user link with talk and contributions links, groups and block state |
| `time(ts, style)` | A time in the reader's zone and language: `datetime`, `date`, `relative`, `iso` |
| `number(n, style)` | A localised number: `plain`, `bytes`, `delta` |
| `can(right, target?)` | Whether the reader holds a right |
| `feature(name)` | Whether a wiki feature is on |
| `asset(path)` | The content-hashed URL of a file in the skin |

Codex components are Plain's macros, not functions, and so are a form's controls: a labelled text field, an inline check box, a labelled menu and a fieldset of them are one model and one macro that every form working without JavaScript draws, extended where a control is missing rather than repeated per page ([0097](../decisions/0097-versioning-and-skins.md) A7). The controls are one vocabulary, a control drawn from its model (a checkbox, a radio button, a select, an input, a text area, a value shown; a checkbox's or radio button's own label points at its ID, or wraps it where it has none), placed by layouts: a field under the label that wraps it, for a form that works by `GET`, and the form of one such field that asks for the page to go to; a settings row (label, origin chips, key, help, errors, the control or the value shown, **Reset**, **Lock**), one for preferences and settings alike; and for the moderation forms, which post, Codex's field (the label above the control, its help under it) and an option (a checkbox with its own label and its help under it). A checkbox may be disabled, shown rather than hidden where the viewer may not change it, with a note at the end of its label; an input may be required, and may say which keyboard it wants (`inputmode`) ([0097](../decisions/0097-versioning-and-skins.md) A8, A9, A10, A11). **Menus are data**: the view model carries named menus (`header`, `user-menu`, `page-tabs`, `page-actions`, `tools`, `title-meta`, `footer`), each a list of items with a route or URL, a message key, an icon hint and the right it needs, filled by the binary from the features, the registries and the operator's extra links and filtered by the reader's rights, so the skin only places and draws them. **Every link says what it points to**, with `data-ts-target` and `data-ts-kind` (`page`, `entity`, `user`, `special`, `external`) and, as classes, MediaWiki's where there is one (`new`, `mw-redirect`, `mw-disambig`, `extiw`) and Triplespace's otherwise (foreign, mirrored, blocked user, bot, groups).

**The build ID names the skin**: the build that a page's `ETag` hashes and that a component sends as `X-Triplespace-UI-Build` is the binary's with the name and version of the skin that drew the page, so a skin upgrade or a change of skin is build skew ([03](03-storage-caches-and-search.md) §10.3; [20](20-web-tier.md) §3.2). Changing `skins.default` or upgrading a skin purges the wiki's pages.

**Parity.** Plain was first written to reproduce the HTML the binary drew, and the routes moved to it in twelve groups: the frame and the Codex components first, then entity pages, then the rest. A group switched when its existing tests rendered unchanged, its pages were equal on the corpus, the Playwright and axe suite passed through the skin, a crawl and a replay of the suite under a `compare` renderer, which drew both and logged each route that differed, logged none, and its server render time was within 5% of the built-in renderer's or over it by an amount no reader can perceive ([0097](../decisions/0097-versioning-and-skins.md) §16, A2, A5). The last group switched on 2026-11-03, and the built-in renderer was removed with it: its templates and string builders, the `askama` dependency, and the deployment settings `server.renderer` and `web.renderer` that chose between them ([0097](../decisions/0097-versioning-and-skins.md) A12). What remains of the comparison is a check that **Worksheet is Plain with its stylesheets**: every case of the corpus, every route for a fixture wiki as an anonymous reader, a user and an administrator, for each content model, right to left, in night mode, with feature switches on and off, and on a private tenant, is drawn with both skins, and Worksheet's page with its stylesheet links taken out must equal Plain's (`cargo xtask skin-parity`, which also reports each skin's render time). Pages are compared as normalized DOM, fragments too, so that `&#60;` and `&lt;` are one character ([0097](../decisions/0097-versioning-and-skins.md) A3); `html::esc` is the skin's escaper. The template contract stays at major 0 until a skin that stands alone renders every route.

## 6. What each feature adds to the UI

*Sources: [0019](../decisions/0019-discussions.md) §8; [0049](../decisions/0049-boards.md) §8; [0021](../decisions/0021-notifications.md) §6; [0023](../decisions/0023-moderation.md) §9; [0056](../decisions/0056-security-model.md) §13; [0026](../decisions/0026-sitelinks.md) §9; [0029](../decisions/0029-resolver-namespaces.md) §7; [0031](../decisions/0031-property-constraints.md) §7; [0030](../decisions/0030-edit-filters.md) §10; [0039](../decisions/0039-files-and-media.md) §18; [0045](../decisions/0045-table-content-model.md) §11; [0066](../decisions/0066-lexemes.md) §7; [0060](../decisions/0060-scopes.md) §8; [0059](../decisions/0059-query-service.md) §6; [0067](../decisions/0067-proposals.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6; [0018](../decisions/0018-tenants.md) §11; [0028](../decisions/0028-tenancy-policy.md) §11; [0046](../decisions/0046-primary-tenant.md) §9; [0022](../decisions/0022-federation.md) §11; [0040](../decisions/0040-instance-prerogatives.md) §7.*

### 6.1 Threads, talk pages and boards

*Sources: [0019](../decisions/0019-discussions.md) §8; [0049](../decisions/0049-boards.md) §8.*

The data model of threads, posts and boards is [14](14-discussions.md).

- **A thread page** shows its posts as a tree, indented to the instance's `thread.max_depth`. A post deeper than that is shown at the limit with a "replying to" link to its parent; its stored parent is unaffected. Each post has an anchor `#post-{revid}`, a permalink (`Special:PermanentLink/{revid}`), reply, edit and history controls, and the hiding controls of §3.4 for those who hold the rights.
- **Pinned threads first.** A talk page lists its pinned threads first, in the order they were pinned, marked with a pin, and exempt from the age rule of [0019](../decisions/0019-discussions.md) §6, though not the status rule ([0069](../decisions/0069-synchronized-talk-pages.md) §4). A thread page has **Pin here** and **Unpin** beside each attachment, for those who hold the rights ([0049](../decisions/0049-boards.md) §13).
- **Order.** After the pinned threads, a talk page or board lists its threads in the order they were attached *to that page*, newest last, as MediaWiki readers expect, with a switch to order by last activity. A thread is attached to its home when it is created or moved there, and to a listing when it is listed, so a thread listed on a board today appears as new there rather than among threads from last year. For a thread that never moves and is never listed, attachment order is creation order. Each thread shows its subject, status, home and listings, participant count, last activity, and its posts or a collapsed summary per [0019](../decisions/0019-discussions.md) §6. A "new thread" form creates one attached to this page.
- **Where a thread lives.** A thread's header shows its home first, marked as home, then its listings, as "Also on …" links. On a listing page, the summary in the listing says "Started on Item talk:Q42".
- **Attaching.** The new-thread form has an "Also post to…" field that takes titles of talk pages and boards, up to the limit. A thread page has **List on…** and, beside each listing, **Remove from here**, for those who hold the rights ([0049](../decisions/0049-boards.md) §13). Moving a thread offers "Keep listed on the old page".
- **The composer** is the same for a new thread and a reply: a text area with the markup the tenant's `thread.markup` names shown beneath it, linked to formatting help, and on a thread page a **Status** menu, since a status is set by posting ([14](14-discussions.md) §2.1). A signature an editor types at the end of a post is removed when it is saved, without comment ([14](14-discussions.md) §1.6; [0019](../decisions/0019-discussions.md) A28).
- **A user talk page** takes a new topic as any talk page does, whether or not the user page exists; the thread's home is the user, named by the page's title ([0019](../decisions/0019-discussions.md) A30), and the user's notification is `talk` ([16](16-logs-feeds-and-notifications.md) §5.2).
- **A board page** has the tabs Board, Definition (the source editor of §5.5, as for a table) and History. It shows the new-thread form when `homes` is true.
- **A talk page that follows a repository's talk page** also lists that page's sections as foreign threads, by their first comment's time, each marked with its origin, with a filter by origin; its new-thread form offers to post here or upstream ([0069](../decisions/0069-synchronized-talk-pages.md) §3, §6; [13](13-mirrored-pages.md)).
- **Subject pages** get the talk tab of §1.3, with the thread count.
- **Watching** a subject watches its talk page and the threads attached to it; a thread can be watched on its own. The watchlist is [16](16-logs-feeds-and-notifications.md).
- **Editing another actor's post** is permitted only with `ts-editpost` ([09](09-security-and-moderation.md)) and always shows an "edited by" note, because a post is speech.

### 6.2 Notifications and watching

*Sources: [0021](../decisions/0021-notifications.md) §6, A20, A22.*

- **The bell** in the global header, for a signed-in reader, is a link to `Special:Notifications` with the unread count, to 99 and then `99+`, marked while something unread is newer than `seen_at`; opening the inbox sets `seen_at`, which clears the mark. A panel over the page needs JavaScript and waits for it; without it the bell is the link ([0021](../decisions/0021-notifications.md) A20). Items link to the post they are about (`#post-{revid}` on its thread) or to the user page, and each unread one has mark-read.
- **`Special:Notifications`** is the inbox as a page, filterable by reason and read state, with mark-all-read.
- **`Special:Account`** has the **Notifications** section of §2.4.
- **The watch control** is one menu at the end of the tabs of a page, an entity, a talk page or a thread, before **More**, as forms that work without JavaScript ([0021](../decisions/0021-notifications.md) A22). Its button is the star with **Watch**, or a filled star with **Watching** for a watched title. It opens how long to watch (permanently, a week, a month, three months or six months) and **Notify me of changes**, which sets `notify` on the watch row, submitted with **Watch**, or **Save** for a watched title; a watched title's menu also has **Unwatch**. A watch that lapses says until when, in the menu and on the button's hover. Subscribing to a thread is watching it with `notify` ([16](16-logs-feeds-and-notifications.md) §4.3; [0021](../decisions/0021-notifications.md) A21).
- **`Special:Watchlist`** lists the changes to what the reader watches as recent changes does, mirror syncs included, marking in bold what they have not seen, with **Mark all changes as seen**; **`Special:EditWatchlist`** lists every watch, how long it lasts and whether it notifies, with **Remove selected**. Both are labelled Private and linked from the account menu.
- **The orange bar** is kept in spirit: an unread `talk` notification shows a persistent banner until read, as MediaWiki's "You have new messages" did, because a message on one's talk page is the one notification a wiki must not let pass. It links to the reader's talk page and is not drawn there; reading one's own talk page marks its `talk` notifications read, as visiting it cleared MediaWiki's ([0021](../decisions/0021-notifications.md) A20).

### 6.3 Protection, deletion, hiding, patrolling and Nuke

*Sources: [0023](../decisions/0023-moderation.md) §9; [0056](../decisions/0056-security-model.md) §13; [0085](../decisions/0085-settings-and-preferences.md) §4.*

What each action does is [09](09-security-and-moderation.md).

- **Overflow menu.** Pages and entities gain **Protect…** ([0016](../decisions/0016-permissions-and-access-control.md) §7), whose dialog has a **Read** row with a group picker and the lock-out warning of [0056](../decisions/0056-security-model.md) §4, and **Delete…**, the latter with the "also delete the talk page" option. Deleted pages show **Undelete…** to the group.
- **A deleted page or entity** shows, to everyone outside the group, the frame of §1.3 with the deletion log entry in place of the content, and a link to the log. To the group it shows the content under a banner naming who deleted it and why, with Undelete.
- **History rows** carry the RevisionDelete checkboxes, suppress option and patrol controls of §3.4.
- **Sets** are managed from `Special:ProtectedPages`, which gains a Sets tab: create, rename, add and remove members, restrict. It also gains a **Read** column and lists a confidential restriction only to viewers who may read its target ([0056](../decisions/0056-security-model.md) §5, §13). The **Restricted** pill in the identity line is §1.3.
- **Special pages.** `Special:Log/protect`, `/delete`, `/patrol` and, for the group, `/suppress`; `Special:ProtectedPages`, `Special:ProtectedTitles`, `Special:Undelete`, `Special:DeletedContributions`, and `Special:BlockList` gains the hidden-name column, all where MediaWiki users expect them ([21](21-special-pages.md)).
- **Nuke.** `Special:Nuke` applies these actions in bulk to one account's, or one tag's, contributions: one job that deletes what it created, reverts or rolls back what it changed and hides its posts, patrolled as one row and undone by **Revert this job…** (§3.10).
- **Visibility.** `Special:WikiSettings` (§2.8) has **Visibility**: *Anyone*, *Accounts on this wiki* (`user`), *A group…*, with the account-creation switch beside it and a note on what a private wiki gives up ([0056](../decisions/0056-security-model.md) §3; [08](08-tenants-and-instances.md)). A visitor to a private tenant who may not read it sees a **landing page**: the site name and logo, `site.landing_text`, and Log in; nothing else of the tenant is served, special pages included, except `Special:UserLogin`, the OAuth routes, `/.well-known/` and site assets at `{media base}/asset/`, the logo among them ([12](12-files-and-media.md) §4.5).
- **`instance check`** is also `GET /instance/check` at the farm base, for `owner` ([18](18-api.md)). No special page is added; the CLI and the route are the operator's tools.

### 6.4 Sitelinks, resolvers and constraints

*Sources: [0026](../decisions/0026-sitelinks.md) §9; [0029](../decisions/0029-resolver-namespaces.md) §7; [0031](../decisions/0031-property-constraints.md) §7.*

The models are [06](06-statements-and-properties.md).

**Sitelinks.** The **Sitelinks** tab (§1.3) groups links by host, showing the host as a Domain chip linking to `Domain:{host}`, the title or path, and badges. Adding a link takes a URL and, for an aliased site, offers the site-and-title form with title autocomplete against that site's API, as Wikibase does. A denied host is refused with the list named.

**Resolvers.** A resolver's disambiguation page and its place in the namespace selector are §2.1.

**Constraints.** The entity page's marker sits in the value cell of §4.8 and follows its rule that nothing is drawn where nothing differs: an entity with no violations looks as it did. The property page gains the **Constraints** tab. `Special:ConstraintReport` uses the site frame with the filters of [0031](../decisions/0031-property-constraints.md) §3. Severity and the constraint's source are the two facts every row leads with, since "Wikidata says this is mandatory" and "we suggested this here" call for different responses.

### 6.5 Edit filters

*Sources: [0030](../decisions/0030-edit-filters.md) §10.*

`Special:EditFilter` lists filters with hit counts and last-hit times; `Special:EditFilter/{id}` edits one, with the CEL editor, a syntax check, a **Test** tab running the test of [0030](../decisions/0030-edit-filters.md) §6 over a chosen period, the actions form and the privacy switch; `Special:EditFilter/history/{id}`; `Special:EditFilterLog` with the feed filters of [0030](../decisions/0030-edit-filters.md) §6; `Special:Tags`. A refused write shows the filter's public message in place, with the resubmit control for `warn`. A tagged row shows its tags as chips (§1.5). The UI says **edit filter** everywhere; the `abusefilter-*` names appear only in rights and API modules ([09](09-security-and-moderation.md)).

### 6.6 Files

*Sources: [0039](../decisions/0039-files-and-media.md) §18.*

The file model is [12](12-files-and-media.md).

- **The file page** shows, in the frame of §1.3: the file (thumbnail, or player for audio and video), its dimensions and size, the description text, a **File history** table of versions with thumbnail, date, uploader, dimensions, size and summary, with revert and delete actions for those with the rights; **File usage**, pages and entities; **Metadata**; the Page data tab of §1.4; and, for a foreign file, its source. A taken-down or expunged version shows the operator's notice in place of the file.
- **History** interleaves uploads with text and statement revisions, each row labelled by kind (§3.3).
- **Special pages**, where MediaWiki users expect them: `Special:Upload` (chunked and resumable through the stash), `Special:ListFiles`, `Special:NewFiles`, `Special:MIMESearch`, `Special:MediaStatistics`, `Special:ListDuplicatedFiles`, `Special:FileDuplicateSearch`, `Special:UnusedFiles`, `Special:UncategorizedFiles`, `Special:FilePath`, and in `Special:Undelete` an **Erase permanently** action for `ts-erase`.
- **Operator pages** at the farm base: `Special:Takedowns` (list, file, reinstate, expunge), `Special:Takedown/{id}` (the notice, every affected tenant and page, the bytes behind a click-to-reveal for `ts-viewtakedown`), and `Special:Log/takedown`.

### 6.7 Tables

*Sources: [0045](../decisions/0045-table-content-model.md) §11.*

The table content model is [15](15-structured-pages.md).

- **Reading needs no JavaScript.** The server renders the grid with Codex's CSS-only table and the value cells of §4.8 (§4.9), a page of rows at a time, inside the frame of §1.3. Tabs: Table, Definition, Talk, History.
- **The grid editor** is an interactive component (§5.3): Vue with Codex's Table, Lookup and TextInput, reusing the statement value editor for cells. Keyboard as §4.7, with arrow keys between cells. Paste: tab-separated text maps to the columns from the focused cell, each cell parsed with `wbparsevalue` for its column's data type; a pasted column of IDs into the ID column adds rows ([0045](../decisions/0045-table-content-model.md) §7). Pending rows are marked, and a save bar shows the rows to save, progress, and refused rows with their reasons ([0045](../decisions/0045-table-content-model.md) §6).
- **The Definition tab** is the source editor (§5.5) on the JSON, with schema validation in the browser.
- **Viewer state is not saved.** Sorting and filtering in the grid are the viewer's own; "Make this the default sort" is a definition edit.

### 6.8 Lexemes

*Sources: [0066](../decisions/0066-lexemes.md) §7.*

**The lexeme page** has its own layout: a header with the lemmas, language and lexical category (each editable in place); the lexeme's statements; then **Forms**, each a card with its representations and grammatical features and its statements; then **Senses**, each with its glosses and statements. Every statement block is the statement UI of §4, with the part as subject, so shape detection, ranks, roles and provenance work unchanged. Add form, add sense, remove and reorder are actions on the page. A part's anchor is its ID. The lexeme model is [04](04-entities-and-identifiers.md).

**Special pages** move from `reserved` to `served`: `NewLexeme` (lemma, language, category; appends the first record) and `MergeLexemes` ([0066](../decisions/0066-lexemes.md) §4). The entity suggester, `Special:Search`, `Special:EntityPage` and `Special:EntityData` take lexeme and part IDs ([21](21-special-pages.md)).

### 6.9 Scopes and the query page

*Sources: [0060](../decisions/0060-scopes.md) §8; [0059](../decisions/0059-query-service.md) §6.*

Scopes are [15](15-structured-pages.md); the query service and its protocol are [02](02-graphs-rdf-and-query.md) and [18](18-api.md).

**The scope page** has a **Builder** tab beside the Definition tab, by example, by facet and by algebra, with a live count and sample through `POST /scope/preview` ([0062](../decisions/0062-workspaces.md) §5). It shows the description, the count with its notice and its "as of" (the `computed_at`, and for query scopes the cursor's lag), the members in pages of 100 with their kind and label, a **Refresh** button for scopes with a `query` operand (needs `edit` on the page; rate class `query`), and tabs: Members, Definition (the JSON source editor, as a table's), Related changes, and the usual talk and history. A scope that has never been computed because the query service is off says so instead of a count. "What links here" on `Scope:Women` lists the scopes built on it, since the page projection links a scope to every scope, table, category and property its definition names; members get no link row. The About panel's "In 3 scopes" is §3.1.

**`Special:Query`** is the page for raw SPARQL: a SPARQL editor with the pre-declared prefix set (served at `GET /sparql/prefixes` as Turtle, so that a client can show it), a result table with entity labels resolved and linked, a "Try with scope" button that turns the query into a scope definition ([0060](../decisions/0060-scopes.md) §4), and a shareable URL, `Special:Query?query=…`, so that a query is a link as it is on Wikidata; **Save as Query page** turns the text into a `Query:` page with parameters ([0063](../decisions/0063-query-namespace.md) §5). It is served when `query.enabled` is on (`requires = "query.enabled"`), in the `wikibaserepo` group, unrestricted; it reads nothing but `/sparql`. A result cut by `query.timeout` or `query.max_results` says so in the `Query-Truncated: timeout | results` header, since the result formats have no place for a warning ([0059](../decisions/0059-query-service.md) §6).

### 6.10 Proposals and merging with upstream

*Sources: [0067](../decisions/0067-proposals.md) §7; [0068](../decisions/0068-merging-with-upstream.md) §6.*

Proposals are [14](14-discussions.md) and [17](17-federation-and-publication.md); merging a fork is [13](13-mirrored-pages.md).

**The proposal thread** shows, above the posts: the kind and destination, the state chip with its history, the payload as a diff-shaped table (proposed claims with labels; for a page, the diff of [0068](../decisions/0068-merging-with-upstream.md) §4), the omitted and flagged items, and the actions: **Re-compile**, **Export** (the formats of [0067](../decisions/0067-proposals.md) §4), **Push** (when [0067](../decisions/0067-proposals.md) §6 is on and the person holds a grant), **Withdraw**, **Mark declined**. **The entity page** gains "Propose to {wiki}" in the overflow menu and a proposal count in the About panel ("2 proposals: 1 adopted, 1 offered"; §3.1). **`Special:Proposals`** (new; group `identity`) lists the tenant's proposals by state, wiki and proposer, and is what a board scoped to `proposal-state` shows without configuration.

**`Special:MergeUpstream/{title}`** (new; group `pagetools`; restricted to `edit` on the page) is the guided merge of [0068](../decisions/0068-merging-with-upstream.md) §2, also reached from the About panel's "{n} newer revisions" line. The fork's About panel additions are §3.1.

### 6.11 Tenants, the farm and federation

*Sources: [0018](../decisions/0018-tenants.md) §11; [0028](../decisions/0028-tenancy-policy.md) §11; [0046](../decisions/0046-primary-tenant.md) §9; [0022](../decisions/0022-federation.md) §11; [0040](../decisions/0040-instance-prerogatives.md) §7; [0056](../decisions/0056-security-model.md) §13; [0079](../decisions/0079-derived-issuer-codes.md) §4; [0085](../decisions/0085-settings-and-preferences.md) §4.*

Tenants, the tenancy policy and the primary tenant are [08](08-tenants-and-instances.md); the API halves of these sections are [18](18-api.md).

- **The host selects the tenant.** Every route is served per tenant base. Every special page has a scope in `docs/registry/special-pages.toml`: `farm` pages are served at the farm base, `tenant` pages at each tenant's base, and `both` pages at either ([0047](../decisions/0047-special-pages.md) §3; [21](21-special-pages.md)).
- **`Special:GlobalContributions`** (§2.5) and, for administrators, **`Special:Tenants`** at the farm base. `Special:Tenants` is the page of the tenant list, the `tenants` array of `GET /tenancy` (slug, host, name, status); it marks the primary tenant, lists open offers, and offers **Offer the primary role…** to holders of `ts-primary`. An `owner` of the offered tenant sees **Accept** there and on the Tenancy tab of their own tenant's `Special:WikiSettings` (§2.8; [0046](../decisions/0046-primary-tenant.md) §9).

- **Farm pages.** `Special:InstanceSettings` for the instance's settings and the tenancy policy (§2.8); `Special:GlobalUsers`, `Special:GlobalGroupMembership`, `Special:GlobalBlock`, `Special:GlobalBlockList`, `Special:GlobalRecentChanges` at the farm base; the account page shows the farm account and every linked tenant account under **Linked accounts** (§2.4), labelled Public; a farm signup page states the linking consent of [0028](../decisions/0028-tenancy-policy.md) §2; the identity line of a mirrored provider-tenant entity is unchanged from §1.3 ([0028](../decisions/0028-tenancy-policy.md) §11). `Special:Tenancy` redirects to the Tenancy tabs of the settings pages (§2.8), and `Special:WikiSettings` carries the **Visibility** control of §6.3.
- **Jobs.** Mirror sync jobs are listed at the farm base; local bulk jobs at the tenant's (§3.10).
- **Provider pages.** `Special:Providers` lists each provider with its trust mode, last verified checkpoint and lag, and each page repository with the date of its title index, its event lag and, in `mirror` mode, its sync lag ([0053](../decisions/0053-mirrored-pages.md) §3, §5; [13](13-mirrored-pages.md)); a `verified` chip sits on the identity line of mirrored entities ([0022](../decisions/0022-federation.md) §11). What an instance publishes and verifies is [17](17-federation-and-publication.md).
- **Elsewhere in this chapter:** the account page's **Fediverse** section (§2.4), the talk page header's `Group` handle (§1.3), `Special:InstanceAction/{partition}/{offset}` (§3.4) and the contributions of `instance:{farm code}` (§2.5; [0040](../decisions/0040-instance-prerogatives.md) §7), and a private tenant's landing page (§6.3; [0056](../decisions/0056-security-model.md) §13).
