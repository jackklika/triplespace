# 11. Rendering, templates and modules

This chapter describes how a `wikitext` page becomes HTML and page metadata: expansion then rendering, the Template namespace, the expander and its registry of variables, parser functions, tags and switches, the built-in and Parsoid renderers, the render manifest with the refresh and purge machinery it drives, foreign template repositories, TemplateStyles and its CSS sanitizer, site styles, TemplateData, and Lua modules under the Scribunto contract. It assumes the namespace and content-model tables and the wikitext subset of [10](10-pages-and-content-models.md), the page repositories and title stack of [13](13-mirrored-pages.md), the `view` tables and cache keys of [03](03-storage-caches-and-search.md), the settings catalogue of [23](23-configuration-and-registry.md), the rate classes of [07](07-actors-and-accounts.md), the include rule of [09](09-security-and-moderation.md), the Lua ID space of [04](04-entities-and-identifiers.md), and the crates and conformance material of [22](22-crates-and-stack.md). Files in wikitext are [12](12-files-and-media.md)'s, the API surface [18](18-api.md)'s, editing and preview [19](19-site-ui.md)'s.

## 1. Expansion, then rendering

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §1; [0033](../decisions/0033-backend-stack.md) §9.1.*

### 1.1 Two stages

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §1; [0033](../decisions/0033-backend-stack.md) §9.1.*

**A `wikitext` page renders in two stages, as in MediaWiki:**

1. **Expansion** turns the source into expanded wikitext. It handles templates, template arguments, `<noinclude>`, `<includeonly>` and `<onlyinclude>`, comments, parser functions, variables and, when Lua is on, `{{#invoke:}}` (§7). Extension tags pass through as tags behind strip markers. The result is what MediaWiki's `action=expandtemplates` returns.
2. **Rendering** turns the expanded text into HTML, with the built-in renderer (§1.2) or with Parsoid (§3).

**Expansion is always native.** It is implemented in Rust (§1.4) and runs in the binary. When Parsoid renders, it asks Triplespace to expand (§3.1); it never expands on its own and never reaches another wiki's preprocessor.

**Expansion is part of wikitext.** Every tenant that has `wikitext` pages expands them; no setting turns it off, since a wiki with pages and no templates is not one MediaWiki's clients expect ([0042](../decisions/0042-template-expansion-and-parsoid.md) A19). A tenant with only entities has nothing to expand. Where stage 1 cannot run, as in the editor's in-browser preview ([19](19-site-ui.md) §3.2), it is the identity: template calls and parser functions render as chips.

**The source is still parsed as source.** Positions for conflict marking and link autocomplete ([0010](../decisions/0010-site-ui.md) §4) come from the source, as now. Rendering and page metadata (§4.1) come from the expanded text.

Whether a tenant runs Lua and which renderer serves its HTML are the `site` settings `wikitext.lua` and `wikitext.renderer`, Lua capped by the instance's `wikitext.ceiling`; the Parsoid service's address, timeout and concurrency are deployment configuration. The settings and their defaults are in [23](23-configuration-and-registry.md) ([0042](../decisions/0042-template-expansion-and-parsoid.md) §2, A19).

### 1.2 The source parser and the built-in renderer

*Sources: [0033](../decisions/0033-backend-stack.md) §9.1; [0042](../decisions/0042-template-expansion-and-parsoid.md) §7.*

