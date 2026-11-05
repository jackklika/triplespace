# 0086. Theming: palettes, type sets and modes

- **Status:** Proposed
- **Date:** 2026-10-17
- **Updated:** 2026-11-04 (A12)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0010](0010-site-ui.md), [0011](0011-logs.md), [0012](0012-api-requirements.md), [0015](0015-record-format-and-partition-registry.md), [0018](0018-tenants.md), [0034](0034-frontend-stack.md), [0039](0039-files-and-media.md), [0040](0040-instance-prerogatives.md), [0055](0055-templatestyles-templatedata-and-page-properties.md)
- **Uses:** [0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md), [0077](0077-special-version.md), [0085](0085-settings-and-preferences.md)

## Context

[0034](0034-frontend-stack.md) A7 kept Codex for its accessible components and let the instance or a tenant give Codex's design tokens values of its own, as one flat theme in the `ui.theme` setting, checked for WCAG 2.1 AA contrast and served as a stylesheet named by its hash. A8 shipped the canvases' colours with Newsreader headings over IBM Plex Sans and IBM Plex Mono as the default, `default` in `docs/registry/themes.toml`. That theme had one set of colours, so it had no dark mode ([0034](0034-frontend-stack.md) Q2), no high-contrast mode, and no choice for the reader, and colours and typefaces could not be chosen apart.

Codex 2.7.0 ships the tokens a dark mode needs (`theme-wikimedia-ui-mode-dark.css`) and the text-size modes MediaWiki's reading preferences use (`mode-small`, `mode-large`, `mode-x-large`). Wikipedia's night mode has taught its editors a convention for content: TemplateStyles rules keyed on `html.skin-theme-clientpref-night` and `html.skin-theme-clientpref-os`, and the classes `notheme`, `skin-invert` and `skin-invert-image`; mirrored and forked pages ([0053](0053-mirrored-pages.md), [0054](0054-forking-a-mirrored-page.md)) bring that convention with them.

[0085](0085-settings-and-preferences.md) settled the machinery this ADR uses: the settings registry and how a value resolves from instance to tenant, preferences that resolve from the reader down to the farm, the Appearance menu and how a reader who is not logged in keeps choices in the browser, and site assets, the files that are public because the site needs them, such as a typeface.

### Direction

James's direction, from the design discussion of 2026-10-16 and 2026-10-17:

- **"It would be great if an instance, or a tenant on an instance, or a user, can specify a preferred palette and font set. In terms of palettes, you can have different ones for light mode, dark mode, accessibility, and other things for customization. Different instances/tenants/users will also want different font sets depending on branding, accessibility, and other needs."**
- **"Yes, palette and type set are separate choices. Palette with no dark variant falls back to the default dark mode. Accessibility choices cannot be locked. I would defer on personal palettes. Yes, ship a dark variant. I want the current palette Triplespace has to be called "parchment"."**
- **"For the theming ADR, palette and type-set definitions live in config records. The default type-set should be called "default26" and I think the dark variant should be some kind of brown; you can fill in the gaps from there."**
- **"Palette and type set are per-wiki unless the user specifies they want it to follow across all wikis."**
- On the walnut-brown dark variant, shown beside the light one: **"The color scheme looks good to me. You may draft when ready."**
- On accessibility: **"What if instead of variants for color-vision deficiencies we had specialized palettes? I do like including a dyslexia type set. Atkinson Hyperlegible is SIL OFL 1.1."**
- **"I would call the alternative palette for those with red-green deficiencies "parchment-ob", named after the orange and blue used instead. … As for type, I would start with opendyslexic and add other options later."**
- On the blue–yellow palette, shown simulated beside `parchment` and `parchment-ob`: **"The colors are good and parchment-co is a good name."**
- On the draft: **"Thank you for adding the extends rule; it's a good way of implementing variants. You are right that color cannot be the only cue. ui.theme can be retired with no migration. Palette and type set should be like the logo, favicon, fonts, etc. in that they are inherently public."**
- **"Your font licence allow list is good. Yes, widen the security policy to allow fonts and images from the media domain. Wiki palettes and type sets cannot have names which conflict with the instance's. Palettes and type sets are not changeable by users or instance operators."**

## Decision

### 1. Palettes and type sets are configuration (amends 0034 §1; extends 0015 §3, 0012 §5, 0011 §6.1, 0018 §10 and 0040 §6)

*Changed by A1, A2, A3, A4, A5, A6, A10, A11.*

*Current text: [02](../architecture/02-graphs-rdf-and-query.md) §1.4; [08](../architecture/08-tenants-and-instances.md) §6.3, §8.6; [16](../architecture/16-logs-feeds-and-notifications.md) §2.3; [18](../architecture/18-api.md) §3.2; [19](../architecture/19-site-ui.md) §5.1, §5.9; [23](../architecture/23-configuration-and-registry.md) §2.2.*

### 2. Variants and contrast (amends 0010 §2; settles 0034 Q2)

*Changed by A3, A6, A12.*

*Current text: [19](../architecture/19-site-ui.md) §1.5, §5.9.*

### 3. Type sets (extends 0039 §5)

*Changed by A2, A5, A10, A11.*

