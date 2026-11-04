# MediaWiki + Wikibase API compatibility contract

- **Target:** MediaWiki **1.43 LTS** (point release 1.43.9) with Wikibase **REL1_43** (Repository + Client)
- **Generated:** 2026-09-24
- **Related decisions:** [0000 — Initial proposition](../decisions/0000-init.md) (open question: *compatibility surface*)

This document records the HTTP API contract that MediaWiki 1.43 LTS and Wikibase expose to clients. It is the reference Triplespace measures itself against when it claims MediaWiki or Wikibase compatibility. It does **not** decide how much of this surface Triplespace will implement. That belongs in a separate decision log.

## 1. Reference target and provenance

### 1.1 Why 1.43

MediaWiki 1.43 is the current long-term-support release. It was released on 2024-12-21 and is supported until 2027-12-31. Its latest point release is 1.43.9. Releases 1.44, 1.45 and 1.46 are short-lived non-LTS releases, and no newer LTS has shipped yet. Wikibase does not publish its own releases. It is branched with MediaWiki, so the matching Wikibase is the `REL1_43` branch.

| Component | Ref | Commit | Commit date |
|---|---|---|---|
| MediaWiki core | tag `1.43.9` | `50755a861e6e` | 2026-06-29 |
| Wikibase | branch `REL1_43` | `d561f2905229` | 2026-09-22 |
| mediawiki/vendor | branch `REL1_43` | `e020d9460301` | 2026-08-04 |

### 1.2 How this reference was produced

The Action API documents itself. `Special:ApiHelp`, `api.php?action=help` and `api.php?action=paraminfo` are all generated at runtime from each module's parameter declarations and i18n messages. That makes a running install the authoritative source for its own contract. The pages on mediawiki.org, by contrast, describe the current development branch and Wikimedia production, and they drift away from any particular release.

The reference was therefore **generated from the software itself**:

1. MediaWiki core (`1.43.9`) and Wikibase (`REL1_43`) were cloned from Wikimedia's GitHub mirrors and installed on SQLite.
2. Both `WikibaseRepository` and `WikibaseClient` were enabled using the `ExampleSettings.php` files shipped with Wikibase, and `$wgEnableUploads = true` was set.
3. `action=paraminfo` (`helpformat=wikitext`) was queried recursively for every module and submodule reachable from `main`.
4. The Wikibase REST API's OpenAPI document was fetched from `rest.php/wikibase/v1/openapi.json`.
5. Cross-cutting behaviour (the error, warning, continuation, token and Wikibase response envelopes) was verified with live requests against the same install. Behaviour marked **(observed)** below comes from those requests, not from documentation.

The machine-readable outputs are committed in `docs/samples/wikibase-1.43/` and are normative wherever this prose summarises them:

| File | Contents |
|---|---|
| `../samples/wikibase-1.43/mw-1.43.9-wb-REL1_43.paraminfo.json` | Full `action=paraminfo` output for all 153 modules, plus `main` |
| `../samples/wikibase-1.43/wikibase-rest-v1.openapi.json` | Wikibase REST API OpenAPI document (API version 1.1) |
| `../samples/wikibase-1.43/mw-1.43.9-wb-REL1_43.siteinfo.json` | `meta=siteinfo` (general, extensions, namespaces) for the reference install |

### 1.3 Scope and caveats

- **Modules:** 153 Action API modules in total: 121 from MediaWiki core and 32 from Wikibase (Repo and Client).
- **No other extensions.** Modules supplied by other extensions are **not** part of this contract. That includes WikibaseLexeme (`wbl*`), WikibaseQualityConstraints (`wbcheckconstraints`), WikibaseCirrusSearch, PropertySuggester (`wbsgetsuggestions`), EntitySchema, OAuth, CentralAuth, TimedMediaHandler and similar. **wikidata.org's `api.php` is a superset of this contract**, and it runs weekly `wmf/*` branches of MediaWiki, not 1.43.
- **Config-dependent enumerations are site-specific.** Some parameter values depend on configuration or database content: site IDs (`sites`, `sitefilter`, `site`), badge items, change tags, namespace lists, content models and language codes. The values listed reflect the reference install. For example, the `sites` table is empty, so site-ID enums appear empty. Enumerations longer than 40 values are truncated in this document but are complete in the snapshot.
- **Build environment.** The reference ran on PHP 8.4, which is newer than 1.43 officially targets, with deprecation notices suppressed. Wikibase's Composer dependencies were installed from upstream at the newest versions that satisfy `extensions/Wikibase/composer.json`. None of this changes the API surface.
- **Descriptions are converted.** Parameter descriptions come from the English i18n messages, converted from wikitext to Markdown. Links to `Special:ApiHelp` are reduced to their link text.

## 2. Action API protocol contract

These rules apply to every Action API module. Module-specific parameters are in §4 and §5, and the global parameters that implement several of these rules are listed under the main module in §5.0.

### 2.1 Endpoint and transport

- **Endpoint:** a single endpoint, `{$wgScriptPath}/api.php`. Wikimedia sites use `/w/api.php`, and the reference install uses `/api.php`.
- **Methods:** requests may be `GET` or `POST`. POST bodies may be `application/x-www-form-urlencoded` or `multipart/form-data`, and `action=upload` requires `multipart/form-data`.
- **Write modules:** modules that declare `mustbeposted` reject GET. Every module with a `token` parameter also requires that token (§2.5).
- **Tokens in the query string:** a token placed in the query string instead of the POST body is rejected with `mustpostparams` **(observed)**.
- **Default action:** `action` defaults to `help`. Parameter names and enum values are case-sensitive.
- **Unrecognised parameters:** these are ignored, with a warning (`unrecognizedparams`), not an error.
- **HTTP status:** API errors are returned with **HTTP 200**, and the error code is repeated in the `MediaWiki-API-Error` response header **(observed)**.

### 2.2 Output formats and `formatversion`

- **Format:** `format=json` is the machine format. Also available are `jsonfm` (HTML-pretty), `php`, `phpfm`, `xml`, `xmlfm`, `rawfm` and `none`.
- **`formatversion`:** `formatversion=1` is the default, and `2` is the modern format (`latest` also works). Under formatversion 2:
  - booleans are real JSON `true`/`false` and absent when false, instead of `""` present or absent;
  - lists are JSON arrays, where version 1 uses objects keyed by index or ID;
  - content is no longer placed under `*` keys;
  - UTF-8 is emitted unescaped.
- **Wikibase exceptions (observed):** Wikibase modules keep several version-1 conventions under formatversion 2:
  - `"success": 1` is an integer;
  - `wbgetentities` returns `entities` as an object keyed by ID;
  - missing entities are flagged `"missing": ""`.
  
  `nochange` *does* follow version 2 (`true`). A compatible implementation must reproduce these per-module quirks, not apply formatversion rules uniformly.

### 2.3 Multi-value parameters and limits

- **Separator:** multi-value parameters are separated by `|`. If any value contains `|`, the whole parameter value is prefixed with U+001F, and U+001F is then used as the separator.
- **Per-parameter limits:** each multi-value parameter has a *limit* for normal users and a *high limit* for users with the `apihighlimits` right (bots and sysops). Typically these are 50 and 500.
- **`limit`-type parameters:** these accept an integer up to `max` (or `highmax`), or the literal `max`.
- **Limits are enforced differently by parameter type (observed):**
  - Too many values in a multi-value parameter is an **error**, `toomanyvalues`, with `parameter`, `limit`, `lowlimit` and `highlimit` fields.
  - A `limit`-type value above its maximum is **clamped** with a warning.

### 2.4 Errors and warnings

The error envelope depends on `errorformat`.

**`errorformat=bc` (default)** returns a single error object. Module-specific fields are merged into it, as Wikibase does with `messages` and `id`:

```json
{"error": {"code": "no-such-entity",
           "info": "Could not find an entity with the ID \"X1\".",
           "id": "X1",
           "messages": [{"name": "wikibase-api-no-such-entity", "parameters": ["X1"], "html": "…"}],
           "docref": "See …/api.php for API usage. …"}}
```

Warnings in this format are keyed by module: `{"warnings": {"main": {"warnings": "Unrecognized parameter: foo."}}}`.

**`errorformat=plaintext|wikitext|html|raw|none`** returns arrays, with `code`, `text` (or `key`/`params` for `raw`), `data` and `module` on each entry:

```json
{"errors":   [{"code": "no-such-entity", "text": "Could not find an entity with the ID \"X1\".", "data": {…}, "module": "main"}],
 "warnings": [{"code": "unrecognizedparams", "text": "Unrecognized parameter: foo.", "module": "main"}]}
```

`errorlang` and `errorsuselocal` control localisation. Error codes are part of the contract: clients branch on them. Core codes include `missingparam`, `badvalue`, `badinteger`, `toomanyvalues`, `badtoken`, `mustbeposted`, `mustpostparams`, `permissiondenied`, `maxlag`, `readonly`, `ratelimited` and `assertuserfailed`. Wikibase uses its own codes, such as `no-such-entity`, `nosuchrevid`, `modification-failed`, `failed-save` and `invalid-snak`.

### 2.5 Tokens

- **Fetching tokens:** use `action=query&meta=tokens&type=…`. The types are `createaccount`, `csrf`, `login`, `patrol`, `rollback`, `userrights` and `watch`.
- **Token parameter:** every module with a `token` parameter declares which type it needs (listed per module below). Nearly all write modules use `csrf`.
- **Anonymous token:** an anonymous session's CSRF token is the constant string `+\` **(observed)**.
- **Anonymous editing:** the reference config permits anonymous edits, so every live write in this document was made with that token.
- **Error codes:** a missing token gives `missingparam`, and a wrong token gives `badtoken`.

### 2.6 Continuation

Query results that do not fit in one response carry a `continue` object. Clients must send **every key** in it back, unchanged, with the original parameters to get the next batch. `batchcomplete` signals that all `prop` data for the current set of pages is complete. Example **(observed)**:

```json
{"batchcomplete": true,
 "continue": {"rccontinue": "20260925004332|1", "continue": "-||"},
 "query": {"recentchanges": […]}}
```

The legacy `rawcontinue` mode (a `query-continue` object) is still accepted.

### 2.7 Generators

Any `prop`, `list` or `meta` module flagged as a generator can supply the page set for `prop` modules via `generator=<name>`. The generator's own parameters then take a `g` prefix (for example `generator=allpages&gaplimit=10`). `redirects`, `converttitles`, `titles`, `pageids` and `revids` form the page-set parameters of `action=query` (see [`action=query`](#mod-query) in §5.1).

### 2.8 Control and etiquette parameters

These are global parameters defined on `main` (see §5.0 for the exact definitions):

- **`maxlag`:** fail with code `maxlag` (and a `Retry-After` header) if database replication lag exceeds N seconds.
- **`assert` / `assertuser`:** fail unless the request comes from an `anon`, `user` or `bot` session (`assert`), or from the named user (`assertuser`).
- **`smaxage` / `maxage`:** set cache-control headers.
- **`origin`:** CORS.
- **`uselang`, `variant`:** the language of localised output.
- **`requestid`, `servedby`, `curtimestamp`, `responselanginfo`:** echo or diagnostic fields.

### 2.9 Authentication

- **Sessions:** the Action API uses cookie-based sessions.
- **Programmatic login:** use `action=login` with a bot password (`lgname`, `lgpassword`, `lgtoken`), or `action=clientlogin` with AuthManager requests.
- **OAuth:** OAuth 1.0a/2.0 bearer tokens need the OAuth extension and are outside this contract.
- **Rights:** each module declares whether it needs read or write rights. Rate limits (`$wgRateLimits`) surface as `ratelimited` errors.

### 2.10 Data types on the wire

- **`timestamp`:** accepts ISO 8601 (`2026-09-25T00:45:38Z`), MediaWiki's 14-digit form (`20260925004538`) and `now`. Output is always ISO 8601 UTC with a `Z` suffix.
- **`boolean`:** true if the parameter is present at all, whatever its value. `…&redirects=false` means **true**. Send false by omitting the parameter.
- **`namespace`:** an integer namespace ID.
- **`title` / `user`:** normalised on input (underscores become spaces, the first letter is capitalised per namespace rules), and the normalisation is reported in `normalized`.
- **`upload`:** a multipart file field.
- **`password`:** sensitive. It must be POSTed and is never logged.


## 3. Action API module index

Origin is determined by implementing class namespace. *Method* is whether the module rejects GET (`mustbeposted`). *Write* means the module requires write rights. *Token* is the token type that must be supplied in the `token` parameter.

### 3.1 Actions (`action=`)