**Parser.** `parse-wiki-text-2` (a maintained fork of Fredrik Portström's `parse_wiki_text`, MIT-style licence without a notice clause) is vendored into `scatter-wikitext`. It is pure Rust, gives every node start and end positions, recognises templates, parameters, tags, tables, lists, links, images, categories and comments, reports warnings for malformed markup, and bounds its own running time. Its `Configuration` (namespaces, extension tags, URL protocols) is generated from `namespaces.toml`. Changes are offered upstream where they are general.

**What the parser must do.** Parse any wikitext without failing, because imported history predates the template-flattening revision ([0008](../decisions/0008-namespaces-and-document-pages.md) §8); recognise what it will not render; keep positions for line-level conflict marking and link autocomplete ([0010](../decisions/0010-site-ui.md)); extract `page_link` rows and categories; build for wasm so the preview ([0034](../decisions/0034-frontend-stack.md)) runs the same code as the server.

**Renderer.** Written in `scatter-wikitext`. It shares with the markdown path (`scatter-pages`, `markdown` feature) the `ammonia` sanitizer, wiki-link resolution (entity links rendered with labels, red links) and `page_link` extraction, so both content models render and link-index the same way.

**Rendered subset:**

| Kind | Rendered |
|---|---|
| Blocks | headings, paragraphs, lists (`*`, `#`, `;`, `:`), `----`, tables, leading-space `<pre>` |
| Inline | bold and italic (MediaWiki's apostrophe algorithm), internal links through `namespaces.toml`, external links, bare URLs, `<nowiki>`, comments (dropped) |
| HTML | MediaWiki Sanitizer's allowlisted tags and attributes |
| References | `<ref>`, `<ref name>`, `<references />` |
| Pre-save transform | `~~~`, `~~~~`, `~~~~~` on wikitext pages and wikitext posts, less a post's trailing signature, which is removed ([0019](../decisions/0019-discussions.md) §5) |

Tags and behaviour switches the registry marks `implemented` (§2.2) are rendered by the built-in renderer, after expansion or without it (§1.1); what the registry marks `ignored` has no effect. Everything else renders as a **visible chip** showing its source: templates, parser functions, magic words and variables, and the extension tags whose registry status is `chip`. Files in wikitext (`File:` and `Image:` links) follow [0039](../decisions/0039-files-and-media.md) §13, in [12](12-files-and-media.md). Categories are listed as plain text ([0010](../decisions/0010-site-ui.md)). Magic links (`ISBN`, `RFC`, `PMID`) are off. The subset itself is [0008](../decisions/0008-namespaces-and-document-pages.md) §8's, in [10](10-pages-and-content-models.md).

**The built-in renderer after expansion** renders the expanded text, produced by `scatter-wikitext-expand` (§1.4), with the subset above, and extracts from it. After expansion:

- **No template calls remain to chip.** A missing template renders as MediaWiki renders it: a red link to `Template:Name`.
- **Chips are for what is still outside the subset:** extension tags with status `chip` (§2), and HTML or table attributes the sanitizer drops are dropped as now. `<templatestyles>` renders as a deduplicated `<style>` element holding the sheet's sanitized form (§6.3).
- **Expansion errors** render as MediaWiki's error spans (`<strong class="error">…</strong>`) and add their tracking category (§4.1).

Where a text is rendered unexpanded (§1.1), the "Template not rendered" chip of [0010](../decisions/0010-site-ui.md) §4 shows each call. `tree-sitter-wikitext` (Wikimedia, MIT) is not the core parser; it remains available for editor highlighting in the browser ([0034](../decisions/0034-frontend-stack.md) §7).

### 1.3 The Template namespace and what can be transcluded

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §3.*

**Template (10) is a `pages` namespace and Template talk (11) is enabled** on every tenant, since templates are part of wikitext ([0042](../decisions/0042-template-expansion-and-parsoid.md) A19):

| Namespace | Kind | Models | Normalizer | Subpages | Talk |
|---|---|---|---|---|---|
| 10 Template | `pages` | `wikitext`; `sanitized-css` by the `.css` suffix rule (§6.1) | `first-letter` | Yes, as in MediaWiki | 11 |
| 11 Template talk | `pages` | `triplespace-talk` | — | — | — |

Module (828) and Module talk (829) are the namespaces that wait on a setting, `wikitext.lua`, entered in `namespaces.toml` with `enabled_by` (§7.2). This is a rule of [0008](../decisions/0008-namespaces-and-document-pages.md) §2: **a reserved number can be implemented conditionally**, by a setting named in its `namespaces.toml` entry. The namespace table is [10](10-pages-and-content-models.md)'s.

**Turning a namespace's setting off never removes pages.** Pages that exist keep their records and history. The namespace refuses `create`, `edit` and `move` into it with `ts-namespace-disabled`, and its existing pages stay readable at their titles and can be deleted. Turning the setting on again restores everything. `meta=siteinfo` lists a disabled namespace while it still holds pages, so clients can read them.

**Any `wikitext` page can be transcluded**, as in MediaWiki, provided the include does not widen: a page *A* includes a page *B* only if every group a `read` restriction on *B* names is also named by one on *A*, and otherwise `{{:B}}` renders as a link to a missing page for every viewer, so that one rendering per visibility serves every reader of *A* ([0056](../decisions/0056-security-model.md) §6, in [09](09-security-and-moderation.md)). The forms are MediaWiki's: `{{Foo}}` is `Template:Foo`; `{{:Foo}}` is the main-namespace page; `{{User:Example/box}}` and `{{Project:Notice}}` name their namespace. A page of another text model transcludes its text, as MediaWiki's `TextContent` does. An entity, thread or talk page has no wikitext form and transcludes as a link to itself, `[[:Item:Q42]]`, which is what MediaWiki produces for content it cannot transclude.

### 1.4 The expander

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §4.*

**The expander is a pure crate, `scatter-wikitext-expand`** ([22](22-crates-and-stack.md)). It implements MediaWiki's preprocessor and frame expansion:

- **The preprocessor tree** of MediaWiki's `Preprocessor_Hash`: template, template-argument, comment, ignored, extension-tag and heading nodes, with `<onlyinclude>`, `<includeonly>` and `<noinclude>` resolved for transclusion or for viewing.
- **Frames:** numbered and named arguments, defaults (`{{{1|x}}}`), lazy argument expansion, and the parent chain that `#invoke` reads.
- **Title resolution in transclusion:** `Template:` by default; a leading colon for the main namespace; the `msg:`, `msgnw:` and `raw:` modifiers; `subst:` and `safesubst:` (§1.5); one level of redirect.
- **Loops and limits:** template loop detection, and MediaWiki's limits on expansion depth, template depth, node count, post-expand include size, template argument size and expensive function calls ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16, in [07](07-actors-and-accounts.md)).
- **Strip markers** for extension tags and `<nowiki>`, in MediaWiki's form, so that Lua's `mw.text.unstrip` and `mw.text.killMarkers` behave as they do on MediaWiki.

**It does no I/O.** Every fact it needs comes through a host trait, `ExpandHost`, implemented by `triplespace-render` over the serving model:

- page source by title as of a position, with the page ID and offset, or "missing";
- title facts (existence, redirect target, page ID, size, protection, content language);
- namespaces and site statistics;
- the render's one timestamp ("now"), fixed per render as MediaWiki fixes it per parse;
- file facts ([0039](../decisions/0039-files-and-media.md));
- `#invoke` (§7);
- scope members and counts, table rows, sprint tasks and board listings, for the block functions ([0062](../decisions/0062-workspaces.md) §3, in [15](15-structured-pages.md)).

Given the same host answers, it produces the same output. The same code serves page views, `action=expandtemplates`, pre-save transform, server preview and Parsoid's callbacks.

**What expansion returns:** the expanded text, the strip state, the page properties it set (`DEFAULTSORT`, `DISPLAYTITLE`, `SHORTDESC`, `__DISAMBIG__`, behaviour switches), which the refresh job stores in `view.page_prop` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6, in [10](10-pages-and-content-models.md)), the render manifest (§4.2), the limit report and any errors, and **where each of the page's own calls put its output**: for each call written in the page that transcluded a page, its name, the byte offset of its `{{` and the range of the expanded text it produced.

**Which template drew a block** ([0042](../decisions/0042-template-expansion-and-parsoid.md) A22). Asked for, a render gives each top-level block that lies wholly within one call's output `data-ts-template`, the template's title, and `data-ts-template-at`, the call's offset; a template's inline output in a paragraph of the page's own text marks nothing. `action=parse` takes it as the flag `tstemplates`, and the site as `?tstemplates=1` on a page view, so that a hover shows which template made a block and a census can read the templates from rendered HTML; a public render never carries it unless its address asks.

**It is written from MediaWiki's documented behaviour and test files, not translated from MediaWiki's PHP.** That keeps the crate original code under the shared crates' dual licence ([0005](../decisions/0005-crate-organization.md) §6). The same applies to `#expr` and `#time` (§2).

### 1.5 Pre-save transform

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §6; [0033](../decisions/0033-backend-stack.md) §9.1.*

The pre-save transform expands `~~~`, `~~~~` and `~~~~~` on wikitext pages, and on wikitext posts except a signature at the end of the post, which is removed instead ([0008](../decisions/0008-namespaces-and-document-pages.md) §8; [0019](../decisions/0019-discussions.md) §5, in [14](14-discussions.md) §1.6). **`subst:` and `safesubst:` expand when a page is saved,** as MediaWiki's pre-save transform does, alongside `~~~~`. The saved text is the substituted text. `{{subst:#invoke:…}}` works when Lua is on.

## 2. The parser-function registry

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §5; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §7.*

### 2.1 Registry data and statuses

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §5; [0096](../decisions/0096-adopting-any-mediawiki.md) §4.*

**What the expander knows is registry data**, in `docs/registry/wikitext-functions.toml`, embedded by `scatter-wikitext-expand` ([23](23-configuration-and-registry.md)). Each entry is a variable, parser function, extension tag or behaviour switch, with its synonyms, its origin (MediaWiki core, ParserFunctions, Scribunto, Wikibase Client, Cite, or another extension) and a status:

| Status | Meaning |
|---|---|
| `implemented` | Expanded or rendered as MediaWiki does |
| `chip` | Recognised and rendered as a visible chip showing the source, as unsupported syntax is ([0008](../decisions/0008-namespaces-and-document-pages.md) §8) |
| `ignored` | Recognised and without effect: a behaviour switch that does nothing here, or a tag that renders nothing |

**The registry is what `meta=siteinfo` reports** for `magicwords`, `functionhooks`, `extensiontags`, `variables` and `doubleunderscores`, so clients and Parsoid (§3) tokenize exactly what the expander handles. `chip` entries are reported too: they are installed in the sense that the parser recognises them.

**The extensions an adopted wiki used are chips.** The parser functions of Semantic MediaWiki, Cargo and Page Forms are registered with status `chip`, and Semantic MediaWiki's `__NOFACTBOX__` and `__SHOWFACTBOX__` as `ignored` ([0096](../decisions/0096-adopting-any-mediawiki.md) §4). Old revisions that used them render their calls as chips, and clients tokenize them as parser functions. Their data is converted, not served ([05](05-providers-and-ingest.md) §7.7). Semantic MediaWiki's inline annotations (`[[Has author::Jane Doe]]`) are not syntax the parser knows: they render as links to pages of those titles.

### 2.2 Coverage

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §5; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §3, §5, §7; [0043](../decisions/0043-lua-modules.md) §1, §7.*

| Group | Status |
|---|---|
| **Core variables:** page-name family (`PAGENAME`, `FULLPAGENAME`, `BASEPAGENAME`, `ROOTPAGENAME`, `SUBPAGENAME`, `TALKPAGENAME`, `SUBJECTPAGENAME` and the `E` forms), namespace family, revision family (`REVISIONID`, `REVISIONTIMESTAMP`, `REVISIONUSER`, `REVISIONSIZE` and the date parts), `CURRENT*` and `LOCAL*` time, statistics (`NUMBEROFPAGES`, `NUMBEROFARTICLES`, `NUMBEROFFILES`, `NUMBEROFUSERS`, `NUMBEROFACTIVEUSERS`, `NUMBEROFEDITS`), `PAGEID`, `SITENAME`, `SERVER`, `SERVERNAME`, `SCRIPTPATH`, `CONTENTLANGUAGE`, `DIRECTIONMARK`, `!` and `=` | `implemented` |
| **Core parser functions:** `ns`, `nse`, `lc`, `uc`, `lcfirst`, `ucfirst`, `urlencode`, `anchorencode`, `localurl`, `fullurl`, `canonicalurl` (and `E` forms), `filepath`, `formatnum`, `plural`, `grammar`, `gender`, `padleft`, `padright`, `int`, `#language`, `#special`, `#speciale`, `#tag`, `#formatdate`, `#bcp47`, `#dir`, `#interwikilink`, `#interlanguagelink`, `DEFAULTSORT`, `DISPLAYTITLE`, `PAGESINCATEGORY`, `PAGESIZE`, `PROTECTIONLEVEL`, `PROTECTIONEXPIRY`, `NUMBERINGROUP` | `implemented`. `CASCADINGSOURCES` is implemented and always empty, since there is no cascading protection ([0016](../decisions/0016-permissions-and-access-control.md) §4) |
| **ParserFunctions:** `#expr`, `#if`, `#ifeq`, `#iferror`, `#ifexpr`, `#ifexist`, `#rel2abs`, `#switch`, `#time`, `#timel`, `#timef`, `#timefl`, `#titleparts` | `implemented`. The string functions (`#len`, `#pos` and the rest) are not registered, since ParserFunctions leaves them off by default; a call renders as literal text, as on MediaWiki |
| **Scribunto:** `#invoke` | `implemented` when `wikitext.lua` is on (§7.1); otherwise `chip` |
| **Wikibase Client:** `#property`, `#statements` | `implemented` (§7.7) |
| **Wikibase Client:** `SHORTDESC` | `implemented`, as the page property `wikibase-shortdesc` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6); `__EXPECTED_UNCONNECTED_PAGE__` is `ignored` |
| **Core:** `#REDIRECT` | `implemented` ([0051](../decisions/0051-page-redirects.md) §1, in [10](10-pages-and-content-models.md)) |
| **Triplespace:** `#workspace`, `#table`, `#scope-count`, `#scope-members`, `#completeness`, `#tasks`, `#sprint-progress`, `#board`, `#scope-changes` | `implemented`, `expensive`, and `scoped`: their output depends on a scope's, table's, sprint's or board's state, recorded in the manifest (§4.2) ([0062](../decisions/0062-workspaces.md) §2–3, in [15](15-structured-pages.md)) |
| **Triplespace:** `#query` ([0063](../decisions/0063-query-namespace.md) §6), `#conformance` ([0064](../decisions/0064-entityschema-and-validation.md) §6) | `implemented`, `expensive`, `scoped` |
| **Extension tags:** `templatestyles`, `templatedata` | `implemented` (§6.3, §6.5) |
| **Extension tags:** `nowiki`, `pre`, `ref`, `references`, `gallery`, `indicator`, `poem`; `syntaxhighlight` and `source` as plain `<pre>` | `implemented` |
| **Extension tags:** `math`, `chem`, `ce`, `score`, `timeline`, `graph`, `mapframe`, `maplink`, `categorytree`, `inputbox`, `imagemap`, `hiero`, `charinsert` | `chip` |
| **Extension tag:** `section` (Labeled Section Transclusion) | `ignored`: it renders nothing, as on MediaWiki. `#lst` and its relatives are not registered |
| **Behaviour switches:** `__NOTOC__`, `__TOC__`, `__FORCETOC__`, `__HIDDENCAT__` | `implemented` ([0008](../decisions/0008-namespaces-and-document-pages.md) §8, [0038](../decisions/0038-page-metadata-and-categories.md) §3) |
| **Behaviour switch:** `__DISAMBIG__` | `implemented`, as the page property `disambiguation` ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6) |
| **Behaviour switches:** `__NOEDITSECTION__`, `__NEWSECTIONLINK__`, `__NONEWSECTIONLINK__`, `__NOGALLERY__`, `__INDEX__`, `__NOINDEX__`, `__STATICREDIRECT__`, `__EXPECTUNUSEDCATEGORY__`, `__NOCONTENTCONVERT__`, `__NOTITLECONVERT__` | `ignored` for rendering; each is stored as a page property where MediaWiki stores one ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6) |

