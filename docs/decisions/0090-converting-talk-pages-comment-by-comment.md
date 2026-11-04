# 0090. Converting talk pages comment by comment

- **Status:** Proposed
- **Date:** 2026-10-20
- **Updated:** 2026-10-30 (A4)
- **Author:** James Hare / Claude Opus
- **Changes:** [0005](0005-crate-organization.md), [0008](0008-namespaces-and-document-pages.md), [0013](0013-postgres-storage.md), [0019](0019-discussions.md), [0047](0047-special-pages.md), [0054](0054-forking-a-mirrored-page.md), [0069](0069-synchronized-talk-pages.md)
- **Uses:** [0007](0007-actor-identity.md), [0015](0015-record-format-and-partition-registry.md), [0035](0035-adopting-a-wikibase.md), [0049](0049-boards.md), [0053](0053-mirrored-pages.md), [0085](0085-settings-and-preferences.md), [MediaWiki API contract](../api/mediawiki-compat.md)

## Context

[0054](0054-forking-a-mirrored-page.md) §5 converts a wikitext talk page into threads section by section. Each level-two section becomes one thread, created `archived`, whose opening post is the whole section as wikitext. The text above the first heading becomes a pinned, open thread. Every post is attributed to the person who started the job. The same conversion serves **Fork the talk page**, **Stop following** ([0069](0069-synchronized-talk-pages.md) §7) and `Special:Import` ([0008](0008-namespaces-and-document-pages.md) §9). Two questions were left open: whether imported posts could carry their signers' names (0054 Q4), and how an adopted wiki's talk pages would come across ([0008](0008-namespaces-and-document-pages.md) A41 deferred them).

A section-sized post keeps everything that was said, but it is not a discussion. Nobody can reply to one comment, the tree that indentation drew is lost, and everyone who took part is credited to whoever ran the import. The research of 2026-10-20 (`claude/talk-page-conversion.md` in the project notes) asked how far a comment-level conversion can get with simple rules, against nine English Wikipedia pages chosen for range, and one user talk page with its full history. The pages were Wikipedia:Reference desk/Science, Wikipedia:Administrators' noticeboard/Incidents, Talk:Barack Obama/Archive 1 and Archive 84, Wikipedia:Requests for adminship/Vermont, Talk:Jesus/Archive 1 and Archive 139, Talk:Gdańsk/Vote and User talk:Jimbo Wales/Archive 256; the history was User talk:Harej's 4,677 revisions from 2004 to 2026. `tools/talk_dumps.py` fetched them, with DiscussionTools' own parse of each revision as the reference.

**DiscussionTools.** The extension parses a rendered page into headings and comments. A comment ends at a signature, a user link followed by a timestamp in the wiki's current date format. Its level comes from indentation, and it gets a name and an ID derived from its author, its timestamp and its parent. It does not recognise older date formats, so it finds no comments on Talk:Gdańsk/Vote (2005, `18 Feb 2005` and `Feb 18, 2005`) and one on Talk:Jesus/Archive 1 (2002–03, mostly undated signatures). It merges unsigned text into the next signed comment, which on Talk:Barack Obama/Archive 1 credits Shakam's comments to HailFire.

**A simple splitter.** A prototype of about 150 lines of Python works on the wikitext with the rules of §2:
- **Comments:** of the 2,177 comments DiscussionTools finds on the nine pages, it finds 2,176.
- **Parents:** it agrees on the parent for 2,059 of the 2,111 that can be compared (97.5%). Most of the rest are cases that are ambiguous in the text itself, where a comment's first line and its signature line are indented differently, or DiscussionTools' own structural quirks.
- **Old pages:** with the older date formats and undated signatures it finds 894 comments on Talk:Gdańsk/Vote, 86 on Talk:Jesus/Archive 1 and 124 on Talk:Barack Obama/Archive 1, against DiscussionTools' 0, 1 and 86.
- **False positives:** the undated rule adds none on the seven modern pages.
- **Markup across comments:** six wrapper template pairs and two tables enclosed more than one comment among 3,195 comments.

