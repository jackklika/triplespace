# 0034. Frontend technology stack

- **Status:** Proposed
- **Date:** 2026-09-27
- **Updated:** 2026-11-04 (A15)
- **Author:** James Hare / Claude
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md)
- **Uses:** [0003](0003-statement-ui.md), [0012](0012-api-requirements.md), [0014](0014-caches-and-search.md), [0017](0017-entity-id-grammar.md), [0029](0029-resolver-namespaces.md), [0033](0033-backend-stack.md)

## Context

0003 and 0010 describe what the statement UI and the site UI show. 0010 left open whether pages are rendered on the server or the client, and how mobile layouts work. 0012 §1 requires the UI to use only the public API and to discover capabilities. 0014 caches pages at an HTTP layer with `ETag` and `Cache-Tag`.

James decided on 2026-09-27 that **the designs adopt Codex, Wikimedia's design system, as-is**. Codex ships Vue 3 components and CSS-only components that look identical to them; most Vue components have a CSS-only version, and the accessible interactive ones (Lookup, Combobox, Menu, Dialog, TypeaheadSearch) need Vue. It targets WCAG 2.1 AA with wide i18n and RTL support, and has been bundled with MediaWiki since 1.39.

The backend stack is in 0033.

## Decision

### 1. Principles

*Changed by A6, A7, A8, A12.*

*Current text: [19](../architecture/19-site-ui.md) §1, §5.1.*

### 2. Server rendering, in `triplespace-ui`

*Changed by A6, A8, A14.*

*Current text: [19](../architecture/19-site-ui.md) §1.2, §5.2.*

### 3. Statement groups (uses 0003 §3)

*Current text: [19](../architecture/19-site-ui.md) §4.9.*

### 4. Interactive components

*Changed by A3.*

*Current text: [19](../architecture/19-site-ui.md) §3.10, §5.3.*

### 5. Fragments

*Changed by A6.*

*Current text: [20](../architecture/20-web-tier.md) §3.2.*

### 6. Rust in the browser, in `scatter-wasm`

*Changed by A1, A5, A9.*

*Current text: [19](../architecture/19-site-ui.md) §5.4.*

### 7. Source editor

*Changed by A2, A5.*

*Current text: [19](../architecture/19-site-ui.md) §3.2, §5.5.*

### 8. Build

*Changed by A6, A8.*

*Current text: [22](../architecture/22-crates-and-stack.md) §5.1.*

### 9. Internationalisation

*Changed by A11, A14, A15.*

*Current text: [19](../architecture/19-site-ui.md) §5.6.*

### 10. Accessibility and browser support

*Changed by A7.*

*Current text: [19](../architecture/19-site-ui.md) §5.7.*

### 11. Security

*Changed by A6, A12, A13.*

*Current text: [09](../architecture/09-security-and-moderation.md) §1.4.*

### 12. Testing

*Current text: [22](../architecture/22-crates-and-stack.md) §5.2.*

### 13. Prototyping path

*Changed by A6.*

*Current text: [19](../architecture/19-site-ui.md) §5.8.*

## Alternatives considered

- **Leptos (full-stack Rust, server rendering with islands).** One language and one component tree, but Codex's accessible interactive components would have to be rebuilt in Rust, and Leptos is still pre-1.0. It would also narrow the contributor pool to Rust developers.
- **Vue single-page app with Nuxt server rendering.** Needs a Node runtime in production, against 0033 §1.
- **htmx-style fragments only.** Covers updates but not accessible comboboxes, lookups and dialogs, which Codex already provides.
- **OOUI.** Superseded by Codex in MediaWiki.

## Consequences

- Wikimedia developers who know Vue and Codex can work on the editors without learning Rust; Rust developers own the templates and renderers.
- Pages read and cache without JavaScript, and the statement layouts exist in one implementation.
- The build needs Node, though the running system does not.
- Page rendering can run on machines of its own, as `triplespace-web`, and scale apart from the API ([0057](0057-web-tier.md)).
- Codex upgrades are deliberate: versions are pinned and bumped in their own commits, with snapshot and accessibility tests as the check.

## Open questions