The `templatestyles` and `templatedata` tags, `__DISAMBIG__`, `SHORTDESC` and `__EXPECTED_UNCONNECTED_PAGE__` entries are those of [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §7, which is [23](23-configuration-and-registry.md)'s.

### 2.3 Messages, and how the rows are made

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §5; [0095](../decisions/0095-the-interface-namespace.md) §2; [0095](../decisions/0095-the-interface-namespace.md) §8.*

**Messages.** `{{int:}}`, tracking-category names (§4.1) and Parsoid's `meta=allmessages` calls (§3.1) read the wiki's overrides: a page of the Interface namespace (8, canonical `MediaWiki`) overrides a message for the wiki, `Interface:{Key}` in its own language and `Interface:{Key}/{lang}` in one, the instance's from the primary tenant's pages and the wiki's over them ([0095](../decisions/0095-the-interface-namespace.md) §2). `{{int:key|…}}` expands the override in the wiki's language with its arguments for `$1`…, since a page renders once for every reader; the build ships none of MediaWiki core's messages, so a key the wiki does not override stays a chip. `meta=allmessages` lists overrides over the site's own shipped messages ([0095](../decisions/0095-the-interface-namespace.md) §8).

**The core rows are generated, not hand-kept.** A script reads the reference install's `meta=siteinfo` with the five properties above, once ParserFunctions and Scribunto are installed there ([0042](../decisions/0042-template-expansion-and-parsoid.md) §18, in [22](22-crates-and-stack.md)), and writes the core and extension entries with their synonyms; the statuses are hand-edited. The file in the repository is the seed and the shape, as `sites.toml` is.

## 3. The Parsoid renderer

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §8.*

### 3.1 Parsoid calls back into Triplespace

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §8.1; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5; [0095](../decisions/0095-the-interface-namespace.md) §8.*

Parsoid's host interface, `SiteConfig`, `DataAccess` and `PageConfig`, has an implementation over a remote wiki's Action API in Parsoid itself (`Wikimedia\Parsoid\Config\Api\*`, the code behind `bin/parse.php --apiURL`). **Triplespace points those classes at the tenant's own Action API**, so Triplespace is Parsoid's host and the single source of truth. The requests Parsoid sends, and what serves them:

| Parsoid needs | Request | Served by |
|---|---|---|
| Site configuration | `meta=siteinfo`, `siprop=general\|protocols\|namespaces\|namespacealiases\|magicwords\|interwikimap\|languagevariants\|defaultoptions\|specialpagealiases\|extensiontags\|functionhooks\|variables\|doubleunderscores` | The registries; §2 for the parser properties; `interwikimap` from site aliases ([0026](../decisions/0026-sitelinks.md) §2); `languagevariants` empty |
| Feature detection | `action=paraminfo&modules=query` | Existing |
| Template source | `prop=revisions`, `rvprop=content\|ids`, `rvslots=*`, `redirects` | Page records, then foreign repositories (§5) |
| Link classes (red links, redirects) | `prop=info`, `inprop=linkclasses` | The title resolver |
| Files | `prop=imageinfo` with `iibadfilecontexttitle` and `iiprop=mediatype\|mime\|size\|url\|badfile\|sha1\|timestamp\|canonicaltitle` | [0039](../decisions/0039-files-and-media.md) §17 |
| Expansion | `action=expandtemplates`, `prop=wikitext\|modules\|jsconfigvars\|categories\|properties`, `showstrategykeys` | The expander (§1.4) |
| Tags Parsoid does not implement natively | `action=parse`, `prop=text\|modules\|jsconfigvars\|categories\|properties\|externallinks` | The built-in renderer, never Parsoid (§3.2) |
| Tracking-category names | `meta=allmessages` | The wiki's overrides over the site's shipped messages (§2.3) |
| TemplateData | `action=templatedata` | `view.page_prop` (§6.5). Only Parsoid's HTML-to-wikitext direction uses it, and reading never calls it |

### 3.2 The render-scoped API

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §8.2.*

**Each Parsoid render gets its own API endpoint:** `{tenant base}/w/render/{token}/api.php`, where the token is 128 random bits, valid for `parsoid.timeout`, naming the tenant, the page and the render. Requests under it:

- **read as an anonymous reader**, because shared caches hold only the public form ([0014](../decisions/0014-caches-and-search.md) §1);
- **are not rate-limited**, and serve only the read modules of §3.1; anything else is refused;
- **record what they read into the render's manifest** (§4.2), so a Parsoid render has the same dependencies as a built-in one;
- **never call Parsoid.** `action=parse` under a render token always uses the built-in renderer, so there is no recursion.

Expansion results are memoized within a render, since Parsoid calls `expandtemplates` once per top-level transclusion. Requests to the plain `api.php` are ordinary requests, whatever their origin.

### 3.3 The Parsoid service

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §8.3.*

**Parsoid has no HTTP server of its own outside MediaWiki.** Triplespace therefore ships one, in `services/parsoid/`: a small PHP front controller that requires `wikimedia/parsoid` through Composer, builds the API-backed configuration for the endpoint each request names, and answers one route:

```
POST /transform/wikitext/to/html
{ "api": "{tenant base}/w/render/{token}/api.php",
  "title": "…", "revid": 123, "wikitext": "…", "page_language": "en" }
→ 200 { "html": "…", "data_parsoid": {…}, "data_mw": {…}, "parsoid_version": "…" }
```

- **It is a separate process,** stateless and horizontally scalable, deployed as its own OCI image. Its version of Parsoid is pinned to the one MediaWiki 1.43 bundles ([22](22-crates-and-stack.md)).
- **It is the one optional service that needs a PHP runtime.** [0033](../decisions/0033-backend-stack.md) §1's first principle still holds: a small instance runs with the binary and Postgres, and Parsoid joins Valkey, OpenSearch and QLever among the optional services.
- **It is not linked into the binary,** so Parsoid's GPL-2.0-or-later licence is a separate program's licence and 0033 §1's linking list does not apply to it.

### 3.4 Output and failure

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §8.4.*

**Parsoid's HTML passes through Triplespace's sanitizer** with Parsoid's attribute allowlist (`typeof`, `about`, `data-mw`, `rel`, `property`), and links to entity titles get their labels, as the built-in renderer gives them ([0008](../decisions/0008-namespaces-and-document-pages.md) §8).

**If Parsoid fails or times out, the page renders with the built-in renderer,** with a notice and a 60-second cache lifetime so that the next read tries again. Parsoid being unavailable never stops a page being read.

**Page metadata never depends on Parsoid.** Links, categories and the manifest come from the expander and the source parser (§4.1), so they are the same whichever renderer serves the HTML.

## 4. Metadata, the render manifest, refresh and purging

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §9, §10; [0057](../decisions/0057-web-tier.md) §14.*

### 4.1 Links, categories and other metadata come from expanded output

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §9; [0083](../decisions/0083-write-path-in-three-tiers.md) §1, §6.*

Everything a page links to or belongs to is read from its **expanded** text, as in MediaWiki:

- page and entity links ([0008](../decisions/0008-namespaces-and-document-pages.md) §10);
- category membership, sort keys, `DEFAULTSORT` and `__HIDDENCAT__` ([0038](../decisions/0038-page-metadata-and-categories.md) §3);
- file usage (`view.file_link`, [0039](../decisions/0039-files-and-media.md) §20);
- external links, page properties and indicators.

A template's `<includeonly>` categories reach the pages that transclude it; its `<noinclude>` categories stay on the template page.

**Tracking categories join.** Expansion adds MediaWiki's tracking categories for its own conditions, named by MediaWiki's message keys and so by MediaWiki's names: `expansion-depth-exceeded-category`, `template-loop-category`, `post-expand-template-inclusion-category`, `post-expand-template-argument-category`, `node-count-exceeded-category`, `expensive-parserfunction-category` and `broken-file-category`, Scribunto's `scribunto-common-error-category` (§7.11) and TemplateStyles' `templatestyles-stylesheet-error-category` (§6.3).

**[0038](../decisions/0038-page-metadata-and-categories.md)'s rule that a category is defined in one place still holds.** The place is the expanded text: the page's own text and the templates it uses. Categories remain a projection, never records.

**The expansion-derived rows are written by the refresh job (§4.3), not by the page projection.** Computing them may read other pages and run Lua, which does not belong inside the appending transaction ([0013](../decisions/0013-postgres-storage.md) §7; [0083](../decisions/0083-write-path-in-three-tiers.md) §1). `view.page_link`, `view.page_category` and `view.page_prop` therefore carry two row classes, told apart by `from_render`, with **one writer each**: the page projection writes the source-derived rows, what the stored text itself links to, belongs to and sets, when it applies the record; the refresh job writes the expansion-derived rows, with `from_render = true`, from the render. Each writer owns its class exclusively and readers union them, so a page's source links are indexed as soon as its edit is projected and the links its templates emit follow after the refresh queue, as MediaWiki's deferred links update does. The refresh job never deletes a row it did not write ([0083](../decisions/0083-write-path-in-three-tiers.md) §6; the tables are [10](10-pages-and-content-models.md) §5.2, §5.3 and §5.5's).

### 4.2 The manifest

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §10; [0043](../decisions/0043-lua-modules.md) §10; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §3.*

**Every expansion produces a manifest:** what it read, and until when the result holds.

| Entry | Recorded as |
|---|---|
| A transcluded or tested page | Namespace and title, with page ID and offset, or **missing**, so that creating the page invalidates (red links, `#ifexist`, missing templates). A stylesheet loaded by `<templatestyles>` is one (§6.3) |
| An entity | Entity ID and usage aspects (§7.9) |
| A file | Title and the file version used |
| A foreign repository page | Repository, title, remote revision ID and expiry (§5) |
| A volatile input | Current time, site statistics, `PAGESINCATEGORY` and the like, with the earliest moment the output could change |
| A scoped block | The scope page and its `(definition_revid, computed_at)`, or the table's, sprint's or board's page and revision, with the sprint's last task state change; indexed in `view.render_dep_scoped` so the scope and task projections enqueue the refresh, debounced by `blocks.refresh_debounce` ([0062](../decisions/0062-workspaces.md) §3, in [15](15-structured-pages.md)) |

### 4.3 Tables and the refresh queue

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §10; [0083](../decisions/0083-write-path-in-three-tiers.md) §6.*

**Tables** (`view`, written by the refresh job):

| Table | Holds | MediaWiki counterpart |
|---|---|---|
| `transclusion` | `(page_id, target_ns, target_title, target_page_id NULL, repo NULL)` | `templatelinks` |
| `render_state` | `(page_id, epoch, rendered_offset, expires_at, renderer, settings_hash, limit_report, errors, unheld_misses integer)` | `page_touched`, the parser cache's expiry, the limit report |
| `entity_usage` | Defined in §7.9 | `wbc_entity_usage` |

`unheld_misses` is the count of §7.10. The queue is `ops.render_refresh`: page IDs with a reason, deduplicated.

**These tables are not log-replayed.** Every projection declares its class, log-replayed or view-derived ([0083](../decisions/0083-write-path-in-three-tiers.md) §6), and the render tables are neither a pure function of the log nor a scan of other `view` tables: `transclusion`, `render_state` and the render-owned row classes of §4.1 are **render-derived**, rebuilt by re-rendering every `wikitext` page, and where a render read a foreign repository or the clock, the rebuild may differ from the original. A rebuild truncates the render-owned rows and enqueues every `wikitext` page on `ops.render_refresh`; it re-runs no job of its own and never touches the source-derived rows, which the page projection replays. `entity_usage` is written by the refresh job from the manifest in the same way, and is read by the tier-3 consumer of §7.9. The catalogue is [03](03-storage-caches-and-search.md)'s.

**Reports and statistics.** The refresh job applies report deltas (`view.report_entry`) in the same transaction as the `transclusion`, link, category and `entity_usage` rows it writes ([0047](../decisions/0047-special-pages.md) §4.3). Site statistics, a volatile input here, are read from `view.site_stats` ([0047](../decisions/0047-special-pages.md) §13).

### 4.4 What triggers a refresh

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §10; [0043](../decisions/0043-lua-modules.md) §10; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

1. A page's own new revision.
2. A change to a page another page depends on: a new revision, creation, deletion, undeletion, move, or a `read` ACL. Every page whose `transclusion` row names it, by page ID or by title, gets an epoch bump and a refresh, in batches, as MediaWiki's `HTMLCacheUpdate` and `refreshLinks` jobs do.
3. A composition event for an entity, "entity X composed to version N", consumed by the entity-usage consumer of §7.9 in its own queue, never inside the composing or appending transaction, for usage rows whose aspects the change touches. The consumer first checks `view.entity_usage` by index for the entity and every member of its cluster and stops there when no page uses any of them; it is off while no page on the instance uses an entity, and off during bootstrap and bulk modes until the operator turns it on ([0083](../decisions/0083-write-path-in-three-tiers.md) §3).
4. `expires_at` passing, for volatile and foreign inputs. This is lazy: the next read finds the entry expired and renders again.
5. A change to any `wikitext.*` setting ([23](23-configuration-and-registry.md)).
6. `action=purge` (§4.6).

### 4.5 Caching and cache tags

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §10.*

A rendered page's version gains the page's **render epoch**:

```
p:{pageid}:{gen}:{offset}:{epoch}:{renderer}:{lang}
```

Its lifetime is the lower of [0014](../decisions/0014-caches-and-search.md) §4's ceiling and `expires_at`. A miss renders synchronously under single flight ([0014](../decisions/0014-caches-and-search.md) §4). The `p:` key, like the `css:` (§6.2) and `ld:` (§7.6) keys, also carries the `{vis}` segment after `{gen}` that [03](03-storage-caches-and-search.md) describes, because its value depends on what the viewer may read.

**Which bumps serve stale.** An epoch bump that a **dependency** induces (§4.4 items 2, 3 and 5: a template or module edited, an entity recomposed, a `wikitext.*` setting changed) does not make the next reader wait: the old `p:` entry is served **stale-while-revalidate** and the refresh queue re-renders the page in its own time, so a template edit on a mirror of Wikipedia re-renders ten thousand pages behind the readers rather than in front of them. **The page's own edit keeps "no stale"**: a new revision of the page itself (item 1), and `action=purge` (§4.6), delete its `p:` keys and the next read renders afresh, as MediaWiki renders after `page_touched` moves, so read-your-writes holds for the editor. **Synchronous renders are bounded per tenant** by a render pool, deployment configuration beside the Parsoid concurrency of §1.1; a miss that finds the pool full is served stale where a stale entry exists and enqueued, and waits only where there is none.

**Erasure, deletion and hiding reach transcluding pages, and never serve stale.** Every rendered response carries a `Cache-Tag` for each page and entity in its manifest. Erasing, deleting or hiding a dependency bumps the epoch of every page that depends on it, deletes their `p:` keys, and purges their tags, because the public form of those pages has changed ([0014](../decisions/0014-caches-and-search.md) §5, in [03](03-storage-caches-and-search.md)); the stale entry is unreachable, whatever the pool's state.

### 4.6 Purging

*Sources: [0057](../decisions/0057-web-tier.md) §14; [0042](../decisions/0042-template-expansion-and-parsoid.md) §10.*

- **`index.php?title={title}&action=purge`** is a page of the site: the confirmation form MediaWiki shows, whose submission calls the API's `action=purge`.
- **A plain `action=purge` on a local page** bumps the page's render epoch, deletes its `p:` keys and purges its `Cache-Tag`, so the next read renders afresh, as MediaWiki's purge re-parses; `forcelinkupdate` and `forcerecursivelinkupdate` also queue refreshes of its links and, recursively, of the pages that depend on it. On an entity it purges the entity's `Cache-Tag`. On a foreign title it does what [0053](../decisions/0053-mirrored-pages.md) §6 says ([13](13-mirrored-pages.md)).
- **Any principal who may read the target** may purge it, by `POST` and without a token as in MediaWiki, counted in the `parse` rate class ([07](07-actors-and-accounts.md)) as [0053](../decisions/0053-mirrored-pages.md) has it for foreign titles.
- **The new epoch changes the page's `ETag`,** so the web tier's revalidation ([20](20-web-tier.md)) sees the change on the next request, and the CDN, purged by tag, refills from fresh content.

## 5. Foreign template repositories

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §11.*

### 5.1 A template repository is a page repository

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §11.*

**A template repository is a page repository that serves `Template` and `Module`** ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §1, in [13](13-mirrored-pages.md)): a `config` record of kind `page-repo`, keyed `page-repo:{name}`, in a tenant's or the instance's `config`, listed in inheritance order in the tenant's `pages.repos`, and configured as file repositories are ([0039](../decisions/0039-files-and-media.md) §11). The lookup below is the title stack of [0052](../decisions/0052-page-repositories-and-title-inheritance.md) §3, as it applies to transclusion; `mirror` mode for a repository is [0053](../decisions/0053-mirrored-pages.md) §5.

| Kind | What it is | How it is read |
|---|---|---|
| `tenant` | Another tenant on this instance | Directly, from the source tenant's pages, with its ACLs. The source must set `pages.share = on`; provider reader lists apply ([0028](../decisions/0028-tenancy-policy.md) §5); a private tenant cannot share |
| `mediawiki` | Any MediaWiki Action API, a Wikipedia above all, or a Triplespace tenant elsewhere | `prop=revisions&rvprop=content\|ids&rvslots=main&redirects` through the live upstream client ([0012](../decisions/0012-api-requirements.md) §6): User-Agent and `maxlag` set, rate-limited, cached, never logged |

### 5.2 Lookup

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §11.*

**Lookup is local first, then each repository in order.** It applies to Template and Module titles (§7.2), including data modules. A local page shadows a foreign one of the same title; no special right is needed, since shadowing a template is an ordinary edit. **Lookup restarts at the local tenant at every level:** a foreign `Template:Infobox` that calls `{{Infobox/row}}` gets the local `Template:Infobox/row` if one exists. That is the consistent rule, and it is how a wiki adapts a borrowed template.

### 5.3 Proxying, trust, context and attribution

*Sources: [0042](../decisions/0042-template-expansion-and-parsoid.md) §11.*

**`mediawiki` repositories proxy.** Fetched source is cached for the repository's `repo.cache_ttl`, a field of the `page-repo` record whose default for template source is one hour, and each manifest entry carries the remote revision ID and that expiry, so a page using a remote template refreshes when the cache does (§4.2). **The render shows the remote template as it is now**: a borrowed template behaves like a local one that someone else edits.

**Foreign source is untrusted input,** exactly as an edit is: the same expander, limits and sanitizer apply.

**A remote template is expanded in the local page's context.** `{{PAGENAME}}` is the local page; its links resolve locally.

**Attribution.** A page that used a foreign repository lists it at its foot, "Uses templates from English Wikipedia", linking each foreign page used, because reused wiki text carries its licence's attribution requirement. Files use the same pattern ([0039](../decisions/0039-files-and-media.md) §11, in [12](12-files-and-media.md)).

## 6. TemplateStyles, site styles and TemplateData

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §1–§5.*

### 6.1 The `sanitized-css` content model

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §1.*

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `sanitized-css` | TemplateStyles | text | main | `text/css` | Yes | Template (10), for titles ending in `.css` |

It is a text model under [0041](../decisions/0041-content-models.md) §5, stored in the page's own records and edited with `action=edit`. TemplateStyles' own ID is kept, under [0041](../decisions/0041-content-models.md) §2's rule for a Wikibase or MediaWiki extension's model, so that an export of Wikipedia's `Template:Infobox/styles.css` imports here with its model intact. The content-model table is [10](10-pages-and-content-models.md)'s.

**Where it is allowed.** Template (10), as TemplateStyles' default `$wgTemplateStylesNamespaces`, where a title ending in `.css` takes it by the suffix rule of [0008](../decisions/0008-namespaces-and-document-pages.md) §5; and Project (4), for the site-styles page of §6.4. The title rule joins Module's (§7.3): all of a namespace's models stay text models, so [0041](../decisions/0041-content-models.md) §4's single-source rule holds. `action=changecontentmodel` moves a page to or from it within those namespaces.

**What [0008](../decisions/0008-namespaces-and-document-pages.md) §5 excludes** is `css` and `javascript`, which reach the browser as written; `sanitized-css` reaches it only through the sanitizer (§6.2), which is the difference TemplateStyles was built on.

**The model's operations** ([0041](../decisions/0041-content-models.md) §5):

| Operation | For `sanitized-css` |
|---|---|
| Validate | The text parses under CSS Syntax Level 3, and the sanitizer (§6.2) accepts every rule. A sheet with a syntax error, a forbidden at-rule, a disallowed property or an external URL is **refused at save**, with each error's line, as TemplateStyles refuses one. Nothing that will be dropped at render is ever stored |
| Serialize | The text |
| Plain text for search | The text; the `pages` index marks it `content_model: sanitized-css` |
| Diff | Line diff |
| Render | The page view shows the sheet as highlighted source, with its sanitized form beneath when they differ (they do not, after validation) and the pages that use it ("Pages that use this stylesheet", from `view.transclusion`) |

### 6.2 The sanitizer: `scatter-css`

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2; [0086](../decisions/0086-theming.md) §6.*

**A pure crate, `scatter-css`, parses and sanitizes CSS** ([22](22-crates-and-stack.md)). It builds for `wasm32-unknown-unknown` so the editor can lint a sheet as it is typed ([0034](../decisions/0034-frontend-stack.md) §6). It is written from the CSS Syntax Level 3 specification and from TemplateStyles' documented rules and test cases, not translated from its PHP, so that it is original code under the shared crates' licence ([0005](../decisions/0005-crate-organization.md) §6).

**What it accepts** is TemplateStyles' policy, which Wikipedia's sheets are written against:

| | Allowed | Dropped, or refused at save |
|---|---|---|
| **At-rules** | `@media`, `@supports`, `@font-feature-values`, `@keyframes` (with its rules sanitized), `@page` | `@import`, `@charset`, `@namespace`, `@font-face`, and any at-rule not listed |
| **Properties** | The allow-list TemplateStyles' css-sanitizer publishes: CSS 2.1 and the Level 3 and 4 modules it names (backgrounds, borders, box alignment, flexbox, grid, fonts, text, transforms, transitions, animations, multi-column, writing modes, and so on), kept in `docs/registry/css-properties.toml` with the module each came from | Unknown and vendor-specific properties, `behavior`, `-moz-binding`, `expression()`, `position: fixed` |
| **Values** | Standard syntax per property, with `!important` | Anything the property's grammar rejects |
| **`url()`** | A local file's URL, resolved through the tenant's file lookup ([0039](../decisions/0039-files-and-media.md) §11) and written as the media base URL, so a sheet may use an uploaded or inherited image; `data:` URIs for images under a size cap | Every other URL. Nothing a sheet references can leave the instance |
| **Selectors** | Any selector, **prefixed with `.mw-parser-output`** so that it matches only rendered content and never the frame, as TemplateStyles prefixes them; a `wrapper` attribute on the tag (§6.3) prefixes further. A first compound of `html` with only the classes `skin-theme-clientpref-night`, `skin-theme-clientpref-os` or `skin-theme-clientpref-day` is kept before the prefix, so content written for Wikipedia's night mode works under the site's dark variants ([0086](../decisions/0086-theming.md) §6) | Other selectors that escape the prefix (`html`, `body`, `:root`), which are rewritten to match nothing |
| **Size** | Up to `templatestyles.max_bytes` (`site`, default 100 KB, TemplateStyles' `$wgTemplateStylesMaxStylesheetSize`) | Larger sheets are refused |

**Two modes.** `validate` reports every error and is what saving uses. `sanitize` drops what it cannot accept and reports what it dropped; it is what mirrored HTML ([0053](../decisions/0053-mirrored-pages.md) §2, in [13](13-mirrored-pages.md)) and the site-styles page (§6.4) use at render, so that a sheet written for another wiki's allow-list still renders its acceptable rules here.

**The output is canonical:** a serialized sheet with comments removed, so that two sheets that sanitize to the same text deduplicate (§6.3), and the hash of the output is stable. The sanitized form is cached by the page's ID and offset, L1 key `css:{pageid}:{gen}:{offset}` ([0014](../decisions/0014-caches-and-search.md) §4; the `{vis}` segment is §4.5's), since every page that uses a template sheet renders the same sanitized text.

### 6.3 `<templatestyles>`

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §3, A5; [0042](../decisions/0042-template-expansion-and-parsoid.md) §7.*

**The tag is `implemented`** in `docs/registry/wikitext-functions.toml` (§2.2). In the expansion stage `<templatestyles src="Infobox/styles.css" />` resolves `src` as a title, `Template:` by default and any namespace with a prefix, **through the stack** ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §3), so a fork that left its sheets to the repository still finds them, and a local sheet shadows a foreign one. The tag emits, in place, a `<style data-mw-deduplicate="TemplateStyles:r{revid}">` element holding the sheet's sanitized form, as TemplateStyles does: the key names the sheet's revision, a foreign sheet's as `TemplateStyles:{repository}:r{revid}`, and `/{wrapper}` follows where `wrapper="…"` prefixes every selector further. **One sheet per distinct key per page:** the renderer keeps the first and drops repeats, which is what MediaWiki and Parsoid do, so a page with two hundred citations loads the citation sheet once.