**History.** A line-level blame over User talk:Harej's history ran in about eight seconds. Of the 3,132 signatures that first appear in some revision:
- 85.8% were added by the signer's own account.
- 8.9% were added by an account with another name. These are mostly renames and alternate accounts: 73 by `Messedrocker`, the page owner's former name. The rest are comments moved from elsewhere.
- 4.8% were delivered by bots and signed by the sender.
- 13 were added by SineBot signing for someone else.
- 98.8% of self-signed comments carry the same minute as their revision.
- 5.0% of comments had their signature line changed later by their signer.

Neither source is enough alone. A signature name looked up in today's user table gets usurped names wrong, since `Drecker` in 2006 is not today's `Drecker`. History credits a moved comment to whoever moved it. Archives are created by bots copying text, so the edits that wrote an archive's comments are in the parent talk page's history. Seeding stores text only for the last 100 revisions ([0054](0054-forking-a-mirrored-page.md) A5), so an attribution pass cannot rely on stored history.

### Direction

James's direction, from the design discussion of 2026-10-20:

- **"While getting as close to full coverage of wiki talk page foibles is ideal, it should not come at the expense of making sure that the most common scenarios are covered well."** LiquidThreads and Flow are not a focus.
- **"One post per comment is right. Fall back to whole-section post when having trouble parsing the page, or whole-page post for truly unsalvageable cases."**
- **"Your solution for headings sounds reasonable. Good solution for attribution as well."**
- **"I feel like signature date formats should be part of a universal registry rather than per-wiki, since the same formats will exist on multiple wikis."**
- **"I think your finding that you can stream the data from RevisionChest and use it without persisting everything is correct, allowing us to preserve attribution."**
- **"Since the 100 revision limit is arbitrary, I propose making it possible to waive and include every revision, for projects intending to operate at that scale. It's a sensible default to prevent surprise blow-ups but at some point I will want to aim for completion."** And: "I like 'all' as an explicit indicator."
- **"DiscussionTools comment IDs are worth preserving."**
- **"Testing against DiscussionTools is certainly encouraged."**

## Decision

### 1. One post per comment (amends 0054 §5; settles 0054 Q4)

*Changed by A3.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.12.*

### 2. Finding comments (amends 0054 §5)

*Changed by A2.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.12.*

### 3. Headings, and wrappers that close a discussion (amends 0054 §5; extends 0019 §4)

*Changed by A1, A2, A3.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.14; [14](../architecture/14-discussions.md) §1.5, §1.6, §2.1, §2.4.*

### 4. When the split fails: the section, then the page (amends 0054 §5)

*Changed by A1, A2.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.15.*

### 5. Signature conventions: a universal registry

*Changed by A2.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.16; [23](../architecture/23-configuration-and-registry.md) §4.7.*

### 6. Attribution (amends 0054 §5; uses 0007 §5)

*Changed by A2.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.13.*

### 7. Attribution reads history; it does not store it (amends 0054 §3, §5)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.4, §5.13.*

### 8. The record (amends 0019 §4; extends 0054 §5)

*Changed by A1.*

*Current text: [13](../architecture/13-mirrored-pages.md) §5.13; [14](../architecture/14-discussions.md) §1.5.*

### 9. DiscussionTools names and IDs (extends 0047 §9; extends 0013 §5)

*Current text: [03](../architecture/03-storage-caches-and-search.md) §4.8; [13](../architecture/13-mirrored-pages.md) §5.17; [21](../architecture/21-special-pages.md) §6.1, §8.*

### 10. Where the conversion runs (amends 0008 §9; amends 0069 §7; uses 0035)

*Current text: [10](../architecture/10-pages-and-content-models.md) §6; [13](../architecture/13-mirrored-pages.md) §5.11, §5.12.*

