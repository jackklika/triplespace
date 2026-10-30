# 0097. Versioning, and skins apart from the binary

- **Status:** Proposed
- **Date:** 2026-10-29
- **Updated:** 2026-10-30 (A3)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0015](0015-record-format-and-partition-registry.md), [0034](0034-frontend-stack.md), [0057](0057-web-tier.md), [0077](0077-special-version.md), [0086](0086-theming.md), [0095](0095-the-interface-namespace.md)
- **Uses:** [0012](0012-api-requirements.md), [0028](0028-tenancy-policy.md), [0050](0050-adr-format.md), [0055](0055-templatestyles-templatedata-and-page-properties.md), [0058](0058-packed-record-storage.md), [0085](0085-settings-and-preferences.md)

## Context

Nothing in Triplespace had a version policy. `[workspace.package]` set no `version`, so every crate was `0.0.0`. The log header carried a format version and payload types a `v0` tag ([0015](0015-record-format-and-partition-registry.md) §2), but nothing said what a new one meant. `meta=siteinfo` reported `api_version` as an integer, and the web tier refused to serve below the minimum it was built for ([0057](0057-web-tier.md) §9), but nothing said what a change to that integer promised. The registries carried `version = 1` with no rule for the next number.

The site's HTML was built in `triplespace-ui`: a frame and nine Codex component templates in `askama`, which compiles templates into the binary, and the rest, about 28,000 lines of Rust, writing HTML through string builders ([0034](0034-frontend-stack.md) §2). Changing how the site looked meant rebuilding the binary. Theming ([0086](0086-theming.md)) already let a tenant choose colours and typefaces by setting Codex's tokens, without touching markup.

Triplespace has no wiki-editable JavaScript: user and site scripts are excluded, and adoption drops `MediaWiki:Common.js` ([0095](0095-the-interface-namespace.md) §7). What MediaWiki communities build as gadgets and user scripts has to be built into the binary, done by whoever controls the HTML, or not done.

### Direction

James's direction, from the design discussion of 2026-10-29:

- **"I think we should implement the principles of semantic versioning. Major dot minor dot patch. A patch version addresses pre-existing functionality. You increment the minor version when you introduce a new feature without breaking backwards compatibility. You increment the major version when you break backwards compatibility. If the major version is 0, the compatibility contract is considered unstable."** Asked whether that meant Cargo's stricter reading of 0.x, where `0.3 → 0.4` may break and `0.3.1 → 0.3.2` may not: "The stricter reading you are referring to is what I meant."
- **"What compatibility and backwards compatibility means depends on the thing. … Where we are implementing someone else's contract, we should use their version numbers. Our own APIs should also follow the principles of semantic versioning."**
- **"We could uncouple the concept of a skin from the UI in general. The frontend binary fields requests and produces responses, but the HTML structure is a static document that could easily be uncoupled from it. Anything can be a skin so long as it is compatible with the template structure the binary is expecting. The default appearance would become a skin, and it would have its own versioning. A skin is different from a theme. Theming includes customizing the color palette and type set within the HTML presentation while a skin is the HTML presentation."**
- **"If we uncouple static assets from the binary, we could then separately version themes, skins, interface message bundles, and then finally the URL structure, templating structure, and API contract are what's versioned."**
- **"Skins can only be installed by instance operators."** "Since skins are deployed only by instance operators, there is a higher degree of trust."
- **"We should aim for a minimum number of highly useful functions that target the most desired features. What do people keep reinventing in gadgets and user scripts?"**
- **"I agree that theme tokens are part of the binary."**
- **"Readers can choose from the skins that the instance makes available. Whether a tenant can make a skin available or disable it depends on instance policy."** And, asked whether a release that writes a new record major should first ship it behind a switch so that every minor can be rolled back: "Yes."
- **"The patch number is incremented when a new version of something is published. Something is 'published' when it is compiled as a downloadable bundle, shipped to crates.io, or equivalent thing that marks that the code has stabilized. Once the patch is published, the patch number increases in the repository. That means that a site running directly off of git is going to be at minimum one patch version higher than someone using a published binary. If you add a new feature or make a compat-breaking change, you increase the minor or major version number once it happens, with the patch or minor version set to 0 accordingly. Ideally this sort of work is done on a branch so as to not conflict with existing stable code and stable deployments."**
- **"One way we can test the skinning system is by reimplementing the current frontend UI as a skin, with the goal of getting identical results between the built-in implementation and the skin version. Once equality is accomplished, the internal rendering can be removed."**