**The site serves each sheet as a stylesheet, not inline** ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) A5). The HTML of a page the site shows, and of its `action=render` regions, carries in place of each element a `<link rel="stylesheet" data-mw-deduplicate="…" href="…">` with the same key, which HTML allows in the body, so order and deduplication are unchanged. The sheet is pure CSS already, sanitized and scoped to `.mw-parser-output` (§6.2), so nothing is lost by moving it, and the site's Content Security Policy admits it as any same-origin stylesheet while still refusing `<style>` elements ([09](09-security-and-moderation.md) §1.4). The address follows from the key, and the revision it names never changes:

| Sheet | Address |
|---|---|
| A local revision | `/w/styles/templatestyles/r{revid}.css` |
| A repository's revision ([0052](../decisions/0052-page-repositories-and-title-inheritance.md); [13](13-mirrored-pages.md)) | `/w/styles/templatestyles/{repository}/r{revid}.css` |
| Either, with a wrapper | the same, with `?wrapper={selector}` |

The API serves them under `/w/styles/`, which joins its paths ([20](20-web-tier.md) §2.1): the revision's text, sanitized and wrapped, from the `css:` cache, as `text/css` with `immutable` and a year's lifetime; a repository's revision through the repository's cache. A sheet is served with its page's own visibility, so a public sheet is public; a revision that is hidden, erased or deleted is a `404`, so an erasure reaches the sheet as it reaches the page. **`action=parse` keeps MediaWiki's inline element** for API clients, which embed its `text` elsewhere and expect it; the site asks for links with `tsstyles=link` ([18](18-api.md)).