### 11. `all`: waiving the history limits (amends 0054 §3)

*Current text: [13](../architecture/13-mirrored-pages.md) §3.4; [23](../architecture/23-configuration-and-registry.md) §3.2.*

### 12. Crates (amends 0005 §2; amends 0054 §12)

*Changed by A1, A3.*

*Current text: [22](../architecture/22-crates-and-stack.md) §2.1.*

### 13. Testing

*Changed by A2.*

*Current text: [22](../architecture/22-crates-and-stack.md) §4.9.*

## Alternatives considered

- **Keep one post per section** (0054 §5 as it stood). Rejected by James's direction; it stays as the fallback (§4).
- **Use DiscussionTools' parse as the splitter.** Rejected:
  - It recognises only the current date format.
  - It merges unsigned text.
  - It is unavailable for dumps, for adoption and for wikis without the extension.
  - It cannot attribute.

  It is kept as the source of IDs where available (§9) and as the test oracle (§13).
- **Signatures only.** Rejected: renames and usurped names are attributed wrongly, and unsigned text not at all.
- **History only.** Rejected: moved comments and delivered messages go to the mover and the bot.
- **Store the history the attribution reads.** Rejected: it is the cost A5 removed, and attribution needs only the result.
- **Per-wiki signature formats.** Rejected by James's direction: the formats are shared, and what is per-wiki comes from siteinfo.
- **Drop wrapper templates.** Rejected: the closing statement and the box are part of what a closed discussion was. They are lifted instead (§3).
- **Split partial wrappers into a container post with replies.** Rejected: the enclosed comments would change parents. The section falls back instead.

## Consequences

- **Imported discussions are discussions.** Each comment can be read, linked, watched and replied to on its own. The tree indentation drew is kept, and the posts are attributed to the people who wrote them.
- **Old pages convert better than DiscussionTools reads them,** because the registry knows the older formats and undated signatures.
- **No section is worse off than 0054 §5 left it.** A section the splitter cannot handle becomes the post it would have been.
- **Adopting a wiki brings its talk pages,** so Librarybase's are no longer left behind.
- **Inbound links survive.** `#c-…` anchors and `Special:GoToComment` resolve to the imported posts.
- **Converting a talk page reads its whole history once.** Through the API that is about one request per 50 revisions, unless a RevisionChest store or a dump is at hand. The log stores no more than seeding decides.
- **`all` makes complete histories a setting,** not a code change.
- **Other languages need registry entries** before their old signatures are recognised. Modern ones need only a month-name table.

## Open questions

- **Q1. Later edits as `edit` records.** In the research, 5.0% of comments had their signature line changed later by their author. Whether the conversion should append those versions as `edit` records on the post, rather than importing the latest text only.
- **Q2.** ~~**Closures as statuses.** Whether a lifted wrapper should also set a status on its thread or heading post, with 0069's open question on closure templates.~~ *Settled by A1: a wrapper enclosing a whole section is a closing post with `status: closed`; any other wrapper is a collapsing post (§3).*
- **Q3. Temporary accounts.** How `~2025-…` temporary accounts are named as foreign actors.
- **Q4. Plain-text signatures.** `--MichaelTinkler` and similar, with no link, are found only through history today; whether the registry should recognise them where an account of that name exists.
- **Q5. Archive titles.** Whether `Talk:X/Archive 5` should resolve on a converted talk page to the threads imported from it, so that `Talk:X/Archive 5#c-…` links keep working, not only the talk page's own.
- **Q6.** ~~**Fixtures.** Whether Wikipedia text committed as test fixtures needs attribution beyond the dump's own contributors.~~ *Settled by A2: the fixtures' README names the wiki, the licence (CC BY-SA 4.0), each page and revision with a link to its history, and the changes made, which is the attribution the licence asks for; the fixtures are read at run time and compiled into nothing (§13).*
- **Q7. Converting again.** How a talk page converted by an older splitter is re-converted when the splitter improves, without duplicating threads.