- **Q1.** ~~Codex upgrade cadence, and whether to track the version bundled with a MediaWiki release.~~ *Settled by A8: the latest release, bumped deliberately in its own commit; MediaWiki's bundled version is not tracked.*
- **Q2.** ~~Night mode: adopt Codex's dark-mode tokens when they are stable.~~ *Settled by [0086](0086-theming.md) §2: Codex's dark tokens are the base a palette's `dark` variant overlays, and `parchment` ships a walnut-brown dark variant.*
- **Q3.** Whether the Rust Codex builder becomes a published crate other projects can use.
- **Q4.** An interactive charting library for exploring a property's full view (0003), beyond the static SVGs.
- **Q5.** Visual regression testing.
- **Q6.** The maturity of the `banana-i18n` Rust crate (§9).

## Amendment log

### A1. Expansion preview on the server

- **Date:** 2026-09-30
- **Source:** [0042](0042-template-expansion-and-parsoid.md) §15
- **Change:** amends §6
- **Summary:** When the source needs expansion the browser cannot do, the preview comes from `action=parse` on the server instead.

Replaced text (§6):

> It is built with `wasm-bindgen-cli` and `wasm-opt`, lazy-loaded only by editors, and never needed for reading. This is one reason 0033 §9.1 chose a pure-Rust wikitext parser.

### A2. A Lua mode

- **Date:** 2026-09-30
- **Source:** [0043](0043-lua-modules.md) §13
- **Change:** extends §7
- **Summary:** A Lua mode for module pages.

### A3. The table grid editor

- **Date:** 2026-09-30
- **Source:** [0045](0045-table-content-model.md) §11
- **Change:** extends §4
- **Summary:** A table grid editor (Codex Table, Lookup, TextInput; the statement value editor for cells) over the server-rendered grid of a `Table` page. Reading a table needs no JavaScript.

### A4. Converted to the 0050 format

- **Date:** 2026-10-01
- **Source:** [0050](0050-adr-format.md) §13
- **Change:** consolidates §1–13
- **Summary:** A1–A3 were folded into the Decision. The title's em dash became a full stop, the open questions were numbered, the Author line was added, and the list this ADR kept under "Changes to other ADRs" (quoted below) was replaced by the generated table. That list also claimed changes to 0003 §3, 0012 §5 and 0014 §5 (renderers, the fragment routes, fragment caching); none was ever written into those ADRs, and nothing records them there, so the fragment routes and their caching are stated in §5 of this ADR alone. No decision changed. Before this, A1–A3 were blockquotes. The file before conversion is commit `0b26a3a`.

Replaced text (Changes to other ADRs):

> - **0010:** the server-or-client rendering question is settled by §1–§5; mobile is settled as one responsive site (§2), with detailed layouts still open.
> - **0003:** renderers and chart drawing per §3.
> - **0012:** fragment routes of §5 added to the REST route groups.
> - **0014:** fragments cached and purged like pages (§5).
> - **0005:** new crates `triplespace-ui` (surfaces layer) and `scatter-wasm` (wasm bindings over pure crates); `ui/` workspace noted; changelog row.

### A5. CSS linting and TemplateData in the editor

- **Date:** 2026-10-01
- **Source:** [0055](0055-templatestyles-templatedata-and-page-properties.md) §2, §5
- **Change:** extends §6, §7
- **Summary:** `scatter-wasm` re-exports `scatter-css` for linting stylesheets; the source editor gains a CSS mode, Insert template… and a TemplateData parameter popup.

### A6. The web tier

- **Date:** 2026-10-03
- **Source:** [0057](0057-web-tier.md) §1, §2, §4, §8
- **Change:** amends §1, §2, §5, §8, §11; extends §13
- **Summary:** The server-side renderer is an API client like the browser: it calls the public HTTP API with the viewer's credentials, from `triplespace-web` over the network or from `triplespace-server` in-process, and never the API crates' handlers. Either binary serves the site and its assets. Fragment routes are gone: a component fetches its region with `action=render` and `region` on the page's own URL, which also ends the collision of `GET /page/{id}/render` with 0042 §12's render manifest. In development the web tier can be pointed at Wikidata directly. A consequence is added.

Replaced text (§1):

> 5. **The public API only** (0012 §1). The browser calls public routes. The server-side renderer calls the API crates' handler layer in-process, never `view` tables directly, so every page gets the same per-viewer redaction as the API.

Replaced text (§2):