**Errors** are TemplateStyles': a `src` that does not resolve, or that resolves to a page whose model is not `sanitized-css`, renders an error span (§1.2) and adds the tracking category `templatestyles-stylesheet-error-category` (§4.1). A page of the right model never errors at render, since it was validated at save.

**The manifest records the sheet** (§4.2) as a transcluded page, local with its page ID and offset or foreign with its repository, revision and expiry, so an edit to a sheet refreshes every page that uses it, and `view.transclusion` lists them. `prop=templates` and "Templates used" ([0042](../decisions/0042-template-expansion-and-parsoid.md) §15, in [19](19-site-ui.md)) include sheets, as MediaWiki's do.

**Both renderers.** The built-in renderer renders the element as above. Parsoid asks the render-scoped `action=parse` for tags it does not implement natively (§3.1), and gets the same element with `modules` empty; the site's links replace it as they replace the built-in renderer's. **Mirrored HTML** carries Wikipedia's `<style>` elements already, keyed `TemplateStyles:r{revid}` by the upstream revision; [0053](../decisions/0053-mirrored-pages.md) §2 passes them through `sanitize`, keeps their keys under the repository's name, and the site links them at the repository's address, so a mirrored page and a fork of it deduplicate the same way and neither carries a `<style>` element.

### 6.4 Site styles: one designated page

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §4; [0095](../decisions/0095-the-interface-namespace.md) §2.*