## Decision

### 1. Semantic versioning, read as Cargo reads it

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 2. What is versioned

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 3. Crates (extends 0005 §6)

*Current text: [22](../architecture/22-crates-and-stack.md) §3.4.*

### 4. Record formats (extends 0015 §2)

*Current text: [01](../architecture/01-log-and-records.md) §2.2.*

### 5. The REST API and `api_version` (amends 0057 §9)

*Current text: [20](../architecture/20-web-tier.md) §4.*

### 6. Contracts Triplespace implements

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

### 7. The product version (extends 0077 §6)

*Current text: [21](../architecture/21-special-pages.md) §5.6; [22](../architecture/22-crates-and-stack.md) §3.5.*

### 8. Skins (amends 0034 §2)

*Changed by A2.*

*Current text: [19](../architecture/19-site-ui.md) §5.2, §5.10; [21](../architecture/21-special-pages.md) §5.6; [23](../architecture/23-configuration-and-registry.md) §3.4.*

### 9. The template contract (extends 0034 §2)

*Changed by A2.*

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 10. Functions a template may call (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 11. Menus (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 12. Link classification (extends 0034 §2)

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 13. Message bundles (amends 0034 §9; extends 0095 §2)

*Changed by A2.*

*Current text: [19](../architecture/19-site-ui.md) §5.6.*

### 14. Theme tokens (extends 0086 §5)

*Current text: [19](../architecture/19-site-ui.md) §5.9.*

### 15. Caching and fragments (extends 0057 §6, §8)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §10.3; [19](../architecture/19-site-ui.md) §5.10; [20](../architecture/20-web-tier.md) §3.2.*

### 16. Reaching parity, then removing the built-in rendering

*Changed by A2, A3.*

*Current text: [19](../architecture/19-site-ui.md) §5.10.*

### 17. Crates (amends 0005 §2)

*Changed by A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 18. Releases and the version in the repository

*Current text: [22](../architecture/22-crates-and-stack.md) §3.5.*

## Alternatives considered

- **One version for the whole product, nothing else.** MediaWiki has been 1.x for twenty years because a product major was never defined; operators learn nothing from the number, and a skin or client cannot say what it needs.
- **Calendar versions for the product.** Honest about a product's cadence, but they say nothing about whether an upgrade is safe, which is what an operator needs from the number.
- **Skins as Rust crates compiled in.** Keeps `askama`'s type checking, but every skin change is a rebuild, which is the coupling this ADR removes.
- **Jinja's full power, or logic-less templates.** MediaWiki's SkinMustache made skins logic-less over a documented set of data keys, which is what fixed its skins breaking on every release; Triplespace keeps that idea, a contract made of view models, but uses MiniJinja so that a trusted skin can loop, branch and define its own macros. The functions it adds are kept to §10's nine, since each is a contract.
- **Tenant-installed skins.** A skin is HTML around every reader's session, CSRF tokens and scripts included; tenants keep palettes and type sets (0086).

## Consequences

- **Every release states its bump and why**, from the contracts it changed (§7).
- **The site's look can change without a rebuild**, and an operator can run a skin of their own.
- **What gadgets do on MediaWiki has a home**: data in view models and menus, behaviour in an operator's skin.
- **Interactive components stay Codex's**: the Vue components and editors are the binary's, styled by Codex and the theme under any skin; a skin restyles them only through tokens.
- **The migration is large**: about 28,000 lines of `triplespace-ui` become view models and templates, and every view model it needs is found by doing it.
- **Rendering at runtime costs more than compiled templates.** MiniJinja's compiled templates are expected to keep a page within its budget; the corpus runs measure it.

## Open questions

- **Q1.** ~~**Rollback.** Whether a minor that introduces a record major ships it behind a switch first, off by default for one minor, so that every minor can be rolled back.~~ *Settled by §4: yes; the release that writes it by default is a product major.*
- **Q2. Additive payload changes.** Whether the current payload encodings admit fields an older reader ignores, as §4's minor assumes, or whether every payload change is in practice a major until the encodings are made extensible.
- **Q3.** ~~**A reader's own skin.** Whether a reader may choose among the installed skins, as MediaWiki's skin preference allows, or the wiki's skin is the only choice.~~ *Settled by §8: readers choose among the skins the wiki makes available; what a tenant may make available or disable is the instance's default, ceiling and lock for `skins.available`.*
- **Q4. Skin licences.** Plain and Worksheet are GPL-3.0-or-later with the rest of the repository. Whether an operator's skin, which the binary interprets and does not link, must be.
- **Q5. Extra view-model fields.** Whether a skin may ask for data from a fixed list the binary offers (assessment, protection, short description, edit counts), fetched only when a skin asks for it; deferred until the default skin needs one.

## References

- [Semantic Versioning 2.0.0](https://semver.org/)
- [The Cargo Book: SemVer compatibility](https://doc.rust-lang.org/cargo/reference/semver.html), and `cargo-semver-checks`
- MediaWiki: `SkinMustache` and its template data, `skin.json` and its `requires`, and `mw.util.addPortletLink`
- Gadgets and user scripts on English Wikipedia and Wikidata: linkclassifier, MarkBlocked, markadmins, navigation popups, reference tooltips, comments in local time, prosesize
- [MiniJinja](https://github.com/mitsuhiko/minijinja)
- Codex's release policy for `@wikimedia/codex` and `@wikimedia/codex-design-tokens`

## Amendment log

### A1. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§18
- **Summary:** The Decision's current text now lives in the architecture chapters [01](../architecture/01-log-and-records.md), [03](../architecture/03-storage-caches-and-search.md), [19](../architecture/19-site-ui.md), [20](../architecture/20-web-tier.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.

### A2. Plain and Worksheet; switching by group

- **Date:** 2026-10-30
- **Source:** Direct: James, design discussion of 2026-10-30
- **Change:** amends §8, §13, §16, §17; extends §9
- **Summary:** Two skins ship. **Plain** draws the site's HTML and loads no stylesheet of its own; its DOM (element structure, class names, `data-` attributes, landmarks) is a contract versioned as Plain's version, with Codex's class names where Codex has a component and `ts-` classes filling the gaps. **Worksheet**, the default (`skins.default` defaults to `worksheet`), is Plain plus Codex's and the site's stylesheets. A skin either extends Plain, naming it as parent and adding stylesheets and the files they use and nothing else, so that everything written against Plain's DOM works on it; or stands alone, implementing the template contract with its own DOM, free but outside that compatibility. Plain is useful unstyled to operators building their own look, to readers styling it client-side, and to readers using assistive technology. Plain is written first to reproduce today's HTML, and parity is measured on Worksheet. The migration switches by group, not once: a group switches when its snapshot tests are unchanged, it is equal on the corpus, the Playwright and axe suite passes under both renderers, it is equal under `compare` on staging, and its median and 95th-percentile server render time is within 5% of the built-in renderer's; its built-in code is then deleted. The staging instance is the one the developing agent runs, seeded from a real wiki's dump, and its soak is a crawl of every page and a replay of the Playwright flows with no difference logged. Feature work on the site pauses while the migration runs. A skin's message keys carry `plain-` (a skin that extends Plain adds none). The template contract leaves major 0 when a skin that stands alone renders every route. The chapter text is [19](../architecture/19-site-ui.md) §5.2, §5.6 and §5.10.

Replaced text ([19](../architecture/19-site-ui.md) §5.10, as it stood):

> - **The default skin** is `skins/default/` in the repository, embedded in both binaries and used where no other is installed or chosen; an installed skin of the same name replaces it.

> - **The manifest**, `skin.toml`, gives the name, version, licence and requirements: the template contract and the Codex token major it is written for. A skin whose requirements the binary does not meet is not loaded, and the reason is logged and shown on `Special:Version`.

> **Reaching parity.** The default skin first reproduces today's HTML. Deployment configuration `server.renderer` and `web.renderer` is `builtin` (until parity), `skin` or `compare`, which renders each page both ways, serves the built-in result and logs each route that differs. Results are compared as normalized DOM, fragments byte for byte, with `html::esc` as the skin's escaper, over every route of the routing table for a fixture wiki as an anonymous reader, a user and an administrator, for each content model, right to left, in night mode, with each feature switch on and off, and on a private tenant; the Playwright and axe suite runs against both. The frame moves first, then entity pages, then the rest route by route, each once it is equal on the corpus and has run under `compare` on a staging instance without a difference; then the built-in rendering and `renderer` are removed. The template contract stays at major 0 until a second skin, structurally different from the default, renders every route.

Replaced text ([19](../architecture/19-site-ui.md) §5.2, as it stood):

> - **Skins:** the HTML is drawn by a skin the operator installs (§5.10; [0097](../decisions/0097-versioning-and-skins.md) §8): MiniJinja templates over view models the binary builds. Until the default skin reproduces today's pages, the built-in rendering draws them, in `askama` templates compiled and type-checked with the Rust code, and is then removed.

> - **Codex markup:** the default skin's macros emit Codex CSS-only component markup (buttons, fields, tables, cards, tabs, messages, chips, progress bars), following the approach of WMF's Codex PHP, so a Codex markup change is one edit; until parity, a Rust builder module does the same for the built-in rendering. Another skin may use other components.

Replaced text ([19](../architecture/19-site-ui.md) §5.6, as it stood):

> - Interface messages are **banana JSON** files in **bundles**, each with its own version ([0097](../decisions/0097-versioning-and-skins.md) §13): the binary's in `i18n/` (errors, notifications, mail, API text) and each skin's in the skin's own `i18n/`, each with `en.json`, `qqq.json` for documentation and one file per language, so that each can be registered with translatewiki.net like any MediaWiki extension. **A skin's keys carry its name as a prefix** (`default-…`), as MediaWiki's skins prefix theirs, so a key belongs to one bundle and an Interface override names it unambiguously; switching skins leaves the overrides of the old skin's keys in place and inert. Renaming or removing a key is a major of its bundle; a new translation is a patch.

### A3. Fragments are compared as normalized DOM

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building the parity harness, 2026-10-30; unconfirmed
- **Change:** amends §16
- **Summary:** The parity comparison reads `action=render` fragments the way it reads pages, as normalized DOM, not byte for byte. §16 assumed that with `html::esc` as the skin's escaper "escaping never differs", but the built-in renderer already escapes two ways: `askama` writes `&#60;`, `&#62;`, `&#38;`, `&#34;` in the frame and the Codex component templates, and `html::esc` writes `&lt;`, `&gt;`, `&amp;`, `&quot;` in the string builders, so a fragment that holds a Codex component cannot be reproduced byte for byte by a skin with one escaper. A browser builds the same DOM from either spelling, and the normalizer (`triplespace_ui::parity`) keeps every other difference, whitespace where it renders included. The chapter text is 19 §5.10.

Replaced text ([19](../architecture/19-site-ui.md) §5.10, as it stood):

> Results are compared as normalized DOM, fragments byte for byte, with `html::esc` as the skin's escaper, over every route for a fixture wiki as an anonymous reader, a user and an administrator, for each content model, right to left, in night mode, with each feature switch on and off, and on a private tenant.