*Current text: [12](../architecture/12-files-and-media.md) §4.5; [19](../architecture/19-site-ui.md) §5.2, §5.9.*

### 4. Who chooses (uses 0085 §1)

*Changed by A6.*

*Current text: [07](../architecture/07-actors-and-accounts.md) §8.1; [19](../architecture/19-site-ui.md) §2.7, §5.9.*

### 5. Serving (amends 0034 §1; extends 0034 §11; amends 0012 §4)

*Changed by A5, A6, A7, A9.*

*Current text: [09](../architecture/09-security-and-moderation.md) §1.4; [18](../architecture/18-api.md) §2.2; [19](../architecture/19-site-ui.md) §5.9.*

### 6. Content written for night mode (amends 0055 §2)

*Current text: [11](../architecture/11-rendering-templates-and-modules.md) §6.2.*

### 7. Crates (extends 0005 §2)

*Changed by A2, A3, A5, A6.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

## Alternatives considered

- **Palettes and type sets as pages**, with a JSON content model, would give them history, diffs and talk pages. James chose config records: a palette is configuration, its history is the config log, and records need no page stack.
- **Codex's dark tokens as they are**, as the dark mode of every palette. They are cool grey; a warm palette in light mode turning grey in dark mode reads as a different site. They remain the base a palette's `dark` overlays.
- **A dark mode derived by computation**, inverting lightness in OKLCH. It passes contrast checks unpredictably and gives a brand no say; a palette states its dark colours, and one that does not takes the default's.
- **Colour-vision variants of every palette**, beside `light` and `dark` (Q1 as first drafted). Every branded palette would need them or borrow the default's, and a reader would choose a deficiency and a palette apart. James chose palettes made for a deficiency: one choice, which a wiki may extend for its brand.
- **Text size inside the type set.** A reader who needs larger text needs it whatever the wiki's typeface is.

## Consequences

- **Every wiki has a dark mode and a high-contrast mode**, its own or the default palette's, and every reader can reach both, logged in or not, with or without JavaScript.
- ~~**A reader with a colour-vision deficiency or dyslexia has a palette or type set for it on every wiki**, which no instance or tenant can withhold; where they choose one, it takes the place of the wiki's brand.~~ *A colour-vision alternate is worn over the wiki's palette, not in its place (A6).*
- **A reader with a colour-vision deficiency or dyslexia has an alternate or a type set for it on every wiki**, which no instance or tenant can withhold; a type set takes the place of the wiki's, an alternate is worn over the wiki's palette (A6).
- **A wiki's brand is two records and two settings.** An instance can set or lock the defaults for every wiki through [0085](0085-settings-and-preferences.md) §2.1.
- **A private wiki's palettes, type sets, stylesheets and typeface files are public**, as its logo is: they are what its landing page is made of, and a wiki that would keep a palette's colours or a typeface secret has to keep them off the wiki.
- **Content written for Wikipedia's night mode renders as its authors intended**, through the classes MediaWiki uses.
- **Every colour the site draws is a token**, which a test enforces; a design that needs a new colour needs a token for it in every variant.
- **`ui.theme` records written before this ADR stop applying**, without migration, as James decided; `instance check` names them.
- **A palette or type set can be renamed under a wiki by its instance**, to `{name}~{slug}`, when the instance or a release takes its name; every setting and reader's choice that named it is rewritten with it, and the wiki's history shows the instance's act (A1).
- **Pages written without night mode in mind read poorly in the dark variants** where they colour backgrounds inline: light backgrounds under the dark variant's light text. Wikipedia has the same problem; `notheme` and night-mode TemplateStyles are the remedies its editors use.

## Open questions

- **Q1.** ~~**Named variants beyond the four**, such as a palette for a colour-vision deficiency, and how a reader would choose one.~~ *Withdrawn: James chose accessibility palettes on 2026-10-17 (§2).*
- **Q2. TemplateStyles' exact rule** for `html`-prefixed selectors, to be checked against css-sanitizer's source so §6 keeps what TemplateStyles keeps and no more.
- **Q3.** ~~**A type set for readers with dyslexia**, and which typeface, given the mixed evidence for dyslexia-specific typefaces and their licences.~~ *Settled by §3: James chose on 2026-10-17 to ship `opendyslexic` first; further options are Q7.*
- **Q4.** ~~**Bundling Atkinson Hyperlegible**: the licence of the files added to the build is to be confirmed as OFL-1.1 when they are added, and Atkinson Hyperlegible Next considered.~~ *Withdrawn: James confirmed on 2026-10-17 that Atkinson Hyperlegible is under the SIL OFL 1.1.*
- **Q5. Personal palettes**, written by a reader for themselves, deferred by James.
- **Q6. Verifying a font's licence** against the licence fields inside the font file, rather than trusting the record's declaration.
- **Q7. Further type sets for readers with dyslexia**, such as Lexend, and whether a type set may set letter, word and line spacing, which the British Dyslexia Association's style guide stresses more than the typeface.
- **Q8.** ~~**A name taken by another level later**: a release whose `themes.toml` adds a name an instance or tenant already uses, or a tenant joining an instance that uses one of its names. Proposed: `registry sync` and the join are refused with the colliding records named, so the operator renames them first, since renaming a record silently would change what `theme.palette` and readers' preferences mean.~~ *Settled by A1: the colliding tenant's record is keyed `{name}~{slug}`, or the instance's with the farm slug when the registry takes its name, with every reference rewritten in the same act; an instance write of a name a tenant uses is refused until it keys the tenant's or chooses another name.*