**A tenant may designate one `sanitized-css` page as its site styles,** with the `site` setting **`wikitext.site_styles`** naming a title in the Project namespace, `Project:Site styles.css` by convention. The Interface namespace (8, canonical `MediaWiki`) holds plain-text messages only ([0095](../decisions/0095-the-interface-namespace.md) §1, §2), so there is no `MediaWiki:Common.css`; the page is an ordinary Project page with the model of §6.1, protected as the tenant sees fit with an ACL ([0023](../decisions/0023-moderation.md) §1; the permissions are in [09](09-security-and-moderation.md)), and its history is a page history.

**It is applied to every rendered page** of the tenant, local and mirrored, as a `<link rel="stylesheet">` in the frame to `{base}/w/styles/site.css?v={offset}`, under the API's `/w/styles/` paths (§6.3), served from the sanitized form in the `css:` cache with a long lifetime, versioned by the page's latest offset. Nothing about a page's render depends on it, so editing the site styles bumps no render epoch and refreshes no page; the next request simply links the new version. The sanitizer's scoping is what makes this allowable where [0008](../decisions/0008-namespaces-and-document-pages.md) §5 refused `css`: the sheet reaches only `.mw-parser-output`, and the frame, the editor and every special page are out of its reach by construction.

**What it is for.** The classes Wikipedia's templates leave to `MediaWiki:Common.css`, `.infobox`, `.hatnote`, `.navbox`, `.mw-collapsible`, `.reflist`, which a tenant that mirrors or forks Wikipedia copies by hand from upstream's sheet, under its licence, into this page; and a tenant's own look for its content. A sheet copied from a MediaWiki `Common.css` loses, at save, whatever the sanitizer refuses, and the save reports each line.

**What it is not.** It is not a skin: the frame's own styles are the instance's ([0034](../decisions/0034-frontend-stack.md)), and no tenant CSS reaches them. It is not per-user: user styles (`User:X/common.css`) remain excluded with `css`. It is one page, not a cascade of `MediaWiki:*.css` pages per skin and per gadget.

### 6.5 `<templatedata>` and `action=templatedata`