## References

- [Extension:DiscussionTools](https://www.mediawiki.org/wiki/Extension:DiscussionTools); its source, `includes/CommentParser.php` (`computeName`, `computeId`, `truncateForId`, `buildThreads`), `includes/ThreadItem/CommentItemTrait.php` (`getTimestampString`) and `sql/discussiontools_persistent.json`, as of 2026-10-20
- [API:Discussiontoolspageinfo](https://www.mediawiki.org/wiki/Extension:DiscussionTools/API) and `Special:GoToComment`
- [Help:Signatures](https://en.wikipedia.org/wiki/Help:Signatures), [Template:Unsigned](https://en.wikipedia.org/wiki/Template:Unsigned), [Template:Outdent](https://en.wikipedia.org/wiki/Template:Outdent), [Template:Archive top](https://en.wikipedia.org/wiki/Template:Archive_top)
- [Manual:$wgDefaultUserOptions](https://www.mediawiki.org/wiki/Manual:$wgDefaultUserOptions) (`date`) and MediaWiki's per-language `$dateFormats`
- `tools/talk_dumps.py`, `tools/talk_fixtures.py`, `tools/talk_split.py`, `tools/talk_compare.py`; the samples of 2026-10-21, committed in `crates/scatter-talk/tests/fixtures/`
- The same samples, moved on 2026-11-03 to [`docs/samples/enwiki-talk/`](../samples/enwiki-talk/README.md)
- Project note `claude/talk-page-conversion.md`

## Amendment log

### A1. Closures and collapsed ranges, not wrapper posts

- **Date:** 2026-10-21
- **Source:** Direct: James, design discussion of 2026-10-21
- **Change:** amends §3, §4, §8, §12
- **Summary:** A wrapper template becomes data, not a box. The comments inside every wrapper are split as anywhere else, and its two calls leave the posts' text, kept verbatim in `imported_from.wrapper`. A wrapper enclosing a whole section is a closure: its statement becomes the closer's post with `status: closed`, a new default status (James: "'closed' is a good name for the status"), appended after the comments it closes, and the thread shows collapsed as every closed thread does. Any other wrapper, around a subsection or part of one, closed or not, becomes a collapsing post whose `collapse` names the first and last posts it enclosed; their parents do not change. Collapsing a range is a native feature of threads that any editor may use, not an import-only field (James: "Range collapsing should be a native feature"; [0019](0019-discussions.md) A25). Wrapper posts, their `wrap` field and the placeholder rendering are withdrawn, and so is the section fallback for a wrapper enclosing part of a section, which on the 2026-10-21 samples cost the incidents noticeboard 5 of its 38 threads and 195 of its 989 comments. A wrapper opened inside a comment that ends before it closes is still a construct a boundary may not fall inside. Settles Q2. Chapter text: [13](../architecture/13-mirrored-pages.md) §5.14, §5.15; [14](../architecture/14-discussions.md) §1.5, §1.6, §2.1, §2.4; [22](../architecture/22-crates-and-stack.md) §2.1.

Replaced text (§3):

> **A wrapper** is a registered template pair (§5) that collapses or closes a discussion: `{{archive top}}`…`{{archive bottom}}`, `{{hat}}`…`{{hab}}`, `{{collapse top}}`…`{{collapse bottom}}` and their relatives. Noticeboard and RfC archives are mostly closed discussions wrapped this way.
> - **A wrapper that encloses a whole section or a whole subsection is lifted.** Its two calls are removed from the posts' text and become a **wrapper post**: a `post` whose text is the opening call, usually the closer's signed statement, and whose `wrap` field holds the closing call. It is the first post of its section or subsection, and is attributed to the closer, the signature in the opening call, by §6. The posts it encloses keep their own parents; nothing replies to the wrapper.
> - **A wrapper that encloses only part of a section** is not lifted, and the section falls back (§4).
>
> Heading posts and wrapper posts are written only by imports, as wikitext posts are, and render as what they were. A heading post renders as a heading at its depth. A wrapper post renders by expanding its opening call, a placeholder and its closing call once, then putting the enclosed posts' rendered HTML at the placeholder. So `{{archive top}}`'s box closes around the discussion it closed.

Replaced text (§4):

> - a comment boundary would fall inside a construct that opens before the boundary and closes after it: a table, an HTML block element left open, a template call spanning lines, or a wrapper not lifted by §3;

Replaced text (§8):

> The thread payload ([0019](0019-discussions.md) §4) gains three content-part fields and widens one. All four are written only by imports, and a `post` from an editor that carries any of them is refused.
>
> | `wrap` | `post` (optional) | The closing call of a wrapper post, whose text is the opening call (§3) |

Replaced text (§12):

> | `scatter-threads` | Heading posts, wrapper posts, `imported_from` on `post`, and refusing those fields on an editor's `post` (§3, §8) |
>
> | `triplespace-ui` | Rendering heading and wrapper posts, anchors, the imported post's byline, and `Special:GoToComment` (§3, §9) |

### A2. The splitter as built

- **Date:** 2026-10-21
- **Source:** Direct: James, design discussion of 2026-10-21
- **Change:** amends §2, §3, §4, §5, §6, §13
- **Summary:** 0090 now says what `scatter-talk` does, on James's direction that it "should be updated to reflect what the code ended up doing". **Boxes** (James: "The 'barnstar rule' is a good call (though it occurs in other contexts as well where messages are wrapped in pretty boxes)"): a signature inside a table, block element, template call, HTML comment or in-comment wrapper still open on its line does not end the comment; the comment runs on to the line that closes the construct, and only a second signature or a heading before then makes the section fall back. Without it every barnstar section on a user talk page fell back; on the 2026-10-21 samples it recovered all 18 comments lost in Jimbo Wales's archive. **Level-one headings** are read as level two when anything but blank lines is directly under them, not only comments, and a deeper heading with no thread to belong to starts one. **Parents** may be unsigned posts, and unsigned text just before a wrapper that encloses comments is a post of its own. **Wrappers** are recognised only on lines of their own; one encloses a whole section when it opens before anything else and nothing follows it; one that encloses no complete comment is that comment's text. **Aliases:** a signature name counts as the account's own when it makes up at least half of the signatures the account added under names not its own, as well as at least two, so a delivery bot takes no sender's name. **Zones:** a printed zone abbreviation gives a timestamp's offset; the caller supplies the abbreviations and the wiki's offset from siteinfo's `timezone` and the tz database, and `scatter-talk` stays pure. **Testing:** the samples are committed as fixtures (James: "we can commit the sample fixtures if they are not too big"), about 3.3 MB compressed, with a README carrying their licence and attribution, which settles Q6; the test holds the splitter to agreement thresholds rather than a list of known differences, which was never built. Chapter text: [13](../architecture/13-mirrored-pages.md) §5.12, §5.13, §5.14, §5.15, §5.16; [22](../architecture/22-crates-and-stack.md) §4.9.

Replaced text (§2):

> - **A comment's text** is everything in its section since the previous comment ended, less blank lines at either end. Unsigned text before a signed comment is part of that comment unless history says otherwise (§6). Unsigned text after the last comment of a section is a post of its own, attributed by §6.
>
> - **A comment's depth** is the indentation of its first line. **Its parent** is the nearest earlier comment in the same section or subsection with a smaller depth. A comment at depth 0 has no parent, or the heading post of its subsection (§3). After a registered outdent template (`{{outdent}}`, `{{od}}`, §5), the next comment is a reply to the comment before it, whatever its depth.

Replaced text (§3):

> - **Level 1 is not structure.** A level-one heading's text is dropped and the sections under it are threads like any other. This is how the reference desks date their questions (`= October 7 =`). A level-one heading with comments directly under it and no level-two heading is read as level two.
>
> - **A wrapper that encloses a whole section is a closure.** Its statement, the opening call's `result`, `reason` or first parameter, usually signed by the closer, becomes a **closing post**: a `post` by the closer carrying `status: closed` ([0019](0019-discussions.md) §6), appended after the comments it closes, at the closer's time. The thread then shows collapsed, as every closed thread does, with the closing statement as its summary.
>
> - **A wrapper inside one comment**, whose two calls fall within that comment (`{{cot}}` around a long explanation), is that comment's text, rendered with it.

Replaced text (§4):

> - a comment boundary would fall inside a construct that opens before the boundary and closes after it: a table, an HTML block element left open, a template call spanning lines, or a wrapper opened inside a comment that ends before the wrapper closes (§3);

Replaced text (§5):

> the timezone, in which a timestamp without a zone is read, with the tz database's daylight-saving rules; and the namespace names and aliases and special-page aliases that a user link may use.

Replaced text (§6):

> That account. An account's names are those it signed with in its own additions at least twice in the history read, and its former names in the repository's rename log where that is readable. This catches renames and usurped names, which a lookup of today's names gets wrong

Replaced text (§13):

> **`scatter-talk` has golden tests against DiscussionTools.**
> - **The fixtures** are made by `tools/talk_dumps.py`: a page's revision as a dump, with `discussiontoolspageinfo`'s parse of that same revision.
> - **The comparison:** the test compares comments by name and parents by name.
> - **The reference implementation:** `tools/talk_split.py` is the Python prototype of §2, and `tools/talk_compare.py` its comparison. The Rust splitter is held to the prototype's agreement or better.
> - **Known differences:** where the splitter is right and DiscussionTools is not, such as old date formats or a subsection's first comment, the case goes in a known-differences list with its reason. The test fails when one of them starts matching, as the Librarybase parse goldens do.
> - **Attribution:** it is tested on a page with full history, against the counts of the research.

### A3. `scatter-talk-import`: one mapping for every import job

- **Date:** 2026-10-22
- **Source:** Direct: James, design discussion of 2026-10-22
- **Change:** extends §1, §3, §12
- **Summary:** Mapping a conversion onto thread records is part of the import (James: "it would make sense if the import job handled the mapping, since mapping arguably is part of the import"), done once in a new pure crate that every import job depends on (James: "What if we made it its own crate that others can depend on?"; the name `scatter-talk-import` approved). It keeps `scatter-talk` and `scatter-threads` independent of each other, and spares the fork job and `Special:Import` a dependency on `scatter-ingest`. It plans each thread's records in append order with parents and collapsed ranges by position, and the job resolves positions to revision IDs as it appends. Two rules the mapping needed: a thread's subject is its heading as plain text (§1), and a section that opens with a subheading opens its thread with that heading as text (§3). Chapter text: [13](../architecture/13-mirrored-pages.md) §5.12, §5.14; [22](../architecture/22-crates-and-stack.md) §2.1 (by [0005](0005-crate-organization.md) A103).

### A4. Current text relocated to the architecture chapters

- **Date:** 2026-10-30
- **Source:** [0050](0050-adr-format.md) §14
- **Change:** relocates §1–§13
- **Summary:** The Decision's current text now lives in the architecture chapters [03](../architecture/03-storage-caches-and-search.md), [10](../architecture/10-pages-and-content-models.md), [13](../architecture/13-mirrored-pages.md), [14](../architecture/14-discussions.md), [21](../architecture/21-special-pages.md), [22](../architecture/22-crates-and-stack.md), [23](../architecture/23-configuration-and-registry.md), in the sections each pointer names; this ADR keeps its headings, provenance lines, Context, Consequences, Open questions and this log. The last commit in which this file carried the text is `2e2a7c8`. No decision changed.