## References

- Codex design tokens 2.7.0: `theme-wikimedia-ui.css`, `theme-wikimedia-ui-mode-dark.css`, and the text-size modes.
- MediaWiki's night mode and its client preferences: the `skin-theme-clientpref-*` classes, and the `notheme`, `skin-invert` and `skin-invert-image` classes for content.
- WCAG 2.1: 1.4.3 (contrast, minimum), 1.4.6 (contrast, enhanced), 1.4.11 (non-text contrast).
- Atkinson Hyperlegible, Braille Institute of America.

## Amendment log

### A1. A name taken later is keyed

- **Date:** 2026-10-18
- **Source:** Direct: James, design discussion of 2026-10-18
- **Change:** amends §1
- **Summary:** Where a palette's or type set's name comes to collide across levels, the colliding record is renamed rather than the change refused, which settles Q8. James's direction: "if an instance has a pre-existing palette or type set "foo", and a tenant with their own "foo" wants to be imported, the conflict would resolve by keying the name to the tenant e.g. "foo~tenantname". If an instance wants to introduce a new palette or type set with a name in use by a tenant, it is blocked until the instance renames it with the key or gives it a new name altogether." James then chose, on 2026-10-18: the key is the tenant's slug, and a keyed name stays with the tenant when it leaves; when the instance takes a name a tenant uses, it is the tenant's record that is keyed and the instance takes the bare name; `registry sync` keys a colliding instance or tenant record, the instance's with the farm slug, rather than being refused; and keying in a tenant is a written provision that rewrites every reference to the record in the same act (`theme.palette` and `theme.typeset`, `extends`, and readers' `ui.palette` and `ui.typeset`). Claude proposed reserving `~` for keying, `key=tenants` as the way an instance write keys the tenants' records, and a job record listing the renames as the authority, which also maps the preferences of accounts reclaimed after a join. The join is in [08](../architecture/08-tenants-and-instances.md) §6.3 ([0018](0018-tenants.md) A30) and the written act in [08](../architecture/08-tenants-and-instances.md) §8.6 ([0040](0040-instance-prerogatives.md) A8).

Replaced text (§1):

> **A name means one thing on an instance**, as James decided: a tenant may not write a palette or type set under a name the registry or the instance uses, and the instance may not write one under a name the registry or any of its tenants uses, so a reader's choice never names two different records (Q8 for names that collide when the registry gains one or a tenant joins).

### A2. Encyclopedia and Basic, inspired by Vector 2022

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-24 and 2026-10-25
- **Change:** extends §1, §3, §7
- **Summary:** A palette and a type set inspired by Vector 2022 ship as options beside the defaults, which stay `parchment` and `default26`. James asked on 2026-10-24 for "a theme and type set option (not the default) inspired by Vector 2022", and on 2026-10-25, having seen it on the site: "I would like for the palette to be called "encyclopedia" and the type set "basic". It should use the same default font stack that Vector 2022 uses on Wikimedia wikis." The palette **`encyclopedia`** is Codex's light and dark values, which Vector 2022 wears, with only the tokens that miss the floors of §2 changed: `border-color-base` in `light` from `#A2A9B1` (2.37:1) to `#8C959F`, and in `dark` the placeholder (`#797F85`) and the focus outline (`#6485D1`), which Codex leaves at their light values; its `light-more` and `dark-more` deepen or lighten Codex's hues to 7:1 for text and 4.5:1 for boundaries, and its radius is Codex's `2px`. The type set **`basic`** is MediaWiki core's defaults as Vector 2022 uses them (`mediawiki.skin.defaults.less`, `mediawiki.skinning/elements.less`): `sans-serif` for text, `'Linux Libertine', 'Georgia', 'Times', 'Source Serif Pro', serif` for headings and `monospace, monospace` for code; it serves no typeface, which §3 now allows. Claude first drafted both as `vector22`, with Libertinus Serif served for headings; James chose the names and the stacks. The values are in `themes.toml`.

### A3. Colour-vision palettes named for the deficiency

- **Date:** 2026-10-25
- **Source:** Direct: James, design discussion of 2026-10-25
- **Change:** amends §1, §2, §7
- **Summary:** The accessibility palettes are named and labelled for the deficiency each serves, not for the colours it uses, so that a reader who has it finds it. James asked: "Might we come up with clearer names for the colorblind variants of the parchment theme? If I were a colorblind user looking for these settings, under what name(s) would I expect them?" He then chose the labels "Parchment (protanopia and deuteranopia)" and "Parchment (tritanopia)", and decided to change the slugs too: "I think it's fine to change the slug names since these aren't in use yet." The slugs become `parchment-rg` (red–green), from `parchment-ob`, and `parchment-by` (blue–yellow), from `parchment-co`; Claude proposed these slugs. Neither palette had been served, so no reader's or wiki's choice names the old slugs, and they are changed rather than retired. The colours are unchanged. The values are in `themes.toml`.

Replaced text (§1):

> **What ships:** the palette **`parchment`**, the canvases' colours named on 2026-10-17, with a walnut-brown dark variant and high-contrast variants of both; the accessibility palettes **`parchment-ob`** and **`parchment-co`**, for colour-vision deficiencies (§2); the type set **`default26`**, Newsreader, IBM Plex Sans and IBM Plex Mono; and the accessibility type sets **`hyperlegible`** and **`opendyslexic`** (§3). Beside them, as options a wiki or a reader may choose and not as defaults, ship the palette **`encyclopedia`** and the type set **`basic`**, inspired by Vector 2022, the skin of Wikipedia, as James decided (A2): `encyclopedia` is Codex's own colours, which Vector 2022 wears, changed only where they miss the floors of §2, with high-contrast variants in Codex's hues, and `basic` is Vector 2022's default stacks on Wikimedia wikis, serving no typeface (§3).

Replaced text (§2):

> Their colours were chosen by simulating the deficiency they serve (Machado, Oliveira and Fernandes, 2009) and are held to the same floors as any palette:

> - **`parchment-ob`**, for red–green deficiencies (protanopia and deuteranopia), named for the orange and blue it uses: errors, destructive actions and removed text in vermilion (`#A84300` in `light`), success and added text in cerulean (`#087691`), kept apart from link blue, warnings in umber and visited links in plum. Its lowest text pairs are 4.62:1 and 4.63:1 in `light` and `dark`, and at least 7.15:1 in the high-contrast variants.
> - **`parchment-co`**, for blue–yellow deficiency (tritanopia), named for its crimson and olive: errors in crimson (`#B3263E`), success in olive (`#587114`), which a tritanope tells from link blue, and warnings in an umber they do not confuse with errors. Its lowest text pairs are 4.85:1 and 4.63:1, and at least 7.17:1.

Replaced text (§7):

> The registry part of §1 is in place: `themes.toml` holds `parchment` with its four variants, the accessibility palettes `parchment-ob` and `parchment-co`, which extend it, the palette `encyclopedia` (A2), and the type sets `default26`, `hyperlegible`, `opendyslexic` and `basic` (A2), and `triplespace-ui` reads the default pair from it. The other palettes and type sets are not yet served, and the Atkinson Hyperlegible and OpenDyslexic files are not yet in the build.

### A4. Default 26 set in Source Serif 4, Source Sans 3 and Lilex

- **Date:** 2026-10-27
- **Source:** Direct: James, design discussion of 2026-10-27 and 2026-10-28
- **Change:** amends §1
- **Summary:** The type set `default26` changes its typefaces; its name, label and role as the default type set do not change. James first asked on 2026-10-27: "For the default26 type set, I would like the serif font to be Source Serif 4, with Rubik as the sans-serif font and Lilex as the monospace font." Before that shipped he tried Libre Baskerville and Mukta Mahee on 2026-10-28, then chose for the coverage of writing systems: "Let's go with Source Serif 4 and Source Sans 3." Source Serif 4 replaces Newsreader for headings (`font-family-heading-main`, weights 400 and 500), Source Sans 3 replaces IBM Plex Sans for text (`font-family-base`, weights 400, 500 and 600), and Lilex replaces IBM Plex Mono for code and IDs (`font-family-monospace`, weights 400 and 500), each before the fallback stack it had. All three are under the SIL OFL 1.1 and are served from the site's own origin, as before. `hyperlegible` and `opendyslexic` keep IBM Plex Mono for code. `docs/registry/themes.toml` is the authority.

Replaced text (§1):

> **What ships:** the palette **`parchment`**, the canvases' colours named on 2026-10-17, with a walnut-brown dark variant and high-contrast variants of both; the accessibility palettes **`parchment-rg`** and **`parchment-by`**, for colour-vision deficiencies and named for them (§2, A3); the type set **`default26`**, Newsreader, IBM Plex Sans and IBM Plex Mono; and the accessibility type sets **`hyperlegible`** and **`opendyslexic`** (§3). Beside them, as options a wiki or a reader may choose and not as defaults, ship the palette **`encyclopedia`** and the type set **`basic`**, inspired by Vector 2022, the skin of Wikipedia, as James decided (A2): `encyclopedia` is Codex's own colours, which Vector 2022 wears, changed only where they miss the floors of §2, with high-contrast variants in Codex's hues, and `basic` is Vector 2022's default stacks on Wikimedia wikis, serving no typeface (§3).

### A5. A fallback layer for every script

- **Date:** 2026-10-28
- **Source:** [0093](0093-fallback-typefaces.md) §1, §2, §3, §4
- **Change:** extends §1, §3, §5, §7
- **Summary:** A type set may name a shipped fallback layer, whose typefaces go into its stacks after its own, by generic family, for the scripts its own lack (0093 §1). The layer `noto` ships, and `default26`, `hyperlegible` and `opendyslexic` name it. It has Noto Sans and Noto Serif, a Noto family for each of the 47 scripts of Wikimedia's language data other than Latin, Greek, Cyrillic, Chinese, Japanese, Korean and SignWriting, two symbol families, and Noto Nastaliq Urdu for `ur`: 72 families in weights 400 and 600 (0093 §2). Chinese, Japanese and Korean stay with the device's fonts. The layer's `@font-face` rules are in each such type set's stylesheet, each face limited to its script's ranges and served as WOFF2 alone, so a page fetches the files for its scripts only (0093 §3).

### A6. Colour-vision palettes become keyed alternates

- **Date:** 2026-10-28
- **Source:** [0094](0094-accessibility-alternates.md) §1, §2, §3, §4, §5, §6, §7
- **Change:** amends §1, §2, §4, §5; extends §7
- **Summary:** For colour-vision deficiencies a palette has accessibility alternates, keyed by the registry (`rg`, `by`), which a reader turns on with `ui.vision` over whatever palette they wear, in place of the palettes `parchment-rg` and `parchment-by`, whose colours become `parchment`'s alternates unchanged and whose names are retired and read as `parchment` with the alternate. A palette wears its own alternate, else its base's, else its `alternates-from`'s, else the default palette's; the palettes it names are prerequisites. The alternates' rules are in the palette's stylesheet under `data-vision`.

Replaced text (§1):

> **Keying renames every reference in the same act**, so no setting or choice changes what it shows: the record is written under its keyed name and the old one deleted, and with it the `theme.palette` and `theme.typeset` values that name it (the tenant's own, and the instance's defaults and locks), the `extends` of the palettes that extend it, and the `ui.palette` and `ui.typeset` preferences of tenant and farm accounts. In a tenant, keying is a written provision ([08](../architecture/08-tenants-and-instances.md) §8.6): its records carry the instance attestation, its authority is a job record in the instance `log` that lists the renames, and the tenant may later rename the record to any name that is free. **`~` is reserved for keying**: no one writes a new name containing it, and a keyed record is updated and deleted under its keyed name. Once written, a keyed name is a name like any other: it stays when the tenant leaves, and is keyed again, its key replaced by the tenant's slug there, only if it is taken where the tenant arrives.

Replaced text (§1):

> **What ships:** the palette **`parchment`**, the canvases' colours named on 2026-10-17, with a walnut-brown dark variant and high-contrast variants of both; the accessibility palettes **`parchment-rg`** and **`parchment-by`**, for colour-vision deficiencies and named for them (§2, A3); the type set **`default26`**, Source Serif 4 for headings over Source Sans 3 for text and Lilex for code (A4); the accessibility type sets **`hyperlegible`** and **`opendyslexic`** (§3); and the fallback layer **`noto`**, which those three name, with Noto for every other script of Wikimedia's languages but Chinese, Japanese and Korean ([0093](0093-fallback-typefaces.md) §2). Beside them, as options a wiki or a reader may choose and not as defaults, ship the palette **`encyclopedia`** and the type set **`basic`**, inspired by Vector 2022, the skin of Wikipedia, as James decided (A2): `encyclopedia` is Codex's own colours, which Vector 2022 wears, changed only where they miss the floors of §2, with high-contrast variants in Codex's hues, and `basic` is Vector 2022's default stacks on Wikimedia wikis, serving no typeface (§3).

Replaced text (§2):

> **A palette may extend another** (`extends`): each of its variants is laid over the same variant of that palette as served, rather than over Codex's, so it states only the colours it changes; its `light-more` and `dark-more` take its own `light` and `dark` first, and a variant missing from both falls back as above. A palette may extend a shipped palette or one at its own scope, and not one that itself extends another; a palette that another extends cannot be deleted.

Replaced text (§2):

> **Accessibility palettes.** A palette marked `accessibility = true` can always be chosen by a reader (§4). In place of colour-vision variants of every palette, James chose palettes made for a deficiency, which a reader chooses as they would any palette. Two ship, both extending `parchment` and changing only the colours that carry meaning (errors and destructive actions, success, warnings, visited links, and the subtle backgrounds of each), with all four variants. Their colours were chosen by simulating the deficiency they serve (Machado, Oliveira and Fernandes, 2009), and are named and labelled for that deficiency, as a reader who has it would look for it (A3) and are held to the same floors as any palette:
>
> - **`parchment-rg`**, labelled *Parchment (protanopia and deuteranopia)*, for red–green deficiencies, which uses orange and blue (A3): errors, destructive actions and removed text in vermilion (`#A84300` in `light`), success and added text in cerulean (`#087691`), kept apart from link blue, warnings in umber and visited links in plum. Its lowest text pairs are 4.62:1 and 4.63:1 in `light` and `dark`, and at least 7.15:1 in the high-contrast variants.
> - **`parchment-by`**, labelled *Parchment (tritanopia)*, for blue–yellow deficiency, which uses crimson and olive (A3): errors in crimson (`#B3263E`), success in olive (`#587114`), which a tritanope tells from link blue, and warnings in an umber they do not confuse with errors. Its lowest text pairs are 4.85:1 and 4.63:1, and at least 7.17:1.

Replaced text (§2):

> **Colour is never the only cue** (WCAG 1.4.1): what the site draws tells states apart by text, icon or mark as well, such as the `−` and `+` of a diff beside `color-destructive` and `color-success` on their subtle backgrounds, so an accessibility palette makes the colour a cue the reader can see, not the only one. A wiki that wants its brand in a colour-vision palette writes one that extends its own and marks it `accessibility`.

Replaced text (§4):

> **Accessibility choices are never locked.** `ui.mode`, `ui.contrast`, `ui.text-size` and the accessibility palettes and type sets are offered whatever `theme.choice` and any instance lock say; only the other palettes and type sets, which carry a wiki's brand, can be withheld. A reader who chooses an accessibility palette leaves the wiki's own palette for it.

Replaced text (§5):

> **The reader's choices are attributes on `<html>`**: `data-mode`, `data-contrast` and `data-text-size`, written by the server for a signed-in reader, whose page is private, and by the head script of [0085](0085-settings-and-preferences.md) §6 for a reader who is not logged in, whose page stays public and carries the tenant's default stylesheet. MediaWiki's classes are set beside them, `skin-theme-clientpref-night` for a chosen dark mode, `skin-theme-clientpref-os` for `auto` and `skin-theme-clientpref-day` for a chosen light mode, so content written for Wikipedia's night mode works (§6); a public page is served with `skin-theme-clientpref-os` and no `data-mode`, so it follows the device until the head script says otherwise. Where a reader not logged in has chosen another palette or type set, the head script swaps the stylesheet links before the first paint, from a JSON block on the page that maps the tenant's palettes and type sets to their stylesheet URLs; the default sheets may already be in flight, a cost of one cached request. **Without JavaScript**, the media queries alone follow the device's dark-mode and contrast settings.

Replaced text (§5):

> **`siprop=triplespace` reports the theme**: the palettes and type sets the tenant offers (name, label, which variants a palette has and the palette it extends, whether a palette or type set is for accessibility), `theme.palette`, `theme.typeset` and `theme.choice`, and the stylesheet URL of each offered palette and type set, in place of `ui.theme`'s token map. **Palettes and type sets are public**, as James decided, like the logo, the favicon and the typefaces ([0085](0085-settings-and-preferences.md) §8): an outsider of a private tenant is given the theme in full, the records, the settings and the stylesheets, which `/ui/theme/` serves to anyone. They are what the pages an outsider can reach are made of, and a reader who needs an accessibility palette or type set needs it on the landing page and the login form before signing in.

Replaced text (§7):

> The registry part of §1 is in place: `themes.toml` holds `parchment` with its four variants, the accessibility palettes `parchment-rg` and `parchment-by` (A3), which extend it, the palette `encyclopedia` (A2), and the type sets `default26`, `hyperlegible`, `opendyslexic` and `basic` (A2), and `triplespace-ui` reads the default pair from it. The other palettes and type sets are not yet served, and the Atkinson Hyperlegible and OpenDyslexic files are not yet in the build.

### A7. The token contract is Codex's

- **Date:** 2026-10-29
- **Source:** [0097](0097-versioning-and-skins.md) §14
- **Change:** extends §5
- **Summary:** The token contract's version is the release of Codex's design tokens the binary pins; the binary owns the choice, skins declare the token major they need, and a Codex major that renames or removes tokens is a product major unless the binary maps the old names. The chapter text is [19](../architecture/19-site-ui.md) §5.9.

### A8. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§7
- **Summary:** The Decision's current text now lives in the architecture chapters [02](../architecture/02-graphs-rdf-and-query.md), [07](../architecture/07-actors-and-accounts.md), [08](../architecture/08-tenants-and-instances.md), [09](../architecture/09-security-and-moderation.md), [11](../architecture/11-rendering-templates-and-modules.md), [12](../architecture/12-files-and-media.md), [16](../architecture/16-logs-feeds-and-notifications.md), [18](../architecture/18-api.md), [19](../architecture/19-site-ui.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.

### A9. Theming: loose ends from building it

- **Date:** 2026-10-30
- **Source:** Direct: Claude, while building theming, 2026-10; unconfirmed
- **Change:** extends §5
- **Summary:** Logged from the working notes on their retirement (0099 §5, 24 §15); each call stands as decided until James confirms or overturns it, and the chapter text that states it is folded on confirmation where it is not already there. The calls:
  - §7 still says the theme is "not yet served"; it is served (c288df8, fee0708), so the sentence needs a `corrects` entry.
  - The registry lists OpenDyslexic Mono, but IBM Plex Mono is what's served; one of them is wrong.
  - `theme.choice` and the chip swap are as built and unconfirmed.

### A10. Gruvbox, Nord and Catppuccin, with type sets made for them

- **Date:** 2026-11-01
- **Source:** Direct: Claude, while building the palettes James asked for, 2026-11-01; unconfirmed
- **Change:** extends §1, §3
- **Summary:** James asked on 2026-11-01: "Come up with Triplespace palettes for gruvbox, nord, and catpuccin, with complementing type sets. Font reuse is allowed." Three palettes and three type sets ship as options beside the defaults, which stay `parchment` and `default26`. The colours and faces are Claude's choices, unconfirmed. **`gruvbox`** (morhetz/gruvbox, MIT): `light` is gruvbox light on its cream `bg0` with the faded accents, `dark` gruvbox dark on its neutral grey `bg0`, kept grey so it reads apart from parchment's walnut, `dark-more` on `bg0_h`; links in blue, visited in purple, buttons in the faded blue `#076678`. **`nord`** (nordtheme/nord, MIT): `dark` Polar Night with Snow Storm text, `light` Snow Storm with Polar Night text, links in Frost; Nord's mid-tone accents are darkened in their own hue in `light`, warnings there in Aurora orange, and buttons in a darkened Frost blue. **`catppuccin`** (catppuccin/catppuccin, MIT): `light` the Latte flavour, `dark` the Mocha flavour, `dark-more` on Crust; buttons in Latte Blue in both, since Mocha's pastels cannot carry a white label. Each palette sets all four variants, so none falls back to parchment's, and keys its own `rg` and `by` alternates: parchment's hues moved in lightness to meet the floors of §2 on its backgrounds, which parchment's own miss on gruvbox's and Nord's darks, with subtle backgrounds tinted from its own. Every accent is the scheme's own, lightened or darkened in its own hue only where it misses a floor. The type sets, none for accessibility, each with the layer `noto`: **`retro`** for `gruvbox`, Bitter headings over IBM Plex Sans with IBM Plex Mono (`hyperlegible`'s); **`nordic`** for `nord`, Rubik, the face of Nord's own site, over Inter with JetBrains Mono; **`cozy`** for `catppuccin`, Fraunces over Nunito with Lilex (`default26`'s). All are OFL-1.1, from Fontsource 5.3.0, with the body face's 400 italic served. A palette and a type set remain separate choices; the pairing is a suggestion in the registry's comments. The chapter text is 19 §5.9; the registry is `themes.toml`.

### A11. Greenbar and Cyanotype, original palettes with type sets made for them

- **Date:** 2026-11-01
- **Source:** Direct: Claude, while building the palettes James asked for, 2026-11-01; unconfirmed
- **Change:** extends §1, §3
- **Summary:** Asked for a wildcard palette and type set, Claude proposed Greenbar, with Cyanotype and a card-catalogue palette as runners-up, and James answered on 2026-11-01: "Add Greenbar and Cyanotype to the collection." Two original palettes and two type sets ship as options, built as A10's are: all four variants, their own `rg` and `by` alternates from parchment's hues moved to meet the floors of §2, subtle backgrounds tinted from their own. The colours and faces are Claude's, unconfirmed. **`greenbar`**: `light` fanfold computer paper, near-white with the pale green of the printed bands for shaded boxes, carbon-black text and ribbon-blue links, `light-more` on white; `dark` a green phosphor screen, near-black with pale green text, amber links after amber-screen terminals, `dark-more` on black; buttons a deep green. **`cyanotype`**: `light` a whiteprint, navy text on a blue-white ground with teal links; `dark` a blueprint, off-white on Prussian blue `#1B3F73` with marking-pencil yellow links; buttons a deep teal in both. Type sets, OFL-1.1 from Fontsource 5.3.0, with the layer `noto`: **`mainframe`** for `greenbar`, VT323 headings (one weight, Latin only) over Overpass with Overpass Mono; **`drafting`** for `cyanotype`, Barlow Condensed over Barlow with JetBrains Mono (`nordic`'s). The chapter text is 19 §5.9; the registry is `themes.toml`.

### A12. Links on the page's canvas, and Codex's values under every palette

- **Date:** 2026-11-04
- **Source:** Direct: Claude, while fixing the red-link and link-in-text findings of the end-to-end axe checks, 2026-11-04; unconfirmed
- **Change:** extends §2
- **Summary:** axe found two failures on a page of content: a red link at 4.25:1, and a link in a sentence told apart from its text by colour alone. Three changes follow.
  - **Codex's values are served.** A palette's sheet held only the tokens the palette names in `light`, so a token it left alone, `color-destructive` among them, was undefined in light mode and showed whatever fallback a stylesheet wrote beside `var()` (the red link's `#d73333`), not Codex's value, which the contrast check had assumed. The `light` rule is now Codex's light values with the palette's over them, as the dark rule already was for dark mode. This makes the sheet do what §2 states; it changes no decision.
  - **Two more pairs.** Content sits on the page's canvas, `background-color-neutral-subtle`, as well as on the base, so the check adds `color-visited` and `color-destructive` (which Codex's `color-link-red` is) on it. 35 variants of five palettes failed them, the worst at 3.94:1; asked how to treat them, James chose on 2026-11-04 to "nudge the colours": each failing value keeps its hue and saturation and moves its lightness away from the background until it meets its floor plus 0.05, in the table that sets it, or in the failing variant's own table where it was inherited. Parchment's own variants and Greenbar pass unchanged; parchment's red–green and blue–yellow alternates change in `dark-more`, which Encyclopedia borrows. Where a table set a visited link's hover and active colours, or `color-error` and the destructive and error borders, to the same value, they moved with it. The changes:
  - `catppuccin.light` `color-visited`: `#8839EF` → `#8636EF`.
  - `catppuccin.light-more` `color-visited`: `#6711D7` → `#6110CB`.
  - `catppuccin.alternate.by.dark` `color-visited`: `#CE64AE` → `#D06AB1`, with `color-visited--active`, `color-visited--hover`.
  - `catppuccin.alternate.by.light-more` `color-visited`: `#892C6D` → `#812966`, with `color-visited--active`, `color-visited--hover`.
  - `catppuccin.alternate.rg.dark` `color-visited`: `#A977BB` → `#AC7CBD`, with `color-visited--active`, `color-visited--hover`.
  - `catppuccin.alternate.rg.light-more` `color-visited`: `#6A3D7A` → `#663B75`, with `color-visited--active`, `color-visited--hover`.
  - `cyanotype.light-more` `color-visited`: Codex's or the inherited value → `#693E8C`.
  - `cyanotype.alternate.by.dark` `color-visited`: `#DD93C6` → `#E3A5CF`, with `color-visited--active`, `color-visited--hover`.
  - `cyanotype.alternate.by.dark-more` `color-visited`: `#E4ABD3` → `#E8B8DA`, with `color-visited--active`, `color-visited--hover`.
  - `cyanotype.alternate.rg.dark` `color-visited`: `#C29ECE` → `#CBADD6`, with `color-visited--active`, `color-visited--hover`.
  - `cyanotype.alternate.rg.dark` `color-destructive`: `#FF9C59` → `#FFA161`, with `color-error`, `border-color-destructive`, `border-color-error`.
  - `cyanotype.alternate.rg.dark-more` `color-visited`: `#CFB2D8` → `#D6BEDE`, with `color-visited--active`, `color-visited--hover`.
  - `encyclopedia.light-more` `color-visited`: `#574D9C` → `#544A97`.
  - `parchment.alternate.by.dark-more` `color-visited`: `#DB8EC4` → `#DD95C8`, with `color-visited--hover`.
  - `parchment.alternate.by.dark-more` `color-destructive`: `#E78C9C` → `#E994A3`, with `color-error`, `border-color-error`, `border-color-destructive`.
  - `parchment.alternate.rg.dark-more` `color-visited`: `#BF99CC` → `#C39FCF`, with `color-visited--hover`.
  - `parchment.alternate.rg.dark-more` `color-destructive`: `#FF8B3E` → `#FF8E43`, with `color-error`, `border-color-error`, `border-color-destructive`.
  - `gruvbox.light-more` `color-visited`: `#7E3764` → `#73325C`.
  - `gruvbox.dark-more` `color-visited`: `#D998A9` → `#DEA5B4`.
  - `gruvbox.alternate.by.dark` `color-visited`: `#D06CB2` → `#D47AB9`, with `color-visited--active`, `color-visited--hover`.
  - `gruvbox.alternate.by.light-more` `color-visited`: `#882C6D` → `#7C2864`, with `color-visited--active`, `color-visited--hover`.
  - `gruvbox.alternate.by.dark-more` `color-visited`: `#DC93C6` → `#E0A0CD`, with `color-visited--active`, `color-visited--hover`.
  - `gruvbox.alternate.rg.dark` `color-visited`: `#AD7DBE` → `#B489C4`, with `color-visited--active`, `color-visited--hover`.
  - `gruvbox.alternate.rg.light-more` `color-visited`: `#6A3D7A` → `#633972`, with `color-visited--active`, `color-visited--hover`.
  - `gruvbox.alternate.rg.dark-more` `color-visited`: `#C29DCE` → `#C9A9D4`, with `color-visited--active`, `color-visited--hover`.
  - `nord.light` `color-visited`: `#8C5D84` → `#86597F`.
  - `nord.dark` `color-visited`: `#B691AF` → `#BD9BB6`.
  - `nord.light-more` `color-visited`: `#674461` → `#63415D`.
  - `nord.dark-more` `color-visited`: `#C8ACC2` → `#D3BCCE`.
  - `nord.alternate.by.dark` `color-visited`: `#D67FBC` → `#DA8DC3`, with `color-visited--active`, `color-visited--hover`.
  - `nord.alternate.by.light-more` `color-visited`: `#872B6C` → `#802966`, with `color-visited--active`, `color-visited--hover`.
  - `nord.alternate.by.dark-more` `color-visited`: `#E1A0CD` → `#E7B4D8`, with `color-visited--active`, `color-visited--hover`.
  - `nord.alternate.rg.dark` `color-visited`: `#B78DC6` → `#BE98CB`, with `color-visited--active`, `color-visited--hover`.
  - `nord.alternate.rg.light-more` `color-visited`: `#6A3D7A` → `#653A75`, with `color-visited--active`, `color-visited--hover`.
  - `nord.alternate.rg.dark-more` `color-visited`: `#C9A9D4` → `#D4BADC`, with `color-visited--active`, `color-visited--hover`.
  - **Links in content are underlined.** A link inside rendered wikitext or Markdown (`.mw-parser-output`, `.ts-markdown`) is underlined as well as coloured, since a link's colour is not 3:1 against the text around it in every palette (WCAG 1.4.1), as report rows already were; links in the frame, menus and lists of the site's own pages are not. A red link takes `color-link-red`.