*Sources: [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §5.*

**The tag is `implemented`.** `<templatedata>` holds a JSON object in TemplateData's schema: `description`, `params` with each parameter's `label`, `description`, `type`, `required`, `suggested`, `default`, `example`, `aliases`, `deprecated` and `autovalue`, `paramOrder`, `format`, `sets` and `maps`, with localized strings as language maps. **It is validated at save** against the schema shipped as data from the extension's `TemplateDataValidator`: a page whose `<templatedata>` is invalid is refused with the extension's own error messages (`templatedata-invalid-param`, `templatedata-invalid-type` and the rest), as TemplateData refuses it. A page may carry one `<templatedata>` block; a second is an error. The tag needs no other page, so it is implemented in the in-browser preview too.

**It renders as the parameter table** TemplateData renders: description, then one row per parameter with label, description, type, status and default, in `paramOrder`, localized to the interface language where the JSON carries that language. On a `/doc` subpage that is what readers see; on the template itself, the same.

**It is stored as the page property `templatedata`** ([0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §6, in [10](10-pages-and-content-models.md)), extracted when the page is saved, synchronously, so that `action=templatedata` answers for a template as soon as its documentation is saved. Because `/doc` pages are transcluded by their templates under `<noinclude>`, the property is **also written to the transcluding template** by the refresh job, as the extension's parser hook does, so `action=templatedata&titles=Template:Infobox` finds the data that lives on `Template:Infobox/doc`.

**`action=templatedata`** is served from `view.page_prop`, with MediaWiki's parameters: `titles` and page-set generators, `includeMissingTitles`, `doNotIgnoreMissingTitles`, `lang` (resolving the language maps), `redirects`, and `format`. For a title whose primary is foreign ([0052](../decisions/0052-page-repositories-and-title-inheritance.md) §3) it serves the repository's data, from the bundle's `pageprops` ([0053](../decisions/0053-mirrored-pages.md) §1) or, where the bundle lacks it, from the repository's own `action=templatedata`, cached; so the editor's parameter help works for inherited templates. Parsoid's call (§3.1) is served by it, which is the prerequisite its HTML-to-wikitext direction needs.

**The editor uses it.** The source editor ([0034](../decisions/0034-frontend-stack.md) §7, in [19](19-site-ui.md)) gains **Insert template…**: a search over Template titles through the stack, then a form built from the template's TemplateData, writing the call in the data's `format`; and, with the cursor inside a template call, a parameter popup listing the template's parameters with their descriptions. Without TemplateData a template inserts as `{{Name}}` and the popup says so.

## 7. Lua

*Sources: [0043](../decisions/0043-lua-modules.md) §1–§7, §9–§13.*

### 7.1 Lua is a tenant setting

*Sources: [0043](../decisions/0043-lua-modules.md) §1.*

**`wikitext.lua = on`** ([23](23-configuration-and-registry.md)) enables Lua for a tenant, capped by the instance's `wikitext.ceiling`. While it is off, `{{#invoke:}}` renders as a chip.

### 7.2 The Module namespace

*Sources: [0043](../decisions/0043-lua-modules.md) §2.*

**While `wikitext.lua` is on**, Module (828) and Module talk (829), Scribunto's numbers, are enabled:

| Namespace | Kind | Models | Default | Normalizer | Subpages | Talk |
|---|---|---|---|---|---|---|
| 828 Module | `pages` | `Scribunto`, `wikitext`, `json` | `Scribunto`, with the title rules below | `first-letter` | Yes | 829 |
| 829 Module talk | `pages` | `triplespace-talk` | `triplespace-talk` | — | — | — |

**Title rules for the default model**, as Scribunto applies them: a title ending in `/doc` is `wikitext` (the module's documentation page); a title ending in `.json` is `json` (data for `mw.loadJsonData`); anything else is `Scribunto`. All three are text models, so the namespace satisfies [0041](../decisions/0041-content-models.md) §4's single-source rule.

**While Lua is off**, 828 and 829 are registered and reserved, as §1.3 does for Template: they are entered in `namespaces.toml` with `enabled_by = "wikitext.lua"`, so the numbers are never given another meaning. Unlike core's reserved numbers, they are not listed in `meta=siteinfo` while disabled and empty, because MediaWiki without Scribunto does not list them and clients use their presence to detect Lua. Disabling Lua leaves module pages in place, refuses writes with `ts-namespace-disabled`, and leaves them readable, as §1.3 says for templates.

### 7.3 The `Scribunto` content model

*Sources: [0043](../decisions/0043-lua-modules.md) §3.*

| ID | Origin | Source | Slot | Format | Direct editing | Default in |
|---|---|---|---|---|---|---|
| `Scribunto` | Scribunto | text | main | `text/plain` | Yes | 828, by the title rules of §7.2 |

The ID is Scribunto's own, capitalized as Scribunto registers it, because clients compare it ([0041](../decisions/0041-content-models.md) §2).

**Saving checks syntax,** as Scribunto does: a module that does not compile is refused with the Lua error message, and nothing is appended. The pinned error shape comes from a contract test against the reference install ([22](22-crates-and-stack.md)).

**Rendering a module page** shows its source with syntax highlighting, and transcludes its `/doc` page above it, as Scribunto does.

**[0008](../decisions/0008-namespaces-and-document-pages.md) §5's exclusion of executable models still stands for what it was about:** models whose code reaches the reader's browser (§6.1). A Lua module never does: it runs on the server, in a sandbox (§7.4), and what it returns is wikitext that goes through the expander and the sanitizer like any other. `Scribunto` is therefore allowed; `css` and `javascript` stay excluded, and `sanitized-css` is §6.1's.

### 7.4 The runtime: Lua 5.1 in a sandbox

*Sources: [0043](../decisions/0043-lua-modules.md) §4.*

**Lua is PUC-Rio Lua 5.1.5, through `mlua`** (features `lua51` and `vendored`): the interpreter LuaSandbox embeds, and the language version modules are written for.

**One interpreter per render**, as Scribunto keeps one engine per parse. Every `#invoke` on the page shares it; each module loads into its own environment, and `require` caches within the render. It runs on the blocking thread pool, never on an async worker.

**The sandbox:**

- **Environment.** Modules see exactly the globals Scribunto gives them, the standard library as its reference manual restricts it. A conformance test enumerates the global environment on the reference install and here, and the two must match ([22](22-crates-and-stack.md)).
- **No precompiled chunks.** `load` and `loadstring` accept source text only. Lua 5.1 bytecode is not verified and is a known escape, so this is Triplespace's own rule whatever the reference does.
- **Memory.** A cap on the interpreter's allocator, Scribunto's default of 50 MiB.
- **CPU.** One budget per render covering Lua and the host callbacks it makes, Scribunto's default of 7 seconds, enforced by an instruction-count hook that checks a monotonic clock.
- **Expensive functions** share the expander's count ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16), as Scribunto shares the parser's.
- **Time.** `os.time` and `os.date` record a volatile input in the manifest (§4.2), as Scribunto's wrappers set the parser cache TTL. `os.clock` has reduced precision, as Scribunto reduces it.

The limits are deployment configuration, as the expander's are; a tenant may lower them and not raise them.

### 7.5 The Scribunto contract: Scribunto's Lua, Triplespace's host

*Sources: [0043](../decisions/0043-lua-modules.md) §5; [0095](../decisions/0095-the-interface-namespace.md) §2.*

**Scribunto's Lua libraries are vendored unchanged** from its `REL1_43` branch, matching the reference install, into `triplespace-scribunto` ([22](22-crates-and-stack.md)), with a `VENDOR.md` naming the upstream commit. Fixes that are general go upstream.

**The PHP side is reimplemented in Rust,** behind a host trait that `triplespace-render` implements.

**The base `mw` interface** is Scribunto's fifteen callbacks: `loadPackage`, `loadPHPLibrary`, `frameExists`, `newChildFrame`, `getExpandedArgument`, `getAllExpandedArguments`, `expandTemplate`, `callParserFunction`, `preprocess`, `incrementExpensiveFunctionCount`, `isSubsting`, `getFrameTitle`, `setTTL`, `addWarning` and `loadJsonData`. Frame callbacks are calls into the expander of §1.4, so `frame:expandTemplate`, `frame:preprocess`, `frame:callParserFunction` and `frame:extensionTag` expand exactly as a template would. The 100-frame limit is Scribunto's.

**The libraries**, and where the Rust side gets what it needs:

| Library | Host side |
|---|---|
| `mw.site` | Namespaces from the registry; statistics from the tenant; `interwikiMap` from site aliases ([0026](../decisions/0026-sitelinks.md) §2) |
| `mw.title` | Title parsing and facts from `triplespace-titles`; `getContent` follows the lookup of §5.2 for Template and Module titles and reads local pages otherwise, records a dependency, and returns `nil` for a page whose `read` groups are not a subset of the rendering page's, as `exists` answers false for it ([0056](../decisions/0056-security-model.md) §6); expensive accessors (`exists` on another page, `protectionLevels`, `redirectTarget`) count as MediaWiki counts them |
| `mw.text` | `jsonEncode` and `jsonDecode` with Scribunto's flags; `unstrip`, `unstripNoWiki` and `killMarkers` over the expander's strip state (§1.4); `nowiki`, `tag` and entity tables in Rust |
| `mw.ustring` | Scribunto's pure-Lua implementation, with its PHP fast paths (`find`, `match`, `gmatch`, `gsub`) reimplemented by a **native Lua-pattern matcher over code points**. Normalization through ICU4X |
| `mw.language` | `formatNum`, `formatDate` (the code behind `#time`, §2.2), plurals and case mapping through ICU4X; `fetchLanguageName(s)` from MediaWiki's language-name data, shipped as data; `convertGrammar` and `gender` as MediaWiki defines them for each language, returning the word unchanged where it defines nothing |
| `mw.message` | MediaWiki core's messages shipped as data, under the wiki's Interface pages (§2.3; [0095](../decisions/0095-the-interface-namespace.md) §2) |
| `mw.uri` | URLs under the tenant base |
| `mw.html` | Pure Lua; nothing on the host |
| `mw.hash` | The algorithms modules use in practice, through RustCrypto: `md5`, `sha1`, the SHA-2 and SHA-3 families, `crc32b` and `fnv1a32`/`fnv1a64`. `listAlgorithms` reports exactly those |
| Other libraries on the pinned branch, such as `mw.svg` | Implemented when the pinned branch has them |
| `mw.ext.ParserFunctions.expr` | The `#expr` evaluator of §2.2 |

**Other `mw.ext` libraries are absent**, as on a wiki without the extension: `mw.ext.data` (tabular data on Commons), `mw.ext.TitleBlacklist` and the rest. Modules already test for them.

### 7.6 Data modules

*Sources: [0043](../decisions/0043-lua-modules.md) §6.*

`mw.loadData` and `mw.loadJsonData` return read-only tables, so the same data can serve every page that loads it. **Triplespace caches the parsed data across renders**, keyed by the data page's ID and offset, as an L1 entry `ld:{tenant}:{pageid}:{gen}:{offset}` (with the `{vis}` segment of §4.5), and builds each render's table from the cached tree instead of running the module again. MediaWiki caches it only within one parse. A foreign data module (§5) is keyed by repository, title and remote revision instead.

### 7.7 Wikibase Client: `mw.wikibase`, `#property` and `#statements`

*Sources: [0043](../decisions/0043-lua-modules.md) §7.*

**The tenant is its own Wikibase client.** Its modules read its resolved view: local entities and those of the providers it reads ([0018](../decisions/0018-tenants.md) §3), as `wbgetentities` serves them, with redirects followed as Wikibase Client follows them.

**`mw.wikibase` and `mw.wikibase.entity` are vendored from WikibaseClient's `REL1_43`,** and their callbacks are implemented in Rust: `getEntity`, `getEntityStatements`, `getEntityId`, `getEntityUrl`, `getLabel`, `getLabelByLanguage`, `getDescription`, `getDescriptionByLanguage`, `getSiteLinkPageName`, `getBadges`, `isValidEntityId`, `entityExists`, `renderSnak`, `formatValue`, `renderSnaks`, `formatValues`, `resolvePropertyId`, `orderProperties`, `getPropertyOrder` and `getReferencedEntityId`. `getGlobalSiteId` is §7.8's client site.

**Entities reach Lua in Wikibase's Lua serialization:** its JSON with arrays numbered from 1, after the ID translation of the Lua ID space ([0043](../decisions/0043-lua-modules.md) §8, in [04](04-entities-and-identifiers.md)). Arbitrary access, any entity other than the page's own, counts as an expensive function, as Wikibase Client counts it, and is capped by Wikibase Client's `entityAccessLimit` at its default.

**`{{#property:}}` and `{{#statements:}}`** are implemented with Wikibase Client's parameters (`from=` for arbitrary access) and its output: plain text for `#property`, rich wikitext for `#statements`.

**Formatting** (`renderSnak`, `formatValue`, `#statements`) produces the wikitext Wikibase Client produces for each data type, pinned by contract tests against the reference install. Triplespace's own data types, such as `wikibase-domain`, `wikibase-keyword` and `localMedia`, format as links to their pages.

**`mw.wikibase.lexeme`** ([0066](../decisions/0066-lexemes.md) §9) is provided as WikibaseLexeme documents it, with its `entity` classes for lexemes, forms and senses, once Lexeme (146) is implemented; usage is tracked per lexeme ID.

### 7.8 The client site

*Sources: [0043](../decisions/0043-lua-modules.md) §9.*

**A Wikibase client knows which site it is,** and that decides which item a page is connected to. The `site` setting **`lua.client_site`** names a site ID with a site alias ([0026](../decisions/0026-sitelinks.md) §2). The default is the tenant's own alias, which makes the connected item the paired item of [0038](../decisions/0038-page-metadata-and-categories.md) §6.

`getGlobalSiteId` returns it. `getEntityIdForCurrentPage` and `getEntityIdForTitle` find the item whose sitelink on that site's host matches the title under the alias's article path. `getSitelink` with no site argument uses it. The `S` and `T` usage aspects (§7.9) refer to it.

**A Wikipedia mirror sets `lua.client_site = "enwiki"`.** Its page `Douglas Adams` is then connected to the item with the sitelink `https://en.wikipedia.org/wiki/Douglas_Adams`, which is `WDQ42`, and Lua sees `Q42`.

### 7.9 Usage tracking and invalidation

*Sources: [0043](../decisions/0043-lua-modules.md) §10; [0083](../decisions/0083-write-path-in-three-tiers.md) §3.*

**Every entity read records Wikibase's usage aspects** in the render manifest (§4.2), with Wikibase Client's codes:

| Aspect | Meaning |
|---|---|
| `S` | Sitelinks |
| `L.{lang}` | The label in one language; `L` for all |
| `D.{lang}` | The description in one language; `D` for all |
| `T` | The page's own sitelink on the client site (§7.8) |
| `C.{property}` | Statements of one property; `C` for all |
| `O` | Anything else, such as aliases |
| `X` | Everything |

**They are stored in `view.entity_usage`** `(page_id, entity_id, aspect, modifier)`, with canonical IDs, written by the refresh job (§4.3), and the counterpart of Wikibase's `wbc_entity_usage`.

**Invalidation is a tier-3 consumer of composition** ([0083](../decisions/0083-write-path-in-three-tiers.md) §3). The composer emits "entity X, tenant T, composed to version N" after each composed row commits, and the entity-usage consumer reads those events in its own queue, with its own rate and lag, never inside the appending or composing transaction. For each event it **first checks `view.entity_usage` for existence**, by index, for the canonical ID and every member of its cluster, and does nothing further when no page uses any of them, which is the common case on a mirror of millions of entities; only then does it classify the change from the structured diff of the previous and new composed rows ([0012](../decisions/0012-api-requirements.md) §7, in [18](18-api.md)): which languages' labels and descriptions, which properties' statements, which sitelinks, and whether it was a redirect or a change of cluster. Only pages whose aspects match get an epoch bump and a refresh, served stale-while-revalidate meanwhile (§4.5). A label edit in German does not re-render pages that read only English labels. The consumer is **off while no page on the instance uses an entity**, since no manifest then records one, and off during bootstrap and bulk modes until the operator turns it on; its lag is reported on its own page, and a page's render may be behind an entity by that lag.

### 7.10 Entities the tenant does not hold

*Sources: [0043](../decisions/0043-lua-modules.md) §11.*

**An entity the tenant's view does not hold is missing to Lua,** as an entity that does not exist is: `getEntity` returns `nil` and `entityExists` returns `false`. That happens when a provider is mirrored only in part ([0002](../decisions/0002-source-graphs-and-mass-ingest.md) §8.3, in [05](05-providers-and-ingest.md)). The render records the miss with a short expiry, the tenant `site` setting `lua.unheld_ttl` (default 10 minutes), so the page picks the entity up once a sync brings it in, and `render_state.unheld_misses` counts the misses (§4.3) so an operator can see which unheld entities pages ask for.

Not yet: fetching unheld entities on demand is open.

### 7.11 Errors, limits and tracking categories

*Sources: [0043](../decisions/0043-lua-modules.md) §12.*

**A script error renders as Scribunto renders it:** a "Lua error" span whose details open on click, and the tracking category `scribunto-common-error-category` (§4.1). Limit overruns (memory, CPU, expensive functions, entity access) are script errors with Scribunto's messages. The limit report gains Scribunto's `scribunto-limitreport-timeusage` and `scribunto-limitreport-memusage`.

### 7.12 Editing modules

*Sources: [0043](../decisions/0043-lua-modules.md) §13.*

The source editor gains CodeMirror's Lua mode ([19](19-site-ui.md)). **Previewing a module** runs a chosen page with the edited module in its place, server-side, under the `parse` rate class ([0042](../decisions/0042-template-expansion-and-parsoid.md) §16, in [07](07-actors-and-accounts.md)), as Scribunto's "Preview page with this module" does. Editing needs `edit`, as in MediaWiki; protection works as for any page.

## 8. Conformance

*Sources: [0033](../decisions/0033-backend-stack.md) §9.1; [0055](../decisions/0055-templatestyles-templatedata-and-page-properties.md) §2.*

The wikitext renderer has two test sources:

- A subset of MediaWiki's `tests/parser/parserTests.txt` covering only the rendered subset, run in CI as a fixture. The file is GPL-2.0-or-later, which is compatible with Triplespace's licence; it is test data and is not compiled into the binary.
- A differential test against Parsoid on the MediaWiki 1.43 reference install ([0000](../decisions/0000-init.md)): sampled pages rendered by both, DOMs normalized and compared for the supported constructs.

TemplateStyles' css-sanitizer test suite runs in CI as fixtures (GPL-2.0-or-later test data, not compiled in, as the parser tests are, [0033](../decisions/0033-backend-stack.md) §15), and the differential test of [0042](../decisions/0042-template-expansion-and-parsoid.md) §18 compares the `<style>` elements the reference install emits for the same templates. The conformance of the expander, the registry and the Lua runtime against the reference install ([0042](../decisions/0042-template-expansion-and-parsoid.md) §18; [0043](../decisions/0043-lua-modules.md) §15) is [22](22-crates-and-stack.md)'s.