> - **Assets:** Codex CSS, design tokens and icons come from the pinned `@wikimedia/codex`, `@wikimedia/codex-design-tokens` and `@wikimedia/codex-icons` packages at build time (§8), with hashed file names, served by the binary with long-lived cache headers.

Replaced text (§5):

> ### 5. Fragment routes
>
> After a component saves, it fetches the server's rendering of the changed region and swaps it in. The fragments are public API routes under `rest.php/triplespace/v0` (0012 §2), in the spirit of Wikibase's `wbformatvalue`:
>
> - `GET /entity/{id}/render/statements/{property}`: one statement group.
> - `GET /entity/{id}/render/terms`: the term box.
> - `GET /page/{id}/render`: a document or thread body.
>
> They carry the same `ETag`, `Cache-Tag` and redaction as full pages (0014 §3). Full page renders are assembled from the same fragment functions, so a fragment and its page cannot drift.

Replaced text (§8):

> - The release build embeds `ui/dist` into the binary (`rust-embed`); templates read the manifest to emit `<script type="module">` tags. **Node is a build-time dependency only**; the deliverable stays one binary (0033 §1).
> - In development, the server proxies asset requests to the Vite dev server for hot reload.

Replaced text (§11):

> - Components never insert HTML they built from user input. HTML they insert comes only from fragment routes, which the server has sanitized (0033 §9).

### A7. Codex, themed by tokens

- **Date:** 2026-10-03
- **Source:** Direct: James, design discussion of 2026-10-03
- **Change:** amends §1; extends §10
- **Summary:** Codex stays, for its accessible components, its right-to-left and language support and its CSS-only components, but no longer strictly as-is: the instance or a tenant may set Codex's design tokens to its own values as a theme (`ui.theme`), served as a stylesheet of custom properties and refused unless its colours meet WCAG 2.1 AA contrast. The site reads the theme from `siprop=triplespace`, since it reaches the instance only through the API, and serves it under a URL named by its hash. Components, markup and behaviour stay Codex's. James, asked whether to keep Codex at all: "I like your middle path with Codex. It has more to do with the accessibility and right-to-left benefits than pure visual or UX fidelity. Being able to get HTML-only too is a bonus."

Replaced text (§1):

> 4. **Codex as-is.** Codex tokens, components and icons without overrides. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream.

### A8. The shipped default theme; the Codex release

- **Date:** 2026-10-03
- **Source:** Direct: James, design discussion of 2026-10-03
- **Change:** amends §1; extends §2, §8
- **Summary:** "Let's ship the canvases' palette and Newsreader headings as the shipped default theme." A site with no `ui.theme` wears `default` from the new `docs/registry/themes.toml`: Codex token values taken from the site and statement UI canvases, with Newsreader for headings (`font-family-heading-main`), IBM Plex Sans for text and IBM Plex Mono for IDs, every text pair at WCAG 2.1 AA and the boundaries of controls at 3:1; the canvases' light input borders were darkened to meet the latter. The typefaces are self-hosted under the OFL. "Codex latest release": Codex is pinned at its latest release, 2.7.0, and bumped deliberately, which settles Q1.

Replaced text (§1):

> Without a theme, Codex's own values apply.

### A9. The wasm list lives in 0005 rule 7

- **Date:** 2026-10-09
- **Source:** Direct: James, design discussion of 2026-10-09
- **Change:** corrects §6
- **Summary:** [0005](0005-crate-organization.md) §3 rule 7 is the one wasm list: the nine crates its table marks. §6 and `cargo xtask wasm` read that list rather than keeping a list of their own; §6's four-item enumeration of what `scatter-wasm` re-exports is replaced by a reference to rule 7. (PENDING F23)

Replaced text (§6):

> One `wasm-bindgen` crate re-exports what editors need from the pure crates:
>
> - the 0003 shape classifier;
> - `scatter-normalize` (value normalization and keyed-ID grammars, 0017, 0029);
> - `scatter-wikitext` and the `scatter-pages` markdown renderer, for live preview;
> - `scatter-css`, so the editor lints a `sanitized-css` page as it is typed ([0055](0055-templatestyles-templatedata-and-page-properties.md) §2).

### A10. Current text relocated to the architecture chapters