| Module | Origin | Method | Write | Token | Summary |
|---|---|---|---|---|---|
| [`acquiretempusername`](#mod-acquiretempusername) | Core | POST | yes |  | Acquire a temporary user username and stash it in the current session, if temp account creation is enabled and the current user is logged out. If a name has already been stashed, returns the same name. |
| [`block`](#mod-block) | Core | POST | yes | `csrf` | Block a user. |
| [`changeauthenticationdata`](#mod-changeauthenticationdata) | Core | POST | yes | `csrf` | Change authentication data for the current user. |
| [`changecontentmodel`](#mod-changecontentmodel) | Core | POST | yes | `csrf` | Change the content model of a page |
| [`checktoken`](#mod-checktoken) | Core | GET/POST |  |  | Check the validity of a token from `action=query&meta=tokens`. |
| [`clearhasmsg`](#mod-clearhasmsg) | Core | POST | yes |  | Clears the `hasmsg` flag for the current user. |
| [`clientlogin`](#mod-clientlogin) | Core | POST | yes | `login` | Log in to the wiki using the interactive flow. |
| [`compare`](#mod-compare) | Core | GET/POST |  |  | Get the difference between two pages. |
| [`createaccount`](#mod-createaccount) | Core | POST | yes | `createaccount` | Create a new user account. |
| [`cspreport`](#mod-cspreport) *(internal)* | Core | POST |  |  | Used by browsers to report violations of the Content Security Policy. This module should never be used, except when used automatically by a CSP compliant web browser. |
| [`delete`](#mod-delete) | Core | POST | yes | `csrf` | Delete a page. |
| [`edit`](#mod-edit) | Core | POST | yes | `csrf` | Create and edit pages. |
| [`emailuser`](#mod-emailuser) | Core | POST | yes | `csrf` | Email a user. |
| [`expandtemplates`](#mod-expandtemplates) | Core | GET/POST |  |  | Expands all templates within wikitext. |
| [`feedcontributions`](#mod-feedcontributions) | Core | GET/POST |  |  | Returns a user's contributions feed. |
| [`feedrecentchanges`](#mod-feedrecentchanges) | Core | GET/POST |  |  | Returns a recent changes feed. |
| [`feedwatchlist`](#mod-feedwatchlist) | Core | GET/POST |  |  | Returns a watchlist feed. |
| [`filerevert`](#mod-filerevert) | Core | POST | yes | `csrf` | Revert a file to an old version. |
| [`help`](#mod-help) | Core | GET/POST |  |  | Display help for the specified modules. |
| [`imagerotate`](#mod-imagerotate) | Core | POST | yes | `csrf` | Rotate one or more images. |
| [`import`](#mod-import) | Core | POST | yes | `csrf` | Import a page from another wiki, or from an XML file. |
| [`linkaccount`](#mod-linkaccount) | Core | POST | yes | `csrf` | Link an account from a third-party provider to the current user. |
| [`login`](#mod-login) | Core | POST | yes |  | Log in and get authentication cookies. |
| [`logout`](#mod-logout) | Core | POST | yes | `csrf` | Log out and clear session data. |
| [`managetags`](#mod-managetags) | Core | POST | yes | `csrf` | Perform management tasks relating to change tags. |
| [`mergehistory`](#mod-mergehistory) | Core | POST | yes | `csrf` | Merge page histories. |
| [`move`](#mod-move) | Core | POST | yes | `csrf` | Move a page. |
| [`opensearch`](#mod-opensearch) | Core | GET/POST |  |  | Search the wiki using the OpenSearch protocol. |
| [`options`](#mod-options) | Core | POST | yes | `csrf` | Change preferences of the current user. |
| [`paraminfo`](#mod-paraminfo) | Core | GET/POST |  |  | Obtain information about API modules. |
| [`parse`](#mod-parse) | Core | GET/POST |  |  | Parses content and returns parser output. |
| [`patrol`](#mod-patrol) | Core | POST | yes | `patrol` | Patrol a page or revision. |
| [`protect`](#mod-protect) | Core | POST | yes | `csrf` | Change the protection level of a page. |
| [`purge`](#mod-purge) | Core | POST | yes |  | Purge the cache for the given titles. |
| [`query`](#mod-query) | Core | GET/POST |  |  | Fetch data from and about MediaWiki. |
| [`removeauthenticationdata`](#mod-removeauthenticationdata) | Core | POST | yes | `csrf` | Remove authentication data for the current user. |
| [`resetpassword`](#mod-resetpassword) | Core | POST | yes | `csrf` | Send a password reset email to a user. |
| [`revisiondelete`](#mod-revisiondelete) | Core | POST | yes | `csrf` | Delete and undelete revisions. |
| [`rollback`](#mod-rollback) | Core | POST | yes | `rollback` | Undo the last edit to the page. |
| [`rsd`](#mod-rsd) | Core | GET/POST |  |  | Export an RSD (Really Simple Discovery) schema. |
| [`setnotificationtimestamp`](#mod-setnotificationtimestamp) | Core | POST | yes | `csrf` | Update the notification timestamp for watched pages. |
| [`setpagelanguage`](#mod-setpagelanguage) | Core | POST | yes | `csrf` | Change the language of a page. |
| [`stashedit`](#mod-stashedit) *(internal)* | Core | POST | yes | `csrf` | Prepare an edit in shared cache. |
| [`tag`](#mod-tag) | Core | POST | yes | `csrf` | Add or remove change tags from individual revisions or log entries. |
| [`unblock`](#mod-unblock) | Core | POST | yes | `csrf` | Unblock a user. |
| [`undelete`](#mod-undelete) | Core | POST | yes | `csrf` | Undelete revisions of a deleted page. |
| [`unlinkaccount`](#mod-unlinkaccount) | Core | POST | yes | `csrf` | Remove a linked third-party account from the current user. |
| [`upload`](#mod-upload) | Core | POST | yes | `csrf` | Upload a file, or get the status of pending uploads. |
| [`userrights`](#mod-userrights) | Core | POST | yes | `userrights` | Change a user's group membership. |
| [`validatepassword`](#mod-validatepassword) | Core | POST |  |  | Validate a password against the wiki's password policies. |
| [`watch`](#mod-watch) | Core | POST | yes | `watch` | Add or remove pages from the current user's watchlist. |
| [`wbavailablebadges`](#mod-wbavailablebadges) | Wikibase Repo | GET/POST |  |  | Queries available badge items. |
| [`wbcreateclaim`](#mod-wbcreateclaim) | Wikibase Repo | POST | yes | `csrf` | Creates Wikibase claims. |
| [`wbcreateredirect`](#mod-wbcreateredirect) | Wikibase Repo | POST | yes | `csrf` | Creates Entity redirects. |
| [`wbeditentity`](#mod-wbeditentity) | Wikibase Repo | POST | yes | `csrf` | Creates a single new Wikibase entity and modifies it with serialised information. |
| [`wbformatentities`](#mod-wbformatentities) | Wikibase Repo | GET/POST |  |  | Formats entity IDs to HTML. |
| [`wbformatvalue`](#mod-wbformatvalue) | Wikibase Repo | GET/POST |  |  | Formats DataValues. |
| [`wbgetclaims`](#mod-wbgetclaims) | Wikibase Repo | GET/POST |  |  | Gets Wikibase claims. |
| [`wbgetentities`](#mod-wbgetentities) | Wikibase Repo | GET/POST |  |  | Gets the data for multiple Wikibase entities. |
| [`wblinktitles`](#mod-wblinktitles) | Wikibase Repo | POST | yes | `csrf` | Associates two pages on two different wikis with a Wikibase item. |
| [`wbmergeitems`](#mod-wbmergeitems) | Wikibase Repo | POST | yes | `csrf` | Merges multiple items. |
| [`wbparsevalue`](#mod-wbparsevalue) | Wikibase Repo | GET/POST |  |  | Parses values using a `ValueParser`. |
| [`wbremoveclaims`](#mod-wbremoveclaims) | Wikibase Repo | POST | yes | `csrf` | Removes Wikibase claims. |
| [`wbremovequalifiers`](#mod-wbremovequalifiers) | Wikibase Repo | POST | yes | `csrf` | Removes a qualifier from a claim. |
| [`wbremovereferences`](#mod-wbremovereferences) | Wikibase Repo | POST | yes | `csrf` | Removes one or more references of the same statement. |
| [`wbsearchentities`](#mod-wbsearchentities) | Wikibase Repo | GET/POST |  |  | Searches for entities using labels and aliases. |
| [`wbsetaliases`](#mod-wbsetaliases) | Wikibase Repo | POST | yes | `csrf` | Sets the aliases for a Wikibase entity. |
| [`wbsetclaim`](#mod-wbsetclaim) | Wikibase Repo | POST | yes | `csrf` | Creates or updates an entire Statement or Claim. |
| [`wbsetclaimvalue`](#mod-wbsetclaimvalue) | Wikibase Repo | POST | yes | `csrf` | Sets the value of a Wikibase claim. |
| [`wbsetdescription`](#mod-wbsetdescription) | Wikibase Repo | POST | yes | `csrf` | Sets a description for a single Wikibase entity. |
| [`wbsetlabel`](#mod-wbsetlabel) | Wikibase Repo | POST | yes | `csrf` | Sets a label for a single Wikibase entity. |
| [`wbsetqualifier`](#mod-wbsetqualifier) | Wikibase Repo | POST | yes | `csrf` | Creates a qualifier or sets the value of an existing one. |
| [`wbsetreference`](#mod-wbsetreference) | Wikibase Repo | POST | yes | `csrf` | Creates a reference or sets the value of an existing one. |
| [`wbsetsitelink`](#mod-wbsetsitelink) | Wikibase Repo | POST | yes | `csrf` | Associates a page on a wiki with a Wikibase item or removes an already made such association. |

### 3.2 Property modules (`action=query&prop=`)

| Module | Origin | Method | Write | Token | Summary |
|---|---|---|---|---|---|
| [`categories`](#mod-query-categories) | Core | GET/POST |  |  | List all categories the pages belong to. |
| [`categoryinfo`](#mod-query-categoryinfo) | Core | GET/POST |  |  | Returns information about the given categories. |
| [`contributors`](#mod-query-contributors) | Core | GET/POST |  |  | Get the list of logged-in contributors and the count of logged-out contributors to a page. |
| [`deletedrevisions`](#mod-query-deletedrevisions) | Core | GET/POST |  |  | Get deleted revision information. |
| [`description`](#mod-query-description) *(internal)* | Wikibase Client | GET/POST |  |  | Get a short description a.k.a. subtitle explaining what the target page is about. |
| [`duplicatefiles`](#mod-query-duplicatefiles) | Core | GET/POST |  |  | List all files that are duplicates of the given files based on hash values. |
| [`entityterms`](#mod-query-entityterms) | Wikibase Repo | GET/POST |  |  | Get the terms (labels, descriptions and aliases) of the entity on this page. |
| [`extlinks`](#mod-query-extlinks) | Core | GET/POST |  |  | Returns all external URLs (not interwikis) from the given pages. |
| [`fileusage`](#mod-query-fileusage) | Core | GET/POST |  |  | Find all pages that use the given files. |
| [`imageinfo`](#mod-query-imageinfo) | Core | GET/POST |  |  | Returns file information and upload history. |
| [`images`](#mod-query-images) | Core | GET/POST |  |  | Returns all files contained on the given pages. |
| [`info`](#mod-query-info) | Core | GET/POST |  |  | Get basic page information. |
| [`iwlinks`](#mod-query-iwlinks) | Core | GET/POST |  |  | Returns all interwiki links from the given pages. |
| [`langlinks`](#mod-query-langlinks) | Core | GET/POST |  |  | Returns all interlanguage links from the given pages. |
| [`links`](#mod-query-links) | Core | GET/POST |  |  | Returns all links from the given pages. |
| [`linkshere`](#mod-query-linkshere) | Core | GET/POST |  |  | Find all pages that link to the given pages. |
| [`pageprops`](#mod-query-pageprops) | Core | GET/POST |  |  | Get various page properties defined in the page content. |
| [`pageterms`](#mod-query-pageterms) | Wikibase Client | GET/POST |  |  | Get the Triplespace Ref terms (typically labels, descriptions and aliases) associated with a page via a sitelink. |
| [`redirects`](#mod-query-redirects) | Core | GET/POST |  |  | Returns all redirects to the given pages. |
| [`revisions`](#mod-query-revisions) | Core | GET/POST |  |  | Get revision information. |
| [`stashimageinfo`](#mod-query-stashimageinfo) | Core | GET/POST |  |  | Returns file information for stashed files. |
| [`templates`](#mod-query-templates) | Core | GET/POST |  |  | Returns all pages transcluded on the given pages. |
| [`transcludedin`](#mod-query-transcludedin) | Core | GET/POST |  |  | Find all pages that transclude the given pages. |
| [`wbentityusage`](#mod-query-wbentityusage) | Wikibase Client | GET/POST |  |  | Returns all entity IDs used in the given pages. |

### 3.3 List modules (`action=query&list=`)

| Module | Origin | Method | Write | Token | Summary |
|---|---|---|---|---|---|
| [`allcategories`](#mod-query-allcategories) | Core | GET/POST |  |  | Enumerate all categories. |
| [`alldeletedrevisions`](#mod-query-alldeletedrevisions) | Core | GET/POST |  |  | List all deleted revisions by a user or in a namespace. |
| [`allfileusages`](#mod-query-allfileusages) | Core | GET/POST |  |  | List all file usages, including non-existing. |
| [`allimages`](#mod-query-allimages) | Core | GET/POST |  |  | Enumerate all images sequentially. |
| [`alllinks`](#mod-query-alllinks) | Core | GET/POST |  |  | Enumerate all links that point to a given namespace. |
| [`allpages`](#mod-query-allpages) | Core | GET/POST |  |  | Enumerate all pages sequentially in a given namespace. |
| [`allredirects`](#mod-query-allredirects) | Core | GET/POST |  |  | List all redirects to a namespace. |
| [`allrevisions`](#mod-query-allrevisions) | Core | GET/POST |  |  | List all revisions. |
| [`alltransclusions`](#mod-query-alltransclusions) | Core | GET/POST |  |  | List all transclusions (pages embedded using {{x}}), including non-existing. |
| [`allusers`](#mod-query-allusers) | Core | GET/POST |  |  | Enumerate all registered users. |
| [`backlinks`](#mod-query-backlinks) | Core | GET/POST |  |  | Find all pages that link to the given page. |
| [`blocks`](#mod-query-blocks) | Core | GET/POST |  |  | List all blocked users and IP addresses. |
| [`categorymembers`](#mod-query-categorymembers) | Core | GET/POST |  |  | List all pages in a given category. |
| [`deletedrevs`](#mod-query-deletedrevs) *(deprecated)* | Core | GET/POST |  |  | List deleted revisions. |
| [`embeddedin`](#mod-query-embeddedin) | Core | GET/POST |  |  | Find all pages that embed (transclude) the given title. |
| [`exturlusage`](#mod-query-exturlusage) | Core | GET/POST |  |  | Enumerate pages that contain a given URL. |
| [`filearchive`](#mod-query-filearchive) | Core | GET/POST |  |  | Enumerate all deleted files sequentially. |
| [`imageusage`](#mod-query-imageusage) | Core | GET/POST |  |  | Find all pages that use the given image title. |
| [`iwbacklinks`](#mod-query-iwbacklinks) | Core | GET/POST |  |  | Find all pages that link to the given interwiki link. |
| [`langbacklinks`](#mod-query-langbacklinks) | Core | GET/POST |  |  | Find all pages that link to the given language link. |
| [`logevents`](#mod-query-logevents) | Core | GET/POST |  |  | Get events from logs. |
| [`mystashedfiles`](#mod-query-mystashedfiles) | Core | GET/POST |  |  | Get a list of files in the current user's upload stash. |
| [`pagepropnames`](#mod-query-pagepropnames) | Core | GET/POST |  |  | List all page property names in use on the wiki. |
| [`pageswithprop`](#mod-query-pageswithprop) | Core | GET/POST |  |  | List all pages using a given page property. |
| [`prefixsearch`](#mod-query-prefixsearch) | Core | GET/POST |  |  | Perform a prefix search for page titles. |
| [`protectedtitles`](#mod-query-protectedtitles) | Core | GET/POST |  |  | List all titles protected from creation. |
| [`querypage`](#mod-query-querypage) | Core | GET/POST |  |  | Get a list provided by a QueryPage-based special page. |
| [`random`](#mod-query-random) | Core | GET/POST |  |  | Get a set of random pages. |
| [`recentchanges`](#mod-query-recentchanges) | Core | GET/POST |  |  | Enumerate recent changes. |
| [`search`](#mod-query-search) | Core | GET/POST |  |  | Perform a full text search. |
| [`tags`](#mod-query-tags) | Core | GET/POST |  |  | List change tags. |
| [`usercontribs`](#mod-query-usercontribs) | Core | GET/POST |  |  | Get all edits by a user. |
| [`users`](#mod-query-users) | Core | GET/POST |  |  | Get information about a list of users. |
| [`watchlist`](#mod-query-watchlist) | Core | GET/POST |  |  | Get recent changes to pages in the current user's watchlist. |
| [`watchlistraw`](#mod-query-watchlistraw) | Core | GET/POST |  |  | Get all pages on the current user's watchlist. |
| [`wblistentityusage`](#mod-query-wblistentityusage) | Wikibase Client | GET/POST |  |  | Returns all pages that use the given entity IDs. |
| [`wbsearch`](#mod-query-wbsearch) *(internal)* | Wikibase Repo | GET/POST |  |  | Searches for entities using labels and aliases. |
| [`wbsubscribers`](#mod-query-wbsubscribers) | Wikibase Repo | GET/POST |  |  | Get subscriptions to given entities. |

### 3.4 Meta modules (`action=query&meta=`)

| Module | Origin | Method | Write | Token | Summary |
|---|---|---|---|---|---|
| [`allmessages`](#mod-query-allmessages) | Core | GET/POST |  |  | Return messages from this site. |
| [`authmanagerinfo`](#mod-query-authmanagerinfo) | Core | GET/POST |  |  | Retrieve information about the current authentication status. |
| [`filerepoinfo`](#mod-query-filerepoinfo) | Core | GET/POST |  |  | Return meta information about image repositories configured on the wiki. |
| [`languageinfo`](#mod-query-languageinfo) | Core | GET/POST |  |  | Return information about available languages. |
| [`siteinfo`](#mod-query-siteinfo) | Core | GET/POST |  |  | Return general information about the site. |
| [`tokens`](#mod-query-tokens) | Core | GET/POST |  |  | Gets tokens for data-modifying actions. |
| [`userinfo`](#mod-query-userinfo) | Core | GET/POST |  |  | Get information about the current user. |
| [`wbcontentlanguages`](#mod-query-wbcontentlanguages) | Wikibase Repo | GET/POST |  |  | Returns information about the content languages Wikibase accepts in different contexts. |
| [`wikibase`](#mod-query-wikibase) | Wikibase Client | GET/POST |  |  | Get information about the Wikibase client and the associated Wikibase repository. |

### 3.5 Output formats (`format=`)

| Module | Origin | Method | Write | Token | Summary |
|---|---|---|---|---|---|
| [`json`](#mod-json) | Core | GET/POST |  |  | Output data in JSON format. |
| [`jsonfm`](#mod-jsonfm) | Core | GET/POST |  |  | Output data in JSON format (pretty-print in HTML). |
| [`none`](#mod-none) | Core | GET/POST |  |  | Output nothing. |
| [`php`](#mod-php) | Core | GET/POST |  |  | Output data in serialized PHP format. |
| [`phpfm`](#mod-phpfm) | Core | GET/POST |  |  | Output data in serialized PHP format (pretty-print in HTML). |
| [`rawfm`](#mod-rawfm) | Core | GET/POST |  |  | Output data, including debugging elements, in JSON format (pretty-print in HTML). |
| [`xml`](#mod-xml) | Core | GET/POST |  |  | Output data in XML format. |
| [`xmlfm`](#mod-xmlfm) | Core | GET/POST |  |  | Output data in XML format (pretty-print in HTML). |

## 4. Wikibase Action API reference

Every module added by Wikibase Repository and Wikibase Client, with full parameter contracts as reported by `action=paraminfo` on the reference build. Wikibase-specific response semantics that paraminfo does not capture are noted in §4.0.

### 4.0 Wikibase-specific semantics

paraminfo records parameters. It does not record response shapes. The following were verified against the reference install **(observed)** unless a source is cited.

**Entity pages and namespaces.**
- Entities are stored as wiki pages whose content model is `wikibase-item` or `wikibase-property`.
- With the shipped `ExampleSettings.php`, items live in namespace 120 (`Item:Q1`) and properties in namespace 122 (`Property:P1`). Wikidata instead keeps items in namespace 0.
- Page titles, page IDs and namespaces appear in responses (`pageid`, `ns`, `title`), and they depend on this configuration.
- Entity IDs are `Q<n>` for items and `P<n>` for properties. Statement GUIDs take the form `<EntityId>$<UUID>`, for example `Q1$C7C2A847-90FB-4764-A616-066DA578A9C9`.

**Common write parameters.** Every Wikibase write module takes:
- `token` (`csrf`)
- `summary` (appended to an auto-generated summary)
- `tags` (change tags)
- `bot` (marks the edit as a bot edit, if the user has the right)
- `baserevid`
- `returnto`, `returntoquery` and `returntoanchor` (used for temporary-account redirects)

**`baserevid` and conflict handling.** `baserevid` names the revision the client based its edit on.
- If the entity has changed since then, Wikibase tries to **patch** the client's change onto the latest revision. On success the edit is saved with warning `wikibase-conflict-patched`.
- A patch that cannot be applied fails with an edit-conflict error.
- A `baserevid` that does not exist fails with `nosuchrevid`.
- **Quirk:** when `wbeditentity` patches around a conflict, the `entity` in its response is built from the *base* revision plus the submitted change. It is not the stored entity, so labels, aliases and statements added in between are missing from the response. Clients must re-fetch.

**Response envelopes.**
- Every successful Wikibase response carries `"success": 1`, an integer, even under `formatversion=2`.
- **`wbeditentity`** returns `{"entity": {…full serialization…, "lastrevid": N}, "success": 1}`.
- **`wbsetlabel`, `wbsetdescription`, `wbsetaliases` and `wbsetsitelink`** return a *partial* entity containing only the touched term, plus `id`, `type` and `lastrevid`:
  ```json
  {"entity": {"labels": {"de": {"language": "de", "value": "Douglas Adams"}}, "id": "Q1", "type": "item", "lastrevid": 5}, "success": 1}
  ```
- **No-op edits:** these return `"nochange": true` (formatversion 2) inside `entity`, and no revision is created.
- **Statement modules** return `{"pageinfo": {"lastrevid": N}, "success": 1, …}`. Some add more, per the module source:
  - `wbcreateclaim`, `wbsetclaim`, `wbsetclaimvalue` and `wbsetqualifier` add the full resulting statement as `claim`;
  - `wbsetreference` adds `reference`;
  - `wbremoveclaims` adds `claims`, a list of the removed GUIDs;
  - `wbremovequalifiers` and `wbremovereferences` return only `pageinfo`.
- **`wbgetentities`** returns `{"entities": {"<id>": {…}}, "success": 1}`. Missing IDs are not an error: they come back as `{"id": "Q999", "missing": ""}`. With `languagefallback`, a term served from a fallback language carries `language` (the actual language) and `for-language` (the requested one).
- **Errors** use Wikibase codes and add a `messages` array (`name`, `parameters`, `html`) to the standard envelope (§2.4).

**Entity JSON.** The Action API serialises entities in the canonical Wikibase JSON format (`docs/topics/json.md` in the Wikibase tree):
- Top-level keys are `type`, `id`, `labels`, `descriptions`, `aliases`, `claims`, `sitelinks` (items only), `datatype` (properties only) and `lastrevid`.
- `wbgetentities` with `props=info` also adds `pageid`, `ns`, `title` and `modified`.
- Statements live under **`claims`**, keyed by property ID. The REST API calls the same thing `statements` and flattens terms (§6.2).

**Data types.** The reference install registers 12 property data types. Wikidata registers more through other extensions: `math`, `musical-notation`, `wikibase-lexeme`/`form`/`sense`, `entity-schema` and others.

| Data type | Value type |
|---|---|
| `commonsMedia`, `external-id`, `geo-shape`, `string`, `tabular-data`, `url` | `string` |
| `globe-coordinate` | `globecoordinate` |
| `monolingualtext` | `monolingualtext` |
| `quantity` | `quantity` |
| `time` | `time` |
| `wikibase-item`, `wikibase-property` | `wikibase-entityid` |

**Config-dependent enumerations.** Several parameters take values that depend on the site:
- `site`, `sites` and `sitefilter` take site IDs from the `sites` table. That table is empty on the reference install, so these enums appear empty.
- `badges` takes the items configured in `badgeItems`.
- `languages` takes the 605 term-language codes on the reference install.
- `datatype` takes the registered data types.


### 4.1 Wikibase actions

<a id="mod-wbavailablebadges"></a>
#### `action=wbavailablebadges`

*Wikibase Repo* · `Wikibase\Repo\Api\AvailableBadges` · GET or POST

Queries available badge items.

Examples:

- `api.php?action=wbavailablebadges` — Queries all available badge items

<a id="mod-wbcreateclaim"></a>
#### `action=wbcreateclaim`

*Wikibase Repo* · `Wikibase\Repo\Api\CreateClaim` · POST only · write · token `csrf`

Creates Wikibase claims.

Parameters:

- **`entity`** — string; **required**  
  ID of the entity the claim is being added to
- **`snaktype`** — enum: `novalue`, `somevalue`, `value`; **required**  
  The type of the snak
- **`property`** — string; **required**  
  ID of the snaks property
- **`value`** — text  
  Value of the snak when creating a claim with a snak that has a value
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbcreateclaim&entity=Q999999998&property=P9001&snaktype=novalue` — Creates a claim for item `Q999999998` of property `P9001` with a "no value" snak.
- `api.php?action=wbcreateclaim&entity=Q999999998&property=P9002&snaktype=value&value="itsastring"` — Creates a claim for item `Q999999998` of property `P9002` with string value "`itsastring`"
- `api.php?action=wbcreateclaim&entity=Q999999998&property=P9003&snaktype=value&value={"entity-type":"item","numeric-id":1}` — Creates a claim for item `Q999999998` of property `P9003` with a value of item `Q1`
- `api.php?action=wbcreateclaim&entity=Q999999998&property=P9004&snaktype=value&value={"latitude":40.748433,"longitude":-73.985656,"globe":"http://www.wikidata.org/entity/Q2","precision":0.000001}` — Creates a claim for item `Q999999998` of property `P9004` with a coordinate snak value

<a id="mod-wbcreateredirect"></a>
#### `action=wbcreateredirect`

*Wikibase Repo* · `Wikibase\Repo\Api\CreateRedirect` · POST only · write · token `csrf`

Creates Entity redirects.

Parameters:

- **`from`** — string; **required**  
  Entity ID to make a redirect
- **`to`** — string; **required**  
  Entity ID to point the redirect to
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbcreateredirect&from=Q999999998&to=Q999999999` — Turn `Q999999998` into a redirect to `Q999999999`

<a id="mod-wbeditentity"></a>
#### `action=wbeditentity`

*Wikibase Repo* · `Wikibase\Repo\Api\EditEntity` · POST only · write · token `csrf`

Creates a single new Wikibase entity and modifies it with serialised information.

Parameters:

- **`id`** — string  
  The identifier for the entity, including the prefix.
  Use either `id` or `site` and `title` together.
- **`new`** — enum: `item`, `property`  
  If set, a new entity will be created.
  Set this to the type of the entity to be created.
  It is not allowed to have this set when `id` is also set.
- **`site`** — enum (no values in reference config — site-/config-dependent)  
  An identifier for the site on which the page resides.
  Use together with `title` to make a complete sitelink.
- **`title`** — string  
  Title of the page to associate.
  Use together with `site` to make a complete sitelink.
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`data`** — text; **required**  
  The serialized object that is used as the data source.
  A newly created entity will be assigned an 'id'.
- **`clear`** — boolean  
  If set, the complete entity is emptied before proceeding.
  The entity will not be saved before it is filled with the "`data`", possibly with parts excluded.

Examples:

- `api.php?action=wbeditentity&new=item&data={}` — Create a new empty item, return full entity structure
- `api.php?action=wbeditentity&new=item&data={"labels":{"de":{"language":"de","value":"de-value"},"en":{"language":"en","value":"en-value"}}}` — Create a new item and set labels for `de` and `en`
- `api.php?action=wbeditentity&new=property&data={"labels":{"en-gb":{"language":"en-gb","value":"Propertylabel"}},"descriptions":{"en-gb":{"language":"en-gb","value":"Propertydescription"}},"datatype":"string"}` — Create a new property containing the json data, return full entity structure
- `api.php?action=wbeditentity&clear=true&id=Q999999998&data={}` — Clear all data from entity with ID `Q999999998`
- `api.php?action=wbeditentity&clear=true&id=Q999999998&data={"labels":{"en":{"language":"en","value":"en-value"}}}` — Clear all data from entity with ID `Q999999998` and set a label for `en`
- `api.php?action=wbeditentity&id=Q999999998&data={"labels":[{"language":"no","value":"Bar","add":""}]}` — Adds a label without overwriting it if it already exists
- `api.php?action=wbeditentity&id=Q999999998&data={"labels":[{"language":"en","value":"Foo","remove":""}]}` — Removes a label
- `api.php?action=wbeditentity&id=Q999999998&data={"sitelinks":{"nowiki":{"site":"nowiki","title":"København"}}}` — Sets sitelink for `nowiki`, overwriting it if it already exists
- `api.php?action=wbeditentity&id=Q999999998&data={"descriptions":{"nb":{"language":"nb","value":"nb-Description-Here"}}}` — Sets description for `nb`, overwriting it if it already exists
- `api.php?action=wbeditentity&id=Q999999998&data={"claims":[{"mainsnak":{"snaktype":"value","property":"P56","datavalue":{"value":"ExampleString","type":"string"}},"type":"statement","rank":"normal"}]}` — Creates a new claim on the item for the property `P56` and a value of "`ExampleString`"
- `api.php?action=wbeditentity&id=Q999999998&data={"claims":[{"id":"Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F","remove":""},{"id":"Q999999998$GH678DSA-01PQ-28XC-HJ90-DDFD9990126X","remove":""}]}` — Removes the claims from the item with the provided GUIDs
- `api.php?action=wbeditentity&id=Q999999998&data={"claims":[{"id":"Q999999998$GH678DSA-01PQ-28XC-HJ90-DDFD9990126X","mainsnak":{"snaktype":"value","property":"P56","datavalue":{"value":"ChangedString","type":"string"}},"type":"statement","rank":"normal"}]}` — Sets the claim with the GUID to the value of the claim

<a id="mod-wbformatentities"></a>
#### `action=wbformatentities`

*Wikibase Repo* · `Wikibase\Repo\Api\FormatEntities` · GET or POST

Formats entity IDs to HTML.

The language can be specified with the global `uselang` parameter.

Parameters:

- **`ids`** — string; multi-value (max 50; 500 for high-limit users)  
  The entity IDs to format.

Examples:

- `api.php?action=wbformatentities&ids=Q2` — Format a single item ID.
- `api.php?action=wbformatentities&ids=Q2|P2` — Format an item ID and a property ID.
- `api.php?action=wbformatentities&ids=Q2|Q3|Q4&uselang=fr` — Format three item IDs in French.

<a id="mod-wbformatvalue"></a>
#### `action=wbformatvalue`

*Wikibase Repo* · `Wikibase\Repo\Api\FormatSnakValue` · GET or POST

Formats DataValues.

Parameters:

- **`generate`** — enum: `text/html`, `text/html; disposition=verbose`, `text/html; disposition=verbose-preview`, `text/plain`, `text/x-wiki`; default `text/x-wiki`  
  The desired output format to generate.
- **`datavalue`** — text; **required**  
  The data to format. This has to be the JSON serialization of a DataValue object.
- **`datatype`** — enum: `commonsMedia`, `external-id`, `geo-shape`, `globe-coordinate`, `monolingualtext`, `quantity`, `string`, `tabular-data`, `time`, `url`, `wikibase-item`, `wikibase-property`  
  The value's data type. This is distinct from the value's type
- **`property`** — string  
  Property ID the data value belongs to, should be used instead of the `datatype` parameter.
- **`options`** — text  
  The options the formatter should use. Provided as a JSON object.

Examples:

- `api.php?action=wbformatvalue&datavalue=%7B%22value%22%3A%22hello%22%2C%22type%22%3A%22string%22%7D` — Format a simple string value.
- `api.php?action=wbformatvalue&datavalue=%7B%22value%22%3A%22http%3A%5C%2F%5C%2Facme.org%22%2C%22type%22%3A%22string%22%7D&datatype=url&generate=text%2Fhtml` — Format a string value as a URL in HTML.
- `api.php?action=wbformatvalue&datavalue=%7B%22value%22%3A%7B%22time%22%3A%22%2B1879-03-14T00%3A00%3A00Z%22%2C%22timezone%22%3A0%2C%22before%22%3A0%2C%22after%22%3A0%2C%22precision%22%3A11%2C%22calendarmodel%22%3A%22http%3A%5C%2F%5C%2Fwww.wikidata.org%5C%2Fentity%5C%2FQ1985727%22%7D%2C%22type%22%3A%22time%22%7D&datatype=time&generate=text%2Fplain&options=%7B%22showcalendar%22%3A%22auto%22%7D` — Format a time value as plain text, automatically showing the calendar model if needed.

<a id="mod-wbgetclaims"></a>
#### `action=wbgetclaims`

*Wikibase Repo* · `Wikibase\Repo\Api\GetClaims` · GET or POST

Gets Wikibase claims.

Parameters:

- **`entity`** — string  
  ID of the entity from which to obtain claims. Required unless claim GUID is provided.
- **`property`** — string  
  Optional filter to only return claims with a main snak that has the specified property.
- **`claim`** — string  
  A GUID identifying the claim. Required unless entity is provided. The GUID is the globally unique identifier for a claim, e.g. "`q42$D8404CDA-25E4-4334-AF13-A3290BCD9C0F`".
- **`rank`** — enum: `deprecated`, `normal`, `preferred`  
  Optional filter to return only the claims that have the specified rank
- **`props`** — enum: `references`; multi-value (max 50; 500 for high-limit users); default `references`  
  Some parts of the claim are returned optionally. This parameter controls which ones are returned.

Examples:

- `api.php?action=wbgetclaims&entity=Q42` — Get claims for item with ID `Q42`
- `api.php?action=wbgetclaims&entity=Q42&property=P31` — Get claims for item with ID `Q42` and property with ID `P31`
- `api.php?action=wbgetclaims&entity=Q42&rank=normal` — Get claims for item with ID `Q42` that are ranked as normal
- `api.php?action=wbgetclaims&claim=Q42$D8404CDA-25E4-4334-AF13-A3290BCD9C0F` — Get claim with GUID of `Q42$D8404CDA-25E4-4334-AF13-A3290BCD9C0F`

<a id="mod-wbgetentities"></a>
#### `action=wbgetentities`

*Wikibase Repo* · `Wikibase\Repo\Api\GetEntities` · GET or POST

Gets the data for multiple Wikibase entities.

Parameters:

- **`ids`** — string; multi-value (max 50; 500 for high-limit users)  
  The IDs of the entities to get the data from
- **`sites`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users); duplicates allowed  
  Identifier for the site on which the corresponding page resides.
  Use together with `title`, but only give one site for several titles or several sites for one title.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users); duplicates allowed  
  The title of the corresponding page.
  Use together with `sites`, but only give one site for several titles or several sites for one title.
- **`redirects`** — enum: `no`, `yes`; default `yes`  
  Whether redirects shall be resolved.
  If set to "no", redirects will be treated like deleted entities.
- **`props`** — enum: `aliases`, `claims`, `datatype`, `descriptions`, `info`, `labels`, `sitelinks`, `sitelinks/urls`; multi-value (max 50; 500 for high-limit users); default `info|sitelinks|aliases|labels|descriptions|claims|datatype`  
  The names of the properties to get back from each entity.
  Will be further filtered by any languages given.
- **`languages`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent); multi-value (max 50; 500 for high-limit users)  
  By default the internationalized values are returned in all available languages.
  This parameter allows filtering these down to one or more languages by providing one or more language codes.
- **`languagefallback`** — boolean  
  Apply language fallback for languages defined in the `languages` parameter, with the current context of API call.
- **`normalize`** — boolean  
  Try to normalize the page title against the client site.
  This only works if exactly one site and one page have been given.
- **`sitefilter`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users); duplicates allowed  
  Filter sitelinks in entities to those with these site IDs.

Examples:

- `api.php?action=wbgetentities&ids=Q42` — Get entities with ID `Q42` with all available attributes in all available languages
- `api.php?action=wbgetentities&ids=P17` — Get entities with ID `P17` with all available attributes in all available languages
- `api.php?action=wbgetentities&ids=Q42|P17` — Get entities with IDs `Q42` and `P17` with all available attributes in all available languages
- `api.php?action=wbgetentities&ids=Q42&languages=en` — Get entities with ID `Q42` with all available attributes in English language
- `api.php?action=wbgetentities&ids=Q42&languages=ii&languagefallback=` — Get entities with ID `Q42` with all available attributes in any possible fallback language for the `ii` language
- `api.php?action=wbgetentities&ids=Q42&props=labels` — Get entities with ID `Q42` showing all labels in all available languages
- `api.php?action=wbgetentities&ids=P17|P3&props=datatype` — Get entities with IDs `P17` and `P3` showing only datatypes
- `api.php?action=wbgetentities&ids=Q42&props=aliases&languages=en` — Get entities with ID `Q42` showing all aliases in English language
- `api.php?action=wbgetentities&ids=Q1|Q42&props=descriptions&languages=en|de|fr` — Get entities with IDs `Q1` and `Q42` showing descriptions in English, German and French languages
- `api.php?action=wbgetentities&sites=enwiki&titles=Berlin&languages=en` — Get the item for page "`Berlin`" on the site "`enwiki`", with language attributes in English language
- `api.php?action=wbgetentities&sites=enwiki&titles=berlin&normalize=` — Get the item for page "`Berlin`" on the site "`enwiki`" after normalizing the title from "`berlin`"
- `api.php?action=wbgetentities&ids=Q42&props=sitelinks` — Get the sitelinks for item `Q42`
- `api.php?action=wbgetentities&ids=Q42&sitefilter=enwiki` — Get entities with ID `Q42` showing only sitelinks from "`enwiki`"

<a id="mod-wblinktitles"></a>
#### `action=wblinktitles`

*Wikibase Repo* · `Wikibase\Repo\Api\LinkTitles` · POST only · write · token `csrf`

Associates two pages on two different wikis with a Wikibase item.

Parameters:

- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`tosite`** — enum (no values in reference config — site-/config-dependent); **required**  
  An identifier for the site on which the page resides.
  Use together with `totitle` to make a complete sitelink.
- **`totitle`** — string; **required**  
  Title of the page to associate.
  Use together with `tosite` to make a complete sitelink.
- **`fromsite`** — enum (no values in reference config — site-/config-dependent); **required**  
  An identifier for the site on which the page resides.
  Use together with `fromtitle` to make a complete sitelink.
- **`fromtitle`** — string; **required**  
  Title of the page to associate.
  Use together with `fromsite` to make a complete sitelink.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "bot".

Examples:

- `api.php?action=wblinktitles&fromsite=enwiki&fromtitle=Hydrogen&tosite=dewiki&totitle=Wasserstoff` — Add a link "Hydrogen" from the English page to "Wasserstoff" at the German page

<a id="mod-wbmergeitems"></a>
#### `action=wbmergeitems`

*Wikibase Repo* · `Wikibase\Repo\Api\MergeItems` · POST only · write · token `csrf`

Merges multiple items.

Parameters:

- **`fromid`** — string; **required**  
  The ID to merge from
- **`toid`** — string; **required**  
  The ID to merge to
- **`ignoreconflicts`** — enum: `description`, `sitelink`, `statement`; multi-value (max 50; 500 for high-limit users)  
  Array of elements of the item to ignore conflicts for. Can only contain values of "`description`", "`sitelink`" and "`statement`"
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revisions.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "bot".
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbmergeitems&fromid=Q999999998&toid=Q999999999` — Merges data from `Q999999998` into `Q999999999`
- `api.php?action=wbmergeitems&fromid=Q999999998&toid=Q999999999&ignoreconflicts=sitelink` — Merges data from `Q999999998` into `Q999999999` ignoring any conflicting sitelinks
- `api.php?action=wbmergeitems&fromid=Q999999998&toid=Q999999999&ignoreconflicts=sitelink|description` — Merges data from `Q999999998` into `Q999999999` ignoring any conflicting sitelinks and descriptions

<a id="mod-wbparsevalue"></a>
#### `action=wbparsevalue`

*Wikibase Repo* · `Wikibase\Repo\Api\ParseValue` · GET or POST

Parses values using a `ValueParser`.

Parameters:

- **`datatype`** — enum: `commonsMedia`, `external-id`, `geo-shape`, `globe-coordinate`, `monolingualtext`, `quantity`, `string`, `tabular-data`, `time`, `url`, `wikibase-item`, `wikibase-property`  
  Datatype of the value to parse. Determines the parser to use.
- **`property`** — string  
  Property ID the value to parse belongs to. Determines the parser to use.
- **`parser`** — enum: `commonsMedia`, `external-id`, `geo-shape`, `globe-coordinate`, `globecoordinate`, `monolingualtext`, `null`, `quantity`, `string`, `tabular-data`, `time`, `url`, `wikibase-entityid`, `wikibase-item`, `wikibase-property`; **deprecated**  
  ID of the `ValueParser` to use. Deprecated. Use the `datatype` parameter instead.
- **`values`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  The values to parse
- **`options`** — text  
  The options the parser should use. Provided as a JSON object.
- **`validate`** — boolean  
  Whether to additionally verify the data passed in.

Examples:

- `api.php?action=wbparsevalue&datatype=string&values=foo|bar` — Parse a plain string into a StringValue object.
- `api.php?action=wbparsevalue&datatype=time&values=1994-02-08&options={"precision":9}` — Parse 1994-02-08 to a TimeValue object with a precision of 9 (year).
- `api.php?action=wbparsevalue&datatype=time&validate&values=1994-02-08&options={"precision":14}` — Parse 1994-02-08 to a TimeValue object with a precision of 14 (second) with validation enabled, resulting in a validation failure.
- `api.php?action=wbparsevalue&property=P123&validate&values=foo` — Parse foo into an object of whatever datatype P123 is, with validation enabled, potentially resulting in a validation failure depending on P123's datatype's expected input.

<a id="mod-wbremoveclaims"></a>
#### `action=wbremoveclaims`

*Wikibase Repo* · `Wikibase\Repo\Api\RemoveClaims` · POST only · write · token `csrf`

Removes Wikibase claims.

Parameters:

- **`claim`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  One GUID or several (pipe-separated) GUIDs identifying the claims to be removed.
  All claims must belong to the same entity.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "bot".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbremoveclaims&claim=Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0N&token=foobar&baserevid=7201010` — Remove claim with GUID of "Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0N"

<a id="mod-wbremovequalifiers"></a>
#### `action=wbremovequalifiers`

*Wikibase Repo* · `Wikibase\Repo\Api\RemoveQualifiers` · POST only · write · token `csrf`

Removes a qualifier from a claim.

Parameters:

- **`claim`** — string; **required**  
  A GUID identifying the claim from which to remove qualifiers
- **`qualifiers`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  Snak hashes of the qualifiers to remove
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbremovequalifiers&claim=Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F&qualifiers=1eb8793c002b1d9820c833d234a1b54c8e94187e&token=foobar&baserevid=7201010` — Remove qualifier with hash "`1eb8793c002b1d9820c833d234a1b54c8e94187e`" from claim with GUID of "`Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F`"

<a id="mod-wbremovereferences"></a>
#### `action=wbremovereferences`

*Wikibase Repo* · `Wikibase\Repo\Api\RemoveReferences` · POST only · write · token `csrf`

Removes one or more references of the same statement.

Parameters:

- **`statement`** — string; **required**  
  A GUID identifying the statement for which a reference is being set
- **`references`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  The hashes of the references that should be removed
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbremovereferences&statement=Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F&references=455481eeac76e6a8af71a6b493c073d54788e7e9&token=foobar&baserevid=7201010` — Remove reference with hash "`455481eeac76e6a8af71a6b493c073d54788e7e9`" from the statement with GUID of "`Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F`"

<a id="mod-wbsearchentities"></a>
#### `action=wbsearchentities`

*Wikibase Repo* · `Wikibase\Repo\Api\SearchEntities` · GET or POST

Searches for entities using labels and aliases.

Returns a label and description for the entity in the user language if possible.
Returns details of the matched term.
The matched term text is also present in the aliases key if different from the display label.

Parameters:

- **`search`** — string; **required**  
  Search for this text.
- **`language`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent); **required**  
  Search in this language. This only affects how entities are selected, not the language in which the results are returned: this is controlled by the "uselang" parameter.
- **`strictlanguage`** — boolean  
  Whether to disable language fallback
- **`type`** — enum: `item`, `property`; default `item`  
  Search for this type of entity.
- **`limit`** — limit (1–50; 500 for high-limit users; or `max`); default `7`  
  Maximal number of results
- **`continue`** — integer (min 0, max 10000)  
  Offset where to continue a search
- **`props`** — enum: `url`; multi-value (max 50; 500 for high-limit users); default `url`  
  Return these properties for each entity.
- **`profile`** — enum: `default`; default `default`  
  The search profile to use.
  
  - **default**: The default profile, suitable for most purposes.

Examples:

- `api.php?action=wbsearchentities&search=abc&language=en` — Search for "abc" in English language, with defaults for type and limit
- `api.php?action=wbsearchentities&search=abc&language=en&limit=50` — Search for "abc" in English language with a limit of 50
- `api.php?action=wbsearchentities&search=abc&language=en&limit=2&continue=2` — Search for "abc" in English language with a limit of 2 and an offset of 2
- `api.php?action=wbsearchentities&search=alphabet&language=en&type=property` — Search only properties for "alphabet" in English language
- `api.php?action=wbsearchentities&search=alphabet&language=en&props=` — Search for "alphabet" in English language omitting url parameter
- `api.php?action=wbsearchentities&search=Q1234&language=en` — Search for "Q1234" in English language, to match the entity ID.

<a id="mod-wbsetaliases"></a>
#### `action=wbsetaliases`

*Wikibase Repo* · `Wikibase\Repo\Api\SetAliases` · POST only · write · token `csrf`

Sets the aliases for a Wikibase entity.

Parameters:

- **`id`** — string  
  The identifier for the entity, including the prefix.
  Use either `id` or `site` and `title` together.
- **`new`** — enum: `item`, `property`  
  If set, a new entity will be created.
  Set this to the type of the entity you want to create.
- **`site`** — enum (no values in reference config — site-/config-dependent)  
  An identifier for the site on which the page resides.
  Use together with `title` to make a complete sitelink.
- **`title`** — string  
  Title of the page to associate.
  Use together with `site` to make a complete sitelink.
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`add`** — string; multi-value (max 50; 500 for high-limit users)  
  List of aliases to add (can be combined with `remove`)
- **`remove`** — string; multi-value (max 50; 500 for high-limit users)  
  List of aliases to remove (can be combined with `add`)
- **`set`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of aliases that will replace the current list (cannot be combined with neither `add` nor `remove`)
- **`language`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent); **required**  
  The language for which to set the aliases

Examples:

- `api.php?action=wbsetaliases&language=en&id=Q999999998&set=Foo|Bar` — Set the English aliases for the entity with ID `Q999999998` to `Foo` and `Bar`
- `api.php?action=wbsetaliases&language=en&id=Q999999998&add=Foo|Bar` — Add `Foo` and `Bar` to the list of English aliases for the entity with ID `Q999999998`
- `api.php?action=wbsetaliases&language=en&id=Q999999998&remove=Foo|Bar` — Remove `Foo` and `Bar` from the list of English aliases for the entity with ID `Q999999998`
- `api.php?action=wbsetaliases&language=en&id=Q999999998&remove=Foo&add=Bar` — Remove `Foo` from the list of English aliases for the entity with ID `Q999999998` while adding `Bar` to it

<a id="mod-wbsetclaim"></a>
#### `action=wbsetclaim`

*Wikibase Repo* · `Wikibase\Repo\Api\SetClaim` · POST only · write · token `csrf`

Creates or updates an entire Statement or Claim.

Parameters:

- **`claim`** — text; **required**  
  Statement or Claim serialization
- **`index`** — integer  
  The index within the entity's list of statements to move the statement to. Optional. Be aware that when setting an index that specifies a position not next to a statement whose main snak does not feature the same property, the whole group of statements whose main snaks feature the same property is moved. When not provided, an existing statement will stay in place while a new statement will be appended to the last one whose main snak features the same property.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`ignoreduplicatemainsnak`** — boolean  
  If this is true, and the entity already has a claim with the same main snak as the claim being sent in the request, then the request is ignored
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbsetclaim&claim={"id":"Q999999998$5627445f-43cb-ed6d-3adb-760e85bd17ee","type":"claim","mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"City","type":"string"}}}` — Set the claim with the given ID to property `P1` with a string value of "`City`"
- `api.php?action=wbsetclaim&claim={"id":"Q999999998$5627445f-43cb-ed6d-3adb-760e85bd17ee","type":"claim","mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"City","type":"string"}}}&index=0` — Set the claim with the given ID to property `P1` with a string value of "`City`" and move the claim to the topmost position within the entity's subgroup of claims that feature the main snak property `P1`. In addition, move the whole subgroup to the top of all subgroups aggregated by property.
- `api.php?action=wbsetclaim&claim={"id":"Q999999998$5627445f-43cb-ed6d-3adb-760e85bd17ee","type":"statement","mainsnak":{"snaktype":"value","property":"P1","datavalue":{"value":"City","type":"string"}},"references":[{"snaks":{"P2":[{"snaktype":"value","property":"P2","datavalue":{"value":"The Economy of Cities","type":"string"}}]},"snaks-order":["P2"]}],"rank":"normal"}` — Set the Statement with the given ID to Property `P1` with a string value of "`City`" and set the Statement's References to a single Reference featuring the string value "`The Economy of Cities`" assigned to the Property `P2`.

<a id="mod-wbsetclaimvalue"></a>
#### `action=wbsetclaimvalue`

*Wikibase Repo* · `Wikibase\Repo\Api\SetClaimValue` · POST only · write · token `csrf`

Sets the value of a Wikibase claim.

Parameters:

- **`claim`** — string; **required**  
  A GUID identifying the claim
- **`value`** — text  
  The value to set the DataValue of the main snak of the claim to
- **`snaktype`** — enum: `novalue`, `somevalue`, `value`; **required**  
  The type of the snak
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbsetclaimvalue&claim=Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F&snaktype=value&value={"entity-type":"item","numeric-id":1}&token=foobar&baserevid=7201010` — Sets the claim with the GUID of `Q999999998$D8404CDA-25E4-4334-AF13-A3290BCD9C0F` to a value of `Q1`

<a id="mod-wbsetdescription"></a>
#### `action=wbsetdescription`

*Wikibase Repo* · `Wikibase\Repo\Api\SetDescription` · POST only · write · token `csrf`

Sets a description for a single Wikibase entity.

Parameters:

- **`id`** — string  
  The identifier for the entity, including the prefix.
  Use either `id` or `site` and `title` together.
- **`new`** — enum: `item`, `property`  
  If set, a new entity will be created.
  Set this to the type of the entity you want to create.
- **`site`** — enum (no values in reference config — site-/config-dependent)  
  An identifier for the site on which the page resides.
  Use together with `title` to make a complete sitelink.
- **`title`** — string  
  Title of the page to associate.
  Use together with `site` to make a complete sitelink.
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`language`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent); **required**  
  Language of the description
- **`value`** — string  
  The value to set for the description

Examples:

- `api.php?action=wbsetdescription&id=Q999999998&language=en&value=An%20encyclopedia%20that%20everyone%20can%20edit` — Set the string "`An encyclopedia that everyone can edit`" for page with ID "`Q999999998`" as a description in English language
- `api.php?action=wbsetdescription&site=enwiki&title=Wikipedia&language=en&value=An%20encyclopedia%20that%20everyone%20can%20edit` — Set the string "`An encyclopedia that everyone can edit`" as a description in English language for page with a sitelink to `enwiki:Wikipedia`

<a id="mod-wbsetlabel"></a>
#### `action=wbsetlabel`

*Wikibase Repo* · `Wikibase\Repo\Api\SetLabel` · POST only · write · token `csrf`

Sets a label for a single Wikibase entity.

Parameters:

- **`id`** — string  
  The identifier for the entity, including the prefix.
  Use either `id` or `site` and `title` together.
- **`new`** — enum: `item`, `property`  
  If set, a new entity will be created.
  Set this to the type of the entity you want to create.
- **`site`** — enum (no values in reference config — site-/config-dependent)  
  An identifier for the site on which the page resides.
  Use together with `title` to make a complete sitelink.
- **`title`** — string  
  Title of the page to associate.
  Use together with `site` to make a complete sitelink.
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`language`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent); **required**  
  Language of the label
- **`value`** — string  
  The value of the label

Examples:

- `api.php?action=wbsetlabel&id=Q999999998&language=en&value=Wikimedia&format=jsonfm` — Set the string "Wikimedia" for page with ID "`Q999999998`" as a label in English language and report it as pretty printed JSON.
- `api.php?action=wbsetlabel&site=enwiki&title=Earth&language=en&value=Earth` — Set the English language label to "`Earth`" for the item with sitelink `enwiki` => "`Earth`".

<a id="mod-wbsetqualifier"></a>
#### `action=wbsetqualifier`

*Wikibase Repo* · `Wikibase\Repo\Api\SetQualifier` · POST only · write · token `csrf`

Creates a qualifier or sets the value of an existing one.

Parameters:

- **`claim`** — string; **required**  
  A GUID identifying the claim for which a qualifier is being set
- **`property`** — string  
  ID of the snaks property.
  Should only be provided when creating a new qualifier or changing the property of an existing one
- **`value`** — text  
  The new value of the qualifier.
  Should only be provided for PropertyValueSnak qualifiers
- **`snaktype`** — enum: `novalue`, `somevalue`, `value`  
  The type of the snak.
  Should only be provided when creating a new qualifier or changing the type of an existing one
- **`snakhash`** — string  
  The hash of the snak to modify.
  Should only be provided for existing qualifiers
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "Bots".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbsetqualifier&claim=Q999999998$4554c0f4-47b2-1cd9-2db9-aa270064c9f3&property=P1&value="GdyjxP8I6XB3"&snaktype=value&token=foobar` — Set the qualifier for the given claim with property `P1` to string value `GdyjxP8I6XB3`

<a id="mod-wbsetreference"></a>
#### `action=wbsetreference`

*Wikibase Repo* · `Wikibase\Repo\Api\SetReference` · POST only · write · token `csrf`

Creates a reference or sets the value of an existing one.

Parameters:

- **`statement`** — string; **required**  
  A GUID identifying the statement for which a reference is being set
- **`snaks`** — text; **required**  
  The snaks to set the reference to. JSON object with property IDs pointing to arrays containing the snaks for that property
- **`snaks-order`** — string  
  The order of the snaks. JSON array of property ID strings
- **`reference`** — string  
  A hash of the reference that should be updated. Optional. When not provided, a new reference is created
- **`index`** — integer  
  The index within the statement's list of references where to move the reference to. Optional. When not provided, an existing reference will stay in place while a new reference will be appended.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "bot".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=wbsetreference&statement=Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF&snaks={"P212":[{"snaktype":"value","property":"P212","datavalue":{"type":"string","value":"foo"}}]}&baserevid=7201010&token=foobar` — Create a new reference for claim with GUID Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF
- `api.php?action=wbsetreference&statement=Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF&reference=1eb8793c002b1d9820c833d234a1b54c8e94187e&snaks={"P212":[{"snaktype":"value","property":"P212","datavalue":{"type":"string","value":"bar"}}]}&baserevid=7201010&token=foobar` — Set reference for claim with GUID Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF which has hash of 1eb8793c002b1d9820c833d234a1b54c8e94187e
- `api.php?action=wbsetreference&statement=Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF&snaks={"P212":[{"snaktype":"novalue","property":"P212"}]}&index=0&baserevid=7201010&token=foobar` — Creates a new reference for the claim with GUID Q999999998$D4FDE516-F20C-4154-ADCE-7C5B609DFDFF and inserts the new reference at the top of the list of references instead of appending it to the bottom.

<a id="mod-wbsetsitelink"></a>
#### `action=wbsetsitelink`

*Wikibase Repo* · `Wikibase\Repo\Api\SetSiteLink` · POST only · write · token `csrf`

Associates a page on a wiki with a Wikibase item or removes an already made such association.

Parameters:

- **`id`** — string  
  The identifier for the entity, including the prefix.
  Use either `id` or `site` and `title` together.
- **`new`** — enum: `item`, `property`  
  If set, a new entity will be created.
  Set this to the type of the entity you want to create.
- **`site`** — enum (no values in reference config — site-/config-dependent)  
  An identifier for the site on which the page resides.
  Use together with `title` to make a complete sitelink.
- **`title`** — string  
  Title of the page to associate.
  Use together with `site` to make a complete sitelink.
- **`baserevid`** — integer  
  The numeric identifier for the revision to base the modification on.
  This is used for detecting conflicts during save.
- **`summary`** — string  
  Summary for the edit.
  Will be prepended by an automatically generated comment. The length limit of the autocomment together with the summary is 260 characters. Be aware that everything above that limit will be cut off.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
- **`bot`** — boolean  
  Mark this edit as bot. This URL flag will only be respected if the user belongs to the group "bot".
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.
- **`linksite`** — enum (no values in reference config — site-/config-dependent); **required**  
  The identifier of the site on which the page to link resides
- **`linktitle`** — string  
  The title of the page to link. If this parameter is an empty string or both `linktitle` and `badges` are not set, the link will be removed.
- **`badges`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  The IDs of items to be set as badges. They will replace the current ones. If this parameter is not set, the badges will not be changed

Examples:

- `api.php?action=wbsetsitelink&id=Q999999998&linksite=enwiki&linktitle=Hydrogen` — Add a sitelink to the English page "Hydrogen" to the item with ID `Q999999998`, if the sitelink does not exist
- `api.php?action=wbsetsitelink&id=Q999999998&linksite=enwiki&linktitle=Hydrogen&summary=Loves%20Oxygen` — Add a sitelink to the English page "Hydrogen" to the item with ID `Q999999998`, if the sitelink does not exist. Also appends "Loves Oxygen" to the edit summary.
- `api.php?action=wbsetsitelink&site=enwiki&title=Hydrogen&linksite=dewiki&linktitle=Wasserstoff` — Add a sitelink to the German page "Wasserstoff" to the item that is linked with the English page "Hydrogen", if the sitelink does not exist
- `api.php?action=wbsetsitelink&site=enwiki&title=Hydrogen&linksite=dewiki` — Remove the German sitelink from the item
- `api.php?action=wbsetsitelink&site=enwiki&title=Hydrogen&linksite=plwiki&linktitle=Wodór&badges=Q149` — Add a sitelink to the Polish page "Wodór" to the item that is linked with the English page "Hydrogen", with one badge pointing to the item with ID "Q149"
- `api.php?action=wbsetsitelink&id=Q999999998&linksite=plwiki&badges=Q2|Q149` — Change badges for the link to the Polish page from the item with ID `Q999999998` to two badges pointing to the items with IDs "Q2" and "Q149" without providing the link title
- `api.php?action=wbsetsitelink&id=Q999999998&linksite=plwiki&linktitle=Warszawa` — Change the link to the Polish page from the item with ID `Q999999998` without changing badges
- `api.php?action=wbsetsitelink&id=Q999999998&linksite=plwiki&linktitle=Wodór&badges=` — Change the link to the Polish page from the item with ID `Q999999998` and remove all of its badges

### 4.2 Wikibase query submodules

<a id="mod-query-description"></a>
#### `action=query&prop=description`

*Wikibase Client* · `Wikibase\Client\Api\Description` · prefix `desc` · GET or POST · **internal**

Get a short description a.k.a. subtitle explaining what the target page is about.

The description is plain text, on a single line, but otherwise arbitrary (potentially including raw HTML tags, which also should be interpreted as plain text). It must not be used in HTML unescaped!

Parameters:

- **`desccontinue`** — integer  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`descprefersource`** — enum: `central`, `local`; default `local`  
  Which description source to prefer if present:
  
  - **local**: Local descriptions via `<nowiki>{{SHORTDESC:...}}</nowiki>` parser function in the wikitext of the page.
  - **central**: Central descriptions from the associated Triplespace Ref item.

Examples:

- `api.php?action=query&prop=description&titles=London` — Get the description for the page 'London'.
- `api.php?action=query&prop=description&titles=London&descprefersource=central` — Get the description for the page 'London', preferring the central description if it exists.

<a id="mod-query-entityterms"></a>
#### `action=query&prop=entityterms`

*Wikibase Repo* · `Wikibase\Repo\Api\EntityTerms` · prefix `wbet` · GET or POST

Get the terms (labels, descriptions and aliases) of the entity on this page.

Parameters:

- **`wbetcontinue`** — integer  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`wbetlanguage`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (606 values total; config-dependent); default `uselang`  
  The language code to get terms in. If not specified, the user language is used.
- **`wbetterms`** — enum: `alias`, `description`, `label`; multi-value (max 50; 500 for high-limit users); default `alias|label|description`  
  The types of terms to get, e.g. 'description', each returned as an array of strings keyed by their type, e.g. {"description": ["foo"]}. If not specified, all types are returned.

Examples:

- `api.php?action=query&prop=entityterms&titles=Q84` — Get labels and aliases of item Q84.

<a id="mod-query-pageterms"></a>
#### `action=query&prop=pageterms`

*Wikibase Client* · `Wikibase\Client\Api\PageTerms` · prefix `wbpt` · GET or POST

Get the Triplespace Ref terms (typically labels, descriptions and aliases) associated with a page via a sitelink.

Parameters:

- **`wbptcontinue`** — integer  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`wbptlanguage`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (606 values total; config-dependent); default `uselang`  
  The language code to get terms in. If not specified, the user language is used.
- **`wbptterms`** — enum: `alias`, `description`, `label`; multi-value (max 50; 500 for high-limit users); default `alias|label|description`  
  The types of terms to get, e.g. 'description', each returned as an array of strings keyed by their type, e.g. {"description": ["foo"]}. If not specified, all types are returned.

Examples:

- `api.php?action=query&prop=pageterms&titles=London` — Get all terms associated with the page 'London', in the user language.
- `api.php?action=query&prop=pageterms&titles=London&wbptterms=label|alias&wbptlanguage=en` — Get labels and aliases associated with the page 'London', in English.

<a id="mod-query-wbentityusage"></a>
#### `action=query&prop=wbentityusage`

*Wikibase Client* · `Wikibase\Client\Api\ApiPropsEntityUsage` · prefix `wbeu` · GET or POST

Returns all entity IDs used in the given pages.

Parameters:

- **`wbeuprop`** — enum: `url`; multi-value (max 50; 500 for high-limit users)  
  Properties to add to the result.
  
  - **url**: If enabled url of entity will be added
- **`wbeuaspect`** — enum: `C`, `D`, `L`, `O`, `S`, `T`, `X`; multi-value (max 50; 500 for high-limit users)  
  Only return entity IDs that used this aspect.
  
  - **S**: The entity's sitelinks are used
  - **L**: The entity's label is used
  - **D**: The entity's description is used
  - **T**: The title of the local page corresponding to the entity is used
  - **C**: Statements from the entity are used
  - **X**: All aspects of an entity are or may be used
  - **O**: Something else about the entity is used. This currently implies alias usage and explicit checks for entity existence.
- **`wbeuentities`** — string; multi-value (max 50; 500 for high-limit users)  
  Only return page that used these entities.
- **`wbeulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many entity usages to return.
- **`wbeucontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=wbentityusage&titles=Main%20Page` — Get entities used in the page `Main Page`.

<a id="mod-query-wblistentityusage"></a>
#### `action=query&list=wblistentityusage`

*Wikibase Client* · `Wikibase\Client\Api\ApiListEntityUsage` · prefix `wbleu` · GET or POST · usable as generator (`generator=wblistentityusage`)

Returns all pages that use the given entity IDs.

Parameters:

- **`wbleuprop`** — enum: `url`; multi-value (max 50; 500 for high-limit users)  
  Properties to add to the result.
  
  - **url**: If enabled the url of the entity will be added to the result.
- **`wbleuaspect`** — enum: `C`, `D`, `L`, `O`, `S`, `T`, `X`; multi-value (max 50; 500 for high-limit users)  
  Only return entity IDs that used this aspect.
  
  - **S**: The entity's sitelinks are used
  - **L**: The entity's label is used
  - **D**: The entity's description is used
  - **T**: The title of the local page corresponding to the entity is used
  - **C**: Statements from the entity are used
  - **X**: All aspects of an entity are or may be used
  - **O**: Something else about the entity is used. This currently implies alias usage and explicit checks for entity existence.
- **`wbleuentities`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  Entities that have been used.
- **`wbleulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many entity usages to return.
- **`wbleucontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=wblistentityusage&wbleuentities=Q2` — Get pages that use the entity `Q2`.
- `api.php?action=query&list=wblistentityusage&wbleuentities=Q2&wbleuprop=url` — Get pages that use the entity `Q2` with URL included.
- `api.php?action=query&list=wblistentityusage&wbleuentities=Q2&wbleuaspect=S|O` — Get pages that use the entity `Q2` and the aspect was sitelink or statement.

<a id="mod-query-wbsearch"></a>
#### `action=query&list=wbsearch`

*Wikibase Repo* · `Wikibase\Repo\Api\QuerySearchEntities` · prefix `wbs` · GET or POST · usable as generator (`generator=wbsearch`) · **internal**

Searches for entities using labels and aliases.

This can be used as a generator for other queries.
Returns the matched term that should be displayed.

Parameters:

- **`wbssearch`** — string; **required**  
  Search for this text.
- **`wbslanguage`** — enum: `aa`, `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `agq`, `aln`, `als`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, … (605 values total; config-dependent)  
  Search in this language.
- **`wbsstrictlanguage`** — boolean  
  Whether to disable language fallback
- **`wbstype`** — enum: `item`, `property`; default `item`  
  Search for this type of entity.
- **`wbslimit`** — limit (1–50; 500 for high-limit users; or `max`); default `7`  
  Maximal number of results
- **`wbsprofile`** — enum: `default`; default `default`  
  The search profile to use.
  
  - **default**: The default profile, suitable for most purposes.

Examples:

- `api.php?action=query&list=wbsearch&wbssearch=abc&wbslanguage=en` — Search for "abc" in English language, with defaults for type and limit
- `api.php?action=query&list=wbsearch&wbssearch=abc&wbslanguage=en&wbslimit=50` — Search for "abc" in English language with a limit of 50
- `api.php?action=query&list=wbsearch&wbssearch=alphabet&wbslanguage=en&wbstype=property` — Search only properties for "alphabet" in English language
- `api.php?action=query&generator=wbsearch&gwbssearch=alphabet&gwbslanguage=en` — Search for "alphabet" in English language as a generator

<a id="mod-query-wbsubscribers"></a>
#### `action=query&list=wbsubscribers`

*Wikibase Repo* · `Wikibase\Repo\Api\ListSubscribers` · prefix `wbls` · GET or POST

Get subscriptions to given entities.

Parameters:

- **`wblsentities`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  Entities to get subscribers
- **`wblsprop`** — enum: `url`; multi-value (max 50; 500 for high-limit users)  
  Properties to add to result
- **`wblslimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  Maximal number of results
- **`wblscontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=wbsubscribers&wblsentities=Q42` — Get subscribers to entity `Q42`
- `api.php?action=query&list=wbsubscribers&wblsentities=Q42&wblsprop=url` — Get subscribers to entity `Q42` with URL to `Special:EntityUsage` included

<a id="mod-query-wbcontentlanguages"></a>
#### `action=query&meta=wbcontentlanguages`

*Wikibase Repo* · `Wikibase\Repo\Api\MetaContentLanguages` · prefix `wbcl` · GET or POST

Returns information about the content languages Wikibase accepts in different contexts.

Parameters:

- **`wbclcontext`** — enum: `monolingualtext`, `term`; default `term`  
  The context in which the content languages should be valid.
  
  - **term**: The terms (label, description, aliases) of an entity.
  - **monolingualtext**: A monolingual text value in a statement.
- **`wbclprop`** — enum: `autonym`, `code`, `name`; multi-value (max 50; 500 for high-limit users); default `code`  
  The properties that should be returned about each language.
  
  - **code**: The language code.
  - **autonym**: The autonym of the language, that is, the name of the language in that language. May not be known for all languages.
  - **name**: The name of the language in the current language (specified via the `uselang` parameter), with language fallbacks applied if necessary. Usually, at least an English name is known for all content languages Wikibase accepts.

Examples:

- `api.php?action=query&meta=wbcontentlanguages` — Get the valid language codes for the terms of an entity.
- `api.php?action=query&meta=wbcontentlanguages&wbclcontext=monolingualtext&wbclprop=code|autonym` — Get the valid languages, with language code and autonym, for monolingual text values.

<a id="mod-query-wikibase"></a>
#### `action=query&meta=wikibase`

*Wikibase Client* · `Wikibase\Client\Api\ApiClientInfo` · prefix `wb` · GET or POST

Get information about the Wikibase client and the associated Wikibase repository.

Parameters:

- **`wbprop`** — enum: `siteid`, `url`; multi-value (max 50; 500 for high-limit users); default `url|siteid`  
  Which properties to get:
  
  - **url**: Base URL, script path and article path of the Wikibase repository.
  - **siteid**: The siteid of this site.

Examples:

- `api.php?action=query&meta=wikibase` — Get URL path and other information about Wikibase client and repository.

## 5. MediaWiki core Action API reference

Every core module, with full parameter contracts as reported by `action=paraminfo` on the reference build.

### 5.0 Main module and global parameters

<a id="mod-main"></a>
#### `api.php` (main module)

*Core* · `MediaWiki\Api\ApiMain` · GET or POST

- Documentation
- Etiquette & usage guidelines
- FAQ
- [Mailing list](https://lists.wikimedia.org/postorius/lists/mediawiki-api.lists.wikimedia.org/)
- [API Announcements](https://lists.wikimedia.org/postorius/lists/mediawiki-api-announce.lists.wikimedia.org/)
- [Bugs & requests](https://phabricator.wikimedia.org/maniphest/query/GebfyV4uCaLd/#R)

Status: The MediaWiki API is a mature and stable interface that is actively supported and improved. While we try to avoid it, we may occasionally need to make breaking changes; subscribe to [the mediawiki-api-announce mailing list](https://lists.wikimedia.org/hyperkitty/list/mediawiki-api-announce@lists.wikimedia.org/) for notice of updates.

Erroneous requests: When erroneous requests are sent to the API, an HTTP header will be sent with the key "MediaWiki-API-Error" and then both the value of the header and the error code sent back will be set to the same value. For more information see API: Errors and warnings.

Testing: For ease of testing API requests, see Special:ApiSandbox.

Parameters:

- **`action`** — submodule: `acquiretempusername`, `block`, `changeauthenticationdata`, `changecontentmodel`, `checktoken`, `clearhasmsg`, `clientlogin`, `compare`, `createaccount`, `cspreport`, `delete`, `edit`, `emailuser`, `expandtemplates`, `feedcontributions`, `feedrecentchanges`, `feedwatchlist`, `filerevert`, `help`, `imagerotate`, `import`, `linkaccount`, `login`, `logout`, `managetags`, `mergehistory`, `move`, `opensearch`, `options`, `paraminfo`, `parse`, `patrol`, `protect`, `purge`, `query`, `removeauthenticationdata`, `resetpassword`, `revisiondelete`, `rollback`, `rsd`, `setnotificationtimestamp`, `setpagelanguage`, `stashedit`, `tag`, `unblock`, `undelete`, `unlinkaccount`, `upload`, `userrights`, `validatepassword`, `watch`, `wbavailablebadges`, `wbcreateclaim`, `wbcreateredirect`, `wbeditentity`, `wbformatentities`, `wbformatvalue`, `wbgetclaims`, `wbgetentities`, `wblinktitles`, `wbmergeitems`, `wbparsevalue`, `wbremoveclaims`, `wbremovequalifiers`, `wbremovereferences`, `wbsearchentities`, `wbsetaliases`, `wbsetclaim`, `wbsetclaimvalue`, `wbsetdescription`, `wbsetlabel`, `wbsetqualifier`, `wbsetreference`, `wbsetsitelink`; internal values: `cspreport`, `stashedit`
- **`format`** — submodule: `json`, `jsonfm`, `none`, `php`, `phpfm`, `rawfm`, `xml`, `xmlfm`
- **`maxlag`** — integer  
  Maximum lag can be used when MediaWiki is installed on a database replicated cluster. To save actions causing any more site replication lag, this parameter can make the client wait until the replication lag is less than the specified value. In case of excessive lag, error code `maxlag` is returned with a message like `Waiting for $host: $lag seconds lagged`.See Manual: Maxlag parameter for more information.
- **`smaxage`** — integer (min 0)  
  Set the `s-maxage` HTTP cache control header to this many seconds. Errors are never cached.
- **`maxage`** — integer (min 0)  
  Set the `max-age` HTTP cache control header to this many seconds. Errors are never cached.
- **`assert`** — enum: `anon`, `bot`, `user`  
  Verify that the user is logged in (including possibly as a temporary user) if set to `user`, not logged in if set to `anon`, or has the bot user right if `bot`.
- **`assertuser`** — user  
  Verify the current user is the named user.
- **`requestid`** — string  
  Any value given here will be included in the response. May be used to distinguish requests.
- **`servedby`** — boolean  
  Include the hostname that served the request in the results.
- **`curtimestamp`** — boolean  
  Include the current timestamp in the result.
- **`responselanginfo`** — boolean  
  Include the languages used for `uselang` and `errorlang` in the result.
- **`origin`** — string  
  When accessing the API using a cross-domain AJAX request (CORS), set this to the originating domain. This must be included in any pre-flight request, and therefore must be part of the request URI (not the POST body).
  
  For authenticated requests, this must match one of the origins in the `Origin` header exactly, so it has to be set to something like `https://en.wikipedia.org` or `https://meta.wikimedia.org`. If this parameter does not match the `Origin` header, a 403 response will be returned. If this parameter matches the `Origin` header and the origin is allowed, the `Access-Control-Allow-Origin` and `Access-Control-Allow-Credentials` headers will be set.
  
  For non-authenticated requests, specify the value `*`. This will cause the `Access-Control-Allow-Origin` header to be set, but `Access-Control-Allow-Credentials` will be `false` and all user-specific data will be restricted.
- **`uselang`** — string; default `user`  
  Language to use for message translations. `action=query&meta=siteinfo&siprop=languages` returns a list of language codes. You can specify `user` to use the current user's language preference or `content` to use this wiki's content language.
- **`variant`** — string  
  Variant of the language. Only works if the base language supports variant conversion.
- **`errorformat`** — enum: `bc`, `html`, `none`, `plaintext`, `raw`, `wikitext`; default `bc`  
  Format to use for warning and error text output
  
  - **plaintext**: Wikitext with HTML tags removed and entities replaced.
  - **wikitext**: Unparsed wikitext.
  - **html**: HTML
  - **raw**: Message key and parameters.
  - **none**: No text output, only the error codes.
  - **bc**: Format used prior to MediaWiki 1.29. `errorlang` and `errorsuselocal` are ignored.
- **`errorlang`** — string; default `uselang`  
  Language to use for warnings and errors. `action=query&meta=siteinfo&siprop=languages` returns a list of language codes. Specify `content` to use this wiki's content language or `uselang` to use the same value as the `uselang` parameter.
- **`errorsuselocal`** — boolean  
  If given, error texts will use locally-customized messages from the MediaWiki namespace.

Examples:

- `api.php?action=help` — Help for the main module.
- `api.php?action=help&recursivesubmodules=1` — All help in one page.

### 5.1 Core actions

<a id="mod-acquiretempusername"></a>
#### `action=acquiretempusername`

*Core* · `MediaWiki\Api\ApiAcquireTempUserName` · POST only · write

Acquire a temporary user username and stash it in the current session, if temp account creation is enabled and the current user is logged out. If a name has already been stashed, returns the same name.

If the user later performs an action that results in temp account creation, the stashed username will be used for their account. It may also be used in previews. However, the account is not created yet, and the name is not visible to other users.

<a id="mod-block"></a>
#### `action=block`

*Core* · `MediaWiki\Api\ApiBlock` · POST only · write · token `csrf`

Block a user.

Parameters:

- **`user`** — user  
  User to block.
- **`userid`** — integer; **deprecated**  
  Specify `user=#ID` instead.
- **`expiry`** — string; default `never`  
  Expiry time. May be relative (e.g. `5 months` or `2 weeks`) or absolute (e.g. `2014-09-18T12:34:56Z`). If set to `infinite`, `indefinite`, or `never`, the block will never expire.
- **`reason`** — string  
  Reason for block.
- **`anononly`** — boolean  
  Block anonymous users only (i.e. disable anonymous edits for this IP address, including temporary account edits).
- **`nocreate`** — boolean  
  Prevent account creation.
- **`autoblock`** — boolean  
  Automatically block the last used IP address, and any subsequent IP addresses they try to login from.
- **`noemail`** — boolean  
  Prevent user from sending email through the wiki. (Requires the `blockemail` right).
- **`hidename`** — boolean  
  Hide the username from the block log. (Requires the `hideuser` right).
- **`allowusertalk`** — boolean  
  Allow the user to edit their own talk page (depends on `$wgBlockAllowsUTEdit`).
- **`reblock`** — boolean  
  If the user is already blocked, overwrite the existing block.
- **`watchuser`** — boolean  
  Watch the user's or IP address's user and talk pages.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the block log.
- **`partial`** — boolean  
  Block user from specific pages or namespaces rather than the entire site.
- **`pagerestrictions`** — title; multi-value (max 10; 10 for high-limit users)  
  List of titles to block the user from editing. Only applies when `partial` is set to true.
- **`namespacerestrictions`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  List of namespace IDs to block the user from editing. Only applies when `partial` is set to true.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=block&user=192.0.2.5&expiry=3%20days&reason=First%20strike&token=123ABC` — Block IP address `192.0.2.5` for three days with a reason.
- `api.php?action=block&user=Vandal&expiry=never&reason=Vandalism&nocreate=&autoblock=&noemail=&token=123ABC` — Block user `Vandal` indefinitely with a reason, and prevent new account creation and email sending.

<a id="mod-changeauthenticationdata"></a>
#### `action=changeauthenticationdata`

*Core* · `MediaWiki\Api\ApiChangeAuthenticationData` · prefix `changeauth` · POST only · write · token `csrf`

Change authentication data for the current user.

Parameters:

- **`changeauthrequest`** — string; **required**  
  Use this authentication request, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=change`.
- **`changeauthtoken`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Dynamic parameters: This module accepts additional parameters depending on the available authentication requests. Use `action=query&meta=authmanagerinfo` with `amirequestsfor=change` (or a previous response from this module, if applicable) to determine the requests available and the fields that they use.

Examples:

- `api.php?action=changeauthenticationdata&changeauthrequest=MediaWiki%5CAuth%5CPasswordAuthenticationRequest&password=ExamplePassword&retype=ExamplePassword&changeauthtoken=123ABC` — Attempt to change the current user's password to `ExamplePassword`.

<a id="mod-changecontentmodel"></a>
#### `action=changecontentmodel`

*Core* · `MediaWiki\Api\ApiChangeContentModel` · POST only · write · token `csrf`

Change the content model of a page

Parameters:

- **`title`** — string  
  Title of the page to change the contentmodel of. Cannot be used together with `pageid`.
- **`pageid`** — integer  
  Page ID of the page to change the contentmodel of. Cannot be used together with `title`.
- **`summary`** — string  
  Edit summary and log entry reason
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the log entry and edit.
- **`model`** — enum: `css`, `javascript`, `json`, `text`, `wikitext`; **required**  
  Content model of the new content.
- **`bot`** — boolean  
  Mark the content model change with a bot flag.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=changecontentmodel&title=Main Page&model=text&token=123ABC` — Change the main page to have the `text` content model

<a id="mod-checktoken"></a>
#### `action=checktoken`

*Core* · `MediaWiki\Api\ApiCheckToken` · GET or POST

Check the validity of a token from `action=query&meta=tokens`.

Parameters:

- **`type`** — enum: `createaccount`, `csrf`, `login`, `patrol`, `rollback`, `userrights`, `watch`; **required**  
  Type of token being tested.
- **`token`** — string; **required**; sensitive  
  Token to test.
- **`maxtokenage`** — integer  
  Maximum allowed age of the token, in seconds.

Examples:

- `api.php?action=checktoken&type=csrf&token=123ABC` — Test the validity of a `csrf` token.

<a id="mod-clearhasmsg"></a>
#### `action=clearhasmsg`

*Core* · `MediaWiki\Api\ApiClearHasMsg` · POST only · write

Clears the `hasmsg` flag for the current user.

Examples:

- `api.php?action=clearhasmsg` — Clear the `hasmsg` flag for the current user.

<a id="mod-clientlogin"></a>
#### `action=clientlogin`

*Core* · `MediaWiki\Api\ApiClientLogin` · prefix `login` · POST only · write · token `login`

Log in to the wiki using the interactive flow.

The general procedure to use this module is:
# Fetch the fields available from `action=query&meta=authmanagerinfo` with `amirequestsfor=login`, and a `login` token from `action=query&meta=tokens`.
# Present the fields to the user, and obtain their submission.
# Post to this module, supplying `loginreturnurl` and any relevant fields.
# Check the `status` in the response.
#* If you received `PASS` or `FAIL`, you're done. The operation either succeeded or it didn't.
#* If you received `UI`, present the new fields to the user and obtain their submission. Then post to this module with `logincontinue` and the relevant fields set, and repeat step 4.
#* If you received `REDIRECT`, direct the user to the `redirecttarget` and wait for the return to `loginreturnurl`. Then post to this module with `logincontinue` and any fields passed to the return URL, and repeat step 4.
#* If you received `RESTART`, that means the authentication worked but we don't have a linked user account. You might treat this as `UI` or as `FAIL`.

Parameters:

- **`loginrequests`** — string; multi-value (max 50; 500 for high-limit users)  
  Only use these authentication requests, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=login` or from a previous response from this module.
- **`loginmessageformat`** — enum: `html`, `none`, `raw`, `wikitext`; default `wikitext`  
  Format to use for returning messages.
- **`loginmergerequestfields`** — boolean  
  Merge field information for all authentication requests into one array.
- **`loginpreservestate`** — boolean  
  Preserve state from a previous failed login attempt, if possible.
- **`loginreturnurl`** — string  
  Return URL for third-party authentication flows, must be absolute. Either this or `logincontinue` is required.
  
  Upon receiving a `REDIRECT` response, you will typically open a browser or web view to the specified `redirecttarget` URL for a third-party authentication flow. When that completes, the third party will send the browser or web view to this URL. You should extract any query or POST parameters from the URL and pass them as a `logincontinue` request to this API module.
- **`logincontinue`** — boolean  
  This request is a continuation after an earlier `UI` or `REDIRECT` response. Either this or `loginreturnurl` is required.
- **`logintoken`** — string; **required**; token type `login`; sensitive  
  A "login" token retrieved from action=query&meta=tokens

Dynamic parameters: This module accepts additional parameters depending on the available authentication requests. Use `action=query&meta=authmanagerinfo` with `amirequestsfor=login` (or a previous response from this module, if applicable) to determine the requests available and the fields that they use.

Examples:

- `api.php?action=clientlogin&username=Example&password=ExamplePassword&loginreturnurl=http://example.org/&logintoken=123ABC` — Start the process of logging in to the wiki as user `Example` with password `ExamplePassword`.
- `api.php?action=clientlogin&logincontinue=1&OATHToken=987654&logintoken=123ABC` — Continue logging in after a `UI` response for two-factor auth, supplying an `OATHToken` of `987654`.

<a id="mod-compare"></a>
#### `action=compare`

*Core* · `MediaWiki\Api\ApiComparePages` · GET or POST

Get the difference between two pages.

A revision number, a page title, a page ID, text, or a relative reference for both "from" and "to" must be passed.

Parameters:

- **`fromtitle`** — string  
  First title to compare.
- **`fromid`** — integer  
  First page ID to compare.
- **`fromrev`** — integer  
  First revision to compare.
- **`fromslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users)  
  Override content of the revision specified by `fromtitle`, `fromid` or `fromrev`.
  
  This parameter specifies the slots that are to be modified. Use `fromtext-{slot}`, `fromcontentmodel-{slot}`, and `fromcontentformat-{slot}` to specify content for each slot.
- **`frompst`** — boolean  
  Do a pre-save transform on `fromtext-{slot}`.
- **`fromtext`** — text; **deprecated**  
  Specify `fromslots=main` and use `fromtext-main` instead.
- **`fromcontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Specify `fromslots=main` and use `fromcontentformat-main` instead.
- **`fromcontentmodel`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`; **deprecated**  
  Specify `fromslots=main` and use `fromcontentmodel-main` instead.
- **`fromsection`** — string; **deprecated**  
  Only use the specified section of the specified 'from' content.
- **`totitle`** — string  
  Second title to compare.
- **`toid`** — integer  
  Second page ID to compare.
- **`torev`** — integer  
  Second revision to compare.
- **`torelative`** — enum: `cur`, `next`, `prev`  
  Use a revision relative to the revision determined from `fromtitle`, `fromid` or `fromrev`. All of the other 'to' options will be ignored.
- **`toslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users)  
  Override content of the revision specified by `totitle`, `toid` or `torev`.
  
  This parameter specifies the slots that are to be modified. Use `totext-{slot}`, `tocontentmodel-{slot}`, and `tocontentformat-{slot}` to specify content for each slot.
- **`topst`** — boolean  
  Do a pre-save transform on `totext`.
- **`totext`** — text; **deprecated**  
  Specify `toslots=main` and use `totext-main` instead.
- **`tocontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Specify `toslots=main` and use `tocontentformat-main` instead.
- **`tocontentmodel`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`; **deprecated**  
  Specify `toslots=main` and use `tocontentmodel-main` instead.
- **`tosection`** — string; **deprecated**  
  Only use the specified section of the specified 'to' content.
- **`prop`** — enum: `comment`, `diff`, `diffsize`, `ids`, `parsedcomment`, `rel`, `size`, `timestamp`, `title`, `user`; multi-value (max 50; 500 for high-limit users); default `diff|ids|title`  
  Which pieces of information to get.
  
  - **diff**: The diff HTML.
  - **diffsize**: The size of the diff HTML, in bytes.
  - **rel**: The revision IDs of the revision previous to 'from' and after 'to', if any.
  - **ids**: The page and revision IDs of the 'from' and 'to' revisions.
  - **title**: The page titles of the 'from' and 'to' revisions.
  - **user**: The username and ID of the 'from' and 'to' revisions. If the user has been revision deleted, a `fromuserhidden` or `touserhidden` property will be returned.
  - **comment**: The comment on the 'from' and 'to' revisions. If the comment has been revision deleted, a `fromcommenthidden` or `tocommenthidden` property will be returned.
  - **parsedcomment**: The parsed comment on the 'from' and 'to' revisions. If the comment has been revision deleted, a `fromcommenthidden` or `tocommenthidden` property will be returned.
  - **size**: The size of the 'from' and 'to' revisions.
  - **timestamp**: The timestamp of the 'from' and 'to' revisions.
- **`slots`** — enum: `main`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Return individual diffs for these slots, rather than one combined diff for all slots.
- **`difftype`** — enum: `table`, `unified`; default `table`  
  Return the comparison formatted as inline HTML.
- **`fromtext-{slot}`** — text; templated: `{slot}` ← values of `fromslots`  
  Text of the specified slot. If omitted, the slot is removed from the revision.
- **`fromsection-{slot}`** — string; templated: `{slot}` ← values of `fromslots`  
  When `fromtext-{slot}` is the content of a single section, this is the section identifier. It will be merged into the revision specified by `fromtitle`, `fromid` or `fromrev` as if for a section edit.
- **`fromcontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `fromslots`  
  Content serialization format of `fromtext-{slot}`.
- **`fromcontentmodel-{slot}`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`; templated: `{slot}` ← values of `fromslots`  
  Content model of `fromtext-{slot}`. If not supplied, it will be guessed based on the other parameters.
- **`totext-{slot}`** — text; templated: `{slot}` ← values of `toslots`  
  Text of the specified slot. If omitted, the slot is removed from the revision.
- **`tosection-{slot}`** — string; templated: `{slot}` ← values of `toslots`  
  When `totext-{slot}` is the content of a single section, this is the section identifier. It will be merged into the revision specified by `totitle`, `toid` or `torev` as if for a section edit.
- **`tocontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `toslots`  
  Content serialization format of `totext-{slot}`.
- **`tocontentmodel-{slot}`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`; templated: `{slot}` ← values of `toslots`  
  Content model of `totext-{slot}`. If not supplied, it will be guessed based on the other parameters.

Examples:

- `api.php?action=compare&fromrev=1&torev=2` — Create a diff between revision 1 and 2.

<a id="mod-createaccount"></a>
#### `action=createaccount`

*Core* · `MediaWiki\Api\ApiAMCreateAccount` · prefix `create` · POST only · write · token `createaccount`

Create a new user account.

The general procedure to use this module is:
# Fetch the fields available from `action=query&meta=authmanagerinfo` with `amirequestsfor=create`, and a `createaccount` token from `action=query&meta=tokens`.
# Present the fields to the user, and obtain their submission.
# Post to this module, supplying `createreturnurl` and any relevant fields.
# Check the `status` in the response.
#* If you received `PASS` or `FAIL`, you're done. The operation either succeeded or it didn't.
#* If you received `UI`, present the new fields to the user and obtain their submission. Then post to this module with `createcontinue` and the relevant fields set, and repeat step 4.
#* If you received `REDIRECT`, direct the user to the `redirecttarget` and wait for the return to `createreturnurl`. Then post to this module with `createcontinue` and any fields passed to the return URL, and repeat step 4.
#* If you received `RESTART`, that means the authentication worked but we don't have a linked user account. You might treat this as `UI` or as `FAIL`.

Parameters:

- **`createrequests`** — string; multi-value (max 50; 500 for high-limit users)  
  Only use these authentication requests, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=create` or from a previous response from this module.
- **`createmessageformat`** — enum: `html`, `none`, `raw`, `wikitext`; default `wikitext`  
  Format to use for returning messages.
- **`createmergerequestfields`** — boolean  
  Merge field information for all authentication requests into one array.
- **`createpreservestate`** — boolean  
  Preserve state from a previous failed login attempt, if possible.
  
  If `action=query&meta=authmanagerinfo` returned true for `hasprimarypreservedstate`, requests marked as `primary-required` should be omitted. If it returned a non-empty value for `preservedusername`, that username must be used for the `username` parameter.
- **`createreturnurl`** — string  
  Return URL for third-party authentication flows, must be absolute. Either this or `createcontinue` is required.
  
  Upon receiving a `REDIRECT` response, you will typically open a browser or web view to the specified `redirecttarget` URL for a third-party authentication flow. When that completes, the third party will send the browser or web view to this URL. You should extract any query or POST parameters from the URL and pass them as a `createcontinue` request to this API module.
- **`createcontinue`** — boolean  
  This request is a continuation after an earlier `UI` or `REDIRECT` response. Either this or `createreturnurl` is required.
- **`createtoken`** — string; **required**; token type `createaccount`; sensitive  
  A "createaccount" token retrieved from action=query&meta=tokens

Dynamic parameters: This module accepts additional parameters depending on the available authentication requests. Use `action=query&meta=authmanagerinfo` with `amirequestsfor=create` (or a previous response from this module, if applicable) to determine the requests available and the fields that they use.

Examples:

- `api.php?action=createaccount&username=Example&password=ExamplePassword&retype=ExamplePassword&createreturnurl=http://example.org/&createtoken=123ABC` — Start the process of creating the user `Example` with the password `ExamplePassword`.

<a id="mod-cspreport"></a>
#### `action=cspreport`

*Core* · `MediaWiki\Api\ApiCSPReport` · POST only · **internal**

Used by browsers to report violations of the Content Security Policy. This module should never be used, except when used automatically by a CSP compliant web browser.

Parameters:

- **`reportonly`** — boolean  
  Mark as being a report from a monitoring policy, not an enforced policy
- **`source`** — string; default `internal`  
  What generated the CSP header that triggered this report

<a id="mod-delete"></a>
#### `action=delete`

*Core* · `MediaWiki\Api\ApiDelete` · POST only · write · token `csrf`

Delete a page.

Parameters:

- **`title`** — string  
  Title of the page to delete. Cannot be used together with `pageid`.
- **`pageid`** — integer  
  Page ID of the page to delete. Cannot be used together with `title`.
- **`reason`** — string  
  Reason for the deletion. If not set, an automatically generated reason will be used.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the deletion log.
- **`deletetalk`** — boolean  
  Delete the talk page, if it exists.
- **`watch`** — boolean; **deprecated**  
  Add the page to the current user's watchlist.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`unwatch`** — boolean; **deprecated**  
  Remove the page from the current user's watchlist.
- **`oldimage`** — string  
  The name of the old image to delete as provided by action=query&prop=imageinfo&iiprop=archivename.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=delete&title=Main%20Page&token=123ABC` — Delete Main Page.
- `api.php?action=delete&title=Main%20Page&token=123ABC&reason=Preparing%20for%20move` — Delete Main Page with the reason `Preparing for move`.

<a id="mod-edit"></a>
#### `action=edit`

*Core* · `MediaWiki\Api\ApiEditPage` · POST only · write · token `csrf`

Create and edit pages.

Parameters:

- **`title`** — string  
  Title of the page to edit. Cannot be used together with `pageid`.
- **`pageid`** — integer  
  Page ID of the page to edit. Cannot be used together with `title`.
- **`section`** — string  
  Section identifier. `0` for the top section, `new` for a new section. Often a positive integer, but can also be non-numeric.
- **`sectiontitle`** — string  
  The title for a new section when using `section=new`.
- **`text`** — text  
  Page content.
- **`summary`** — string  
  Edit summary.
  
  When this parameter is not provided or empty, an edit summary may be generated automatically.
  
  When using `section=new` and `sectiontitle` is not provided, the value of this parameter is used for the section title instead, and an edit summary is generated automatically.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the revision.
- **`minor`** — boolean  
  Mark this edit as a minor edit.
- **`notminor`** — boolean  
  Do not mark this edit as a minor edit even if the "Mark all edits minor by default" user preference is set.
- **`bot`** — boolean  
  Mark this edit as a bot edit.
- **`baserevid`** — integer  
  ID of the base revision, used to detect edit conflicts. May be obtained through action=query&prop=revisions. Self-conflicts cause the edit to fail unless basetimestamp is set.
- **`basetimestamp`** — timestamp  
  Timestamp of the base revision, used to detect edit conflicts. May be obtained through action=query&prop=revisions&rvprop=timestamp. Self-conflicts are ignored.
- **`starttimestamp`** — timestamp  
  Timestamp when the editing process began, used to detect edit conflicts. An appropriate value may be obtained using `curtimestamp` when beginning the edit process (e.g. when loading the page content to edit).
- **`recreate`** — boolean  
  Override any errors about the page having been deleted in the meantime.
- **`createonly`** — boolean  
  Don't edit the page if it exists already.
- **`nocreate`** — boolean  
  Throw an error if the page doesn't exist.
- **`watch`** — boolean; **deprecated**  
  Add the page to the current user's watchlist.
- **`unwatch`** — boolean; **deprecated**  
  Remove the page from the current user's watchlist.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`md5`** — string  
  The MD5 hash of the text parameter, or the prependtext and appendtext parameters concatenated. If set, the edit won't be done unless the hash is correct.
- **`prependtext`** — text  
  Add this text to the beginning of the page or section. Overrides text.
- **`appendtext`** — text  
  Add this text to the end of the page or section. Overrides text.
  
  Use section=new to append a new section, rather than this parameter.
- **`undo`** — integer (min 0)  
  Undo this revision. Overrides text, prependtext and appendtext.
- **`undoafter`** — integer (min 0)  
  Undo all revisions from undo to this one. If not set, just undo one revision.
- **`redirect`** — boolean  
  Automatically resolve redirects.
- **`contentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`  
  Content serialization format used for the input text.
- **`contentmodel`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`  
  Content model of the new content.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens
  
  The token should always be sent as the last parameter, or at least after the text parameter.
- **`returnto`** — title  
  Page title. If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to the given page, instead of the page that was edited.
- **`returntoquery`** — string  
  URL query parameters (with leading `?`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given query parameters.
- **`returntoanchor`** — string  
  URL fragment (with leading `#`). If saving the edit created a temporary account, the API may respond with an URL that the client should visit to complete logging in. If this parameter is provided, the URL will redirect to a page with the given fragment.

Examples:

- `api.php?action=edit&title=Test&summary=test%20summary&text=article%20content&baserevid=1234567&token=123ABC` — Edit a page.
- `api.php?action=edit&title=Test&summary=NOTOC&minor=&prependtext=__NOTOC__%0A&basetimestamp=2007-08-24T12:34:54Z&token=123ABC` — Prepend `__NOTOC__` to a page.
- `api.php?action=edit&title=Test&undo=13585&undoafter=13579&basetimestamp=2007-08-24T12:34:54Z&token=123ABC` — Undo revisions 13579 through 13585 with autosummary.

<a id="mod-emailuser"></a>
#### `action=emailuser`

*Core* · `MediaWiki\Api\ApiEmailUser` · POST only · write · token `csrf`

Email a user.

Parameters:

- **`target`** — string; **required**  
  User to send the email to.
- **`subject`** — string; **required**  
  Subject header.
- **`text`** — text; **required**  
  Email body.
- **`ccme`** — boolean  
  Send a copy of this mail to me.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=emailuser&target=WikiSysop&text=Content&token=123ABC` — Send an email to the user `WikiSysop` with the text `Content`.

<a id="mod-expandtemplates"></a>
#### `action=expandtemplates`

*Core* · `MediaWiki\Api\ApiExpandTemplates` · GET or POST

Expands all templates within wikitext.

Parameters:

- **`title`** — string  
  Title of the page.
- **`text`** — text; **required**  
  Wikitext to convert.
- **`revid`** — integer  
  Revision ID, for `<nowiki>{{REVISIONID}}</nowiki>` and similar variables.
- **`prop`** — enum: `categories`, `encodedjsconfigvars`, `jsconfigvars`, `modules`, `parsetree`, `properties`, `ttl`, `volatile`, `wikitext`; multi-value (max 50; 500 for high-limit users)  
  Which pieces of information to get.
  
  Note that if no values are selected, the result will contain the wikitext, but the output will be in a deprecated format.
  
  - **wikitext**: The expanded wikitext.
  - **categories**: Any categories present in the input that are not represented in the wikitext output.
  - **properties**: Page properties defined by expanded magic words in the wikitext.
  - **volatile**: Whether the output is volatile and should not be reused elsewhere within the page.
  - **ttl**: The maximum time after which caches of the result should be invalidated.
  - **modules**: Any ResourceLoader modules that parser functions have requested be added to the output. Either `jsconfigvars` or `encodedjsconfigvars` must be requested jointly with `modules`.
  - **jsconfigvars**: Gives the JavaScript configuration variables specific to the page.
  - **encodedjsconfigvars**: Gives the JavaScript configuration variables specific to the page as a JSON string.
  - **parsetree**: The XML parse tree of the input.
- **`includecomments`** — boolean  
  Whether to include HTML comments in the output.
- **`showstrategykeys`** — boolean  
  Whether to include internal merge strategy information in jsconfigvars.
- **`generatexml`** — boolean; **deprecated**  
  Generate XML parse tree (replaced by prop=parsetree).

Examples:

- `api.php?action=expandtemplates&text={{Project:Sandbox}}` — Expand the wikitext `<nowiki>{{Project:Sandbox}}</nowiki>`.

<a id="mod-feedcontributions"></a>
#### `action=feedcontributions`

*Core* · `MediaWiki\Api\ApiFeedContributions` · GET or POST

Returns a user's contributions feed.

Parameters:

- **`feedformat`** — enum: `atom`, `rss`; default `rss`  
  The format of the feed.
- **`user`** — user; **required**  
  What users to get the contributions for.
- **`namespace`** — namespace  
  Which namespace to filter the contributions by.
- **`year`** — integer  
  From year (and earlier).
- **`month`** — integer  
  From month (and earlier).
- **`tagfilter`** — enum: `mw-blank`, `mw-changed-redirect-target`, `mw-contentmodelchange`, `mw-manual-revert`, `mw-new-redirect`, `mw-removed-redirect`, `mw-replace`, `mw-reverted`, `mw-rollback`, `mw-server-side-upload`, `mw-undo`; multi-value (max 50; 500 for high-limit users)  
  Filter contributions that have these tags.
- **`deletedonly`** — boolean  
  Show only deleted contributions.
- **`toponly`** — boolean  
  Only show edits that are the latest revisions.
- **`newonly`** — boolean  
  Only show edits that are page creations.
- **`hideminor`** — boolean  
  Hide minor edits.
- **`showsizediff`** — boolean  
  Show the size difference between revisions.

Examples:

- `api.php?action=feedcontributions&user=Example` — Return contributions for user `Example`.

<a id="mod-feedrecentchanges"></a>
#### `action=feedrecentchanges`

*Core* · `MediaWiki\Api\ApiFeedRecentChanges` · GET or POST

Returns a recent changes feed.

Parameters:

- **`feedformat`** — enum: `atom`, `rss`; default `rss`  
  The format of the feed.
- **`namespace`** — namespace  
  Namespace to limit the results to.
- **`invert`** — boolean  
  All namespaces but the selected one.
- **`associated`** — boolean  
  Include associated (talk or main) namespace.
- **`days`** — integer (min 1); default `7`  
  Days to limit the results to.
- **`limit`** — integer (min 1, max 50); default `50`  
  Maximum number of results to return.
- **`from`** — timestamp  
  Show changes since then.
- **`hideminor`** — boolean  
  Hide minor changes.
- **`hidebots`** — boolean  
  Hide changes made by bots.
- **`hideanons`** — boolean  
  Hide changes made by anonymous users.
- **`hideliu`** — boolean  
  Hide changes made by registered users.
- **`hidepatrolled`** — boolean  
  Hide patrolled changes.
- **`hidemyself`** — boolean  
  Hide changes made by the current user.
- **`hidecategorization`** — boolean  
  Hide category membership changes.
- **`tagfilter`** — string  
  Filter by tag.
- **`inverttags`** — boolean  
  All edits except ones tagged with the selected ones.
- **`target`** — string  
  Show only changes on pages linked from this page.
- **`showlinkedto`** — boolean  
  Show changes on pages linked to the selected page instead.

Examples:

- `api.php?action=feedrecentchanges` — Show recent changes.
- `api.php?action=feedrecentchanges&days=30` — Show recent changes for 30 days.

<a id="mod-feedwatchlist"></a>
#### `action=feedwatchlist`

*Core* · `MediaWiki\Api\ApiFeedWatchlist` · GET or POST

Returns a watchlist feed.

Parameters:

- **`feedformat`** — enum: `atom`, `rss`; default `rss`  
  The format of the feed.
- **`hours`** — integer (min 1, max 72); default `24`  
  List pages modified within this many hours from now.
- **`linktosections`** — boolean  
  Link directly to changed sections if possible.
- **`allrev`** — boolean  
  Include multiple revisions of the same page within given timeframe.
- **`wlowner`** — user  
  Used along with token to access a different user's watchlist.
- **`wltoken`** — string; sensitive  
  A security token (available in the user's preferences) to allow access to another user's watchlist.
- **`wlshow`** — enum: `!anon`, `!autopatrolled`, `!bot`, `!minor`, `!patrolled`, `!unread`, `anon`, `autopatrolled`, `bot`, `minor`, `patrolled`, `unread`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria. For example, to see only minor edits done by logged-in users, set show=minor|!anon.
- **`wltype`** — enum: `categorize`, `edit`, `external`, `log`, `new`; multi-value (max 50; 500 for high-limit users); default `edit|new|log|categorize`  
  Which types of changes to show:
  
  - **edit**: Regular page edits.
  - **new**: Page creations.
  - **log**: Log entries.
  - **external**: External changes.
  - **categorize**: Category membership changes.
- **`wlexcludeuser`** — user  
  Don't list changes by this user.

Examples:

- `api.php?action=feedwatchlist` — Show the watchlist feed.
- `api.php?action=feedwatchlist&allrev=&hours=6` — Show all changes to watched pages in the past 6 hours.

<a id="mod-filerevert"></a>
#### `action=filerevert`

*Core* · `MediaWiki\Api\ApiFileRevert` · POST only · write · token `csrf`

Revert a file to an old version.

Parameters:

- **`filename`** — string; **required**  
  Target filename, without the File: prefix.
- **`comment`** — string  
  Upload comment.
- **`archivename`** — string; **required**  
  Archive name of the revision to revert to.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=filerevert&filename=Wiki.png&comment=Revert&archivename=20110305152740!Wiki.png&token=123ABC` — Revert `Wiki.png` to the version of `2011-03-05T15:27:40Z`.

<a id="mod-help"></a>
#### `action=help`

*Core* · `MediaWiki\Api\ApiHelp` · GET or POST

Display help for the specified modules.

Parameters:

- **`modules`** — string; multi-value (max 50; 500 for high-limit users); default `main`  
  Modules to display help for (values of the `action` and `format` parameters, or `main`). Can specify submodules with a `+`.
- **`submodules`** — boolean  
  Include help for submodules of the named module.
- **`recursivesubmodules`** — boolean  
  Include help for submodules recursively.
- **`wrap`** — boolean  
  Wrap the output in a standard API response structure.
- **`toc`** — boolean  
  Include a table of contents in the HTML output.

Examples:

- `api.php?action=help` — Help for the main module.
- `api.php?action=help&modules=query&submodules=1` — Help for `action=query` and all its submodules.
- `api.php?action=help&recursivesubmodules=1` — All help in one page.
- `api.php?action=help&modules=help` — Help for the help module itself.
- `api.php?action=help&modules=query+info|query+categorymembers` — Help for two query submodules.

<a id="mod-imagerotate"></a>
#### `action=imagerotate`

*Core* · `MediaWiki\Api\ApiImageRotate` · POST only · write · token `csrf`

Rotate one or more images.

Parameters:

- **`rotation`** — enum: `90`, `180`, `270`; **required**  
  Degrees to rotate image clockwise.
- **`continue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Tags to apply to the entry in the upload log.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of titles to work on.
- **`pageids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of page IDs to work on.
- **`revids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of revision IDs to work on. Note that almost all query modules will convert revision IDs to the corresponding page ID and work on the latest revision instead. Only `prop=revisions` uses exact revisions for its response.
- **`generator`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `backlinks`, `categories`, `categorymembers`, `deletedrevisions`, `duplicatefiles`, `embeddedin`, `exturlusage`, `fileusage`, `images`, `imageusage`, `iwbacklinks`, `langbacklinks`, `links`, `linkshere`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `redirects`, `revisions`, `search`, `templates`, `transcludedin`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`; internal values: `wbsearch`
- **`redirects`** — boolean  
  Automatically resolve redirects in `titles`, `pageids`, and `revids`, and in pages returned by `generator`.
- **`converttitles`** — boolean  
  Convert titles to other variants if necessary. Only works if the wiki's content language supports variant conversion. Languages that support variant conversion include ban, en, crh, gan, iu, ku, mni, sh, shi, sr, tg, tly, uz, wuu, zgh and zh.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=imagerotate&titles=File:Example.jpg&rotation=90&token=123ABC` — Rotate `File:Example.png` by `90` degrees.
- `api.php?action=imagerotate&generator=categorymembers&gcmtitle=Category:Flip&gcmtype=file&rotation=180&token=123ABC` — Rotate all images in `Category:Flip` by `180` degrees.

<a id="mod-import"></a>
#### `action=import`

*Core* · `MediaWiki\Api\ApiImport` · POST only · write · token `csrf`

Import a page from another wiki, or from an XML file.

Note that the HTTP POST must be done as a file upload (i.e. using multipart/form-data) when sending a file for the `xml` parameter.

Parameters:

- **`summary`** — string  
  Log entry import summary.
- **`xml`** — upload  
  Uploaded XML file.
- **`interwikiprefix`** — string  
  For uploaded imports: interwiki prefix to apply to unknown usernames (and known users if `assignknownusers` is set).
- **`interwikisource`** — enum (no values in reference config — site-/config-dependent)  
  For interwiki imports: wiki to import from.
- **`interwikipage`** — string  
  For interwiki imports: page to import.
- **`fullhistory`** — boolean  
  For interwiki imports: import the full history, not just the current version.
- **`templates`** — boolean  
  For interwiki imports: import all included templates as well.
- **`namespace`** — namespace  
  Import to this namespace. Cannot be used together with `rootpage`.
- **`assignknownusers`** — boolean  
  Assign edits to local users where the named user exists locally.
- **`rootpage`** — string  
  Import as subpage of this page. Cannot be used together with `namespace`.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the import log and to the null revision on the imported pages.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=import&interwikisource=meta&interwikipage=Help:ParserFunctions&namespace=100&fullhistory=&token=123ABC` — Import meta:Help:ParserFunctions to namespace 100 with full history.

<a id="mod-linkaccount"></a>
#### `action=linkaccount`

*Core* · `MediaWiki\Api\ApiLinkAccount` · prefix `link` · POST only · write · token `csrf`

Link an account from a third-party provider to the current user.

The general procedure to use this module is:
# Fetch the fields available from `action=query&meta=authmanagerinfo` with `amirequestsfor=link`, and a `csrf` token from `action=query&meta=tokens`.
# Present the fields to the user, and obtain their submission.
# Post to this module, supplying `linkreturnurl` and any relevant fields.
# Check the `status` in the response.
#* If you received `PASS` or `FAIL`, you're done. The operation either succeeded or it didn't.
#* If you received `UI`, present the new fields to the user and obtain their submission. Then post to this module with `linkcontinue` and the relevant fields set, and repeat step 4.
#* If you received `REDIRECT`, direct the user to the `redirecttarget` and wait for the return to `linkreturnurl`. Then post to this module with `linkcontinue` and any fields passed to the return URL, and repeat step 4.
#* If you received `RESTART`, that means the authentication worked but we don't have a linked user account. You might treat this as `UI` or as `FAIL`.

Parameters:

- **`linkrequests`** — string; multi-value (max 50; 500 for high-limit users)  
  Only use these authentication requests, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=link` or from a previous response from this module.
- **`linkmessageformat`** — enum: `html`, `none`, `raw`, `wikitext`; default `wikitext`  
  Format to use for returning messages.
- **`linkmergerequestfields`** — boolean  
  Merge field information for all authentication requests into one array.
- **`linkreturnurl`** — string  
  Return URL for third-party authentication flows, must be absolute. Either this or `linkcontinue` is required.
  
  Upon receiving a `REDIRECT` response, you will typically open a browser or web view to the specified `redirecttarget` URL for a third-party authentication flow. When that completes, the third party will send the browser or web view to this URL. You should extract any query or POST parameters from the URL and pass them as a `linkcontinue` request to this API module.
- **`linkcontinue`** — boolean  
  This request is a continuation after an earlier `UI` or `REDIRECT` response. Either this or `linkreturnurl` is required.
- **`linktoken`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Dynamic parameters: This module accepts additional parameters depending on the available authentication requests. Use `action=query&meta=authmanagerinfo` with `amirequestsfor=link` (or a previous response from this module, if applicable) to determine the requests available and the fields that they use.

Examples:

- `api.php?action=linkaccount&provider=Example&linkreturnurl=http://example.org/&linktoken=123ABC` — Start the process of linking to an account from `Example`.

<a id="mod-login"></a>
#### `action=login`

*Core* · `MediaWiki\Api\ApiLogin` · prefix `lg` · POST only · write

Log in and get authentication cookies.

This action should only be used in combination with Special:BotPasswords; use for main-account login is deprecated and may fail without warning. To safely log in to the main account, use `action=clientlogin`.

Parameters:

- **`lgname`** — string  
  Username.
- **`lgpassword`** — password; sensitive  
  Password.
- **`lgdomain`** — string  
  Domain (optional).
- **`lgtoken`** — string; sensitive  
  A "login" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=login&lgname=user&lgpassword=password&lgtoken=123ABC` — Log in.

<a id="mod-logout"></a>
#### `action=logout`

*Core* · `MediaWiki\Api\ApiLogout` · POST only · write · token `csrf`

Log out and clear session data.

Parameters:

- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=logout&token=123ABC` — Log the current user out.

<a id="mod-managetags"></a>
#### `action=managetags`

*Core* · `MediaWiki\Api\ApiManageTags` · POST only · write · token `csrf`

Perform management tasks relating to change tags.

Parameters:

- **`operation`** — enum: `activate`, `create`, `deactivate`, `delete`; **required**  
  Which operation to perform:
  
  - **create**: Create a new change tag for manual use.
  - **delete**: Remove a change tag from the database, including removing the tag from all revisions, recent change entries and log entries on which it is used.
  - **activate**: Activate a change tag, allowing users to apply it manually.
  - **deactivate**: Deactivate a change tag, preventing users from applying it manually.
- **`tag`** — string; **required**  
  Tag to create, delete, activate or deactivate. For tag creation, the tag must not exist. For tag deletion, the tag must exist. For tag activation, the tag must exist and not be in use by an extension. For tag deactivation, the tag must be currently active and manually defined.
- **`reason`** — string  
  An optional reason for creating, deleting, activating or deactivating the tag.
- **`ignorewarnings`** — boolean  
  Whether to ignore any warnings that are issued during the operation.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the tag management log.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=managetags&operation=create&tag=spam&reason=For+use+in+edit+patrolling&token=123ABC` — Create a tag named `spam` with the reason `For use in edit patrolling`
- `api.php?action=managetags&operation=delete&tag=vandlaism&reason=Misspelt&token=123ABC` — Delete the `vandlaism` tag with the reason `Misspelt`
- `api.php?action=managetags&operation=activate&tag=spam&reason=For+use+in+edit+patrolling&token=123ABC` — Activate a tag named `spam` with the reason `For use in edit patrolling`
- `api.php?action=managetags&operation=deactivate&tag=spam&reason=No+longer+required&token=123ABC` — Deactivate a tag named `spam` with the reason `No longer required`

<a id="mod-mergehistory"></a>
#### `action=mergehistory`

*Core* · `MediaWiki\Api\ApiMergeHistory` · POST only · write · token `csrf`

Merge page histories.

Parameters:

- **`from`** — string  
  Title of the page from which history will be merged. Cannot be used together with `fromid`.
- **`fromid`** — integer  
  Page ID of the page from which history will be merged. Cannot be used together with `from`.
- **`to`** — string  
  Title of the page to which history will be merged. Cannot be used together with `toid`.
- **`toid`** — integer  
  Page ID of the page to which history will be merged. Cannot be used together with `to`.
- **`timestamp`** — timestamp  
  Timestamp up to which revisions will be moved from the source page's history to the destination page's history. If omitted, the entire page history of the source page will be merged into the destination page.
- **`reason`** — string  
  Reason for the history merge.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=mergehistory&from=Oldpage&to=Newpage&token=123ABC&reason=Reason` — Merge the entire history of `Oldpage` into `Newpage`.
- `api.php?action=mergehistory&from=Oldpage&to=Newpage&token=123ABC&reason=Reason&timestamp=2015-12-31T04%3A37%3A41Z` — Merge the page revisions of `Oldpage` dating up to `2015-12-31T04:37:41Z` into `Newpage`.

<a id="mod-move"></a>
#### `action=move`

*Core* · `MediaWiki\Api\ApiMove` · POST only · write · token `csrf`

Move a page.

Parameters:

- **`from`** — string  
  Title of the page to rename. Cannot be used together with `fromid`.
- **`fromid`** — integer  
  Page ID of the page to rename. Cannot be used together with `from`.
- **`to`** — string; **required**  
  Title to rename the page to.
- **`reason`** — string  
  Reason for the rename.
- **`movetalk`** — boolean  
  Rename the talk page, if it exists.
- **`movesubpages`** — boolean  
  Rename subpages, if applicable.
- **`noredirect`** — boolean  
  Don't create a redirect.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`ignorewarnings`** — boolean  
  Ignore any warnings.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the move log and to the null revision on the destination page.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=move&from=Badtitle&to=Goodtitle&token=123ABC&reason=Misspelled%20title&movetalk=&noredirect=` — Move `Badtitle` to `Goodtitle` without leaving a redirect.

<a id="mod-opensearch"></a>
#### `action=opensearch`

*Core* · `MediaWiki\Api\ApiOpenSearch` · GET or POST

Search the wiki using the OpenSearch protocol.

Parameters:

- **`search`** — string; **required**  
  Search string.
- **`namespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Namespaces to search. Ignored if `search` begins with a valid namespace prefix.
- **`limit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  Maximum number of results to return.
- **`suggest`** — boolean; **deprecated**  
  No longer used.
- **`redirects`** — enum: `resolve`, `return`  
  How to handle redirects:
  
  - **return**: Return the redirect itself.
  - **resolve**: Return the target page. May return fewer than limit results.
  
  For historical reasons, the default is "return" for format=json and "resolve" for other formats.
- **`format`** — enum: `json`, `jsonfm`, `xml`, `xmlfm`; default `json`  
  The format of the output.
- **`warningsaserror`** — boolean  
  If warnings are raised with `format=json`, return an API error instead of ignoring them.

Examples:

- `api.php?action=opensearch&search=Te` — Find pages beginning with `Te`.

<a id="mod-options"></a>
#### `action=options`

*Core* · `MediaWiki\Api\ApiOptions` · POST only · write · token `csrf`

Change preferences of the current user.

Only options which are registered in core or in one of installed extensions, or options with keys prefixed with `userjs-` (intended to be used by user scripts), can be set.

Parameters:

- **`reset`** — boolean  
  Resets preferences to the site defaults.
- **`resetkinds`** — enum: `all`, `registered`, `registered-checkmatrix`, `registered-multiselect`, `special`, `unused`, `userjs`; multi-value (max 50; 500 for high-limit users); default `all`  
  List of types of options to reset when the `reset` option is set.
- **`change`** — string; multi-value (max 50; 500 for high-limit users)  
  List of changes, formatted name=value (e.g. skin=vector). If no value is given (not even an equals sign), e.g., optionname|otheroption|..., the option will be reset to its default value. If any value passed contains the pipe character (`|`), use the alternative multiple-value separator for correct operation.
- **`optionname`** — string  
  The name of the option that should be set to the value given by `optionvalue`.
- **`optionvalue`** — string  
  The value for the option specified by `optionname`. When `optionname` is set but `optionvalue` is omitted, the option will be reset to its default value.
- **`global`** — enum: `ignore`, `override`, `update`; default `ignore`  
  What to do if the option was set globally using the GlobalPreferences extension.
  
  - `ignore`: Do nothing. The option remains with its previous value.
  - `override`: Add a local override.
  - `update`: Update the option globally.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=options&reset=&token=123ABC` — Reset all preferences.
- `api.php?action=options&change=skin=vector|hideminor=1&token=123ABC` — Change `skin` and `hideminor` preferences.
- `api.php?action=options&reset=&change=skin=monobook&optionname=nickname&optionvalue=[[User:Beau|Beau]]%20([[User_talk:Beau|talk]])&token=123ABC` — Reset all preferences, then set `skin` and `nickname`.

<a id="mod-paraminfo"></a>
#### `action=paraminfo`

*Core* · `MediaWiki\Api\ApiParamInfo` · GET or POST

Obtain information about API modules.

Parameters:

- **`modules`** — string; multi-value (max 50; 500 for high-limit users)  
  List of module names (values of the `action` and `format` parameters, or `main`). Can specify submodules with a `+`, or all submodules with `+*`, or all submodules recursively with `+**`.
- **`helpformat`** — enum: `html`, `none`, `raw`, `wikitext`; default `none`  
  Format of help strings.
- **`querymodules`** — enum: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allmessages`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `allusers`, `authmanagerinfo`, `backlinks`, `blocks`, `categories`, `categoryinfo`, `categorymembers`, `contributors`, `deletedrevisions`, `deletedrevs`, `description`, `duplicatefiles`, `embeddedin`, `entityterms`, `extlinks`, `exturlusage`, `filearchive`, `filerepoinfo`, `fileusage`, `imageinfo`, `images`, `imageusage`, `info`, `iwbacklinks`, `iwlinks`, `langbacklinks`, `langlinks`, `languageinfo`, `links`, `linkshere`, … (71 values total; config-dependent); multi-value (max 50; 500 for high-limit users); **deprecated**  
  List of query module names (value of `prop`, `meta` or `list` parameter). Use `modules=query+foo` instead of `querymodules=foo`.
- **`mainmodule`** — string; **deprecated**  
  Get information about the main (top-level) module as well. Use `modules=main` instead.
- **`pagesetmodule`** — string; **deprecated**  
  Get information about the pageset module (providing titles= and friends) as well.
- **`formatmodules`** — enum: `json`, `jsonfm`, `none`, `php`, `phpfm`, `rawfm`, `xml`, `xmlfm`; multi-value (max 50; 500 for high-limit users); **deprecated**  
  List of format module names (value of `format` parameter). Use `modules` instead.

Examples:

- `api.php?action=paraminfo&modules=parse|phpfm|query%2Ballpages|query%2Bsiteinfo` — Show info for `action=parse`, `format=jsonfm`, `action=query&list=allpages`, and `action=query&meta=siteinfo`.
- `api.php?action=paraminfo&modules=query%2B*` — Show info for all submodules of `action=query`.

<a id="mod-parse"></a>
#### `action=parse`

*Core* · `MediaWiki\Api\ApiParse` · GET or POST

Parses content and returns parser output.

See the various prop-modules of `action=query` to get information from the current version of a page.

There are several ways to specify the text to parse:
# Specify a page or revision, using `page`, `pageid`, or `oldid`.
# Specify content explicitly, using `text`, `title`, `revid`, and `contentmodel`.
# Specify only a summary to parse. `prop` should be given an empty value.

Parameters:

- **`title`** — string  
  Title of page the text belongs to. If omitted, `contentmodel` must be specified, and API will be used as the title.
- **`text`** — text  
  Text to parse. Use `title` or `contentmodel` to control the content model.
- **`revid`** — integer  
  Revision ID, for `<nowiki>{{REVISIONID}}</nowiki>` and similar variables.
- **`summary`** — string  
  Summary to parse.
- **`page`** — string  
  Parse the content of this page. Cannot be used together with `text` and `title`.
- **`pageid`** — integer  
  Parse the content of this page. Overrides `page`.
- **`redirects`** — boolean  
  If `page` or `pageid` is set to a redirect, resolve it.
- **`oldid`** — integer  
  Parse the content of this revision. Overrides `page` and `pageid`.
- **`prop`** — enum: `categories`, `categorieshtml`, `displaytitle`, `encodedjsconfigvars`, `externallinks`, `headhtml`, `images`, `indicators`, `iwlinks`, `jsconfigvars`, `langlinks`, `limitreportdata`, `limitreporthtml`, `links`, `modules`, `parsetree`, `parsewarnings`, `parsewarningshtml`, `properties`, `revid`, `sections`, `subtitle`, `templates`, `text`, `tocdata`, `wikitext`, `headitems`; multi-value (max 50; 500 for high-limit users); default `text|langlinks|categories|links|templates|images|externallinks|sections|tocdata|revid|displaytitle|iwlinks|properties|parsewarnings`; deprecated values: `headitems`  
  Which pieces of information to get:
  
  - **text**: Gives the parsed text of the wikitext.
  - **langlinks**: Gives the language links in the parsed wikitext.
  - **categories**: Gives the categories in the parsed wikitext.
  - **categorieshtml**: Gives the HTML version of the categories.
  - **links**: Gives the internal links in the parsed wikitext.
  - **templates**: Gives the templates in the parsed wikitext.
  - **images**: Gives the images in the parsed wikitext.
  - **externallinks**: Gives the external links in the parsed wikitext.
  - **sections**: Gives the sections in the parsed wikitext.
  - **tocdata**: Gives the table of contents information in the parsed wikitext.
  - **revid**: Adds the revision ID of the parsed page.
  - **displaytitle**: Adds the title of the parsed wikitext.
  - **subtitle**: Adds the page subtitle for the parsed page.
  - **headhtml**: Gives parsed doctype, opening `<html>`, `<head>` element and opening `<body>` of the page.
  - **modules**: Gives the ResourceLoader modules used on the page. To load, use `mw.loader.using()`. Either `jsconfigvars` or `encodedjsconfigvars` must be requested jointly with `modules`.
  - **jsconfigvars**: Gives the JavaScript configuration variables specific to the page. To apply, use `mw.config.set()`.
  - **encodedjsconfigvars**: Gives the JavaScript configuration variables specific to the page as a JSON string.
  - **indicators**: Gives the HTML of page status indicators used on the page.
  - **iwlinks**: Gives interwiki links in the parsed wikitext.
  - **wikitext**: Gives the original wikitext that was parsed.
  - **properties**: Gives various properties defined in the parsed wikitext.
  - **limitreportdata**: Gives the limit report in a structured way. Gives no data, when `disablelimitreport` is set.
  - **limitreporthtml**: Gives the HTML version of the limit report. Gives no data, when `disablelimitreport` is set.
  - **parsetree**: The XML parse tree of revision content (requires content model `wikitext`)
  - **parsewarnings**: Gives the warnings that occurred while parsing content (as wikitext).
  - **parsewarningshtml**: Gives the warnings that occurred while parsing content (as HTML).
  - **headitems**: Deprecated. Gives items to put in the `<head>` of the page.
- **`wrapoutputclass`** — string; default `mw-parser-output`  
  CSS class to use to wrap the parser output.
- **`usearticle`** — boolean  
  Use the ArticleParserOptions hook to ensure the options used match those used for article page views
- **`parsoid`** — boolean  
  Generate HTML conforming to the MediaWiki DOM spec using Parsoid.
- **`pst`** — boolean  
  Do a pre-save transform on the input before parsing it. Only valid when used with text.
- **`onlypst`** — boolean  
  Do a pre-save transform (PST) on the input, but don't parse it. Returns the same wikitext, after a PST has been applied. Only valid when used with `text`.
- **`effectivelanglinks`** — boolean; **deprecated**  
  Includes language links supplied by extensions (for use with `prop=langlinks`).
- **`section`** — string  
  Only parse the content of the section with this identifier.
  
  When `new`, parse `text` and `sectiontitle` as if adding a new section to the page.
  
  `new` is allowed only when specifying `text`.
- **`sectiontitle`** — string  
  New section title when `section` is `new`.
  
  Unlike page editing, this does not fall back to `summary` when omitted or empty.
- **`disablepp`** — boolean; **deprecated**  
  Use `disablelimitreport` instead.
- **`disablelimitreport`** — boolean  
  Omit the limit report ("NewPP limit report") from the parser output.
- **`disableeditsection`** — boolean  
  Omit edit section links from the parser output.
- **`disablestylededuplication`** — boolean  
  Do not deduplicate inline stylesheets in the parser output.
- **`showstrategykeys`** — boolean  
  Whether to include internal merge strategy information in jsconfigvars.
- **`generatexml`** — boolean; **deprecated**  
  Generate XML parse tree (requires content model `wikitext`; replaced by `prop=parsetree`).
- **`preview`** — boolean  
  Parse in preview mode.
- **`sectionpreview`** — boolean  
  Parse in section preview mode (enables preview mode too).
- **`disabletoc`** — boolean  
  Omit table of contents in output.
- **`useskin`** — enum: `apioutput`, `authentication-popup`, `fallback`, `json`  
  Apply the selected skin to the parser output. May affect the following properties: `text`, `langlinks`, `headitems`, `modules`, `jsconfigvars`, `indicators`.
- **`contentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`  
  Content serialization format used for the input text. Only valid when used with text.
- **`contentmodel`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`  
  Content model of the input text. If omitted, title must be specified, and default will be the model of the specified title. Only valid when used with text.

Examples:

- `api.php?action=parse&page=Project:Sandbox` — Parse a page.
- `api.php?action=parse&text={{Project:Sandbox}}&contentmodel=wikitext` — Parse wikitext.
- `api.php?action=parse&text={{PAGENAME}}&title=Test` — Parse wikitext, specifying the page title.
- `api.php?action=parse&summary=Some+[[link]]&prop=` — Parse a summary.

<a id="mod-patrol"></a>
#### `action=patrol`

*Core* · `MediaWiki\Api\ApiPatrol` · POST only · write · token `patrol`

Patrol a page or revision.

Parameters:

- **`rcid`** — integer  
  Recentchanges ID to patrol.
- **`revid`** — integer  
  Revision ID to patrol.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the patrol log.
- **`token`** — string; **required**; token type `patrol`; sensitive  
  A "patrol" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=patrol&token=123ABC&rcid=230672766` — Patrol a recent change.
- `api.php?action=patrol&token=123ABC&revid=230672766` — Patrol a revision.

<a id="mod-protect"></a>
#### `action=protect`

*Core* · `MediaWiki\Api\ApiProtect` · POST only · write · token `csrf`

Change the protection level of a page.

Parameters:

- **`title`** — string  
  Title of the page to (un)protect. Cannot be used together with pageid.
- **`pageid`** — integer  
  ID of the page to (un)protect. Cannot be used together with title.
- **`protections`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  List of protection levels, formatted `action=level` (e.g. `edit=sysop`). A level of `all` means everyone is allowed to take the action, i.e. no restriction.
  
  Note: Any actions not listed will have restrictions removed.
- **`expiry`** — string; multi-value (max 50; 500 for high-limit users); duplicates allowed; default `infinite`  
  Expiry timestamps. If only one timestamp is set, it'll be used for all protections. Use `infinite`, `indefinite`, `infinity`, or `never`, for a never-expiring protection.
- **`reason`** — string  
  Reason for (un)protecting.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the protection log.
- **`cascade`** — boolean  
  Enable cascading protection (i.e. protect transcluded templates and images used in this page). Ignored if none of the given protection levels support cascading.
- **`watch`** — boolean; **deprecated**  
  If set, add the page being (un)protected to the current user's watchlist.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=protect&title=Main%20Page&token=123ABC&protections=edit=sysop|move=sysop&cascade=&expiry=20070901163000|never` — Protect a page.
- `api.php?action=protect&title=Main%20Page&token=123ABC&protections=edit=all|move=all&reason=Lifting%20restrictions` — Unprotect a page by setting restrictions to `all` (i.e. everyone is allowed to take the action).
- `api.php?action=protect&title=Main%20Page&token=123ABC&protections=&reason=Lifting%20restrictions` — Unprotect a page by setting no restrictions.

<a id="mod-purge"></a>
#### `action=purge`

*Core* · `MediaWiki\Api\ApiPurge` · POST only · write

Purge the cache for the given titles.

Parameters:

- **`forcelinkupdate`** — boolean  
  Update the links tables and do other secondary data updates.
- **`forcerecursivelinkupdate`** — boolean  
  Same as `forcelinkupdate`, and update the links tables for any page that uses this page as a template.
- **`continue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of titles to work on.
- **`pageids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of page IDs to work on.
- **`revids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of revision IDs to work on. Note that almost all query modules will convert revision IDs to the corresponding page ID and work on the latest revision instead. Only `prop=revisions` uses exact revisions for its response.
- **`generator`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `backlinks`, `categories`, `categorymembers`, `deletedrevisions`, `duplicatefiles`, `embeddedin`, `exturlusage`, `fileusage`, `images`, `imageusage`, `iwbacklinks`, `langbacklinks`, `links`, `linkshere`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `redirects`, `revisions`, `search`, `templates`, `transcludedin`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`; internal values: `wbsearch`
- **`redirects`** — boolean  
  Automatically resolve redirects in `titles`, `pageids`, and `revids`, and in pages returned by `generator`.
- **`converttitles`** — boolean  
  Convert titles to other variants if necessary. Only works if the wiki's content language supports variant conversion. Languages that support variant conversion include ban, en, crh, gan, iu, ku, mni, sh, shi, sr, tg, tly, uz, wuu, zgh and zh.

Examples:

- `api.php?action=purge&titles=Main%20Page|API` — Purge `Main Page` and the `API` page.
- `api.php?action=purge&generator=allpages&gapnamespace=0&gaplimit=10` — Purge the first 10 pages in the main namespace.

<a id="mod-query"></a>
#### `action=query`

*Core* · `MediaWiki\Api\ApiQuery` · GET or POST

Fetch data from and about MediaWiki.

All data modifications will first have to use query to acquire a token to prevent abuse from malicious sites.

Parameters:

- **`prop`** — submodule: `categories`, `categoryinfo`, `contributors`, `deletedrevisions`, `description`, `duplicatefiles`, `entityterms`, `extlinks`, `fileusage`, `imageinfo`, `images`, `info`, `iwlinks`, `langlinks`, `links`, `linkshere`, `pageprops`, `pageterms`, `redirects`, `revisions`, `stashimageinfo`, `templates`, `transcludedin`, `wbentityusage`; multi-value (max 50; 500 for high-limit users); internal values: `description`
- **`list`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `allusers`, `backlinks`, `blocks`, `categorymembers`, `deletedrevs`, `embeddedin`, `exturlusage`, `filearchive`, `imageusage`, `iwbacklinks`, `langbacklinks`, `logevents`, `mystashedfiles`, `pagepropnames`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `search`, `tags`, `usercontribs`, `users`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`, `wbsubscribers`; multi-value (max 50; 500 for high-limit users); deprecated values: `deletedrevs`; internal values: `wbsearch`
- **`meta`** — submodule: `allmessages`, `authmanagerinfo`, `filerepoinfo`, `languageinfo`, `siteinfo`, `tokens`, `userinfo`, `wbcontentlanguages`, `wikibase`; multi-value (max 50; 500 for high-limit users)
- **`indexpageids`** — boolean  
  Include an additional pageids section listing all returned page IDs.
- **`export`** — boolean  
  Export the current revisions of all given or generated pages.
- **`exportnowrap`** — boolean  
  Return the export XML without wrapping it in an XML result (same format as Special:Export). Can only be used with query+export.
- **`exportschema`** — enum: `0.10`, `0.11`; default `0.11`  
  Target the given version of the XML dump format when exporting. Can only be used with `query+export`.
- **`iwurl`** — boolean  
  Whether to get the full URL if the title is an interwiki link.
- **`continue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`rawcontinue`** — boolean  
  Return raw `query-continue` data for continuation.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of titles to work on.
- **`pageids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of page IDs to work on.
- **`revids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of revision IDs to work on. Note that almost all query modules will convert revision IDs to the corresponding page ID and work on the latest revision instead. Only `prop=revisions` uses exact revisions for its response.
- **`generator`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `backlinks`, `categories`, `categorymembers`, `deletedrevisions`, `duplicatefiles`, `embeddedin`, `exturlusage`, `fileusage`, `images`, `imageusage`, `iwbacklinks`, `langbacklinks`, `links`, `linkshere`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `redirects`, `revisions`, `search`, `templates`, `transcludedin`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`; internal values: `wbsearch`
- **`redirects`** — boolean  
  Automatically resolve redirects in `query+titles`, `query+pageids`, and `query+revids`, and in pages returned by `query+generator`.
- **`converttitles`** — boolean  
  Convert titles to other variants if necessary. Only works if the wiki's content language supports variant conversion. Languages that support variant conversion include ban, en, crh, gan, iu, ku, mni, sh, shi, sr, tg, tly, uz, wuu, zgh and zh.

Examples:

- `api.php?action=query&prop=revisions&meta=siteinfo&titles=Main%20Page&rvprop=user|comment&continue=` — Fetch site info and revisions of Main Page.
- `api.php?action=query&generator=allpages&gapprefix=API/&prop=revisions&continue=` — Fetch revisions of pages beginning with `API/`.

<a id="mod-removeauthenticationdata"></a>
#### `action=removeauthenticationdata`

*Core* · `MediaWiki\Api\ApiRemoveAuthenticationData` · POST only · write · token `csrf`

Remove authentication data for the current user.

Parameters:

- **`request`** — string; **required**  
  Use this authentication request, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=remove`.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=removeauthenticationdata&request=FooAuthenticationRequest&token=123ABC` — Attempt to remove the current user's data for `FooAuthenticationRequest`.

<a id="mod-resetpassword"></a>
#### `action=resetpassword`

*Core* · `MediaWiki\Api\ApiResetPassword` · POST only · write · token `csrf`

Send a password reset email to a user.

Parameters:

- **`user`** — user  
  User being reset.
- **`email`** — string  
  Email address of the user being reset.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=resetpassword&user=Example&token=123ABC` — Send a password reset email to user `Example`.
- `api.php?action=resetpassword&user=user@example.com&token=123ABC` — Send a password reset email for all users with email address `user@example.com`.

<a id="mod-revisiondelete"></a>
#### `action=revisiondelete`

*Core* · `MediaWiki\Api\ApiRevisionDelete` · POST only · write · token `csrf`

Delete and undelete revisions.

Parameters:

- **`type`** — enum: `archive`, `filearchive`, `logging`, `oldimage`, `revision`; **required**  
  Type of revision deletion being performed.
- **`target`** — string  
  Page title for the revision deletion, if required for the type.
- **`ids`** — string; **required**; multi-value (max 50; 500 for high-limit users)  
  Identifiers for the revisions to be deleted.
- **`hide`** — enum: `comment`, `content`, `user`; multi-value (max 50; 500 for high-limit users)  
  What to hide for each revision.
- **`show`** — enum: `comment`, `content`, `user`; multi-value (max 50; 500 for high-limit users)  
  What to unhide for each revision.
- **`suppress`** — enum: `no`, `nochange`, `yes`; default `nochange`  
  Whether to suppress data from administrators as well as others.
- **`reason`** — string  
  Reason for the deletion or undeletion.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Tags to apply to the entry in the deletion log.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=revisiondelete&target=Main%20Page&type=revision&ids=12345&hide=content&token=123ABC` — Hide content for revision `12345` on the page Main Page.
- `api.php?action=revisiondelete&type=logging&ids=67890&hide=content|comment|user&reason=BLP%20violation&token=123ABC` — Hide all data on log entry `67890` with the reason `BLP violation`.

<a id="mod-rollback"></a>
#### `action=rollback`

*Core* · `MediaWiki\Api\ApiRollback` · POST only · write · token `rollback`

Undo the last edit to the page.

If the last user who edited the page made multiple edits in a row, they will all be rolled back.

Parameters:

- **`title`** — string  
  Title of the page to roll back. Cannot be used together with `pageid`.
- **`pageid`** — integer  
  Page ID of the page to roll back. Cannot be used together with `title`.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Tags to apply to the rollback.
- **`user`** — user; **required**  
  Name of the user whose edits are to be rolled back.
- **`summary`** — string  
  Custom edit summary. If empty, default summary will be used.
- **`markbot`** — boolean  
  Mark the reverted edits and the revert as bot edits.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`token`** — string; **required**; token type `rollback`; sensitive  
  A "rollback" token retrieved from action=query&meta=tokens
  
  For compatibility, the token used in the web UI is also accepted.

Examples:

- `api.php?action=rollback&title=Main%20Page&user=Example&token=123ABC` — Roll back the last edits to page Main Page by user `Example`.
- `api.php?action=rollback&title=Main%20Page&user=192.0.2.5&token=123ABC&summary=Reverting%20vandalism&markbot=1` — Roll back the last edits to page Main Page by IP user `192.0.2.5` with summary `Reverting vandalism`, and mark those edits and the revert as bot edits.

<a id="mod-rsd"></a>
#### `action=rsd`

*Core* · `MediaWiki\Api\ApiRsd` · GET or POST

Export an RSD (Really Simple Discovery) schema.

Examples:

- `api.php?action=rsd` — Export the RSD schema.

<a id="mod-setnotificationtimestamp"></a>
#### `action=setnotificationtimestamp`

*Core* · `MediaWiki\Api\ApiSetNotificationTimestamp` · POST only · write · token `csrf`

Update the notification timestamp for watched pages.

This affects the highlighting of changed pages in the watchlist and history, and the sending of email when the "Email me when a page or a file on my watchlist is changed" preference is enabled.

Parameters:

- **`entirewatchlist`** — boolean  
  Work on all watched pages.
- **`timestamp`** — timestamp  
  Timestamp to which to set the notification timestamp.
- **`torevid`** — integer  
  Revision to set the notification timestamp to (one page only).
- **`newerthanrevid`** — integer  
  Revision to set the notification timestamp newer than (one page only).
- **`continue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of titles to work on.
- **`pageids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of page IDs to work on.
- **`revids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of revision IDs to work on. Note that almost all query modules will convert revision IDs to the corresponding page ID and work on the latest revision instead. Only `prop=revisions` uses exact revisions for its response.
- **`generator`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `backlinks`, `categories`, `categorymembers`, `deletedrevisions`, `duplicatefiles`, `embeddedin`, `exturlusage`, `fileusage`, `images`, `imageusage`, `iwbacklinks`, `langbacklinks`, `links`, `linkshere`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `redirects`, `revisions`, `search`, `templates`, `transcludedin`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`; internal values: `wbsearch`
- **`redirects`** — boolean  
  Automatically resolve redirects in `titles`, `pageids`, and `revids`, and in pages returned by `generator`.
- **`converttitles`** — boolean  
  Convert titles to other variants if necessary. Only works if the wiki's content language supports variant conversion. Languages that support variant conversion include ban, en, crh, gan, iu, ku, mni, sh, shi, sr, tg, tly, uz, wuu, zgh and zh.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=setnotificationtimestamp&entirewatchlist=&token=123ABC` — Reset the notification status for the entire watchlist.
- `api.php?action=setnotificationtimestamp&titles=Main%20Page&token=123ABC` — Reset the notification status for Main Page.
- `api.php?action=setnotificationtimestamp&titles=Main%20Page&timestamp=2012-01-01T00:00:00Z&token=123ABC` — Set the notification timestamp for Main Page so all edits since 1 January 2012 are unviewed.
- `api.php?action=setnotificationtimestamp&generator=allpages&gapnamespace=2&token=123ABC` — Reset the notification status for pages in the `User` namespace.

<a id="mod-setpagelanguage"></a>
#### `action=setpagelanguage`

*Core* · `MediaWiki\Api\ApiSetPageLanguage` · POST only · write · token `csrf`

Change the language of a page.

Changing the language of a page is not allowed on this wiki.

Enable `$wgPageLanguageUseDB` to use this action.

Parameters:

- **`title`** — string  
  Title of the page whose language you wish to change. Cannot be used together with `pageid`.
- **`pageid`** — integer  
  Page ID of the page whose language you wish to change. Cannot be used together with `title`.
- **`lang`** — enum: `aae`, `ab`, `abs`, `ace`, `acf`, `acm`, `ady`, `ady-cyrl`, `aeb`, `aeb-arab`, `aeb-latn`, `af`, `aln`, `alt`, `am`, `ami`, `an`, `ang`, `ann`, `anp`, `apc`, `ar`, `arc`, `arn`, `arq`, `ary`, `arz`, `as`, `ase`, `ast`, `atj`, `av`, `avk`, `awa`, `ay`, `az`, `azb`, `ba`, `ban`, `ban-bali`, … (527 values total; config-dependent); **required**  
  Language code of the language to change the page to. Use `default` to reset the page to the wiki's default content language.
- **`reason`** — string  
  Reason for the change.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the log entry resulting from this action.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=setpagelanguage&title=Main%20Page&lang=eu&token=123ABC` — Change the language of the page Main Page to Basque.
- `api.php?action=setpagelanguage&pageid=123&lang=default&token=123ABC` — Change the language of the page with ID 123 to the wiki's default content language.

<a id="mod-stashedit"></a>
#### `action=stashedit`

*Core* · `MediaWiki\Api\ApiStashEdit` · POST only · write · token `csrf` · **internal**

Prepare an edit in shared cache.

This is intended to be used via AJAX from the edit form to improve the performance of the page save.

Parameters:

- **`title`** — string; **required**  
  Title of the page being edited.
- **`section`** — string  
  Section identifier. `0` for the top section, `new` for a new section.
- **`sectiontitle`** — string  
  The title for a new section.
- **`text`** — text  
  Page content.
- **`stashedtexthash`** — string  
  Page content hash from a prior stash to use instead.
- **`summary`** — string  
  Change summary.
- **`contentmodel`** — enum: `css`, `javascript`, `json`, `text`, `unknown`, `wikibase-item`, `wikibase-property`, `wikitext`; **required**  
  Content model of the new content.
- **`contentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **required**  
  Content serialization format used for the input text.
- **`baserevid`** — integer; **required**  
  Revision ID of the base revision.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

<a id="mod-tag"></a>
#### `action=tag`

*Core* · `MediaWiki\Api\ApiTag` · POST only · write · token `csrf`

Add or remove change tags from individual revisions or log entries.

Parameters:

- **`rcid`** — integer; multi-value (max 50; 500 for high-limit users)  
  One or more recent changes IDs from which to add or remove the tag.
- **`revid`** — integer; multi-value (max 50; 500 for high-limit users)  
  One or more revision IDs from which to add or remove the tag.
- **`logid`** — integer; multi-value (max 50; 500 for high-limit users)  
  One or more log entry IDs from which to add or remove the tag.
- **`add`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Tags to add. Only manually defined tags can be added.
- **`remove`** — string; multi-value (max 50; 500 for high-limit users)  
  Tags to remove. Only tags that are either manually defined or completely undefined can be removed.
- **`reason`** — string  
  Reason for the change.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Tags to apply to the log entry that will be created as a result of this action.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=tag&revid=123&add=vandalism&token=123ABC` — Add the `vandalism` tag to revision ID 123 without specifying a reason
- `api.php?action=tag&logid=123&remove=spam&reason=Wrongly+applied&token=123ABC` — Remove the `spam` tag from log entry ID 123 with the reason `Wrongly applied`

<a id="mod-unblock"></a>
#### `action=unblock`

*Core* · `MediaWiki\Api\ApiUnblock` · POST only · write · token `csrf`

Unblock a user.

Parameters:

- **`id`** — integer  
  ID of the block to unblock (obtained through `list=blocks`). Cannot be used together with `user`.
- **`user`** — user  
  User to unblock. Cannot be used together with `id`.
- **`userid`** — integer; **deprecated**  
  Specify `user=#ID` instead.
- **`reason`** — string  
  Reason for unblock.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the block log.
- **`watchuser`** — boolean  
  Watch the user's or IP address's user and talk pages.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=unblock&id=105` — Unblock block ID #`105`.
- `api.php?action=unblock&user=Bob&reason=Sorry%20Bob` — Unblock user `Bob` with reason `Sorry Bob`.

<a id="mod-undelete"></a>
#### `action=undelete`

*Core* · `MediaWiki\Api\ApiUndelete` · POST only · write · token `csrf`

Undelete revisions of a deleted page.

A list of deleted revisions (including timestamps) can be retrieved through prop=deletedrevisions, and a list of deleted file IDs can be retrieved through list=filearchive.

Parameters:

- **`title`** — string; **required**  
  Title of the page to undelete.
- **`reason`** — string  
  Reason for restoring.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the deletion log.
- **`timestamps`** — timestamp; multi-value (max 50; 500 for high-limit users)  
  Timestamps of the revisions to undelete. If both `timestamps` and `fileids` are empty, all will be undeleted.
- **`fileids`** — integer; multi-value (max 50; 500 for high-limit users)  
  IDs of the file revisions to restore. If both `timestamps` and `fileids` are empty, all will be restored.
- **`undeletetalk`** — boolean  
  Undelete all revisions of the associated talk page, if any.
- **`watchlist`** — enum: `nochange`, `preferences`, `unwatch`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=undelete&title=Main%20Page&token=123ABC&reason=Restoring%20Main%20Page` — Undelete page Main Page.
- `api.php?action=undelete&title=Main%20Page&token=123ABC&timestamps=2007-07-03T22:00:45Z|2007-07-02T19:48:56Z` — Undelete two revisions of page Main Page.

<a id="mod-unlinkaccount"></a>
#### `action=unlinkaccount`

*Core* · `MediaWiki\Api\ApiRemoveAuthenticationData` · POST only · write · token `csrf`

Remove a linked third-party account from the current user.

Parameters:

- **`request`** — string; **required**  
  Use this authentication request, by the `id` returned from `action=query&meta=authmanagerinfo` with `amirequestsfor=unlink`.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=unlinkaccount&request=FooAuthenticationRequest&token=123ABC` — Attempt to remove the current user's link for the provider associated with `FooAuthenticationRequest`.

<a id="mod-upload"></a>
#### `action=upload`

*Core* · `MediaWiki\Api\ApiUpload` · POST only · write · token `csrf`

Upload a file, or get the status of pending uploads.

Several methods are available:
- Upload file contents directly, using the `file` parameter.
- Upload the file in pieces, using the `filesize`, `chunk`, and `offset` parameters.
- Have the MediaWiki server fetch a file from a URL, using the `url` parameter.
- Complete an earlier upload that failed due to warnings, using the `filekey` parameter.
Note that the HTTP POST must be done as a file upload (i.e. using `multipart/form-data`) when sending the `file`.

Parameters:

- **`filename`** — string  
  Target filename.
- **`comment`** — string  
  Upload comment. Also used as the initial page text for new files if `text` is not specified.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the upload log entry and file page revision.
- **`text`** — text  
  Initial page text for new files.
- **`watch`** — boolean; **deprecated**  
  Watch the page.
- **`watchlist`** — enum: `nochange`, `preferences`, `watch`; default `preferences`  
  Unconditionally add or remove the page from the current user's watchlist, use preferences (ignored for bot users) or do not change watch.
- **`ignorewarnings`** — boolean  
  Ignore any warnings.
- **`file`** — upload  
  File contents.
- **`url`** — string  
  URL to fetch the file from.
- **`filekey`** — string  
  Key that identifies a previous upload that was stashed temporarily.
- **`sessionkey`** — string; **deprecated**  
  Same as filekey, maintained for backward compatibility.
- **`stash`** — boolean  
  If set, the server will stash the file temporarily instead of adding it to the repository.
- **`filesize`** — integer (min 0, max 104857600)  
  Filesize of entire upload.
- **`offset`** — integer (min 0)  
  Offset of chunk in bytes.
- **`chunk`** — upload  
  Chunk contents.
- **`async`** — boolean  
  Make potentially large file operations asynchronous when possible.
- **`checkstatus`** — boolean  
  Only fetch the upload status for the given file key.
- **`token`** — string; **required**; token type `csrf`; sensitive  
  A "csrf" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=upload&filename=Wiki.png&url=http%3A//upload.wikimedia.org/wikipedia/en/b/bc/Wiki.png&token=123ABC` — Upload from a URL.
- `api.php?action=upload&filename=Wiki.png&filekey=filekey&ignorewarnings=1&token=123ABC` — Complete an upload that failed due to warnings.

<a id="mod-userrights"></a>
#### `action=userrights`

*Core* · `MediaWiki\Api\ApiUserrights` · POST only · write · token `userrights`

Change a user's group membership.

Parameters:

- **`user`** — user  
  User.
- **`userid`** — integer; **deprecated**  
  Specify `user=#ID` instead.
- **`add`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Add the user to these groups, or if they are already a member, update the expiry of their membership in that group.
- **`expiry`** — string; multi-value (max 50; 500 for high-limit users); duplicates allowed; default `infinite`  
  Expiry timestamps. May be relative (e.g. `5 months` or `2 weeks`) or absolute (e.g. `2014-09-18T12:34:56Z`). If only one timestamp is set, it will be used for all groups passed to the `add` parameter. Use `infinite`, `indefinite`, `infinity`, or `never` for a never-expiring user group.
- **`remove`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Remove the user from these groups.
- **`reason`** — string  
  Reason for the change.
- **`token`** — string; **required**; token type `userrights`; sensitive  
  A "userrights" token retrieved from action=query&meta=tokens
  
  For compatibility, the token used in the web UI is also accepted.
- **`tags`** — enum (no values in reference config — site-/config-dependent); multi-value (max 50; 500 for high-limit users)  
  Change tags to apply to the entry in the user rights log.
- **`watchuser`** — boolean  
  Watch the user's user and talk pages.

Examples:

- `api.php?action=userrights&user=FooBot&add=bot&remove=sysop|bureaucrat&token=123ABC` — Add user `FooBot` to group `bot`, and remove from groups `sysop` and `bureaucrat`.
- `api.php?action=userrights&userid=123&add=bot&remove=sysop|bureaucrat&token=123ABC` — Add the user with ID `123` to group `bot`, and remove from groups `sysop` and `bureaucrat`.
- `api.php?action=userrights&user=SometimeSysop&add=sysop&expiry=1%20month&token=123ABC` — Add user `SometimeSysop` to group `sysop` for 1 month.

<a id="mod-validatepassword"></a>
#### `action=validatepassword`

*Core* · `MediaWiki\Api\ApiValidatePassword` · POST only

Validate a password against the wiki's password policies.

Validity is reported as `Good` if the password is acceptable, `Change` if the password may be used for login but must be changed, or `Invalid` if the password is not usable.

Parameters:

- **`password`** — password; **required**; sensitive  
  Password to validate.
- **`user`** — user  
  Username, for use when testing account creation. The named user must not exist.
- **`email`** — string  
  Email address, for use when testing account creation.
- **`realname`** — string  
  Real name, for use when testing account creation.

Examples:

- `api.php?action=validatepassword&password=foobar` — Validate the password `foobar` for the current user.
- `api.php?action=validatepassword&password=querty&user=Example` — Validate the password `qwerty` for creating user `Example`.

<a id="mod-watch"></a>
#### `action=watch`

*Core* · `MediaWiki\Api\ApiWatch` · POST only · write · token `watch`

Add or remove pages from the current user's watchlist.

Parameters:

- **`title`** — string; **deprecated**  
  The page to (un)watch. Use `titles` instead.
- **`unwatch`** — boolean  
  If set the page will be unwatched rather than watched.
- **`continue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`titles`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of titles to work on.
- **`pageids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of page IDs to work on.
- **`revids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of revision IDs to work on. Note that almost all query modules will convert revision IDs to the corresponding page ID and work on the latest revision instead. Only `prop=revisions` uses exact revisions for its response.
- **`generator`** — submodule: `allcategories`, `alldeletedrevisions`, `allfileusages`, `allimages`, `alllinks`, `allpages`, `allredirects`, `allrevisions`, `alltransclusions`, `backlinks`, `categories`, `categorymembers`, `deletedrevisions`, `duplicatefiles`, `embeddedin`, `exturlusage`, `fileusage`, `images`, `imageusage`, `iwbacklinks`, `langbacklinks`, `links`, `linkshere`, `pageswithprop`, `prefixsearch`, `protectedtitles`, `querypage`, `random`, `recentchanges`, `redirects`, `revisions`, `search`, `templates`, `transcludedin`, `watchlist`, `watchlistraw`, `wblistentityusage`, `wbsearch`; internal values: `wbsearch`
- **`redirects`** — boolean  
  Automatically resolve redirects in `titles`, `pageids`, and `revids`, and in pages returned by `generator`.
- **`converttitles`** — boolean  
  Convert titles to other variants if necessary. Only works if the wiki's content language supports variant conversion. Languages that support variant conversion include ban, en, crh, gan, iu, ku, mni, sh, shi, sr, tg, tly, uz, wuu, zgh and zh.
- **`token`** — string; **required**; token type `watch`; sensitive  
  A "watch" token retrieved from action=query&meta=tokens

Examples:

- `api.php?action=watch&titles=Main%20Page&token=123ABC` — Watch the page Main Page.
- `api.php?action=watch&titles=Main%20Page&unwatch=&token=123ABC` — Unwatch the page Main Page.
- `api.php?action=watch&generator=allpages&gapnamespace=0&token=123ABC` — Watch the first few pages in the main namespace.

### 5.2 Core `prop` modules

<a id="mod-query-categories"></a>
#### `action=query&prop=categories`

*Core* · `MediaWiki\Api\ApiQueryCategories` · prefix `cl` · GET or POST · usable as generator (`generator=categories`)

List all categories the pages belong to.

Parameters:

- **`clprop`** — enum: `hidden`, `sortkey`, `timestamp`; multi-value (max 50; 500 for high-limit users)  
  Which additional properties to get for each category:
  
  - **sortkey**: Adds the sortkey (hexadecimal string) and sortkey prefix (human-readable part) for the category.
  - **timestamp**: Adds timestamp of when the category was added.
  - **hidden**: Tags categories that are hidden with `__HIDDENCAT__`.
- **`clshow`** — enum: `!hidden`, `hidden`; multi-value (max 50; 500 for high-limit users)  
  Which kind of categories to show.
- **`cllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many categories to return.
- **`clcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`clcategories`** — string; multi-value (max 50; 500 for high-limit users)  
  Only list these categories. Useful for checking whether a certain page is in a certain category.
- **`cldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&prop=categories&titles=Albert%20Einstein` — Get a list of categories the page `Albert Einstein` belongs to.
- `api.php?action=query&generator=categories&titles=Albert%20Einstein&prop=info` — Get information about all categories used in the page `Albert Einstein`.

<a id="mod-query-categoryinfo"></a>
#### `action=query&prop=categoryinfo`

*Core* · `MediaWiki\Api\ApiQueryCategoryInfo` · prefix `ci` · GET or POST

Returns information about the given categories.

Parameters:

- **`cicontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=categoryinfo&titles=Category:Foo|Category:Bar` — Get information about `Category:Foo` and `Category:Bar`.

<a id="mod-query-contributors"></a>
#### `action=query&prop=contributors`

*Core* · `MediaWiki\Api\ApiQueryContributors` · prefix `pc` · GET or POST

Get the list of logged-in contributors and the count of logged-out contributors to a page.

Parameters:

- **`pcgroup`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Only include users in the given groups. Does not include implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`pcexcludegroup`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Exclude users in the given groups. Does not include implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`pcrights`** — enum: `apihighlimits`, `applychangetags`, `autoconfirmed`, `autocreateaccount`, `autopatrol`, `bigdelete`, `block`, `blockemail`, `bot`, `browsearchive`, `changetags`, `createaccount`, `createpage`, `createtalk`, `delete`, `delete-redirect`, `deletechangetags`, `deletedhistory`, `deletedtext`, `deletelogentry`, `deleterevision`, `edit`, `editcontentmodel`, `editinterface`, `editmyoptions`, `editmyprivateinfo`, `editmyusercss`, `editmyuserjs`, `editmyuserjson`, `editmyuserjsredirect`, `editmywatchlist`, `editprotected`, `editsemiprotected`, `editsitecss`, `editsitejs`, `editsitejson`, `editusercss`, `edituserjs`, `edituserjson`, `hideuser`, … (86 values total; config-dependent); multi-value (max 50; 500 for high-limit users)  
  Only include users having the given rights. Does not include rights granted by implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`pcexcluderights`** — enum: `apihighlimits`, `applychangetags`, `autoconfirmed`, `autocreateaccount`, `autopatrol`, `bigdelete`, `block`, `blockemail`, `bot`, `browsearchive`, `changetags`, `createaccount`, `createpage`, `createtalk`, `delete`, `delete-redirect`, `deletechangetags`, `deletedhistory`, `deletedtext`, `deletelogentry`, `deleterevision`, `edit`, `editcontentmodel`, `editinterface`, `editmyoptions`, `editmyprivateinfo`, `editmyusercss`, `editmyuserjs`, `editmyuserjson`, `editmyuserjsredirect`, `editmywatchlist`, `editprotected`, `editsemiprotected`, `editsitecss`, `editsitejs`, `editsitejson`, `editusercss`, `edituserjs`, `edituserjson`, `hideuser`, … (86 values total; config-dependent); multi-value (max 50; 500 for high-limit users)  
  Exclude users having the given rights. Does not include rights granted by implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`pclimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many contributors to return.
- **`pccontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=contributors&titles=Main%20Page` — Show contributors to the page Main Page.

<a id="mod-query-deletedrevisions"></a>
#### `action=query&prop=deletedrevisions`

*Core* · `MediaWiki\Api\ApiQueryDeletedRevisions` · prefix `drv` · GET or POST · usable as generator (`generator=deletedrevisions`)

Get deleted revision information.

May be used in several ways:
# Get deleted revisions for a set of pages, by setting titles or pageids. Ordered by title and timestamp.
# Get data about a set of deleted revisions by setting their IDs with revids. Ordered by revision ID.

Parameters:

- **`drvprop`** — enum: `comment`, `content`, `contentmodel`, `flags`, `ids`, `parsedcomment`, `roles`, `sha1`, `size`, `slotsha1`, `slotsize`, `tags`, `timestamp`, `user`, `userid`, `parsetree`; multi-value (max 50; 500 for high-limit users); default `ids|timestamp|flags|comment|user`; deprecated values: `parsetree`  
  Which properties to get for each revision:
  
  - **ids**: The ID of the revision.
  - **flags**: Revision flags (minor).
  - **timestamp**: The timestamp of the revision.
  - **user**: User that made the revision. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: User ID of the revision creator. If the user has been revision deleted, a `userhidden` property will be returned.
  - **size**: Length (bytes) of the revision.
  - **slotsize**: Length (bytes) of each revision slot.
  - **sha1**: SHA-1 (base 16) of the revision. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **slotsha1**: SHA-1 (base 16) of each revision slot. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **contentmodel**: Content model ID of each revision slot.
  - **comment**: Comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parsed comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **content**: Content of each revision slot. If the content has been revision deleted, a `texthidden` property will be returned. For performance reasons, if this option is used, `drvlimit` is enforced to 50.
  - **tags**: Tags for the revision.
  - **roles**: List content slot roles that exist in the revision.
  - **parsetree**: Deprecated. Use `action=expandtemplates` or `action=parse` instead. The XML parse tree of revision content (requires content model `wikitext`). For performance reasons, if this option is used, `drvlimit` is enforced to 50.
- **`drvslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Which revision slots to return data for, when slot-related properties are included in `drvprops`. If omitted, data from the `main` slot will be returned in a backwards-compatible format.
- **`drvlimit`** — limit (1–500; 5000 for high-limit users; or `max`)  
  Limit how many revisions will be returned. If `drvprop=content`, `drvprop=parsetree`, `drvdiffto` or `drvdifftotext` is used, the limit is 50. If `drvparse` is used, the limit is 1.
- **`drvexpandtemplates`** — boolean; **deprecated**  
  Use `action=expandtemplates` instead. Expand templates in revision content (requires drvprop=content).
- **`drvgeneratexml`** — boolean; **deprecated**  
  Use `action=expandtemplates` or `action=parse` instead. Generate XML parse tree for revision content (requires drvprop=content).
- **`drvparse`** — boolean; **deprecated**  
  Use `action=parse` instead. Parse revision content (requires `drvprop=content`). For performance reasons, if this option is used, `drvlimit` is enforced to 1.
- **`drvsection`** — string  
  Only retrieve the content of the section with this identifier.
- **`drvdiffto`** — string; **deprecated**  
  Use `action=compare` instead. Revision ID to diff each revision to. Use `prev`, `next` and `cur` for the previous, next and current revision respectively. For performance reasons, if this option is used, `drvlimit` is enforced to 50.
- **`drvdifftotext`** — string; **deprecated**  
  Use `action=compare` instead. Text to diff each revision to. Only diffs a limited number of revisions. Overrides `drvdiffto`. If `drvsection` is set, only that section will be diffed against this text. For performance reasons, if this option is used, `drvlimit` is enforced to 50.
- **`drvdifftotextpst`** — boolean; **deprecated**  
  Use `action=compare` instead. Perform a pre-save transform on the text before diffing it. Only valid when used with `drvdifftotext`.
- **`drvcontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Serialization format used for `drvdifftotext` and expected for output of content.
- **`drvstart`** — timestamp  
  The timestamp to start enumerating from. Ignored when processing a list of revision IDs.
- **`drvend`** — timestamp  
  The timestamp to stop enumerating at. Ignored when processing a list of revision IDs.
- **`drvdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: drvstart has to be before drvend.
  - **older**: List newest first (default). Note: drvstart has to be later than drvend.
- **`drvtag`** — string  
  Only list revisions tagged with this tag.
- **`drvuser`** — user  
  Only list revisions by this user.
- **`drvexcludeuser`** — user  
  Don't list revisions by this user.
- **`drvcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`drvcontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `drvslots`  
  Content serialization format used for output of content.

Examples:

- `api.php?action=query&prop=deletedrevisions&revids=123456` — List the information for deleted revision `123456`.
- `api.php?action=query&prop=deletedrevisions&titles=Main%20Page|Talk%3AMain%20Page&drvslots=*&drvprop=user|comment|content` — List the deleted revisions of the pages Main Page and its talk page with content.

<a id="mod-query-duplicatefiles"></a>
#### `action=query&prop=duplicatefiles`

*Core* · `MediaWiki\Api\ApiQueryDuplicateFiles` · prefix `df` · GET or POST · usable as generator (`generator=duplicatefiles`)

List all files that are duplicates of the given files based on hash values.

Parameters:

- **`dflimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many duplicate files to return.
- **`dfcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`dfdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`dflocalonly`** — boolean  
  Look only for files in the local repository.

Examples:

- `api.php?action=query&titles=File:Albert_Einstein_Head.jpg&prop=duplicatefiles` — Look for duplicates of :File:Albert Einstein Head.jpg.
- `api.php?action=query&generator=allimages&prop=duplicatefiles` — Look for duplicates of all files.

<a id="mod-query-extlinks"></a>
#### `action=query&prop=extlinks`

*Core* · `MediaWiki\Api\ApiQueryExternalLinks` · prefix `el` · GET or POST

Returns all external URLs (not interwikis) from the given pages.

Parameters:

- **`ellimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many links to return.
- **`elcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`elprotocol`** — enum: ``, `bitcoin`, `ftp`, `ftps`, `geo`, `git`, `gopher`, `http`, `https`, `irc`, `ircs`, `magnet`, `mailto`, `matrix`, `mms`, `news`, `nntp`, `redis`, `sftp`, `sip`, `sips`, `sms`, `ssh`, `svn`, `tel`, `telnet`, `urn`, `worldwind`, `xmpp`  
  Protocol of the URL. If empty and `elquery` is set, the protocol is `http` and `https`. Leave both this and `elquery` empty to list all external links.
- **`elquery`** — string  
  Search string without protocol. Useful for checking whether a certain page contains a certain external url.
- **`elexpandurl`** — boolean; **deprecated**  
  Expand protocol-relative URLs with the canonical protocol.

Examples:

- `api.php?action=query&prop=extlinks&titles=Main%20Page` — Get a list of external links on the page Main Page.

<a id="mod-query-fileusage"></a>
#### `action=query&prop=fileusage`

*Core* · `MediaWiki\Api\ApiQueryBacklinksprop` · prefix `fu` · GET or POST · usable as generator (`generator=fileusage`)

Find all pages that use the given files.

Parameters:

- **`fuprop`** — enum: `pageid`, `redirect`, `title`; multi-value (max 50; 500 for high-limit users); default `pageid|title|redirect`  
  Which properties to get:
  
  - **pageid**: Page ID of each page.
  - **title**: Title of each page.
  - **redirect**: Flag if the page is a redirect.
- **`funamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only include pages in these namespaces.
- **`fushow`** — enum: `!redirect`, `redirect`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria:
  
  - **redirect**: Only show redirects.
  - **!redirect**: Only show non-redirects.
- **`fulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many to return.
- **`fucontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=fileusage&titles=File%3AExample.jpg` — Get a list of pages using :File:Example.jpg.
- `api.php?action=query&generator=fileusage&titles=File%3AExample.jpg&prop=info` — Get information about pages using :File:Example.jpg.

<a id="mod-query-imageinfo"></a>
#### `action=query&prop=imageinfo`

*Core* · `MediaWiki\Api\ApiQueryImageInfo` · prefix `ii` · GET or POST

Returns file information and upload history.

Parameters:

- **`iiprop`** — enum: `archivename`, `badfile`, `bitdepth`, `canonicaltitle`, `comment`, `commonmetadata`, `dimensions`, `extmetadata`, `mediatype`, `metadata`, `mime`, `parsedcomment`, `sha1`, `size`, `thumbmime`, `timestamp`, `uploadwarning`, `url`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `timestamp|user`  
  Which file information to get:
  
  - **timestamp**: Adds timestamp for the uploaded version.
  - **user**: Adds the user who uploaded each file version. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: Add the ID of the user that uploaded each file version. If the user has been revision deleted, a `userhidden` property will be returned.
  - **comment**: Comment on the version. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parse the comment on the version. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **canonicaltitle**: Adds the canonical title of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **url**: Gives URL to the file and the description page. If the file has been revision deleted, a `filehidden` property will be returned.
  - **size**: Adds the size of the file in bytes and the height, width and page count (if applicable).
  - **dimensions**: Alias for size.
  - **sha1**: Adds SHA-1 hash for the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **mime**: Adds MIME type of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **thumbmime**: Adds MIME type of the image thumbnail (requires url and param iiurlwidth). If the file has been revision deleted, a `filehidden` property will be returned.
  - **mediatype**: Adds the media type of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **metadata**: Lists Exif metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **commonmetadata**: Lists file format generic metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **extmetadata**: Lists formatted metadata combined from multiple sources. Results are HTML formatted. If the file has been revision deleted, a `filehidden` property will be returned.
  - **archivename**: Adds the filename of the archive version for non-latest versions. If the file has been revision deleted, a `filehidden` property will be returned.
  - **bitdepth**: Adds the bit depth of the version. If the file has been revision deleted, a `filehidden` property will be returned.
  - **uploadwarning**: Used by the Special:Upload page to get information about an existing file. Not intended for use outside MediaWiki core.
  - **badfile**: Adds whether the file is on the MediaWiki:Bad image list
- **`iilimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `1`  
  How many file revisions to return per file.
- **`iistart`** — timestamp  
  Timestamp to start listing from.
- **`iiend`** — timestamp  
  Timestamp to stop listing at.
- **`iiurlwidth`** — integer; default `-1`  
  If iiprop=url is set, a URL to an image scaled to this width will be returned.
  For performance reasons if this option is used, no more than 50 scaled images will be returned.
- **`iiurlheight`** — integer; default `-1`  
  Similar to iiurlwidth.
- **`iimetadataversion`** — string; default `1`  
  Version of metadata to use. If `latest` is specified, use latest version. Defaults to `1` for backwards compatibility.
- **`iiextmetadatalanguage`** — string; default `en`  
  What language to fetch extmetadata in. This affects both which translation to fetch, if multiple are available, as well as how things like numbers and various values are formatted.
- **`iiextmetadatamultilang`** — boolean  
  If translations for extmetadata property are available, fetch all of them.
- **`iiextmetadatafilter`** — string; multi-value (max 50; 500 for high-limit users)  
  If specified and non-empty, only these keys will be returned for iiprop=extmetadata.
- **`iiurlparam`** — string  
  A handler specific parameter string. For example, PDFs might use `page15-100px`. `iiurlwidth` must be used and be consistent with `iiurlparam`.
- **`iibadfilecontexttitle`** — string  
  If `badfilecontexttitleprop=badfile` is set, this is the page title used when evaluating the MediaWiki:Bad image list
- **`iicontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`iilocalonly`** — boolean  
  Look only for files in the local repository.

Examples:

- `api.php?action=query&titles=File:Albert%20Einstein%20Head.jpg&prop=imageinfo` — Fetch information about the current version of :File:Albert Einstein Head.jpg.
- `api.php?action=query&titles=File:Test.jpg&prop=imageinfo&iilimit=50&iiend=2007-12-31T23:59:59Z&iiprop=timestamp|user|url` — Fetch information about versions of :File:Test.jpg from 2008 and later.

<a id="mod-query-images"></a>
#### `action=query&prop=images`

*Core* · `MediaWiki\Api\ApiQueryImages` · prefix `im` · GET or POST · usable as generator (`generator=images`)

Returns all files contained on the given pages.

Parameters:

- **`imlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many files to return.
- **`imcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`imimages`** — string; multi-value (max 50; 500 for high-limit users)  
  Only list these files. Useful for checking whether a certain page has a certain file.
- **`imdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&prop=images&titles=Main%20Page` — Get a list of files used on the page Main Page.
- `api.php?action=query&generator=images&titles=Main%20Page&prop=info` — Get information about all files used on the page Main Page.

<a id="mod-query-info"></a>
#### `action=query&prop=info`

*Core* · `MediaWiki\Api\ApiQueryInfo` · prefix `in` · GET or POST

Get basic page information.

Parameters:

- **`inprop`** — enum: `associatedpage`, `displaytitle`, `editintro`, `linkclasses`, `notificationtimestamp`, `preloadcontent`, `protection`, `subjectid`, `talkid`, `url`, `varianttitles`, `visitingwatchers`, `watched`, `watchers`, `preload`, `readable`; multi-value (max 50; 500 for high-limit users); deprecated values: `preload`, `readable`  
  Which additional properties to get:
  
  - **protection**: List the protection level of each page.
  - **talkid**: The page ID of the talk page for each non-talk page.
  - **watched**: List the watched status of each page.
  - **watchers**: The number of watchers, if allowed.
  - **visitingwatchers**: The number of watchers of each page who have visited recent edits to that page, if allowed.
  - **notificationtimestamp**: The watchlist notification timestamp of each page.
  - **subjectid**: The page ID of the parent page for each talk page.
  - **associatedpage**: The prefixed title of the associated subject or talk page.
  - **url**: Gives a full URL, an edit URL, and the canonical URL for each page.
  - **readable**: Deprecated. Whether the user can read this page. Use `intestactions=read` instead.
  - **preload**: Deprecated. Gives the text returned by EditFormPreloadText. Use `preloadcontent` instead, which supports other kinds of preloaded text too.
  - **preloadcontent**: Gives the content to be shown in the editor when the page does not exist or while adding a new section.
  - **editintro**: Gives the intro messages that should be shown to the user while editing this page or revision, as HTML.
  - **displaytitle**: Gives the manner in which the page title is actually displayed.
  - **varianttitles**: Gives the display title in all variants of the site content language.
  - **linkclasses**: Gives the additional CSS classes (e.g. link colors) used for links to this page if they were to appear on the page named by `inlinkcontext`.
- **`inlinkcontext`** — title; default `Main Page`  
  The context title to use when determining extra CSS classes (e.g. link colors) when `inprop` contains `linkclasses`.
- **`intestactions`** — string; multi-value (max 50; 500 for high-limit users)  
  Test whether the current user can perform certain actions on the page.
- **`intestactionsdetail`** — enum: `boolean`, `full`, `quick`; default `boolean`  
  Detail level for `intestactions`. Use the main module's `errorformat` and `errorlang` parameters to control the format of the messages returned.
  
  - **boolean**: Return a boolean value for each action.
  - **full**: Return messages describing why the action is disallowed, or an empty array if it is allowed.
  - **quick**: Like `full` but skipping expensive checks.
- **`intestactionsautocreate`** — boolean  
  Test whether performing `intestactions` would automatically create a temporary account.
- **`inpreloadcustom`** — string  
  Title of a custom page to use as preloaded content.
  Only used when `inprop` contains `preloadcontent`.
- **`inpreloadparams`** — string; multi-value (max 50; 500 for high-limit users)  
  Parameters for the custom page being used as preloaded content.
  Only used when `inprop` contains `preloadcontent`.
- **`inpreloadnewsection`** — boolean  
  Return preloaded content for a new section on the page, rather than a new page.
  Only used when `inprop` contains `preloadcontent`.
- **`ineditintrostyle`** — enum: `lessframes`, `moreframes`; default `moreframes`  
  Some intro messages come with optional wrapper frames. Use `moreframes` to include them or `lessframes` to omit them.
  Only used when `inprop` contains `editintro`.
- **`ineditintroskip`** — string; multi-value (max 50; 500 for high-limit users)  
  List of intro messages to remove from the response. Use this if a specific message is not relevant to your tool, or if the information is conveyed in a different way.
  Only used when `inprop` contains `editintro`.
- **`ineditintrocustom`** — string  
  Title of a custom page to use as an additional intro message.
  Only used when `inprop` contains `editintro`.
- **`incontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=info&titles=Main%20Page` — Get information about the page Main Page.
- `api.php?action=query&prop=info&inprop=protection&titles=Main%20Page` — Get general and protection information about the page Main Page.

<a id="mod-query-iwlinks"></a>
#### `action=query&prop=iwlinks`

*Core* · `MediaWiki\Api\ApiQueryIWLinks` · prefix `iw` · GET or POST

Returns all interwiki links from the given pages.

Parameters:

- **`iwprop`** — enum: `url`; multi-value (max 50; 500 for high-limit users)  
  Which additional properties to get for each interwiki link:
  
  - **url**: Adds the full URL.
- **`iwprefix`** — string  
  Only return interwiki links with this prefix.
- **`iwtitle`** — string  
  Interwiki link to search for. Must be used with `iwprefix`.
- **`iwdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`iwlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many interwiki links to return.
- **`iwcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`iwurl`** — boolean; **deprecated**  
  Whether to get the full URL (cannot be used with iwprop).

Examples:

- `api.php?action=query&prop=iwlinks&titles=Main%20Page` — Get interwiki links from the page Main Page.

<a id="mod-query-langlinks"></a>
#### `action=query&prop=langlinks`

*Core* · `MediaWiki\Api\ApiQueryLangLinks` · prefix `ll` · GET or POST

Returns all interlanguage links from the given pages.

Parameters:

- **`llprop`** — enum: `autonym`, `langname`, `url`; multi-value (max 50; 500 for high-limit users)  
  Which additional properties to get for each interlanguage link:
  
  - **url**: Adds the full URL.
  - **langname**: Adds the localised language name (best effort). Use `llinlanguagecode` to control the language.
  - **autonym**: Adds the native language name.
- **`lllang`** — string  
  Only return language links with this language code.
- **`lltitle`** — string  
  Link to search for. Must be used with `lllang`.
- **`lldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`llinlanguagecode`** — string; default `en`  
  Language code for localised language names.
- **`lllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many langlinks to return.
- **`llcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`llurl`** — boolean; **deprecated**  
  Whether to get the full URL (cannot be used with `llprop`).

Examples:

- `api.php?action=query&prop=langlinks&titles=Main%20Page&redirects=` — Get interlanguage links from the page Main Page.

<a id="mod-query-links"></a>
#### `action=query&prop=links`

*Core* · `MediaWiki\Api\ApiQueryLinks` · prefix `pl` · GET or POST · usable as generator (`generator=links`)

Returns all links from the given pages.

Parameters:

- **`plnamespace`** — namespace ID; multi-value (max 50; 500 for high-limit users); `*` = all  
  Show links in these namespaces only.
- **`pllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many links to return.
- **`plcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`pltitles`** — string; multi-value (max 50; 500 for high-limit users)  
  Only list links to these titles. Useful for checking whether a certain page links to a certain title.
- **`pldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&prop=links&titles=Main%20Page` — Get links from the page Main Page
- `api.php?action=query&generator=links&titles=Main%20Page&prop=info` — Get information about the link pages in the page Main Page.
- `api.php?action=query&prop=links&titles=Main%20Page&plnamespace=2|10` — Get links from the page Main Page in the User and Template namespaces.

<a id="mod-query-linkshere"></a>
#### `action=query&prop=linkshere`

*Core* · `MediaWiki\Api\ApiQueryBacklinksprop` · prefix `lh` · GET or POST · usable as generator (`generator=linkshere`)

Find all pages that link to the given pages.

Parameters:

- **`lhprop`** — enum: `pageid`, `redirect`, `title`; multi-value (max 50; 500 for high-limit users); default `pageid|title|redirect`  
  Which properties to get:
  
  - **pageid**: Page ID of each page.
  - **title**: Title of each page.
  - **redirect**: Flag if the page is a redirect.
- **`lhnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only include pages in these namespaces.
- **`lhshow`** — enum: `!redirect`, `redirect`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria:
  
  - **redirect**: Only show redirects.
  - **!redirect**: Only show non-redirects.
- **`lhlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many to return.
- **`lhcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=linkshere&titles=Main%20Page` — Get a list of pages linking to the Main Page.
- `api.php?action=query&generator=linkshere&titles=Main%20Page&prop=info` — Get information about pages linking to the Main Page.

<a id="mod-query-pageprops"></a>
#### `action=query&prop=pageprops`

*Core* · `MediaWiki\Api\ApiQueryPageProps` · prefix `pp` · GET or POST

Get various page properties defined in the page content.

Parameters:

- **`ppcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`ppprop`** — string; multi-value (max 50; 500 for high-limit users)  
  Only list these page properties (`action=query&list=pagepropnames` returns page property names in use). Useful for checking whether pages use a certain page property.

Examples:

- `api.php?action=query&prop=pageprops&titles=Main%20Page|MediaWiki` — Get properties for the pages `Main Page` and `MediaWiki`.

<a id="mod-query-redirects"></a>
#### `action=query&prop=redirects`

*Core* · `MediaWiki\Api\ApiQueryBacklinksprop` · prefix `rd` · GET or POST · usable as generator (`generator=redirects`)

Returns all redirects to the given pages.

Parameters:

- **`rdprop`** — enum: `fragment`, `pageid`, `title`; multi-value (max 50; 500 for high-limit users); default `pageid|title`  
  Which properties to get:
  
  - **pageid**: Page ID of each redirect.
  - **title**: Title of each redirect.
  - **fragment**: Fragment of each redirect, if any.
- **`rdnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only include pages in these namespaces.
- **`rdshow`** — enum: `!fragment`, `fragment`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria:
  
  - **fragment**: Only show redirects with a fragment.
  - **!fragment**: Only show redirects without a fragment.
- **`rdlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many redirects to return.
- **`rdcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=redirects&titles=Main%20Page` — Get a list of redirects to the Main Page.
- `api.php?action=query&generator=redirects&titles=Main%20Page&prop=info` — Get information about all redirects to the Main Page.

<a id="mod-query-revisions"></a>
#### `action=query&prop=revisions`

*Core* · `MediaWiki\Api\ApiQueryRevisions` · prefix `rv` · GET or POST · usable as generator (`generator=revisions`)

Get revision information.

May be used in several ways:
# Get data about a set of pages (last revision), by setting titles or pageids.
# Get revisions for one given page, by using titles or pageids with start, end, or limit.
# Get data about a set of revisions by setting their IDs with revids.

Parameters:

- **`rvprop`** — enum: `comment`, `content`, `contentmodel`, `flags`, `ids`, `parsedcomment`, `roles`, `sha1`, `size`, `slotsha1`, `slotsize`, `tags`, `timestamp`, `user`, `userid`, `parsetree`; multi-value (max 50; 500 for high-limit users); default `ids|timestamp|flags|comment|user`; deprecated values: `parsetree`  
  Which properties to get for each revision:
  
  - **ids**: The ID of the revision.
  - **flags**: Revision flags (minor).
  - **timestamp**: The timestamp of the revision.
  - **user**: User that made the revision. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: User ID of the revision creator. If the user has been revision deleted, a `userhidden` property will be returned.
  - **size**: Length (bytes) of the revision.
  - **slotsize**: Length (bytes) of each revision slot.
  - **sha1**: SHA-1 (base 16) of the revision. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **slotsha1**: SHA-1 (base 16) of each revision slot. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **contentmodel**: Content model ID of each revision slot.
  - **comment**: Comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parsed comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **content**: Content of each revision slot. If the content has been revision deleted, a `texthidden` property will be returned. For performance reasons, if this option is used, `rvlimit` is enforced to 50.
  - **tags**: Tags for the revision.
  - **roles**: List content slot roles that exist in the revision.
  - **parsetree**: Deprecated. Use `action=expandtemplates` or `action=parse` instead. The XML parse tree of revision content (requires content model `wikitext`). For performance reasons, if this option is used, `rvlimit` is enforced to 50.
- **`rvslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Which revision slots to return data for, when slot-related properties are included in `rvprops`. If omitted, data from the `main` slot will be returned in a backwards-compatible format.
- **`rvlimit`** — limit (1–500; 5000 for high-limit users; or `max`)  
  Limit how many revisions will be returned. If `rvprop=content`, `rvprop=parsetree`, `rvdiffto` or `rvdifftotext` is used, the limit is 50. If `rvparse` is used, the limit is 1.
  May only be used with a single page (mode #2).
- **`rvexpandtemplates`** — boolean; **deprecated**  
  Use `action=expandtemplates` instead. Expand templates in revision content (requires rvprop=content).
- **`rvgeneratexml`** — boolean; **deprecated**  
  Use `action=expandtemplates` or `action=parse` instead. Generate XML parse tree for revision content (requires rvprop=content).
- **`rvparse`** — boolean; **deprecated**  
  Use `action=parse` instead. Parse revision content (requires `rvprop=content`). For performance reasons, if this option is used, `rvlimit` is enforced to 1.
- **`rvsection`** — string  
  Only retrieve the content of the section with this identifier.
- **`rvdiffto`** — string; **deprecated**  
  Use `action=compare` instead. Revision ID to diff each revision to. Use `prev`, `next` and `cur` for the previous, next and current revision respectively. For performance reasons, if this option is used, `rvlimit` is enforced to 50.
- **`rvdifftotext`** — string; **deprecated**  
  Use `action=compare` instead. Text to diff each revision to. Only diffs a limited number of revisions. Overrides `rvdiffto`. If `rvsection` is set, only that section will be diffed against this text. For performance reasons, if this option is used, `rvlimit` is enforced to 50.
- **`rvdifftotextpst`** — boolean; **deprecated**  
  Use `action=compare` instead. Perform a pre-save transform on the text before diffing it. Only valid when used with `rvdifftotext`.
- **`rvcontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Serialization format used for `rvdifftotext` and expected for output of content.
- **`rvstartid`** — integer  
  Start enumeration from this revision's timestamp. The revision must exist, but need not belong to this page.
  May only be used with a single page (mode #2).
- **`rvendid`** — integer  
  Stop enumeration at this revision's timestamp. The revision must exist, but need not belong to this page.
  May only be used with a single page (mode #2).
- **`rvstart`** — timestamp  
  From which revision timestamp to start enumeration.
  May only be used with a single page (mode #2).
- **`rvend`** — timestamp  
  Enumerate up to this timestamp.
  May only be used with a single page (mode #2).
- **`rvdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: rvstart has to be before rvend.
  - **older**: List newest first (default). Note: rvstart has to be later than rvend.
  May only be used with a single page (mode #2).
- **`rvuser`** — user  
  Only include revisions made by user.
  May only be used with a single page (mode #2).
- **`rvexcludeuser`** — user  
  Exclude revisions made by user.
  May only be used with a single page (mode #2).
- **`rvtag`** — string  
  Only list revisions tagged with this tag.
- **`rvcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`rvcontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `rvslots`  
  Content serialization format used for output of content.

Examples:

- `api.php?action=query&prop=revisions&titles=API|Main%20Page&rvslots=*&rvprop=timestamp|user|comment|content` — Get data with content for the last revision of titles `API` and Main Page.
- `api.php?action=query&prop=revisions&titles=Main%20Page&rvlimit=5&rvprop=timestamp|user|comment` — Get last 5 revisions of the Main Page.
- `api.php?action=query&prop=revisions&titles=Main%20Page&rvlimit=5&rvprop=timestamp|user|comment&rvdir=newer` — Get first 5 revisions of the Main Page.
- `api.php?action=query&prop=revisions&titles=Main%20Page&rvlimit=5&rvprop=timestamp|user|comment&rvdir=newer&rvstart=2006-05-01T00:00:00Z` — Get first 5 revisions of the Main Page made after 2006-05-01.
- `api.php?action=query&prop=revisions&titles=Main%20Page&rvlimit=5&rvprop=timestamp|user|comment&rvexcludeuser=127.0.0.1` — Get first 5 revisions of the Main Page that were not made by anonymous user `127.0.0.1`.
- `api.php?action=query&prop=revisions&titles=Main%20Page&rvlimit=5&rvprop=timestamp|user|comment&rvuser=MediaWiki%20default` — Get first 5 revisions of the Main Page that were made by the user `MediaWiki default`.

<a id="mod-query-stashimageinfo"></a>
#### `action=query&prop=stashimageinfo`

*Core* · `MediaWiki\Api\ApiQueryStashImageInfo` · prefix `sii` · GET or POST

Returns file information for stashed files.

Parameters:

- **`siifilekey`** — string; multi-value (max 50; 500 for high-limit users)  
  Key that identifies a previous upload that was stashed temporarily.
- **`siisessionkey`** — string; multi-value (max 50; 500 for high-limit users); **deprecated**  
  Alias for siifilekey, for backward compatibility.
- **`siiprop`** — enum: `badfile`, `bitdepth`, `canonicaltitle`, `commonmetadata`, `dimensions`, `extmetadata`, `metadata`, `mime`, `sha1`, `size`, `thumbmime`, `timestamp`, `url`; multi-value (max 50; 500 for high-limit users); default `timestamp|url`  
  Which file information to get:
  
  - **timestamp**: Adds timestamp for the uploaded version.
  - **canonicaltitle**: Adds the canonical title of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **url**: Gives URL to the file and the description page. If the file has been revision deleted, a `filehidden` property will be returned.
  - **size**: Adds the size of the file in bytes and the height, width and page count (if applicable).
  - **dimensions**: Alias for size.
  - **sha1**: Adds SHA-1 hash for the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **mime**: Adds MIME type of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **thumbmime**: Adds MIME type of the image thumbnail (requires url and param siiurlwidth). If the file has been revision deleted, a `filehidden` property will be returned.
  - **metadata**: Lists Exif metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **commonmetadata**: Lists file format generic metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **extmetadata**: Lists formatted metadata combined from multiple sources. Results are HTML formatted. If the file has been revision deleted, a `filehidden` property will be returned.
  - **bitdepth**: Adds the bit depth of the version. If the file has been revision deleted, a `filehidden` property will be returned.
  - **badfile**: Adds whether the file is on the MediaWiki:Bad image list
- **`siiurlwidth`** — integer; default `-1`  
  If siiprop=url is set, a URL to an image scaled to this width will be returned.
  For performance reasons if this option is used, no more than 50 scaled images will be returned.
- **`siiurlheight`** — integer; default `-1`  
  Similar to siiurlwidth.
- **`siiurlparam`** — string  
  A handler specific parameter string. For example, PDFs might use `page15-100px`. `siiurlwidth` must be used and be consistent with `siiurlparam`.

Examples:

- `api.php?action=query&prop=stashimageinfo&siifilekey=124sd34rsdf567` — Returns information for a stashed file.
- `api.php?action=query&prop=stashimageinfo&siifilekey=b34edoe3|bceffd4&siiurlwidth=120&siiprop=url` — Returns thumbnails for two stashed files.

<a id="mod-query-templates"></a>
#### `action=query&prop=templates`

*Core* · `MediaWiki\Api\ApiQueryLinks` · prefix `tl` · GET or POST · usable as generator (`generator=templates`)

Returns all pages transcluded on the given pages.

Parameters:

- **`tlnamespace`** — namespace ID; multi-value (max 50; 500 for high-limit users); `*` = all  
  Show templates in these namespaces only.
- **`tllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many templates to return.
- **`tlcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`tltemplates`** — string; multi-value (max 50; 500 for high-limit users)  
  Only list these templates. Useful for checking whether a certain page uses a certain template.
- **`tldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&prop=templates&titles=Main%20Page` — Get the templates used on the page Main Page.
- `api.php?action=query&generator=templates&titles=Main%20Page&prop=info` — Get information about the template pages used on the page Main Page.
- `api.php?action=query&prop=templates&titles=Main%20Page&tlnamespace=2|10` — Get pages in the User and Template namespaces that are transcluded on the page Main Page.

<a id="mod-query-transcludedin"></a>
#### `action=query&prop=transcludedin`

*Core* · `MediaWiki\Api\ApiQueryBacklinksprop` · prefix `ti` · GET or POST · usable as generator (`generator=transcludedin`)

Find all pages that transclude the given pages.

Parameters:

- **`tiprop`** — enum: `pageid`, `redirect`, `title`; multi-value (max 50; 500 for high-limit users); default `pageid|title|redirect`  
  Which properties to get:
  
  - **pageid**: Page ID of each page.
  - **title**: Title of each page.
  - **redirect**: Flag if the page is a redirect.
- **`tinamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only include pages in these namespaces.
- **`tishow`** — enum: `!redirect`, `redirect`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria:
  
  - **redirect**: Only show redirects.
  - **!redirect**: Only show non-redirects.
- **`tilimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many to return.
- **`ticontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&prop=transcludedin&titles=Main%20Page` — Get a list of pages transcluding Main Page.
- `api.php?action=query&generator=transcludedin&titles=Main%20Page&prop=info` — Get information about pages transcluding Main Page.

### 5.3 Core `list` modules

<a id="mod-query-allcategories"></a>
#### `action=query&list=allcategories`

*Core* · `MediaWiki\Api\ApiQueryAllCategories` · prefix `ac` · GET or POST · usable as generator (`generator=allcategories`)

Enumerate all categories.

Parameters:

- **`acfrom`** — string  
  The category to start enumerating from.
- **`accontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`acto`** — string  
  The category to stop enumerating at.
- **`acprefix`** — string  
  Search for all category titles that begin with this value.
- **`acdir`** — enum: `ascending`, `descending`; default `ascending`  
  Direction to sort in.
- **`acmin`** — integer  
  Only return categories with at least this many members.
- **`acmax`** — integer  
  Only return categories with at most this many members.
- **`aclimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many categories to return.
- **`acprop`** — enum: `hidden`, `size`; multi-value (max 50; 500 for high-limit users)  
  Which properties to get:
  
  - **size**: Adds number of pages in the category.
  - **hidden**: Tags categories that are hidden with `__HIDDENCAT__`.

Examples:

- `api.php?action=query&list=allcategories&acprop=size` — List categories with information on the number of pages in each.
- `api.php?action=query&generator=allcategories&gacprefix=List&prop=info` — Retrieve info about the category page itself for categories beginning `List`.

<a id="mod-query-alldeletedrevisions"></a>
#### `action=query&list=alldeletedrevisions`

*Core* · `MediaWiki\Api\ApiQueryAllDeletedRevisions` · prefix `adr` · GET or POST · usable as generator (`generator=alldeletedrevisions`)

List all deleted revisions by a user or in a namespace.

Parameters:

- **`adrprop`** — enum: `comment`, `content`, `contentmodel`, `flags`, `ids`, `parsedcomment`, `roles`, `sha1`, `size`, `slotsha1`, `slotsize`, `tags`, `timestamp`, `user`, `userid`, `parsetree`; multi-value (max 50; 500 for high-limit users); default `ids|timestamp|flags|comment|user`; deprecated values: `parsetree`  
  Which properties to get for each revision:
  
  - **ids**: The ID of the revision.
  - **flags**: Revision flags (minor).
  - **timestamp**: The timestamp of the revision.
  - **user**: User that made the revision. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: User ID of the revision creator. If the user has been revision deleted, a `userhidden` property will be returned.
  - **size**: Length (bytes) of the revision.
  - **slotsize**: Length (bytes) of each revision slot.
  - **sha1**: SHA-1 (base 16) of the revision. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **slotsha1**: SHA-1 (base 16) of each revision slot. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **contentmodel**: Content model ID of each revision slot.
  - **comment**: Comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parsed comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **content**: Content of each revision slot. If the content has been revision deleted, a `texthidden` property will be returned. For performance reasons, if this option is used, `adrlimit` is enforced to 50.
  - **tags**: Tags for the revision.
  - **roles**: List content slot roles that exist in the revision.
  - **parsetree**: Deprecated. Use `action=expandtemplates` or `action=parse` instead. The XML parse tree of revision content (requires content model `wikitext`). For performance reasons, if this option is used, `adrlimit` is enforced to 50.
- **`adrslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Which revision slots to return data for, when slot-related properties are included in `adrprops`. If omitted, data from the `main` slot will be returned in a backwards-compatible format.
- **`adrlimit`** — limit (1–500; 5000 for high-limit users; or `max`)  
  Limit how many revisions will be returned. If `adrprop=content`, `adrprop=parsetree`, `adrdiffto` or `adrdifftotext` is used, the limit is 50. If `adrparse` is used, the limit is 1.
- **`adrexpandtemplates`** — boolean; **deprecated**  
  Use `action=expandtemplates` instead. Expand templates in revision content (requires adrprop=content).
- **`adrgeneratexml`** — boolean; **deprecated**  
  Use `action=expandtemplates` or `action=parse` instead. Generate XML parse tree for revision content (requires adrprop=content).
- **`adrparse`** — boolean; **deprecated**  
  Use `action=parse` instead. Parse revision content (requires `adrprop=content`). For performance reasons, if this option is used, `adrlimit` is enforced to 1.
- **`adrsection`** — string  
  Only retrieve the content of the section with this identifier.
- **`adrdiffto`** — string; **deprecated**  
  Use `action=compare` instead. Revision ID to diff each revision to. Use `prev`, `next` and `cur` for the previous, next and current revision respectively. For performance reasons, if this option is used, `adrlimit` is enforced to 50.
- **`adrdifftotext`** — string; **deprecated**  
  Use `action=compare` instead. Text to diff each revision to. Only diffs a limited number of revisions. Overrides `adrdiffto`. If `adrsection` is set, only that section will be diffed against this text. For performance reasons, if this option is used, `adrlimit` is enforced to 50.
- **`adrdifftotextpst`** — boolean; **deprecated**  
  Use `action=compare` instead. Perform a pre-save transform on the text before diffing it. Only valid when used with `adrdifftotext`.
- **`adrcontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Serialization format used for `adrdifftotext` and expected for output of content.
- **`adruser`** — user  
  Only list revisions by this user.
- **`adrnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only list pages in this namespace.
- **`adrstart`** — timestamp  
  The timestamp to start enumerating from.
  May only be used with `adruser`.
- **`adrend`** — timestamp  
  The timestamp to stop enumerating at.
  May only be used with `adruser`.
- **`adrdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: adrstart has to be before adrend.
  - **older**: List newest first (default). Note: adrstart has to be later than adrend.
- **`adrfrom`** — string  
  Start listing at this title.
  Cannot be used with `adruser`.
- **`adrto`** — string  
  Stop listing at this title.
  Cannot be used with `adruser`.
- **`adrprefix`** — string  
  Search for all page titles that begin with this value.
  Cannot be used with `adruser`.
- **`adrexcludeuser`** — user  
  Don't list revisions by this user.
  Cannot be used with `adruser`.
- **`adrtag`** — string  
  Only list revisions tagged with this tag.
- **`adrcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`adrgeneratetitles`** — boolean  
  When being used as a generator, generate titles rather than revision IDs.
- **`adrcontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `adrslots`  
  Content serialization format used for output of content.

Examples:

- `api.php?action=query&list=alldeletedrevisions&adruser=Example&adrlimit=50` — List the last 50 deleted contributions by user `Example`.
- `api.php?action=query&list=alldeletedrevisions&adrdir=newer&adrnamespace=0&adrlimit=50` — List the first 50 deleted revisions in the main namespace.

<a id="mod-query-allfileusages"></a>
#### `action=query&list=allfileusages`

*Core* · `MediaWiki\Api\ApiQueryAllLinks` · prefix `af` · GET or POST · usable as generator (`generator=allfileusages`)

List all file usages, including non-existing.

Parameters:

- **`afcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`affrom`** — string  
  The title of the file to start enumerating from.
- **`afto`** — string  
  The title of the file to stop enumerating at.
- **`afprefix`** — string  
  Search for all file titles that begin with this value.
- **`afunique`** — boolean  
  Only show distinct file titles. Cannot be used with afprop=ids.
  When used as a generator, yields target pages instead of source pages.
- **`afprop`** — enum: `ids`, `title`; multi-value (max 50; 500 for high-limit users); default `title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page IDs of the using pages (cannot be used with afunique).
  - **title**: Adds the title of the file.
- **`aflimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total items to return.
- **`afdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=allfileusages&affrom=B&afprop=ids|title` — List file titles, including missing ones, with page IDs they are from, starting at `B`.
- `api.php?action=query&list=allfileusages&afunique=&affrom=B` — List unique file titles.
- `api.php?action=query&generator=allfileusages&gafunique=&gaffrom=B` — Gets all file titles, marking the missing ones.
- `api.php?action=query&generator=allfileusages&gaffrom=B` — Gets pages containing the files.

<a id="mod-query-allimages"></a>
#### `action=query&list=allimages`

*Core* · `MediaWiki\Api\ApiQueryAllImages` · prefix `ai` · GET or POST · usable as generator (`generator=allimages`)

Enumerate all images sequentially.

Parameters:

- **`aisort`** — enum: `name`, `timestamp`; default `name`  
  Property to sort by.
- **`aidir`** — enum: `ascending`, `descending`, `newer`, `older`; default `ascending`  
  The direction in which to list.
- **`aifrom`** — string  
  The image title to start enumerating from. Can only be used with aisort=name.
- **`aito`** — string  
  The image title to stop enumerating at. Can only be used with aisort=name.
- **`aicontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`aistart`** — timestamp  
  The timestamp to start enumerating from. Can only be used with aisort=timestamp.
- **`aiend`** — timestamp  
  The timestamp to end enumerating. Can only be used with aisort=timestamp.
- **`aiprop`** — enum: `badfile`, `bitdepth`, `canonicaltitle`, `comment`, `commonmetadata`, `dimensions`, `extmetadata`, `mediatype`, `metadata`, `mime`, `parsedcomment`, `sha1`, `size`, `timestamp`, `url`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `timestamp|url`  
  Which file information to get:
  
  - **timestamp**: Adds timestamp for the uploaded version.
  - **user**: Adds the user who uploaded each file version. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: Add the ID of the user that uploaded each file version. If the user has been revision deleted, a `userhidden` property will be returned.
  - **comment**: Comment on the version. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parse the comment on the version. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **canonicaltitle**: Adds the canonical title of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **url**: Gives URL to the file and the description page. If the file has been revision deleted, a `filehidden` property will be returned.
  - **size**: Adds the size of the file in bytes and the height, width and page count (if applicable).
  - **dimensions**: Alias for size.
  - **sha1**: Adds SHA-1 hash for the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **mime**: Adds MIME type of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **mediatype**: Adds the media type of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **metadata**: Lists Exif metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **commonmetadata**: Lists file format generic metadata for the version of the file. If the file has been revision deleted, a `filehidden` property will be returned.
  - **extmetadata**: Lists formatted metadata combined from multiple sources. Results are HTML formatted. If the file has been revision deleted, a `filehidden` property will be returned.
  - **bitdepth**: Adds the bit depth of the version. If the file has been revision deleted, a `filehidden` property will be returned.
  - **badfile**: Adds whether the file is on the MediaWiki:Bad image list
- **`aiprefix`** — string  
  Search for all image titles that begin with this value. Can only be used with aisort=name.
- **`aiminsize`** — integer  
  Limit to images with at least this many bytes.
- **`aimaxsize`** — integer  
  Limit to images with at most this many bytes.
- **`aisha1`** — string  
  SHA1 hash of image. Overrides aisha1base36.
- **`aisha1base36`** — string  
  SHA1 hash of image in base 36 (used in MediaWiki).
- **`aiuser`** — user  
  Only return files where the last version was uploaded by this user. Can only be used with aisort=timestamp. Cannot be used together with aifilterbots.
- **`aifilterbots`** — enum: `all`, `bots`, `nobots`; default `all`  
  How to filter files uploaded by bots. Can only be used with aisort=timestamp. Cannot be used together with aiuser.
- **`aimime`** — string; multi-value (max 50; 500 for high-limit users)  
  What MIME types to search for, e.g. `image/jpeg`.
- **`ailimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many images in total to return.

Examples:

- `api.php?action=query&list=allimages&aifrom=B` — Show a list of files starting at the letter `B`.
- `api.php?action=query&list=allimages&aiprop=user|timestamp|url&aisort=timestamp&aidir=older` — Show a list of recently uploaded files, similar to Special:NewFiles.
- `api.php?action=query&list=allimages&aimime=image/png|image/gif` — Show a list of files with MIME type `image/png` or `image/gif`
- `api.php?action=query&generator=allimages&gailimit=4&gaifrom=T&prop=imageinfo` — Show info about 4 files starting at the letter `T`.

<a id="mod-query-alllinks"></a>
#### `action=query&list=alllinks`

*Core* · `MediaWiki\Api\ApiQueryAllLinks` · prefix `al` · GET or POST · usable as generator (`generator=alllinks`)

Enumerate all links that point to a given namespace.

Parameters:

- **`alcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`alfrom`** — string  
  The title of the link to start enumerating from.
- **`alto`** — string  
  The title of the link to stop enumerating at.
- **`alprefix`** — string  
  Search for all linked titles that begin with this value.
- **`alunique`** — boolean  
  Only show distinct linked titles. Cannot be used with `alprop=ids`.
  When used as a generator, yields target pages instead of source pages.
- **`alprop`** — enum: `ids`, `title`; multi-value (max 50; 500 for high-limit users); default `title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page ID of the linking page (cannot be used with `alunique`).
  - **title**: Adds the title of the link.
- **`alnamespace`** — namespace ID  
  The namespace to enumerate.
- **`allimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total items to return.
- **`aldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=alllinks&alfrom=B&alprop=ids|title` — List linked titles, including missing ones, with page IDs they are from, starting at `B`.
- `api.php?action=query&list=alllinks&alunique=&alfrom=B` — List unique linked titles.
- `api.php?action=query&generator=alllinks&galunique=&galfrom=B` — Gets all linked titles, marking the missing ones.
- `api.php?action=query&generator=alllinks&galfrom=B` — Gets pages containing the links.

<a id="mod-query-allpages"></a>
#### `action=query&list=allpages`

*Core* · `MediaWiki\Api\ApiQueryAllPages` · prefix `ap` · GET or POST · usable as generator (`generator=allpages`)

Enumerate all pages sequentially in a given namespace.

Parameters:

- **`apfrom`** — string  
  The page title to start enumerating from.
- **`apcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`apto`** — string  
  The page title to stop enumerating at.
- **`apprefix`** — string  
  Search for all page titles that begin with this value.
- **`apnamespace`** — namespace  
  The namespace to enumerate.
- **`apfilterredir`** — enum: `all`, `nonredirects`, `redirects`; default `all`  
  Which pages to list.
- **`apfilterlanglinks`** — enum: `all`, `withlanglinks`, `withoutlanglinks`; default `all`  
  Filter based on whether a page has langlinks. Note that this may not consider langlinks added by extensions.
- **`apminsize`** — integer  
  Limit to pages with at least this many bytes.
- **`apmaxsize`** — integer  
  Limit to pages with at most this many bytes.
- **`apprtype`** — enum: `edit`, `move`, `upload`; multi-value (max 50; 500 for high-limit users)  
  Limit to protected pages only.
- **`apprlevel`** — enum: ``, `autoconfirmed`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Filter protections based on protection level (must be used with apprtype= parameter).
- **`apprfiltercascade`** — enum: `all`, `cascading`, `noncascading`; default `all`  
  Filter protections based on cascadingness (ignored when apprtype isn't set).
- **`apprexpiry`** — enum: `all`, `definite`, `indefinite`; default `all`  
  Which protection expiry to filter the page on:
  
  - **indefinite**: Get only pages with indefinite protection expiry.
  - **definite**: Get only pages with a definite (specific) protection expiry.
  - **all**: Get pages with any protections expiry.
- **`aplimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.
- **`apdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=allpages&apfrom=B` — Show a list of pages starting at the letter `B`.
- `api.php?action=query&generator=allpages&gaplimit=4&gapfrom=T&prop=info` — Show info about 4 pages starting at the letter `T`.
- `api.php?action=query&generator=allpages&gaplimit=2&gapfilterredir=nonredirects&gapfrom=Re&prop=revisions&rvprop=content` — Show content of first 2 non-redirect pages beginning at `Re`.

<a id="mod-query-allredirects"></a>
#### `action=query&list=allredirects`

*Core* · `MediaWiki\Api\ApiQueryAllLinks` · prefix `ar` · GET or POST · usable as generator (`generator=allredirects`)

List all redirects to a namespace.

Parameters:

- **`arcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`arfrom`** — string  
  The title of the redirect to start enumerating from.
- **`arto`** — string  
  The title of the redirect to stop enumerating at.
- **`arprefix`** — string  
  Search for all target pages that begin with this value.
- **`arunique`** — boolean  
  Only show distinct target pages. Cannot be used with arprop=ids|fragment|interwiki.
  When used as a generator, yields target pages instead of source pages.
- **`arprop`** — enum: `fragment`, `ids`, `interwiki`, `title`; multi-value (max 50; 500 for high-limit users); default `title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page ID of the redirecting page (cannot be used with `arunique`).
  - **title**: Adds the title of the redirect.
  - **fragment**: Adds the fragment from the redirect, if any (cannot be used with `arunique`).
  - **interwiki**: Adds the interwiki prefix from the redirect, if any (cannot be used with `arunique`).
- **`arnamespace`** — namespace ID  
  The namespace to enumerate.
- **`arlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total items to return.
- **`ardir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=allredirects&arfrom=B&arprop=ids|title` — List target pages, including missing ones, with page IDs they are from, starting at `B`.
- `api.php?action=query&list=allredirects&arunique=&arfrom=B` — List unique target pages.
- `api.php?action=query&generator=allredirects&garunique=&garfrom=B` — Gets all target pages, marking the missing ones.
- `api.php?action=query&generator=allredirects&garfrom=B` — Gets pages containing the redirects.

<a id="mod-query-allrevisions"></a>
#### `action=query&list=allrevisions`

*Core* · `MediaWiki\Api\ApiQueryAllRevisions` · prefix `arv` · GET or POST · usable as generator (`generator=allrevisions`)

List all revisions.

Parameters:

- **`arvprop`** — enum: `comment`, `content`, `contentmodel`, `flags`, `ids`, `parsedcomment`, `roles`, `sha1`, `size`, `slotsha1`, `slotsize`, `tags`, `timestamp`, `user`, `userid`, `parsetree`; multi-value (max 50; 500 for high-limit users); default `ids|timestamp|flags|comment|user`; deprecated values: `parsetree`  
  Which properties to get for each revision:
  
  - **ids**: The ID of the revision.
  - **flags**: Revision flags (minor).
  - **timestamp**: The timestamp of the revision.
  - **user**: User that made the revision. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: User ID of the revision creator. If the user has been revision deleted, a `userhidden` property will be returned.
  - **size**: Length (bytes) of the revision.
  - **slotsize**: Length (bytes) of each revision slot.
  - **sha1**: SHA-1 (base 16) of the revision. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **slotsha1**: SHA-1 (base 16) of each revision slot. If the content has been revision deleted, a `sha1hidden` property will be returned.
  - **contentmodel**: Content model ID of each revision slot.
  - **comment**: Comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Parsed comment by the user for the revision. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **content**: Content of each revision slot. If the content has been revision deleted, a `texthidden` property will be returned. For performance reasons, if this option is used, `arvlimit` is enforced to 50.
  - **tags**: Tags for the revision.
  - **roles**: List content slot roles that exist in the revision.
  - **parsetree**: Deprecated. Use `action=expandtemplates` or `action=parse` instead. The XML parse tree of revision content (requires content model `wikitext`). For performance reasons, if this option is used, `arvlimit` is enforced to 50.
- **`arvslots`** — enum: `main`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Which revision slots to return data for, when slot-related properties are included in `arvprops`. If omitted, data from the `main` slot will be returned in a backwards-compatible format.
- **`arvlimit`** — limit (1–500; 5000 for high-limit users; or `max`)  
  Limit how many revisions will be returned. If `arvprop=content`, `arvprop=parsetree`, `arvdiffto` or `arvdifftotext` is used, the limit is 50. If `arvparse` is used, the limit is 1.
- **`arvexpandtemplates`** — boolean; **deprecated**  
  Use `action=expandtemplates` instead. Expand templates in revision content (requires arvprop=content).
- **`arvgeneratexml`** — boolean; **deprecated**  
  Use `action=expandtemplates` or `action=parse` instead. Generate XML parse tree for revision content (requires arvprop=content).
- **`arvparse`** — boolean; **deprecated**  
  Use `action=parse` instead. Parse revision content (requires `arvprop=content`). For performance reasons, if this option is used, `arvlimit` is enforced to 1.
- **`arvsection`** — string  
  Only retrieve the content of the section with this identifier.
- **`arvdiffto`** — string; **deprecated**  
  Use `action=compare` instead. Revision ID to diff each revision to. Use `prev`, `next` and `cur` for the previous, next and current revision respectively. For performance reasons, if this option is used, `arvlimit` is enforced to 50.
- **`arvdifftotext`** — string; **deprecated**  
  Use `action=compare` instead. Text to diff each revision to. Only diffs a limited number of revisions. Overrides `arvdiffto`. If `arvsection` is set, only that section will be diffed against this text. For performance reasons, if this option is used, `arvlimit` is enforced to 50.
- **`arvdifftotextpst`** — boolean; **deprecated**  
  Use `action=compare` instead. Perform a pre-save transform on the text before diffing it. Only valid when used with `arvdifftotext`.
- **`arvcontentformat`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; **deprecated**  
  Serialization format used for `arvdifftotext` and expected for output of content.
- **`arvuser`** — user  
  Only list revisions by this user.
- **`arvnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only list pages in this namespace.
- **`arvstart`** — timestamp  
  The timestamp to start enumerating from.
- **`arvend`** — timestamp  
  The timestamp to stop enumerating at.
- **`arvdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: arvstart has to be before arvend.
  - **older**: List newest first (default). Note: arvstart has to be later than arvend.
- **`arvexcludeuser`** — user  
  Don't list revisions by this user.
- **`arvcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`arvgeneratetitles`** — boolean  
  When being used as a generator, generate titles rather than revision IDs.
- **`arvcontentformat-{slot}`** — enum: `application/json`, `application/octet-stream`, `application/unknown`, `application/vnd.php.serialized`, `application/x-binary`, `text/css`, `text/javascript`, `text/plain`, `text/unknown`, `text/x-wiki`, `unknown/unknown`; templated: `{slot}` ← values of `arvslots`  
  Content serialization format used for output of content.

Examples:

- `api.php?action=query&list=allrevisions&arvuser=Example&arvlimit=50` — List the last 50 contributions by user `Example`.
- `api.php?action=query&list=allrevisions&arvdir=newer&arvlimit=50` — List the first 50 revisions in any namespace.

<a id="mod-query-alltransclusions"></a>
#### `action=query&list=alltransclusions`

*Core* · `MediaWiki\Api\ApiQueryAllLinks` · prefix `at` · GET or POST · usable as generator (`generator=alltransclusions`)

List all transclusions (pages embedded using {{x}}), including non-existing.

Parameters:

- **`atcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`atfrom`** — string  
  The title of the transclusion to start enumerating from.
- **`atto`** — string  
  The title of the transclusion to stop enumerating at.
- **`atprefix`** — string  
  Search for all transcluded titles that begin with this value.
- **`atunique`** — boolean  
  Only show distinct transcluded titles. Cannot be used with atprop=ids.
  When used as a generator, yields target pages instead of source pages.
- **`atprop`** — enum: `ids`, `title`; multi-value (max 50; 500 for high-limit users); default `title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page ID of the transcluding page (cannot be used with atunique).
  - **title**: Adds the title of the transclusion.
- **`atnamespace`** — namespace ID; default `10`  
  The namespace to enumerate.
- **`atlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total items to return.
- **`atdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=alltransclusions&atfrom=B&atprop=ids|title` — List transcluded titles, including missing ones, with page IDs they are from, starting at `B`.
- `api.php?action=query&list=alltransclusions&atunique=&atfrom=B` — List unique transcluded titles.
- `api.php?action=query&generator=alltransclusions&gatunique=&gatfrom=B` — Gets all transcluded titles, marking the missing ones.
- `api.php?action=query&generator=alltransclusions&gatfrom=B` — Gets pages containing the transclusions.

<a id="mod-query-allusers"></a>
#### `action=query&list=allusers`

*Core* · `MediaWiki\Api\ApiQueryAllUsers` · prefix `au` · GET or POST

Enumerate all registered users.

Parameters:

- **`aufrom`** — string  
  The username to start enumerating from.
- **`auto`** — string  
  The username to stop enumerating at.
- **`auprefix`** — string  
  Search for all users that begin with this value.
- **`audir`** — enum: `ascending`, `descending`; default `ascending`  
  Direction to sort in.
- **`augroup`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Only include users in the given groups. Does not include implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`auexcludegroup`** — enum: `bot`, `bureaucrat`, `interface-admin`, `suppress`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Exclude users in the given groups.
- **`aurights`** — enum: `apihighlimits`, `applychangetags`, `autoconfirmed`, `autocreateaccount`, `autopatrol`, `bigdelete`, `block`, `blockemail`, `bot`, `browsearchive`, `changeemail`, `changetags`, `confirmemail`, `createaccount`, `createpage`, `createtalk`, `delete`, `delete-redirect`, `deletechangetags`, `deletedhistory`, `deletedtext`, `deletelogentry`, `deleterevision`, `edit`, `editcontentmodel`, `editinterface`, `editmyoptions`, `editmyprivateinfo`, `editmyusercss`, `editmyuserjs`, `editmyuserjson`, `editmyuserjsredirect`, `editmywatchlist`, `editprotected`, `editsemiprotected`, `editsitecss`, `editsitejs`, `editsitejson`, `editusercss`, `edituserjs`, … (96 values total; config-dependent); multi-value (max 50; 500 for high-limit users)  
  Only include users with the given rights. Does not include rights granted by implicit or auto-promoted groups like *, user, or autoconfirmed.
- **`auprop`** — enum: `blockinfo`, `centralids`, `editcount`, `groups`, `implicitgroups`, `registration`, `rights`; multi-value (max 50; 500 for high-limit users)  
  Which pieces of information to include:
  
  - **blockinfo**: Adds the information about a current block on the user.
  - **groups**: Lists groups that the user is in. This uses more server resources and may return fewer results than the limit.
  - **implicitgroups**: Lists all the groups the user is automatically in.
  - **rights**: Lists rights that the user has.
  - **editcount**: Adds the edit count of the user.
  - **registration**: Adds the timestamp of when the user registered if available (may be blank).
  - **centralids**: Adds the central IDs and attachment status for the user.
- **`aulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total usernames to return.
- **`auwitheditsonly`** — boolean  
  Only list users who have made edits.
- **`auactiveusers`** — boolean  
  Only list users active in the last 30 days.
- **`auattachedwiki`** — string  
  With `auprop=centralids`, also indicate whether the user is attached with the wiki identified by this ID.
- **`auexcludenamed`** — boolean  
  Exclude users of named accounts.
- **`auexcludetemp`** — boolean  
  Exclude users of temporary accounts.

Examples:

- `api.php?action=query&list=allusers&aufrom=Y` — List users starting at `Y`.

<a id="mod-query-backlinks"></a>
#### `action=query&list=backlinks`

*Core* · `MediaWiki\Api\ApiQueryBacklinks` · prefix `bl` · GET or POST · usable as generator (`generator=backlinks`)

Find all pages that link to the given page.

Parameters:

- **`bltitle`** — string  
  Title to search. Cannot be used together with `blpageid`.
- **`blpageid`** — integer  
  Page ID to search. Cannot be used together with `bltitle`.
- **`blcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`blnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  The namespace to enumerate.
- **`bldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`blfilterredir`** — enum: `all`, `nonredirects`, `redirects`; default `all`  
  How to filter for redirects. If set to `nonredirects` when `blredirect` is enabled, this is only applied to the second level.
- **`bllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return. If `blredirect` is enabled, the limit applies to each level separately (which means up to 2 * `bllimit` results may be returned).
- **`blredirect`** — boolean  
  If linking page is a redirect, find all pages that link to that redirect as well. Maximum limit is halved.

Examples:

- `api.php?action=query&list=backlinks&bltitle=Main%20Page` — Show links to Main Page.
- `api.php?action=query&generator=backlinks&gbltitle=Main%20Page&prop=info` — Get information about pages linking to Main Page.

<a id="mod-query-blocks"></a>
#### `action=query&list=blocks`

*Core* · `MediaWiki\Api\ApiQueryBlocks` · prefix `bk` · GET or POST

List all blocked users and IP addresses.

Parameters:

- **`bkstart`** — timestamp  
  The timestamp to start enumerating from.
- **`bkend`** — timestamp  
  The timestamp to stop enumerating at.
- **`bkdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: bkstart has to be before bkend.
  - **older**: List newest first (default). Note: bkstart has to be later than bkend.
- **`bkids`** — integer; multi-value (max 50; 500 for high-limit users)  
  List of block IDs to list (optional).
- **`bkusers`** — user; multi-value (max 50; 500 for high-limit users)  
  List of users to search for (optional).
- **`bkip`** — string  
  Get all blocks applying to this IP address or CIDR range, including range blocks.
  Cannot be used together with `bkusers`. CIDR ranges broader than IPv4/16 or IPv6/19 are not accepted.
- **`bklimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of blocks to list.
- **`bkprop`** — enum: `by`, `byid`, `expiry`, `flags`, `id`, `range`, `reason`, `restrictions`, `timestamp`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `id|user|by|timestamp|expiry|reason|flags`  
  Which properties to get:
  
  - **id**: Adds the ID of the block.
  - **user**: Adds the username of the blocked user.
  - **userid**: Adds the user ID of the blocked user.
  - **by**: Adds the username of the blocking user.
  - **byid**: Adds the user ID of the blocking user.
  - **timestamp**: Adds the timestamp of when the block was given.
  - **expiry**: Adds the timestamp of when the block expires.
  - **reason**: Adds the reason given for the block.
  - **range**: Adds the range of IP addresses affected by the block.
  - **flags**: Tags the ban with (autoblock, anononly, etc.).
  - **restrictions**: Adds the partial block restrictions if the block is not sitewide.
- **`bkshow`** — enum: `!account`, `!ip`, `!range`, `!temp`, `account`, `ip`, `range`, `temp`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria.
  For example, to see only indefinite blocks on IP addresses, set `bkshow=ip|!temp`.
- **`bkcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=blocks` — List blocks.
- `api.php?action=query&list=blocks&bkusers=Alice|Bob` — List blocks of users `Alice` and `Bob`.

<a id="mod-query-categorymembers"></a>
#### `action=query&list=categorymembers`

*Core* · `MediaWiki\Api\ApiQueryCategoryMembers` · prefix `cm` · GET or POST · usable as generator (`generator=categorymembers`)

List all pages in a given category.

Parameters:

- **`cmtitle`** — string  
  Which category to enumerate (required). Must include the `Category:` prefix. Cannot be used together with `cmpageid`.
- **`cmpageid`** — integer  
  Page ID of the category to enumerate. Cannot be used together with `cmtitle`.
- **`cmprop`** — enum: `ids`, `sortkey`, `sortkeyprefix`, `timestamp`, `title`, `type`; multi-value (max 50; 500 for high-limit users); default `ids|title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page ID.
  - **title**: Adds the title and namespace ID of the page.
  - **sortkey**: Adds the sortkey used for sorting in the category (hexadecimal string).
  - **sortkeyprefix**: Adds the sortkey prefix used for sorting in the category (human-readable part of the sortkey).
  - **type**: Adds the type that the page has been categorised as (`page`, `subcat` or `file`).
  - **timestamp**: Adds the timestamp of when the page was included.
- **`cmnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only include pages in these namespaces. Note that `cmtype=subcat` or `cmtype=file` may be used instead of `cmnamespace=14` or `6`.
- **`cmtype`** — enum: `file`, `page`, `subcat`; multi-value (max 50; 500 for high-limit users); default `page|subcat|file`  
  Which type of category members to include. Ignored when `cmsort=timestamp` is set.
- **`cmcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`cmlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of pages to return.
- **`cmsort`** — enum: `sortkey`, `timestamp`; default `sortkey`  
  Property to sort by.
- **`cmdir`** — enum: `asc`, `ascending`, `desc`, `descending`, `newer`, `older`; default `ascending`  
  In which direction to sort.
- **`cmstart`** — timestamp  
  Timestamp to start listing from. Can only be used with `cmsort=timestamp`.
- **`cmend`** — timestamp  
  Timestamp to end listing at. Can only be used with `cmsort=timestamp`.
- **`cmstarthexsortkey`** — string  
  Sortkey to start listing from, as returned by `cmprop=sortkey`. Can only be used with `cmsort=sortkey`.
- **`cmendhexsortkey`** — string  
  Sortkey to end listing at, as returned by `cmprop=sortkey`. Can only be used with `cmsort=sortkey`.
- **`cmstartsortkeyprefix`** — string  
  Sortkey prefix to start listing from. Can only be used with `cmsort=sortkey`. Overrides `cmstarthexsortkey`.
- **`cmendsortkeyprefix`** — string  
  Sortkey prefix to end listing before (not at; if this value occurs it will not be included!). Can only be used with cmsort=sortkey. Overrides cmendhexsortkey.
- **`cmstartsortkey`** — string; **deprecated**  
  Use cmstarthexsortkey instead.
- **`cmendsortkey`** — string; **deprecated**  
  Use cmendhexsortkey instead.

Examples:

- `api.php?action=query&list=categorymembers&cmtitle=Category:Physics` — Get first 10 pages in `Category:Physics`.
- `api.php?action=query&generator=categorymembers&gcmtitle=Category:Physics&prop=info` — Get page info about first 10 pages in `Category:Physics`.

<a id="mod-query-deletedrevs"></a>
#### `action=query&list=deletedrevs`

*Core* · `MediaWiki\Api\ApiQueryDeletedrevs` · prefix `dr` · GET or POST · **deprecated**

List deleted revisions.

Operates in three modes:
# List deleted revisions for the given titles, sorted by timestamp.
# List deleted contributions for the given user, sorted by timestamp (no titles specified).
# List all deleted revisions in the given namespace, sorted by title and timestamp (no titles specified, druser not set).

Certain parameters only apply to some modes and are ignored in others.

Parameters:

- **`drstart`** — timestamp  
  The timestamp to start enumerating from.
  Modes: 1, 2
- **`drend`** — timestamp  
  The timestamp to stop enumerating at.
  Modes: 1, 2
- **`drdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: drstart has to be before drend.
  - **older**: List newest first (default). Note: drstart has to be later than drend.
  Modes: 1, 3
- **`drfrom`** — string  
  Start listing at this title.
  Mode: 3
- **`drto`** — string  
  Stop listing at this title.
  Mode: 3
- **`drprefix`** — string  
  Search for all page titles that begin with this value.
  Mode: 3
- **`drunique`** — boolean  
  List only one revision for each page.
  Mode: 3
- **`drnamespace`** — namespace  
  Only list pages in this namespace.
  Mode: 3
- **`drtag`** — string  
  Only list revisions tagged with this tag.
- **`druser`** — user  
  Only list revisions by this user.
- **`drexcludeuser`** — user  
  Don't list revisions by this user.
- **`drprop`** — enum: `comment`, `content`, `len`, `minor`, `parentid`, `parsedcomment`, `revid`, `sha1`, `tags`, `user`, `userid`, `token`; multi-value (max 50; 500 for high-limit users); default `user|comment`; deprecated values: `token`  
  Which properties to get:
  
  - **revid**: Adds the revision ID of the deleted revision.
  - **parentid**: Adds the revision ID of the previous revision to the page.
  - **user**: Adds the user who made the revision.
  - **userid**: Adds the ID of the user who made the revision.
  - **comment**: Adds the comment of the revision.
  - **parsedcomment**: Adds the parsed comment of the revision.
  - **minor**: Tags if the revision is minor.
  - **len**: Adds the length (bytes) of the revision.
  - **sha1**: Adds the SHA-1 (base 16) of the revision.
  - **content**: Adds the content of the revision. For performance reasons, if this option is used, `drlimit` is enforced to 50.
  - **token**: Deprecated. Gives the edit token.
  - **tags**: Tags for the revision.
- **`drlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum amount of revisions to list. If `drprop=content` is used, the limit is 50.
- **`drcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=deletedrevs&titles=Main%20Page|Talk%3AMain%20Page&drprop=user|comment|content` — List the last deleted revisions of the pages Main Page and `Talk:Main Page`, with content (mode 1).
- `api.php?action=query&list=deletedrevs&druser=Bob&drlimit=50` — List the last 50 deleted contributions by `Bob` (mode 2).
- `api.php?action=query&list=deletedrevs&drdir=newer&drlimit=50` — List the first 50 deleted revisions in the main namespace (mode 3).
- `api.php?action=query&list=deletedrevs&drdir=newer&drlimit=50&drnamespace=1&drunique=` — List the first 50 deleted pages in the Talk namespace (mode 3).

<a id="mod-query-embeddedin"></a>
#### `action=query&list=embeddedin`

*Core* · `MediaWiki\Api\ApiQueryBacklinks` · prefix `ei` · GET or POST · usable as generator (`generator=embeddedin`)

Find all pages that embed (transclude) the given title.

Parameters:

- **`eititle`** — string  
  Title to search. Cannot be used together with eipageid.
- **`eipageid`** — integer  
  Page ID to search. Cannot be used together with eititle.
- **`eicontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`einamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  The namespace to enumerate.
- **`eidir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`eifilterredir`** — enum: `all`, `nonredirects`, `redirects`; default `all`  
  How to filter for redirects.
- **`eilimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.

Examples:

- `api.php?action=query&list=embeddedin&eititle=Template:Stub` — Show pages transcluding `Template:Stub`.
- `api.php?action=query&generator=embeddedin&geititle=Template:Stub&prop=info` — Get information about pages transcluding `Template:Stub`.

<a id="mod-query-exturlusage"></a>
#### `action=query&list=exturlusage`

*Core* · `MediaWiki\Api\ApiQueryExtLinksUsage` · prefix `eu` · GET or POST · usable as generator (`generator=exturlusage`)

Enumerate pages that contain a given URL.

Parameters:

- **`euprop`** — enum: `ids`, `title`, `url`; multi-value (max 50; 500 for high-limit users); default `ids|title|url`  
  Which pieces of information to include:
  
  - **ids**: Adds the ID of page.
  - **title**: Adds the title and namespace ID of the page.
  - **url**: Adds the URL used in the page.
- **`eucontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`euprotocol`** — enum: ``, `bitcoin`, `ftp`, `ftps`, `geo`, `git`, `gopher`, `http`, `https`, `irc`, `ircs`, `magnet`, `mailto`, `matrix`, `mms`, `news`, `nntp`, `redis`, `sftp`, `sip`, `sips`, `sms`, `ssh`, `svn`, `tel`, `telnet`, `urn`, `worldwind`, `xmpp`  
  Protocol of the URL. If empty and `euquery` is set, the protocol is `http` and `https`. Leave both this and `euquery` empty to list all external links.
- **`euquery`** — string  
  Search string without protocol. See Special:LinkSearch. Leave empty to list all external links.
- **`eunamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  The page namespaces to enumerate.
- **`eulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many pages to return.
- **`euexpandurl`** — boolean; **deprecated**  
  Expand protocol-relative URLs with the canonical protocol.

Examples:

- `api.php?action=query&list=exturlusage&euquery=www.mediawiki.org` — Show pages linking to `https://www.mediawiki.org`.

<a id="mod-query-filearchive"></a>
#### `action=query&list=filearchive`

*Core* · `MediaWiki\Api\ApiQueryFilearchive` · prefix `fa` · GET or POST

Enumerate all deleted files sequentially.

Parameters:

- **`fafrom`** — string  
  The image title to start enumerating from.
- **`fato`** — string  
  The image title to stop enumerating at.
- **`faprefix`** — string  
  Search for all image titles that begin with this value.
- **`fadir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`fasha1`** — string  
  SHA1 hash of image. Overrides fasha1base36.
- **`fasha1base36`** — string  
  SHA1 hash of image in base 36 (used in MediaWiki).
- **`faprop`** — enum: `archivename`, `bitdepth`, `description`, `dimensions`, `mediatype`, `metadata`, `mime`, `parseddescription`, `sha1`, `size`, `timestamp`, `user`; multi-value (max 50; 500 for high-limit users); default `timestamp`  
  Which image information to get:
  
  - **sha1**: Adds SHA-1 hash for the image.
  - **timestamp**: Adds timestamp for the uploaded version.
  - **user**: Adds user who uploaded the image version.
  - **size**: Adds the size of the image in bytes and the height, width and page count (if applicable).
  - **dimensions**: Alias for size.
  - **description**: Adds description of the image version.
  - **parseddescription**: Parse the description of the version.
  - **mime**: Adds MIME of the image.
  - **mediatype**: Adds the media type of the image.
  - **metadata**: Lists Exif metadata for the version of the image.
  - **bitdepth**: Adds the bit depth of the version.
  - **archivename**: Adds the filename of the archive version for non-latest versions.
- **`falimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many images to return in total.
- **`facontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=filearchive` — Show a list of all deleted files.

<a id="mod-query-imageusage"></a>
#### `action=query&list=imageusage`

*Core* · `MediaWiki\Api\ApiQueryBacklinks` · prefix `iu` · GET or POST · usable as generator (`generator=imageusage`)

Find all pages that use the given image title.

Parameters:

- **`iutitle`** — string  
  Title to search. Cannot be used together with iupageid.
- **`iupageid`** — integer  
  Page ID to search. Cannot be used together with iutitle.
- **`iucontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`iunamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  The namespace to enumerate.
- **`iudir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`iufilterredir`** — enum: `all`, `nonredirects`, `redirects`; default `all`  
  How to filter for redirects. If set to nonredirects when iuredirect is enabled, this is only applied to the second level.
- **`iulimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return. If `iuredirect` is enabled, the limit applies to each level separately (which means up to 2 * `iulimit` results may be returned).
- **`iuredirect`** — boolean  
  If linking page is a redirect, find all pages that link to that redirect as well. Maximum limit is halved.

Examples:

- `api.php?action=query&list=imageusage&iutitle=File:Albert%20Einstein%20Head.jpg` — Show pages using :File:Albert Einstein Head.jpg.
- `api.php?action=query&generator=imageusage&giutitle=File:Albert%20Einstein%20Head.jpg&prop=info` — Get information about pages using :File:Albert Einstein Head.jpg.

<a id="mod-query-iwbacklinks"></a>
#### `action=query&list=iwbacklinks`

*Core* · `MediaWiki\Api\ApiQueryIWBacklinks` · prefix `iwbl` · GET or POST · usable as generator (`generator=iwbacklinks`)

Find all pages that link to the given interwiki link.

Can be used to find all links with a prefix, or all links to a title (with a given prefix). Using neither parameter is effectively "all interwiki links".

Parameters:

- **`iwblprefix`** — string  
  Prefix for the interwiki.
- **`iwbltitle`** — string  
  Interwiki link to search for. Must be used with `iwblblprefix`.
- **`iwblcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`iwbllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.
- **`iwblprop`** — enum: `iwprefix`, `iwtitle`; multi-value (max 50; 500 for high-limit users)  
  Which properties to get:
  
  - **iwprefix**: Adds the prefix of the interwiki.
  - **iwtitle**: Adds the title of the interwiki.
- **`iwbldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=iwbacklinks&iwbltitle=Test&iwblprefix=wikibooks` — Get pages linking to wikibooks:Test.
- `api.php?action=query&generator=iwbacklinks&giwbltitle=Test&giwblprefix=wikibooks&prop=info` — Get information about pages linking to wikibooks:Test.

<a id="mod-query-langbacklinks"></a>
#### `action=query&list=langbacklinks`

*Core* · `MediaWiki\Api\ApiQueryLangBacklinks` · prefix `lbl` · GET or POST · usable as generator (`generator=langbacklinks`)

Find all pages that link to the given language link.

Can be used to find all links with a language code, or all links to a title (with a given language). Using neither parameter is effectively "all language links".

Note that this may not consider language links added by extensions.

Parameters:

- **`lbllang`** — string  
  Language for the language link.
- **`lbltitle`** — string  
  Language link to search for. Must be used with lbllang.
- **`lblcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`lbllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.
- **`lblprop`** — enum: `lllang`, `lltitle`; multi-value (max 50; 500 for high-limit users)  
  Which properties to get:
  
  - **lllang**: Adds the language code of the language link.
  - **lltitle**: Adds the title of the language link.
- **`lbldir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.

Examples:

- `api.php?action=query&list=langbacklinks&lbltitle=Test&lbllang=fr` — Get pages linking to :fr:Test.
- `api.php?action=query&generator=langbacklinks&glbltitle=Test&glbllang=fr&prop=info` — Get information about pages linking to :fr:Test.

<a id="mod-query-logevents"></a>
#### `action=query&list=logevents`

*Core* · `MediaWiki\Api\ApiQueryLogEvents` · prefix `le` · GET or POST

Get events from logs.

Parameters:

- **`leprop`** — enum: `comment`, `details`, `ids`, `parsedcomment`, `tags`, `timestamp`, `title`, `type`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `ids|title|type|user|timestamp|comment|details`  
  Which properties to get:
  
  - **ids**: Adds the ID of the log event.
  - **title**: Adds the title of the page for the log event.
  - **type**: Adds the type of log event.
  - **user**: Adds the user responsible for the log event. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: Adds the user ID who was responsible for the log event. If the user has been revision deleted, a `userhidden` property will be returned.
  - **timestamp**: Adds the timestamp for the log event.
  - **comment**: Adds the comment of the log event. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Adds the parsed comment of the log event. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **details**: Lists additional details about the log event. If the log event has been revision deleted, an `actionhidden` property will be returned.
  - **tags**: Lists tags for the log event.
- **`letype`** — enum: ``, `block`, `contentmodel`, `create`, `delete`, `import`, `managetags`, `merge`, `move`, `newusers`, `patrol`, `protect`, `renameuser`, `rights`, `suppress`, `tag`, `upload`  
  Filter log entries to only this type.
- **`leaction`** — enum: `block/block`, `block/reblock`, `block/unblock`, `contentmodel/change`, `contentmodel/new`, `create/create`, `delete/delete`, `delete/delete_redir`, `delete/delete_redir2`, `delete/event`, `delete/restore`, `delete/revision`, `import/interwiki`, `import/upload`, `managetags/activate`, `managetags/create`, `managetags/deactivate`, `managetags/delete`, `merge/merge`, `move/move`, `move/move_redir`, `newusers/autocreate`, `newusers/byemail`, `newusers/create`, `newusers/create2`, `newusers/newusers`, `patrol/autopatrol`, `patrol/patrol`, `protect/modify`, `protect/move_prot`, `protect/protect`, `protect/unprotect`, `renameuser/renameuser`, `rights/autopromote`, `rights/rights`, `suppress/block`, `suppress/delete`, `suppress/event`, `suppress/reblock`, `suppress/revision`, … (44 values total; config-dependent)  
  Filter log actions to only this action. Overrides `letype`. In the list of possible values, values with the asterisk wildcard such as `action/*` can have different strings after the slash (/).
- **`lestart`** — timestamp  
  The timestamp to start enumerating from.
- **`leend`** — timestamp  
  The timestamp to end enumerating.
- **`ledir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: lestart has to be before leend.
  - **older**: List newest first (default). Note: lestart has to be later than leend.
- **`leuser`** — user  
  Filter entries to those made by the given user.
- **`letitle`** — string  
  Filter entries to those related to a page.
- **`lenamespace`** — namespace ID  
  Filter entries to those in the given namespace.
- **`leprefix`** — string  
  Filter entries that start with this prefix.
- **`letag`** — string  
  Only list event entries tagged with this tag.
- **`lelimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total event entries to return.
- **`lecontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=logevents` — List recent log events.

<a id="mod-query-mystashedfiles"></a>
#### `action=query&list=mystashedfiles`

*Core* · `MediaWiki\Api\ApiQueryMyStashedFiles` · prefix `msf` · GET or POST

Get a list of files in the current user's upload stash.

Parameters:

- **`msfprop`** — enum: `size`, `type`; multi-value (max 50; 500 for high-limit users)  
  Which properties to fetch for the files.
  
  - **size**: Fetch the file size and image dimensions.
  - **type**: Fetch the file's MIME type and media type.
- **`msflimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many files to get.
- **`msfcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=mystashedfiles&msfprop=size` — Get the filekey, file size, and pixel size of files in the current user's upload stash.

<a id="mod-query-pagepropnames"></a>
#### `action=query&list=pagepropnames`

*Core* · `MediaWiki\Api\ApiQueryPagePropNames` · prefix `ppn` · GET or POST

List all page property names in use on the wiki.

Parameters:

- **`ppncontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`ppnlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of names to return.

Examples:

- `api.php?action=query&list=pagepropnames` — Get first 10 property names.

<a id="mod-query-pageswithprop"></a>
#### `action=query&list=pageswithprop`

*Core* · `MediaWiki\Api\ApiQueryPagesWithProp` · prefix `pwp` · GET or POST · usable as generator (`generator=pageswithprop`)

List all pages using a given page property.

Parameters:

- **`pwppropname`** — string; **required**  
  Page property for which to enumerate pages (`action=query&list=pagepropnames` returns page property names in use).
- **`pwpprop`** — enum: `ids`, `title`, `value`; multi-value (max 50; 500 for high-limit users); default `ids|title`  
  Which pieces of information to include:
  
  - **ids**: Adds the page ID.
  - **title**: Adds the title and namespace ID of the page.
  - **value**: Adds the value of the page property.
- **`pwpcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`pwplimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of pages to return.
- **`pwpdir`** — enum: `ascending`, `descending`; default `ascending`  
  In which direction to sort.

Examples:

- `api.php?action=query&list=pageswithprop&pwppropname=displaytitle&pwpprop=ids|title|value` — List the first 10 pages using `{{DISPLAYTITLE:}}`.
- `api.php?action=query&generator=pageswithprop&gpwppropname=notoc&prop=info` — Get additional information about the first 10 pages using `__NOTOC__`.

<a id="mod-query-prefixsearch"></a>
#### `action=query&list=prefixsearch`

*Core* · `MediaWiki\Api\ApiQueryPrefixSearch` · prefix `ps` · GET or POST · usable as generator (`generator=prefixsearch`)

Perform a prefix search for page titles.

Despite the similarity in names, this module is not intended to be equivalent to Special:PrefixIndex; for that, see `action=query&list=allpages` with the `apprefix` parameter. The purpose of this module is similar to `action=opensearch`: to take user input and provide the best-matching titles. Depending on the search engine backend, this might include typo correction, redirect avoidance, or other heuristics.

Parameters:

- **`pssearch`** — string; **required**  
  Search string.
- **`psnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Namespaces to search. Ignored if `pssearch` begins with a valid namespace prefix.
- **`pslimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  Maximum number of results to return.
- **`psoffset`** — integer (min 0)  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=prefixsearch&pssearch=meaning` — Search for page titles beginning with `meaning`.

<a id="mod-query-protectedtitles"></a>
#### `action=query&list=protectedtitles`

*Core* · `MediaWiki\Api\ApiQueryProtectedTitles` · prefix `pt` · GET or POST · usable as generator (`generator=protectedtitles`)

List all titles protected from creation.

Parameters:

- **`ptnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only list titles in these namespaces.
- **`ptlevel`** — enum: `autoconfirmed`, `sysop`; multi-value (max 50; 500 for high-limit users)  
  Only list titles with these protection levels.
- **`ptlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.
- **`ptdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: ptstart has to be before ptend.
  - **older**: List newest first (default). Note: ptstart has to be later than ptend.
- **`ptstart`** — timestamp  
  Start listing at this protection timestamp.
- **`ptend`** — timestamp  
  Stop listing at this protection timestamp.
- **`ptprop`** — enum: `comment`, `expiry`, `level`, `parsedcomment`, `timestamp`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `timestamp|level`  
  Which properties to get:
  
  - **timestamp**: Adds the timestamp of when protection was added.
  - **user**: Adds the user that added the protection.
  - **userid**: Adds the user ID that added the protection.
  - **comment**: Adds the comment for the protection.
  - **parsedcomment**: Adds the parsed comment for the protection.
  - **expiry**: Adds the timestamp of when the protection will be lifted.
  - **level**: Adds the protection level.
- **`ptcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=protectedtitles` — List protected titles.
- `api.php?action=query&generator=protectedtitles&gptnamespace=0&prop=linkshere` — Find links to protected titles in the main namespace.

<a id="mod-query-querypage"></a>
#### `action=query&list=querypage`

*Core* · `MediaWiki\Api\ApiQueryQueryPage` · prefix `qp` · GET or POST · usable as generator (`generator=querypage`)

Get a list provided by a QueryPage-based special page.

Parameters:

- **`qppage`** — enum: `Ancientpages`, `BrokenRedirects`, `Deadendpages`, `DoubleRedirects`, `Fewestrevisions`, `ListDuplicatedFiles`, `Listredirects`, `Lonelypages`, `Longpages`, `MediaStatistics`, `Mostcategories`, `Mostimages`, `Mostinterwikis`, `Mostlinked`, `Mostlinkedcategories`, `Mostlinkedtemplates`, `Mostrevisions`, `Shortpages`, `Uncategorizedcategories`, `Uncategorizedimages`, `Uncategorizedpages`, `Uncategorizedtemplates`, `UnconnectedPages`, `Unusedcategories`, `Unusedimages`, `Unusedtemplates`, `Unwatchedpages`, `Wantedcategories`, `Wantedfiles`, `Wantedpages`, `Wantedtemplates`, `Withoutinterwiki`; **required**  
  The name of the special page. Note, this is case-sensitive.
- **`qpoffset`** — integer  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`qplimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  Number of results to return.

Examples:

- `api.php?action=query&list=querypage&qppage=Ancientpages` — Return results from Special:Ancientpages.

<a id="mod-query-random"></a>
#### `action=query&list=random`

*Core* · `MediaWiki\Api\ApiQueryRandom` · prefix `rn` · GET or POST · usable as generator (`generator=random`)

Get a set of random pages.

Pages are listed in a fixed sequence, only the starting point is random. This means that if, for example, `Main Page` is the first random page in the list, `List of fictional monkeys` will always be second, `List of people on stamps of Vanuatu` third, etc.

Parameters:

- **`rnnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Return pages in these namespaces only.
- **`rnfilterredir`** — enum: `all`, `nonredirects`, `redirects`; default `nonredirects`  
  How to filter for redirects.
- **`rnredirect`** — boolean; **deprecated**  
  Use `rnfilterredir=redirects` instead.
- **`rnlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `1`  
  Limit how many random pages will be returned.
- **`rncontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=random&rnnamespace=0&rnlimit=2` — Return two random pages from the main namespace.
- `api.php?action=query&generator=random&grnnamespace=0&grnlimit=2&prop=info` — Return page info about two random pages from the main namespace.

<a id="mod-query-recentchanges"></a>
#### `action=query&list=recentchanges`

*Core* · `MediaWiki\Api\ApiQueryRecentChanges` · prefix `rc` · GET or POST · usable as generator (`generator=recentchanges`)

Enumerate recent changes.

Parameters:

- **`rcstart`** — timestamp  
  The timestamp to start enumerating from.
- **`rcend`** — timestamp  
  The timestamp to end enumerating.
- **`rcdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: rcstart has to be before rcend.
  - **older**: List newest first (default). Note: rcstart has to be later than rcend.
- **`rcnamespace`** — namespace ID; multi-value (max 50; 500 for high-limit users); `*` = all  
  Filter changes to only these namespaces.
- **`rcuser`** — user  
  Only list changes by this user.
- **`rcexcludeuser`** — user  
  Don't list changes by this user.
- **`rctag`** — string  
  Only list changes tagged with this tag.
- **`rcprop`** — enum: `comment`, `flags`, `ids`, `loginfo`, `parsedcomment`, `patrolled`, `redirect`, `sha1`, `sizes`, `tags`, `timestamp`, `title`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `title|timestamp|ids`  
  Include additional pieces of information:
  
  - **user**: Adds the user responsible for the edit and tags if they are an IP. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: Adds the user ID responsible for the edit. If the user has been revision deleted, a `userhidden` property will be returned.
  - **comment**: Adds the comment for the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Adds the parsed comment for the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **flags**: Adds flags for the edit.
  - **timestamp**: Adds timestamp of the edit.
  - **title**: Adds the page title of the edit.
  - **ids**: Adds the page ID, recent changes ID and the new and old revision ID.
  - **sizes**: Adds the new and old page length in bytes.
  - **redirect**: Tags edit if page is a redirect.
  - **patrolled**: Tags patrollable edits as being patrolled or unpatrolled.
  - **loginfo**: Adds log information (log ID, log type, etc) to log entries.
  - **tags**: Lists tags for the entry.
  - **sha1**: Adds the content checksum for entries associated with a revision. If the content has been revision deleted, a `sha1hidden` property will be returned.
- **`rcshow`** — enum: `!anon`, `!autopatrolled`, `!bot`, `!minor`, `!patrolled`, `!redirect`, `anon`, `autopatrolled`, `bot`, `minor`, `patrolled`, `redirect`, `unpatrolled`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria. For example, to see only minor edits done by logged-in users, set rcshow=minor|!anon.
- **`rclimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total changes to return.
- **`rctype`** — enum: `categorize`, `edit`, `external`, `log`, `new`; multi-value (max 50; 500 for high-limit users); default `edit|new|log|categorize`  
  Which types of changes to show.
- **`rctoponly`** — boolean  
  Only list changes which are the latest revision.
- **`rctitle`** — string  
  Filter entries to those related to a page.
- **`rccontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`rcgeneraterevisions`** — boolean  
  When being used as a generator, generate revision IDs rather than titles. Recent change entries without associated revision IDs (e.g. most log entries) will generate nothing.
- **`rcslot`** — enum: `main`  
  Only list changes that touch the named slot.

Examples:

- `api.php?action=query&list=recentchanges` — List recent changes.
- `api.php?action=query&generator=recentchanges&grcshow=!patrolled&prop=info` — Get page info about recent unpatrolled changes.

<a id="mod-query-search"></a>
#### `action=query&list=search`

*Core* · `MediaWiki\Api\ApiQuerySearch` · prefix `sr` · GET or POST · usable as generator (`generator=search`)

Perform a full text search.

Parameters:

- **`srsearch`** — string; **required**  
  Search for page titles or content matching this value. You can use the search string to invoke special search features, depending on what the wiki's search backend implements.
- **`srnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Search only within these namespaces.
- **`srlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total pages to return.
- **`sroffset`** — integer (min 0)  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`srwhat`** — enum: `nearmatch`, `text`, `title`  
  Which type of search to perform.
- **`srinfo`** — enum: `rewrittenquery`, `suggestion`, `totalhits`; multi-value (max 50; 500 for high-limit users); default `totalhits|suggestion|rewrittenquery`  
  Which metadata to return.
- **`srprop`** — enum: `categorysnippet`, `extensiondata`, `isfilematch`, `redirectsnippet`, `redirecttitle`, `sectionsnippet`, `sectiontitle`, `size`, `snippet`, `timestamp`, `titlesnippet`, `wordcount`, `hasrelated`, `score`; multi-value (max 50; 500 for high-limit users); default `size|wordcount|timestamp|snippet`; deprecated values: `hasrelated`, `score`  
  Which properties to return:
  
  - **size**: Adds the size of the page in bytes.
  - **wordcount**: Adds the word count of the page.
  - **timestamp**: Adds the timestamp of when the page was last edited.
  - **snippet**: Adds a snippet of the page, with query term highlighting markup.
  - **titlesnippet**: Adds the page title, with query term highlighting markup.
  - **redirecttitle**: Adds the title of the matching redirect.
  - **redirectsnippet**: Adds the title of the matching redirect, with query term highlighting markup.
  - **sectiontitle**: Adds the title of the matching section.
  - **sectionsnippet**: Adds the title of the matching section, with query term highlighting markup.
  - **isfilematch**: Adds a boolean indicating if the search matched file content.
  - **categorysnippet**: Adds the matching category name, with query term highlighting markup.
  - **score**: Deprecated. Ignored.
  - **hasrelated**: Deprecated. Ignored.
  - **extensiondata**: Adds extra data generated by extensions.
- **`srinterwiki`** — boolean  
  Include interwiki results in the search, if available.
- **`srenablerewrites`** — boolean  
  Enable internal query rewriting. Some search backends can rewrite the query into another which is thought to provide better results, for instance by correcting spelling errors.
- **`srsort`** — enum: `relevance`; default `relevance`  
  Set the sort order of returned results.

Examples:

- `api.php?action=query&list=search&srsearch=meaning` — Search for `meaning`.
- `api.php?action=query&list=search&srwhat=text&srsearch=meaning` — Search texts for `meaning`.
- `api.php?action=query&generator=search&gsrsearch=meaning&prop=info` — Get page info about the pages returned for a search for `meaning`.

<a id="mod-query-tags"></a>
#### `action=query&list=tags`

*Core* · `MediaWiki\Api\ApiQueryTags` · prefix `tg` · GET or POST

List change tags.

Parameters:

- **`tgcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`tglimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of tags to list.
- **`tgprop`** — enum: `active`, `defined`, `description`, `displayname`, `hitcount`, `source`; multi-value (max 50; 500 for high-limit users)  
  Which properties to get:
  
  - **displayname**: Adds the displayed name of the tag. This property will be omitted for hidden tags.
  - **description**: Adds the description of the tag.
  - **hitcount**: Adds the number of revisions and log entries that have this tag.
  - **defined**: Indicate whether the tag is defined (see `source`).
  - **source**: Gets the sources of the tag definition, which may include `software` for software-defined tags and `manual` for tags that may be applied manually by users.
  - **active**: Whether the tag is still being applied by the software, or may still be applied by users.

Examples:

- `api.php?action=query&list=tags&tgprop=displayname|description|hitcount|defined` — List available tags.

<a id="mod-query-usercontribs"></a>
#### `action=query&list=usercontribs`

*Core* · `MediaWiki\Api\ApiQueryUserContribs` · prefix `uc` · GET or POST

Get all edits by a user.

Parameters:

- **`uclimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  The maximum number of contributions to return.
- **`ucstart`** — timestamp  
  The start timestamp to return from, i.e. revisions before this timestamp.
- **`ucend`** — timestamp  
  The end timestamp to return to, i.e. revisions after this timestamp.
- **`uccontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`ucuser`** — user; multi-value (max 50; 500 for high-limit users)  
  The users to retrieve contributions for. Cannot be used with `ucuserids`, `ucuserprefix`, or `uciprange`.
- **`ucuserids`** — integer; multi-value (max 50; 500 for high-limit users)  
  The user IDs to retrieve contributions for. Cannot be used with `ucuser`, `ucuserprefix`, or `uciprange`.
- **`ucuserprefix`** — string  
  Retrieve contributions for all users whose names begin with this value. Cannot be used with `ucuser`, `ucuserids`, or `uciprange`.
- **`uciprange`** — string  
  The CIDR range to retrieve contributions for. Cannot be used with `ucuser`, `ucuserprefix`, or `ucuserids`.
- **`ucdir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: ucstart has to be before ucend.
  - **older**: List newest first (default). Note: ucstart has to be later than ucend.
- **`ucnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only list contributions in these namespaces.
- **`ucprop`** — enum: `comment`, `flags`, `ids`, `parsedcomment`, `patrolled`, `size`, `sizediff`, `tags`, `timestamp`, `title`; multi-value (max 50; 500 for high-limit users); default `ids|title|timestamp|comment|size|flags`  
  Include additional pieces of information:
  
  - **ids**: Adds the page ID and revision ID.
  - **title**: Adds the title and namespace ID of the page.
  - **timestamp**: Adds the timestamp of the edit.
  - **comment**: Adds the comment of the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Adds the parsed comment of the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **size**: Adds the new size of the edit.
  - **sizediff**: Adds the size delta of the edit against its parent.
  - **flags**: Adds flags of the edit.
  - **patrolled**: Tags patrolled edits.
  - **tags**: Lists tags for the edit.
- **`ucshow`** — enum: `!autopatrolled`, `!minor`, `!new`, `!patrolled`, `!top`, `autopatrolled`, `minor`, `new`, `patrolled`, `top`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria, e.g. non minor edits only: `ucshow=!minor`.
  
  If `ucshow=patrolled` or `ucshow=!patrolled` is set, revisions older than `$wgRCMaxAge` (7776000 seconds) won't be shown.
- **`uctag`** — string  
  Only list revisions tagged with this tag.
- **`uctoponly`** — boolean; **deprecated**  
  Only list changes which are the latest revision.

Examples:

- `api.php?action=query&list=usercontribs&ucuser=Example` — Show contributions of user `Example`.
- `api.php?action=query&list=usercontribs&ucuserprefix=192.0.2.` — Show contributions from all IP addresses with prefix `192.0.2.`.

<a id="mod-query-users"></a>
#### `action=query&list=users`

*Core* · `MediaWiki\Api\ApiQueryUsers` · prefix `us` · GET or POST

Get information about a list of users.

Parameters:

- **`usprop`** — enum: `blockinfo`, `cancreate`, `centralids`, `editcount`, `emailable`, `gender`, `groupmemberships`, `groups`, `implicitgroups`, `registration`, `rights`; multi-value (max 50; 500 for high-limit users)  
  Which pieces of information to include:
  
  - **blockinfo**: Tags if the user is blocked, by whom, and for what reason.
  - **groups**: Lists all the groups each user belongs to.
  - **groupmemberships**: Lists groups that each user has been explicitly assigned to, including the expiry date of each group membership.
  - **implicitgroups**: Lists all the groups a user is automatically a member of.
  - **rights**: Lists all the rights each user has.
  - **editcount**: Adds the user's edit count.
  - **registration**: Adds the user's registration timestamp.
  - **emailable**: Tags if the user can and wants to receive email through Special:Emailuser.
  - **gender**: Tags the gender of the user. Returns "male", "female", or "unknown".
  - **centralids**: Adds the central IDs and attachment status for the user.
  - **cancreate**: Indicates whether an account for valid but unregistered usernames can be created. To check whether the current user can perform the account creation, use `action=query&meta=userinfo&uiprop=cancreateaccount`.
- **`usattachedwiki`** — string  
  With `usprop=centralids`, indicate whether the user is attached with the wiki identified by this ID.
- **`ususers`** — string; multi-value (max 50; 500 for high-limit users)  
  A list of users to obtain information for.
- **`ususerids`** — integer; multi-value (max 50; 500 for high-limit users)  
  A list of user IDs to obtain information for.

Examples:

- `api.php?action=query&list=users&ususers=Example&usprop=groups|editcount|gender` — Return information for user `Example`.

<a id="mod-query-watchlist"></a>
#### `action=query&list=watchlist`

*Core* · `MediaWiki\Api\ApiQueryWatchlist` · prefix `wl` · GET or POST · usable as generator (`generator=watchlist`)

Get recent changes to pages in the current user's watchlist.

Parameters:

- **`wlallrev`** — boolean  
  Include multiple revisions of the same page within given timeframe.
- **`wlstart`** — timestamp  
  The timestamp to start enumerating from.
- **`wlend`** — timestamp  
  The timestamp to end enumerating.
- **`wlnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Filter changes to only the given namespaces.
- **`wluser`** — user  
  Only list changes by this user.
- **`wlexcludeuser`** — user  
  Don't list changes by this user.
- **`wldir`** — enum: `newer`, `older`; default `older`  
  In which direction to enumerate:
  
  - **newer**: List oldest first. Note: wlstart has to be before wlend.
  - **older**: List newest first (default). Note: wlstart has to be later than wlend.
- **`wllimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total results to return per request.
- **`wlprop`** — enum: `comment`, `expiry`, `flags`, `ids`, `loginfo`, `notificationtimestamp`, `parsedcomment`, `patrol`, `sizes`, `tags`, `timestamp`, `title`, `user`, `userid`; multi-value (max 50; 500 for high-limit users); default `ids|title|flags`  
  Which additional properties to get:
  
  - **ids**: Adds revision IDs and page IDs.
  - **title**: Adds title of the page.
  - **flags**: Adds flags for the edit.
  - **user**: Adds the user who made the edit. If the user has been revision deleted, a `userhidden` property will be returned.
  - **userid**: Adds user ID of whoever made the edit. If the user has been revision deleted, a `userhidden` property will be returned.
  - **comment**: Adds comment of the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **parsedcomment**: Adds parsed comment of the edit. If the comment has been revision deleted, a `commenthidden` property will be returned.
  - **timestamp**: Adds timestamp of the edit.
  - **patrol**: Tags edits that are patrolled.
  - **sizes**: Adds the old and new lengths of the page.
  - **notificationtimestamp**: Adds timestamp of when the user was last notified about the edit.
  - **loginfo**: Adds log information where appropriate.
  - **tags**: Lists tags for the entry.
  - **expiry**: Adds the expiry time.
- **`wlshow`** — enum: `!anon`, `!autopatrolled`, `!bot`, `!minor`, `!patrolled`, `!unread`, `anon`, `autopatrolled`, `bot`, `minor`, `patrolled`, `unread`; multi-value (max 50; 500 for high-limit users)  
  Show only items that meet these criteria. For example, to see only minor edits done by logged-in users, set wlshow=minor|!anon.
- **`wltype`** — enum: `categorize`, `edit`, `external`, `log`, `new`; multi-value (max 50; 500 for high-limit users); default `edit|new|log|categorize`  
  Which types of changes to show:
  
  - **edit**: Regular page edits.
  - **new**: Page creations.
  - **log**: Log entries.
  - **external**: External changes.
  - **categorize**: Category membership changes.
- **`wlowner`** — user  
  Used along with wltoken to access a different user's watchlist.
- **`wltoken`** — string; sensitive  
  A security token (available in the user's preferences) to allow access to another user's watchlist.
- **`wlcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&list=watchlist` — List the top revision for recently changed pages on the current user's watchlist.
- `api.php?action=query&list=watchlist&wlprop=ids|title|timestamp|user|comment` — Fetch additional information about the top revision for recently changed pages on the current user's watchlist.
- `api.php?action=query&list=watchlist&wlprop=ids|title|timestamp|user|comment|expiry` — Fetch additional information about the top revision for recently changed pages on the current user's watchlist, including when temporarily watched items will expire.
- `api.php?action=query&list=watchlist&wlallrev=&wlprop=ids|title|timestamp|user|comment` — Fetch information about all recent changes to pages on the current user's watchlist.
- `api.php?action=query&generator=watchlist&prop=info` — Fetch page info for recently changed pages on the current user's watchlist.
- `api.php?action=query&generator=watchlist&gwlallrev=&prop=revisions&rvprop=timestamp|user` — Fetch revision info for recent changes to pages on the current user's watchlist.
- `api.php?action=query&list=watchlist&wlowner=Example&wltoken=123ABC` — List the top revision for recently changed pages on the watchlist of user `Example`.

<a id="mod-query-watchlistraw"></a>
#### `action=query&list=watchlistraw`

*Core* · `MediaWiki\Api\ApiQueryWatchlistRaw` · prefix `wr` · GET or POST · usable as generator (`generator=watchlistraw`)

Get all pages on the current user's watchlist.

Parameters:

- **`wrcontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.
- **`wrnamespace`** — namespace; multi-value (max 50; 500 for high-limit users); `*` = all  
  Only list pages in the given namespaces.
- **`wrlimit`** — limit (1–500; 5000 for high-limit users; or `max`); default `10`  
  How many total results to return per request.
- **`wrprop`** — enum: `changed`; multi-value (max 50; 500 for high-limit users)  
  Which additional properties to get:
  
  - **changed**: Adds timestamp of when the user was last notified about the edit.
- **`wrshow`** — enum: `!changed`, `changed`; multi-value (max 50; 500 for high-limit users)  
  Only list items that meet these criteria.
- **`wrowner`** — user  
  Used along with wrtoken to access a different user's watchlist.
- **`wrtoken`** — string; sensitive  
  A security token (available in the user's preferences) to allow access to another user's watchlist.
- **`wrdir`** — enum: `ascending`, `descending`; default `ascending`  
  The direction in which to list.
- **`wrfromtitle`** — string  
  Title (with namespace prefix) to begin enumerating from.
- **`wrtotitle`** — string  
  Title (with namespace prefix) to stop enumerating at.

Examples:

- `api.php?action=query&list=watchlistraw` — List pages on the current user's watchlist.
- `api.php?action=query&generator=watchlistraw&gwrshow=changed&prop=info` — Fetch page info for pages on the current user's watchlist.

### 5.4 Core `meta` modules

<a id="mod-query-allmessages"></a>
#### `action=query&meta=allmessages`

*Core* · `MediaWiki\Api\ApiQueryAllMessages` · prefix `am` · GET or POST

Return messages from this site.

Parameters:

- **`ammessages`** — string; multi-value (max 50; 500 for high-limit users); default `*`  
  Which messages to output. `*` (default) means all messages.
- **`amprop`** — enum: `default`; multi-value (max 50; 500 for high-limit users)  
  Which properties to get.
- **`amenableparser`** — boolean  
  Set to enable parser, will preprocess the wikitext of message (substitute magic words, handle templates, etc.).
- **`amnocontent`** — boolean  
  If set, do not include the content of the messages in the output.
- **`amincludelocal`** — boolean  
  Also include local messages, i.e. messages that don't exist in the software but do exist as in the MediaWiki namespace.
  This lists all MediaWiki-namespace pages, so it will also list those that aren't really messages such as Common.js.
- **`amargs`** — string; multi-value (max 50; 500 for high-limit users); duplicates allowed  
  Arguments to be substituted into message.
- **`amfilter`** — string  
  Return only messages with names that contain this string.
- **`amcustomised`** — enum: `all`, `modified`, `unmodified`; default `all`  
  Return only messages in this customisation state.
- **`amlang`** — string  
  Return messages in this language.
- **`amfrom`** — string  
  Return messages starting at this message.
- **`amto`** — string  
  Return messages ending at this message.
- **`amtitle`** — string  
  Page name to use as context when parsing message (for amenableparser option).
- **`amprefix`** — string  
  Return messages with this prefix.

Examples:

- `api.php?action=query&meta=allmessages&amprefix=ipb-` — Show messages starting with `ipb-`.
- `api.php?action=query&meta=allmessages&ammessages=august|mainpage&amlang=de` — Show messages `august` and `mainpage` in German.

<a id="mod-query-authmanagerinfo"></a>
#### `action=query&meta=authmanagerinfo`

*Core* · `MediaWiki\Api\ApiQueryAuthManagerInfo` · prefix `ami` · GET or POST

Retrieve information about the current authentication status.

Parameters:

- **`amisecuritysensitiveoperation`** — string  
  Test whether the user's current authentication status is sufficient for the specified security-sensitive operation.
- **`amirequestsfor`** — enum: `change`, `create`, `create-continue`, `link`, `link-continue`, `login`, `login-continue`, `remove`, `unlink`  
  Fetch information about the authentication requests needed for the specified authentication action.
- **`amimergerequestfields`** — boolean  
  Merge field information for all authentication requests into one array.
- **`amimessageformat`** — enum: `html`, `none`, `raw`, `wikitext`; default `wikitext`  
  Format to use for returning messages.

Examples:

- `api.php?action=query&meta=authmanagerinfo&amirequestsfor=login` — Fetch the requests that may be used when beginning a login.
- `api.php?action=query&meta=authmanagerinfo&amirequestsfor=login&amimergerequestfields=1` — Fetch the requests that may be used when beginning a login, with form fields merged.
- `api.php?action=query&meta=authmanagerinfo&amisecuritysensitiveoperation=foo` — Test whether authentication is sufficient for action `foo`.

<a id="mod-query-filerepoinfo"></a>
#### `action=query&meta=filerepoinfo`

*Core* · `MediaWiki\Api\ApiQueryFileRepoInfo` · prefix `fri` · GET or POST

Return meta information about image repositories configured on the wiki.

Parameters:

- **`friprop`** — enum: `canUpload`, `displayname`, `favicon`, `initialCapital`, `local`, `name`, `rootUrl`, `scriptDirUrl`, `thumbUrl`, `url`; multi-value (max 50; 500 for high-limit users); default `canUpload|displayname|favicon|initialCapital|local|name|rootUrl|scriptDirUrl|thumbUrl|url`  
  Which repository properties to get (properties available may vary on other wikis).
  
  - **canUpload**: Whether files can be uploaded to this repository, e.g. via CORS and shared authentication.
  - **displayname**: The human-readable name of the repository wiki.
  - **favicon**: Repository wiki's favicon URL, from `$wgFavicon`.
  - **initialCapital**: Whether file names implicitly start with a capital letter.
  - **local**: Whether that repository is the local one or not.
  - **name**: The key of the repository - used in e.g. `$wgForeignFileRepos` and imageinfo return values.
  - **rootUrl**: Root URL path for image paths.
  - **scriptDirUrl**: Root URL path for the repository wiki's MediaWiki installation.
  - **thumbUrl**: Root URL path for thumbnail paths.
  - **url**: Public zone URL path.

Examples:

- `api.php?action=query&meta=filerepoinfo&friprop=name|displayname` — Get information about file repositories.

<a id="mod-query-languageinfo"></a>
#### `action=query&meta=languageinfo`

*Core* · `MediaWiki\Api\ApiQueryLanguageinfo` · prefix `li` · GET or POST

Return information about available languages.

Continuation may be applied if retrieving the information takes too long for one request.

Parameters:

- **`liprop`** — enum: `autonym`, `bcp47`, `code`, `dir`, `fallbacks`, `name`, `variantnames`, `variants`; multi-value (max 50; 500 for high-limit users); default `code`  
  Which information to get for each language.
  
  - **code**: The language code. (This code is MediaWiki-specific, though there are overlaps with other standards.)
  - **bcp47**: The BCP-47 language code.
  - **dir**: The writing direction of the language (either `ltr` or `rtl`).
  - **autonym**: The autonym of the language, that is, the name in that language.
  - **name**: The name of the language in the language specified by the `uselang` parameter, with language fallbacks applied if necessary.
  - **variantnames**: The short names for language variants used for language conversion links.
  - **fallbacks**: The language codes of the fallback languages configured for this language. The implicit final fallback to 'en' is not included (but some languages may fall back to 'en' explicitly).
  - **variants**: The language codes of the variants supported by this language.
- **`licode`** — string; multi-value (max 50; 500 for high-limit users); default `*`  
  Language codes of the languages that should be returned, or `*` for all languages.
- **`licontinue`** — string  
  When more results are available, use this to continue. More detailed information on how to continue queries can be found on mediawiki.org.

Examples:

- `api.php?action=query&meta=languageinfo` — Get the language codes of all supported languages.
- `api.php?action=query&meta=languageinfo&liprop=autonym|name&uselang=de` — Get the autonyms and German names of all supported languages.
- `api.php?action=query&meta=languageinfo&liprop=fallbacks|variants&licode=oc` — Get the fallback languages and variants of Occitan.
- `api.php?action=query&meta=languageinfo&liprop=bcp47|dir` — Get the BCP-47 language code and direction of all supported languages.

<a id="mod-query-siteinfo"></a>
#### `action=query&meta=siteinfo`

*Core* · `MediaWiki\Api\ApiQuerySiteinfo` · prefix `si` · GET or POST

Return general information about the site.

Parameters:

- **`siprop`** — enum: `autocreatetempuser`, `autopromote`, `autopromoteonce`, `clientlibraries`, `dbrepllag`, `defaultoptions`, `extensions`, `extensiontags`, `fileextensions`, `functionhooks`, `general`, `interwikimap`, `languages`, `languagevariants`, `libraries`, `magicwords`, `namespacealiases`, `namespaces`, `protocols`, `restrictions`, `rightsinfo`, `showhooks`, `skins`, `specialpagealiases`, `statistics`, `uploaddialog`, `usergroups`, `variables`; multi-value (max 50; 500 for high-limit users); default `general`  
  Which information to get:
  
  - **general**: Overall system information.
  - **namespaces**: List of registered namespaces and their canonical names.
  - **namespacealiases**: List of registered namespace aliases.
  - **specialpagealiases**: List of special page aliases.
  - **magicwords**: List of magic words and their aliases.
  - **interwikimap**: Returns interwiki map (optionally filtered, optionally localised by using `siinlanguagecode`).
  - **dbrepllag**: Returns database server with the highest replication lag.
  - **statistics**: Returns site statistics.
  - **usergroups**: Returns user groups and the associated permissions.
  - **autocreatetempuser**: Returns configuration for the automatic creation of temporary user accounts (also known as IP masking).
  - **clientlibraries**: Returns client-side libraries installed on the wiki
  - **libraries**: Returns libraries installed on the wiki.
  - **extensions**: Returns extensions installed on the wiki.
  - **fileextensions**: Returns list of file extensions (file types) allowed to be uploaded.
  - **rightsinfo**: Returns wiki rights (license) information if available.
  - **restrictions**: Returns information on available restriction (protection) types.
  - **languages**: Returns a list of languages MediaWiki supports (optionally localised by using `siinlanguagecode`).
  - **languagevariants**: Returns a list of language codes for which LanguageConverter is enabled, and the variants supported for each.
  - **skins**: Returns a list of all enabled skins (optionally localised by using `siinlanguagecode`, otherwise in the content language).
  - **extensiontags**: Returns a list of parser extension tags.
  - **functionhooks**: Returns a list of parser function hooks.
  - **showhooks**: Returns a list of all subscribed hooks (contents of `$wgHooks`).
  - **variables**: Returns a list of variable IDs.
  - **protocols**: Returns a list of protocols that are allowed in external links.
  - **defaultoptions**: Returns the default values for user preferences.
  - **uploaddialog**: Returns the upload dialog configuration.
  - **autopromote**: Returns the automatic promotion configuration.
  - **autopromoteonce**: Returns the automatic promotion configuration that are only done once.
- **`sifilteriw`** — enum: `!local`, `local`  
  Return only local or only nonlocal entries of the interwiki map.
- **`sishowalldb`** — boolean  
  List all database servers, not just the one lagging the most.
- **`sinumberingroup`** — boolean  
  Lists the number of users in user groups.
- **`siinlanguagecode`** — string  
  Language code for localised language names (best effort) and skin names.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=general|namespaces|namespacealiases|statistics` — Fetch site information.
- `api.php?action=query&meta=siteinfo&siprop=interwikimap&sifilteriw=local` — Fetch a list of local interwiki prefixes.
- `api.php?action=query&meta=siteinfo&siprop=dbrepllag&sishowalldb=` — Check the current replication lag.

<a id="mod-query-tokens"></a>
#### `action=query&meta=tokens`

*Core* · `MediaWiki\Api\ApiQueryTokens` · GET or POST

Gets tokens for data-modifying actions.

Parameters:

- **`type`** — enum: `createaccount`, `csrf`, `login`, `patrol`, `rollback`, `userrights`, `watch`; multi-value (max 50; 500 for high-limit users); `*` = all; default `csrf`  
  Types of token to request.

Examples:

- `api.php?action=query&meta=tokens` — Retrieve a csrf token (the default).
- `api.php?action=query&meta=tokens&type=watch|patrol` — Retrieve a watch token and a patrol token.

<a id="mod-query-userinfo"></a>
#### `action=query&meta=userinfo`

*Core* · `MediaWiki\Api\ApiQueryUserInfo` · prefix `ui` · GET or POST

Get information about the current user.

Parameters:

- **`uiprop`** — enum: `acceptlang`, `blockinfo`, `cancreateaccount`, `centralids`, `changeablegroups`, `editcount`, `email`, `groupmemberships`, `groups`, `hasmsg`, `implicitgroups`, `latestcontrib`, `options`, `ratelimits`, `realname`, `registrationdate`, `rights`, `theoreticalratelimits`, `unreadcount`; multi-value (max 50; 500 for high-limit users); `*` = all  
  Which pieces of information to include:
  
  - **blockinfo**: Tags if the current user is blocked, by whom, and for what reason.
  - **hasmsg**: Adds a tag `messages` if the current user has pending messages.
  - **groups**: Lists all the groups the current user belongs to.
  - **groupmemberships**: Lists groups that the current user has been explicitly assigned to, including the expiry date of each group membership.
  - **implicitgroups**: Lists all the groups the current user is automatically a member of.
  - **rights**: Lists all the rights the current user has.
  - **changeablegroups**: Lists the groups the current user can add to and remove from.
  - **options**: Lists all preferences the current user has set.
  - **editcount**: Adds the current user's edit count.
  - **ratelimits**: Lists all rate limits applying to the current user.
  - **theoreticalratelimits**: Lists all rate limits that would apply to the current user if they were not exempt from all ratelimits based on user rights or ip
  - **email**: Adds the user's email address and email authentication date.
  - **realname**: Adds the user's real name.
  - **acceptlang**: Echoes the `Accept-Language` header sent by the client in a structured format.
  - **registrationdate**: Adds the user's registration date.
  - **unreadcount**: Adds the count of unread pages on the user's watchlist (maximum 999; returns `1000+` if more).
  - **centralids**: Adds the central IDs and attachment status for the user.
  - **latestcontrib**: Adds the date of user's latest contribution.
  - **cancreateaccount**: Indicates whether the user is allowed to create accounts. To check whether some specific account can be created, use `action=query&list=users&usprop=cancreate`.
- **`uiattachedwiki`** — string  
  With `uiprop=centralids`, indicate whether the user is attached with the wiki identified by this ID.

Examples:

- `api.php?action=query&meta=userinfo` — Get information about the current user.
- `api.php?action=query&meta=userinfo&uiprop=blockinfo|groups|rights|hasmsg` — Get additional information about the current user.

### 5.5 Output formats

<a id="mod-json"></a>
#### `format=json`

*Core* · `MediaWiki\Api\ApiFormatJson` · GET or POST

Output data in JSON format.

Parameters:

- **`callback`** — string  
  If specified, wraps the output into a given function call. For safety, all user-specific data will be restricted.
- **`utf8`** — boolean  
  If specified, encodes most (but not all) non-ASCII characters as UTF-8 instead of replacing them with hexadecimal escape sequences. Default when `formatversion` is not `1`.
- **`ascii`** — boolean  
  If specified, encodes all non-ASCII using hexadecimal escape sequences. Default when `formatversion` is `1`.
- **`formatversion`** — enum: `1`, `2`, `latest`; default `1`  
  Output formatting
  
  - **1**: Backwards-compatible format (XML-style booleans, `*` keys for content nodes, etc.).
  - **2**: Modern format.
  - **latest**: Use the latest format (currently `2`), may change without warning.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=json` — Return the query result in the JSON format.

<a id="mod-jsonfm"></a>
#### `format=jsonfm`

*Core* · `MediaWiki\Api\ApiFormatJson` · GET or POST

Output data in JSON format (pretty-print in HTML).

Parameters:

- **`wrappedhtml`** — boolean  
  Return the pretty-printed HTML and associated ResourceLoader modules as a JSON object.
- **`callback`** — string  
  If specified, wraps the output into a given function call. For safety, all user-specific data will be restricted.
- **`utf8`** — boolean  
  If specified, encodes most (but not all) non-ASCII characters as UTF-8 instead of replacing them with hexadecimal escape sequences. Default when `formatversion` is not `1`.
- **`ascii`** — boolean  
  If specified, encodes all non-ASCII using hexadecimal escape sequences. Default when `formatversion` is `1`.
- **`formatversion`** — enum: `1`, `2`, `latest`; default `1`  
  Output formatting
  
  - **1**: Backwards-compatible format (XML-style booleans, `*` keys for content nodes, etc.).
  - **2**: Modern format.
  - **latest**: Use the latest format (currently `2`), may change without warning.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=jsonfm` — Return the query result in the JSON format.

<a id="mod-none"></a>
#### `format=none`

*Core* · `MediaWiki\Api\ApiFormatNone` · GET or POST

Output nothing.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=none` — Return the query result in the NONE format.

<a id="mod-php"></a>
#### `format=php`

*Core* · `MediaWiki\Api\ApiFormatPhp` · GET or POST

Output data in serialized PHP format.

Parameters:

- **`formatversion`** — enum: `1`, `2`, `latest`; default `1`  
  Output formatting
  
  - **1**: Backwards-compatible format (XML-style booleans, `*` keys for content nodes, etc.).
  - **2**: Modern format.
  - **latest**: Use the latest format (currently `2`), may change without warning.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=php` — Return the query result in the PHP format.

<a id="mod-phpfm"></a>
#### `format=phpfm`

*Core* · `MediaWiki\Api\ApiFormatPhp` · GET or POST

Output data in serialized PHP format (pretty-print in HTML).

Parameters:

- **`wrappedhtml`** — boolean  
  Return the pretty-printed HTML and associated ResourceLoader modules as a JSON object.
- **`formatversion`** — enum: `1`, `2`, `latest`; default `1`  
  Output formatting
  
  - **1**: Backwards-compatible format (XML-style booleans, `*` keys for content nodes, etc.).
  - **2**: Modern format.
  - **latest**: Use the latest format (currently `2`), may change without warning.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=phpfm` — Return the query result in the PHP format.

<a id="mod-rawfm"></a>
#### `format=rawfm`

*Core* · `MediaWiki\Api\ApiFormatJson` · GET or POST

Output data, including debugging elements, in JSON format (pretty-print in HTML).

Parameters:

- **`wrappedhtml`** — boolean  
  Return the pretty-printed HTML and associated ResourceLoader modules as a JSON object.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=rawfm` — Return the query result in the RAW format.

<a id="mod-xml"></a>
#### `format=xml`

*Core* · `MediaWiki\Api\ApiFormatXml` · GET or POST

Output data in XML format.

Parameters:

- **`xslt`** — string  
  If specified, adds the named page as an XSL stylesheet. The value must be a title in the MediaWiki namespace ending in `.xsl`.
- **`includexmlnamespace`** — boolean  
  If specified, adds an XML namespace.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=xml` — Return the query result in the XML format.

<a id="mod-xmlfm"></a>
#### `format=xmlfm`

*Core* · `MediaWiki\Api\ApiFormatXml` · GET or POST

Output data in XML format (pretty-print in HTML).

Parameters:

- **`wrappedhtml`** — boolean  
  Return the pretty-printed HTML and associated ResourceLoader modules as a JSON object.
- **`xslt`** — string  
  If specified, adds the named page as an XSL stylesheet. The value must be a title in the MediaWiki namespace ending in `.xsl`.
- **`includexmlnamespace`** — boolean  
  If specified, adds an XML namespace.

Examples:

- `api.php?action=query&meta=siteinfo&siprop=namespaces&format=xmlfm` — Return the query result in the XML format.

## 6. REST APIs

MediaWiki 1.43 also ships a second API family, served from `{$wgScriptPath}/rest.php`. REST routes return plain JSON bodies and use HTTP status codes for errors. Unlike the Action API, they do not use an envelope. Two REST modules are relevant here.


### 6.1 MediaWiki core REST API (`rest.php/v1`)

Routes registered by default in 1.43 (`includes/Rest/coreRoutes.json`).

| Method | Path | Handler |
|---|---|---|
| GET | `/v1/page/{title}/history` | `PageHistoryHandler` |
| GET | `/v1/page/{title}/history/counts/{type}` | `PageHistoryCountHandler` |
| GET | `/v1/revision/{from}/compare/{to}` | `CompareHandler` |
| GET | `/v1/revision/{id}` | `RevisionSourceHandler` |
| GET | `/v1/revision/{id}/html` | `RevisionHTMLHandler` |
| GET | `/v1/revision/{id}/with_html` | `RevisionHTMLHandler` |
| GET | `/v1/revision/{id}/bare` | `RevisionSourceHandler` |
| GET | `/v1/search` | `OpenSearchDescriptionHandler` |
| GET | `/v1/search/page` | `SearchHandler` |
| GET | `/v1/search/title` | `SearchHandler` |
| GET | `/v1/page/{title}/links/language` | `LanguageLinksHandler` |
| GET | `/v1/page/{title}` | `PageSourceHandler` |
| GET | `/v1/page/{title}/bare` | `PageSourceHandler` |
| GET | `/v1/page/{title}/html` | `PageHTMLHandler` |
| GET | `/v1/page/{title}/with_html` | `PageHTMLHandler` |
| GET | `/v1/page/{title}/links/media` | `MediaLinksHandler` |
| GET | `/v1/file/{title}` | `MediaFileHandler` |
| PUT | `/v1/page/{title}` | `UpdateHandler` |
| POST | `/v1/page` | `CreationHandler` |
| POST | `/v1/transform/{from}/to/{format}/{title}/{revision}` | `TransformHandler` |
| POST | `/v1/transform/{from}/to/{format}` | `TransformHandler` |
| POST | `/v1/transform/{from}/to/{format}/{title}` | `TransformHandler` |

Notes on the core REST API:

- **Titles in paths.** `{title}` is a URL-encoded page title in DB-key or text form.
- **Page response variants.** `…/bare` returns page metadata. The plain route returns the metadata plus `source` (wikitext). `…/with_html` and `…/html` return Parsoid HTML. **(observed)** `GET /v1/page/Main_Page/bare` returns `{"id", "key", "title", "latest": {"id", "timestamp"}, "content_model", "license", "html_url"}`.
- **Writes and authentication.** `PUT /v1/page/{title}` and `POST /v1/page` need an authenticated or anonymous-permitted session. When cookie-based sessions are used, the body must include a CSRF `token`. `latest.id` in the body is used for edit-conflict detection.
- **Opt-in route files.** 1.43 also contains `content.v1.json`, `specs.v0.json` (OpenAPI discovery) and `coreDevelopmentRoutes.json`. These are loaded **only** by `DevelopmentSettings.php` (`$wgRestAPIAdditionalRouteFiles`), are experimental, and are not part of the default contract. Core REST has no OpenAPI document in a default 1.43 install.


### 6.2 Wikibase REST API (`rest.php/wikibase/v1`)

OpenAPI `3.1.0` document, API version `1.1`, served at `rest.php/wikibase/v1/openapi.json`; a copy is committed as `docs/samples/wikibase-1.43/wikibase-rest-v1.openapi.json` and is the normative contract for request/response schemas. Paths below are relative to `rest.php/wikibase/v1`.

Conventions of the Wikibase REST API. Each point is from the OpenAPI document unless marked **(observed)**.

- **Two data formats for one model.** The REST API uses a different JSON shape from the Action API. Statements are under `statements`, not `claims`. Terms are flat maps (`{"en": "Douglas Adams"}`), not `{"en": {"language": "en", "value": …}}`. Sitelinks carry a `url`. The Wikibase tree documents the differences in `repo/rest-api/docs/data_format_differences.md`.
- **Revisions and caching.**
  - `ETag` is the entity's revision ID, and `Last-Modified` is that revision's timestamp **(observed: `ETag: "8"`)**.
  - Reads honour `If-None-Match` and `If-Modified-Since`, returning 304.
  - Writes honour `If-Match` and `If-Unmodified-Since`. A failed precondition returns **412 with an empty body** **(observed)**.
- **Edit metadata.** Write bodies may include `comment`, `tags` (an array) and `bot` (a boolean), alongside the payload.
  - `PATCH` bodies are `application/json-patch+json` with `{"patch": [RFC 6902 operations], …}`.
  - `PUT` and `POST` bodies are `application/json`.
- **Write status codes.** Creates return `201`, and replace or patch operations return `200` with the new representation **(observed: `PUT …/labels/es` returned `201 Created` and the body `"Douglas Adams"`)**.
- **Errors.** The body is `{"code", "message", "context"?}` with codes such as `resource-not-found`, `invalid-path-parameter`, `invalid-value`, `patch-test-failed`, `permission-denied`, `data-policy-violation` and `request-limit-reached`. **(observed)** `GET /entities/items/Q404` returns 404 `{"code": "resource-not-found", "message": "The requested resource does not exist", "context": {"resource_type": "item"}}`.
- **Redirects.** Requests for a redirected item return `308` to the target.
- **Authentication.**
  - OAuth bearer tokens are sent in the `Authorization` header, which needs the OAuth extension.
  - Cookie sessions are also accepted.
  - Anonymous writes succeed without a token when `$wgGroupPermissions['*']['edit']` allows it **(observed)**.
- **Scope.** The API covers items and properties only. `wikibase/v1` has no search or lexeme routes in REL1_43.


| Method | Path | Operation | Summary | Success | Error statuses |
|---|---|---|---|---|---|
| GET | `/openapi.json` | `getOpenApiDoc` | Retrieve the OpenAPI document | 200 |  |
| GET | `/property-data-types` | `getPropertyDataTypes` | Retrieve the map of Property data types to value types | 200 |  |
| POST | `/entities/items` | `addItem` | Create a Wikibase Item | 201 | 400, 403, 422, 429, 500 |
| GET | `/entities/items/{item_id}` | `getItem` | Retrieve a single Wikibase Item by ID | 200 | 304, 308, 400, 404, 412, 500 |
| PATCH | `/entities/items/{item_id}` | `patchItem` | Change a single Wikibase Item by ID | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/sitelinks` | `getSitelinks` | Retrieve an Item's Sitelinks | 200 | 304, 308, 400, 404, 412, 500 |
| PATCH | `/entities/items/{item_id}/sitelinks` | `patchSitelinks` | Change an Item's Sitelinks | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/sitelinks/{site_id}` | `getSitelink` | Retrieve an Item's Sitelink | 200 | 304, 308, 400, 404, 412, 500 |
| PUT | `/entities/items/{item_id}/sitelinks/{site_id}` | `setSitelink` | Add / Replace an Item's Sitelink | 200, 201 | 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/entities/items/{item_id}/sitelinks/{site_id}` | `deleteSitelink` | Delete an Item's Sitelink | 200 | 400, 403, 404, 409, 412, 429, 500 |
| POST | `/entities/properties` | `addProperty` | Create a Wikibase Property | 201 | 400, 403, 422, 429, 500 |
| GET | `/entities/properties/{property_id}` | `getProperty` | Retrieve a single Wikibase Property by ID | 200 | 400, 404, 500 |
| PATCH | `/entities/properties/{property_id}` | `patchProperty` | Change a single Wikibase Property by ID | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/descriptions` | `getItemDescriptions` | Retrieve an Item's descriptions | 200 | 304, 308, 400, 404, 412, 500 |
| PATCH | `/entities/items/{item_id}/descriptions` | `patchItemDescriptions` | Change an Item's descriptions | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/properties/{property_id}/descriptions` | `getPropertyDescriptions` | Retrieve a Property's descriptions | 200 | 304, 400, 404, 412, 500 |
| PATCH | `/entities/properties/{property_id}/descriptions` | `patchPropertyDescriptions` | Change a Property's descriptions | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/descriptions/{language_code}` | `getItemDescription` | Retrieve an Item's description in a specific language | 200 | 304, 308, 400, 404, 412, 500 |
| PUT | `/entities/items/{item_id}/descriptions/{language_code}` | `replaceItemDescription` | Add / Replace an Item's description in a specific language | 200, 201 | 304, 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/entities/items/{item_id}/descriptions/{language_code}` | `deleteItemDescription` | Delete an Item's description in a specific language | 200 | 400, 403, 404, 409, 412, 429, 500 |
| GET | `/entities/items/{item_id}/descriptions_with_language_fallback/{language_code}` | `getItemDescriptionWithFallback` | Retrieve an Item's description in a specific language, with language fallback | 200 | 304, 307, 308, 400, 404, 412, 500 |
| GET | `/entities/properties/{property_id}/descriptions/{language_code}` | `getPropertyDescription` | Retrieve a Property's description in a specific language | 200 | 304, 400, 404, 412, 500 |
| PUT | `/entities/properties/{property_id}/descriptions/{language_code}` | `setPropertyDescription` | Add / Replace a Property's description in a specific language | 200, 201 | 304, 400, 403, 404, 412, 422, 429, 500 |
| DELETE | `/entities/properties/{property_id}/descriptions/{language_code}` | `deletePropertyDescription` | Delete a Property's description in a specific language | 200 | 400, 403, 404, 412, 429, 500 |
| GET | `/entities/properties/{property_id}/descriptions_with_language_fallback/{language_code}` | `getPropertyDescriptionWithFallback` | Retrieve a Property's description in a specific language, with language fallback | 200 | 304, 307, 400, 404, 412, 500 |
| GET | `/entities/items/{item_id}/statements` | `getItemStatements` | Retrieve Statements from an Item | 200 | 304, 308, 400, 404, 412, 500 |
| POST | `/entities/items/{item_id}/statements` | `addItemStatement` | Add a new Statement to an Item | 201 | 400, 403, 404, 409, 412, 429, 500 |
| GET | `/entities/items/{item_id}/statements/{statement_id}` | `getItemStatement` | Retrieve a single Statement from an Item | 200 | 304, 400, 404, 412, 500 |
| PUT | `/entities/items/{item_id}/statements/{statement_id}` | `replaceItemStatement` | Replace a single Statement of an Item | 200 | 400, 403, 404, 412, 429, 500 |
| PATCH | `/entities/items/{item_id}/statements/{statement_id}` | `patchItemStatement` | Change elements of a single Statement of an Item | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/entities/items/{item_id}/statements/{statement_id}` | `deleteItemStatement` | Delete a single Statement from an Item | 200 | 400, 403, 404, 412, 429, 500 |
| GET | `/entities/items/{item_id}/labels` | `getItemLabels` | Retrieve an Item's labels | 200 | 304, 308, 400, 404, 412, 500 |
| PATCH | `/entities/items/{item_id}/labels` | `patchItemLabels` | Change an Item's Labels | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/properties/{property_id}/labels` | `getPropertyLabels` | Retrieve a Property's labels | 200 | 304, 400, 404, 412, 500 |
| PATCH | `/entities/properties/{property_id}/labels` | `patchPropertyLabels` | Change a Property's Labels | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/labels/{language_code}` | `getItemLabel` | Retrieve an Item's label in a specific language | 200 | 304, 308, 400, 404, 412, 500 |
| PUT | `/entities/items/{item_id}/labels/{language_code}` | `replaceItemLabel` | Add / Replace an Item's label in a specific language | 200, 201 | 304, 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/entities/items/{item_id}/labels/{language_code}` | `deleteItemLabel` | Delete an Item's label in a specific language | 200 | 400, 403, 404, 409, 412, 429, 500 |
| GET | `/entities/items/{item_id}/labels_with_language_fallback/{language_code}` | `getItemLabelWithFallback` | Retrieve an Item's label in a specific language, with language fallback | 200 | 304, 307, 308, 400, 404, 412, 500 |
| GET | `/entities/properties/{property_id}/labels/{language_code}` | `getPropertyLabel` | Retrieve a Property's label in a specific language | 200 | 304, 400, 404, 412, 500 |
| PUT | `/entities/properties/{property_id}/labels/{language_code}` | `replacePropertyLabel` | Add / Replace a Property's label in a specific language | 200, 201 | 304, 400, 403, 404, 412, 422, 429, 500 |
| DELETE | `/entities/properties/{property_id}/labels/{language_code}` | `deletePropertyLabel` | Delete a Property's label in a specific language | 200 | 400, 403, 404, 412, 429, 500 |
| GET | `/entities/properties/{property_id}/labels_with_language_fallback/{language_code}` | `getPropertyLabelWithFallback` | Retrieve a Property's label in a specific language, with language fallback | 200 | 304, 307, 400, 404, 412, 500 |
| GET | `/entities/items/{item_id}/aliases` | `getItemAliases` | Retrieve an Item's aliases | 200 | 304, 308, 400, 404, 412, 500 |
| PATCH | `/entities/items/{item_id}/aliases` | `patchItemAliases` | Change an Item's aliases | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/properties/{property_id}/aliases` | `getPropertyAliases` | Retrieve a Property's aliases | 200 | 304, 400, 404, 412, 500 |
| PATCH | `/entities/properties/{property_id}/aliases` | `patchPropertyAliases` | Change a Property's aliases | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| GET | `/entities/items/{item_id}/aliases/{language_code}` | `getItemAliasesInLanguage` | Retrieve an Item's aliases in a specific language | 200 | 304, 308, 400, 404, 412, 500 |
| POST | `/entities/items/{item_id}/aliases/{language_code}` | `addItemAliasesInLanguage` | Create / Add an Item's aliases in a specific language | 200, 201 | 304, 400, 403, 404, 409, 412, 429, 500 |
| GET | `/entities/properties/{property_id}/aliases/{language_code}` | `getPropertyAliasesInLanguage` | Retrieve a Property's aliases in a specific language | 200 | 304, 400, 404, 412, 500 |
| POST | `/entities/properties/{property_id}/aliases/{language_code}` | `addPropertyAliasesInLanguage` | Create / Add a Property's aliases in a specific language | 200, 201 | 304, 400, 403, 404, 412, 429, 500 |
| GET | `/entities/properties/{property_id}/statements` | `getPropertyStatements` | Retrieve Statements from a Property | 200 | 304, 400, 404, 412, 500 |
| POST | `/entities/properties/{property_id}/statements` | `addPropertyStatement` | Add a new Statement to a Property | 201 | 400, 403, 404, 412, 429, 500 |
| GET | `/entities/properties/{property_id}/statements/{statement_id}` | `getPropertyStatement` | Retrieve a single Statement from a Property | 200 | 304, 400, 404, 412, 500 |
| PUT | `/entities/properties/{property_id}/statements/{statement_id}` | `replacePropertyStatement` | Replace a single Statement of a Property | 200 | 400, 403, 404, 412, 429, 500 |
| PATCH | `/entities/properties/{property_id}/statements/{statement_id}` | `patchPropertyStatement` | Change elements of a single Statement of a Property | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/entities/properties/{property_id}/statements/{statement_id}` | `deletePropertyStatement` | Delete a single Statement from a Property | 200 | 400, 403, 404, 412, 429, 500 |
| GET | `/statements/{statement_id}` | `getStatement` | Retrieve a single Statement | 200 | 304, 400, 404, 412, 500 |
| PUT | `/statements/{statement_id}` | `replaceStatement` | Replace a single Statement | 200 | 400, 403, 404, 412, 429, 500 |
| PATCH | `/statements/{statement_id}` | `patchStatement` | Change elements of a single Statement | 200 | 400, 403, 404, 409, 412, 422, 429, 500 |
| DELETE | `/statements/{statement_id}` | `deleteStatement` | Delete a single Statement | 200 | 400, 403, 404, 412, 429, 500 |

## 7. Not covered by this contract

These are adjacent interfaces that MediaWiki and Wikibase clients also depend on. They should each get their own contract document if Triplespace targets them.

- **`Special:EntityData/<id>.{json,ttl,rdf,nt,jsonld,php}`.** Per-entity linked-data export, and the canonical dereferencing target for concept URIs (`{concept base URI}/entity/Q1` → `Special:EntityData`).
- **RDF dumps and the RDF mapping.** The WDQS/QLever ingestion format, described in Wikibase `docs/topics/rdf-binding.md`.
- **JSON dumps.** `dumpJson.php`, one entity serialization per line.
- **`index.php` endpoints.** `action=raw`, `action=history` and HTML page views.
- **Recent changes feeds and EventStreams.** How downstream mirrors and query services follow edits.
- **Lua (`mw.wikibase`) and parser functions (`{{#property}}`, `{{#statements}}`).** These are client-side inclusion interfaces, not HTTP.

## 8. Regenerating this document

The build reproduces from the commits in §1.1:

1. Install MediaWiki `1.43.9` with the `REL1_43` vendor branch and Wikibase `REL1_43`.
2. Load `WikibaseRepository` and `WikibaseClient` with their `ExampleSettings.php` files, and run `update.php`.
3. Starting from `modules=main`, walk every `submodules` map with `api.php?action=paraminfo&helpformat=wikitext&formatversion=2&modules=…`, 50 modules per request.
4. Fetch `rest.php/wikibase/v1/openapi.json`.

The parameter reference in §§3–5 and the route tables in §6 were generated mechanically from those outputs. §§1–2, §4.0, the REST notes and §7 were written by hand and verified against the running install.

## 9. Sources

Primary (the software at the pinned refs):

- MediaWiki core 1.43.9: <https://github.com/wikimedia/mediawiki/tree/1.43.9> (the Action API in `includes/api/`, core REST routes in `includes/Rest/coreRoutes.json`)
- Wikibase REL1_43: <https://github.com/wikimedia/mediawiki-extensions-Wikibase/tree/REL1_43>. This includes:
  - Action API modules in `repo/includes/Api/` and `client/includes/Api/`
  - JSON format in `docs/topics/json.md`
  - REST OpenAPI in `repo/rest-api/specs/`
- Generated snapshots in `docs/samples/wikibase-1.43/` (§1.2)

Secondary. These describe current master and Wikimedia production, not 1.43 specifically:

- Release status: [Version lifecycle](https://www.mediawiki.org/wiki/Version_lifecycle), [MediaWiki 1.43](https://www.mediawiki.org/wiki/MediaWiki_1.43), [endoflife.date: MediaWiki](https://endoflife.date/mediawiki)
- Action API: [API:Main page](https://www.mediawiki.org/wiki/API:Main_page), [API:Data formats](https://www.mediawiki.org/wiki/API:Data_formats), [API:Errors and warnings](https://www.mediawiki.org/wiki/API:Errors_and_warnings), [API:Continue](https://www.mediawiki.org/wiki/API:Continue), [API:Tokens](https://www.mediawiki.org/wiki/API:Tokens), [API:Etiquette](https://www.mediawiki.org/wiki/API:Etiquette)
- REST API: [API:REST API/Reference](https://www.mediawiki.org/wiki/API:REST_API/Reference)
- Wikibase: [Wikibase/API](https://www.mediawiki.org/wiki/Wikibase/API), [Wikibase REST API](https://www.mediawiki.org/wiki/Wikibase/REST_API)