- **Date:** 2026-10-09
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [09](../architecture/09-security-and-moderation.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [22](../architecture/22-crates-and-stack.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `c76d96f`. No decision changed.

### A11. The interface language is negotiated

- **Date:** 2026-10-17
- **Source:** [0085](0085-settings-and-preferences.md) §2.2
- **Change:** amends §9
- **Summary:** A reader is served in their language without having to choose one: the interface language comes from `uselang`, then the reader's `language` preference, then the request's `Accept-Language` matched against the languages the interface has messages for, then the tenant's default (`site.language` unless `preferences.defaults` names another) and the farm's. Readers who are not logged in are served the same way. The chapter text is [19](../architecture/19-site-ui.md) §5.6.

Replaced text ([19](../architecture/19-site-ui.md) §5.6, as it stood):

> - Interface language comes from the `language` preference ([07](../architecture/07-actors-and-accounts.md)) with `uselang` as an override; fallback chains follow MediaWiki's.

### A12. Palettes and type sets replace the flat theme

- **Date:** 2026-10-17
- **Source:** [0086](0086-theming.md) §1, §2, §5
- **Change:** amends §1; extends §11
- **Summary:** The flat theme of A7, one map of Codex token values in `ui.theme`, becomes two choices held as config records: a palette, with `light`, `dark`, `light-more` and `dark-more` colour variants and the shape tokens, each variant held to its contrast floor, and a type set, with the three typeface roles and the typefaces it serves. The shipped default of A8 is the palette `parchment` (the same light values, now with a walnut-brown dark variant and high-contrast variants) with the type set `default26`; for accessibility, the palettes `parchment-ob` and `parchment-co`, for colour-vision deficiencies, and the type sets `hyperlegible` and `opendyslexic` ship, and a reader may always choose them. A reader chooses mode, contrast, text size, palette and type set, the last two within what the wiki offers. One stylesheet per palette and one per type set hold their variants and modes under `data-*` attributes and media queries. `font-src` and `img-src` admit the media origin, where a tenant's typefaces and logo are served as site assets. Q2 is settled: Codex's dark tokens are the base every palette's `dark` overlays. `ui.theme` is retired. The chapter text is [19](../architecture/19-site-ui.md) §5.1, §5.2 and §5.9, [09](../architecture/09-security-and-moderation.md) §1.4, [02](../architecture/02-graphs-rdf-and-query.md) §1.4 and [23](../architecture/23-configuration-and-registry.md) §3.2.

Replaced text ([19](../architecture/19-site-ui.md) §5.1, as it stood):

> 4. **Codex components, themed by tokens.** Codex's components, icons and markup are used without overrides. Their look comes from Codex's design tokens, and the instance or a tenant may give those tokens values of its own as a **theme** (`ui.theme`, [0015](0015-record-format-and-partition-registry.md) §3; [23](../architecture/23-configuration-and-registry.md)): colours, typefaces, radii and spacing, never a component's markup or behaviour. A theme is served as a stylesheet of CSS custom properties, so it needs no inline style under the security policy of [0034](0034-frontend-stack.md) §11 ([09](../architecture/09-security-and-moderation.md)), and it is refused when its colours fail WCAG 2.1 AA contrast for the token pairs Codex uses for text, borders and focus. The theme's values are reported in `meta=siteinfo&siprop=triplespace` ([0012](0012-api-requirements.md) §4; [18](../architecture/18-api.md)), from which the site reads them as it reads any setting, and the site serves them as a stylesheet named by a hash of its values, so a changed theme is a new URL and each one caches as immutable. Without one, the site wears the **shipped default theme**, `default` in `docs/registry/themes.toml`: the palette and type of the site and statement UI canvases, Newsreader for headings over IBM Plex Sans and IBM Plex Mono, on a warm ground; tokens it does not name keep Codex's values. Where a design needs something Codex lacks, it is built from Codex tokens and proposed upstream. Codex is used for its accessible components, its right-to-left and language support, and CSS-only components that need no JavaScript, more than for its look.

Replaced text ([19](../architecture/19-site-ui.md) §5.2, as it stood):

> The default theme's typefaces (Newsreader, IBM Plex Sans, IBM Plex Mono, all under the SIL Open Font License) are served the same way, from the site's own origin, as the policy of [0034](0034-frontend-stack.md) §11 requires.

Replaced text ([02](../architecture/02-graphs-rdf-and-query.md) §1.4, as it stood):

> | `themes.toml` | Themes as values for Codex design tokens, and the typefaces each serves | [0034](0034-frontend-stack.md) §1 |

Replaced text ([23](../architecture/23-configuration-and-registry.md) §3.2, as it stood):

> | `ui.theme` | `site`; the instance or a tenant | tenant | Codex token values | Codex's own values | The theme a site is served with; refused when its colours fail WCAG 2.1 AA contrast | [0034](0034-frontend-stack.md) §1 |

### A13. `style` attributes are allowed

- **Date:** 2026-10-20
- **Source:** Direct: James, review of 2026-10-20
- **Change:** amends §11
- **Summary:** The site's Content Security Policy adds `style-src-attr 'unsafe-inline'`: `style` attributes apply, while stylesheets still come only from the instance's origin and `<style>` elements stay refused. Wikitext writes `style` attributes and MediaWiki's Sanitizer, which the renderer follows ([0008](0008-namespaces-and-document-pages.md) A39), checks each value, so blocking them took the styling off content the API had already accepted: infoboxes, navboxes, coloured table cells and hidden elements, above all on mirrored Wikipedia pages. A style attribute cannot run script; what it can still do is lay content over the page, as on MediaWiki, where moderation answers it. `style-src-attr` is in every current browser engine. The policy does not yet admit the `<style>` elements `<templatestyles>` emits and mirrored Wikipedia HTML carries ([11](../architecture/11-rendering-templates-and-modules.md) §6.3); whether they come in as a hash per element in each page's own policy or as stylesheets served from the origin is not decided here. The chapter text is [09](../architecture/09-security-and-moderation.md) §1.4.

Replaced text ([09](../architecture/09-security-and-moderation.md) §1.4, its first sentence, as it stood):

> The site UI runs under a strict Content Security Policy: scripts only from the instance's own origin, no inline scripts, no `eval`.

### A14. Skins and message bundles

- **Date:** 2026-10-29
- **Source:** [0097](0097-versioning-and-skins.md) §8, §9, §10, §11, §12, §13
- **Change:** amends §2, §9
- **Summary:** The site's HTML is drawn by a skin the operator installs: MiniJinja templates over the view models `triplespace-ui` builds, under a versioned template contract, with nine template functions, named menus and link classification. The default skin is a skin; it first reproduces today's pages, and the built-in `askama` rendering and the Codex builder are then removed, the Codex components becoming the default skin's macros. Interface messages come in versioned bundles, the binary's in `i18n/` and each skin's in its own, a skin's keys carrying its name as a prefix. The chapter text is [19](../architecture/19-site-ui.md) §5.2, §5.6 and §5.10.

Replaced text (§2):

> - **Templates:** `askama`, compiled and type-checked with the Rust code.
> - **Codex markup:** a Rust builder module that emits Codex CSS-only component markup (buttons, fields, tables, cards, tabs, messages, chips, progress bars), following the approach of WMF's Codex PHP. Templates call the builder instead of writing Codex class names by hand, so a Codex markup change is one edit.

Replaced text (§9):

> - Interface messages are **banana JSON** files in `i18n/` (`en.json`, `qqq.json` for documentation, one file per language), so the project can be registered with translatewiki.net like any MediaWiki extension.

### A15. `uselang=qqx` shows message keys

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while building template debugging (SCA-230), 2026-11-04; unconfirmed
- **Change:** extends §9
- **Summary:** `uselang=qqx` makes every interface message show its key, as MediaWiki's `qqx` does (the ShowMessageNames gadget on Wikipedia, Wikidata and Wikisource): `(key)`, or `(key: a, b)` with the parameters given. The calls:
  - The wiki's Interface overrides are not read under `qqx`, so the key shows even for an overridden message, and nothing is expanded (`PLURAL`, `GENDER`, `GRAMMAR`).
  - Messages handed to a script read `(key: $1, $2)`, one `$n` up to the highest the English message names, for the script to fill as it fills any message.
  - `qqx` is the page's `lang` and is written left to right; a term (label, description) is looked up along `qqx`'s chain, which ends at `mul` and English, so content shows as in English.
  - `{{int:key}}` in page text is not affected: a page renders once for every reader in the wiki's language (0042 A21), so `qqx` reaches only the site's own messages.
