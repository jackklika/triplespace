---
name: triplespace-adr
description: Write a new Triplespace ADR, or change an existing ADR, architecture chapter or registry file, following chapter 24 (pointers from birth; fold, log, mark), and validate it with the doc checkers. Use for any change to docs/decisions, docs/architecture or docs/registry.
---

# Changing the Triplespace architecture

The rules are chapter 24, `docs/architecture/24-documentation.md` (decided in ADRs 0050 and
0098). It is the authority; this skill is the working procedure. Copy new ADRs from
`docs/decisions/TEMPLATE.md`, whose comments repeat the rules. Use the `triplespace-architecture`
skill to find what you are changing.

## The one rule to keep in mind

**Text lives in the chapters; an ADR is headings, pointers, and the why.** An ADR's Decision
section is never prose: its body is the one line `*Current text: [03](../architecture/03-….md)
§4.5.*`, which `pointers.py` writes from the chapters' `*Sources:*` lines. So writing or changing
a decision always means editing a chapter section and citing the ADR section in its Sources line.

## Lifecycle

1. **Research.** Read the home chapter, the ADRs its *Sources* lines cite (Context, Open
   questions, Amendment log), `INDEX.md` and `OPEN.md` (what is already open or unconfirmed on
   the subject), and the relevant registry files. List every ADR and
   chapter section the change will touch.
2. **Confirm.** Put the specific choices to James: each option, its cost and your
   recommendation. Name every assumption. Do not draft past an unconfirmed choice.
3. **Draft** the ADR, or the direct change: the text in the chapters, the record in the ADR.
4. **Fold** the change into every affected chapter section, ADR and registry file (below).
5. **Validate** with the checkers (below), and fix every reported line.
6. **Leave it uncommitted** on the checked-out branch, or on a named feature branch if `main` is
   checked out, for James's review. Never commit or push without his go-ahead.

## Two kinds of change

- **A new ADR** decides something and changes other ADRs. It names them in its header's
  `Changes` and its section headings; each target's log entry names the ADR section as its Source.
- **A direct decision** changes an ADR with no new ADR: James decides in discussion. The log
  entry's Source is `Direct: James, design discussion of YYYY-MM-DD`, and its Summary holds the
  decision in full, because nowhere else does.
- **A call made while building** is a direct decision you make: log it in the owning ADR in the
  same change as the code, folded like any other, with Source
  `Direct: Claude, while building <what>, YYYY-MM-DD; unconfirmed`. `OPEN.md` lists it for James;
  he confirms by editing the Source line or overturns it with a new entry. Never park such calls
  in notes outside the ADRs.

## Writing a new ADR

- File `docs/decisions/NNNN-slug.md`, with the next free number (`ls docs/decisions | tail`).
  H1 `# NNNN. Title`. The number and slug never change.
- Header, in order, one line each, values never sentences:
  - `Status: Proposed`. Never edit it: `check_adrs.py --index` writes it from the code's
    `implements` claims (24 §15).
  - `Date`.
  - `Updated` only once there is a log.
  - `Author: James Hare / Claude Fable`. The agent name must be an `[[agent]]` in
    `docs/registry/version.toml`; check 9 enforces this.
  - `Changes` and `Uses`: linked numbers in ascending order, omitted when empty.
  - No `Chapters` field and no *Changes to other ADRs* section: both are derived (24 §2.2).
- **Context** is historical: what was known and wanted. *Direction* quotes James's directives
  verbatim, with the date and occasion.
- **Decision** sections are `### N. Title` and `#### N.M Title`, numbered once and never
  renumbered or reused. A heading names what the section changes elsewhere, with the six verbs only:
  `### 2. Talk pages are composite (amends 0008 §2; extends 0008 §3)`.

  | Verb | Use when | Target log entry | Replaced text |
  |---|---|---|---|
  | `amends` | replaces or contradicts text | yes | required |
  | `extends` | adds, contradicts nothing | yes | none |
  | `supersedes` | a section or the ADR no longer holds | yes | required |
  | `corrects` | the text was wrong; the decision is unchanged | yes | required |
  | `settles` | answers an open question | no | — |
  | `uses` | relies on it, changes nothing | no | — |

  Do not use refines, specifies, adds, allocates, answers or builds on.
- **The section's text goes into the chapters**, not under the heading. For each section, find or
  create the chapter section (`##`/`###`) that holds the subject, write the text there in the
  chapter's voice, and add `[NNNN](../decisions/NNNN-slug.md) §N` to that section's `*Sources:*`
  line (a new section gets a new Sources line as its first line). Trailer sections (Crates, API,
  UI, Storage, Permissions, Search, RDF, Log events, Rate limits, Tenants, Scatterbase) go to the
  chapter that holds that catalogue (24 §10.3): 22, 18, 19, 03, 09, 03, 02, 16, 07, 08, 22.
  A **Crates** trailer is an edit to `docs/registry/crates.toml`: a new `[[crate]]` entry, or a
  sentence with its citation appended to an existing entry's `contents` (and `depends` for a new
  edge); then `python3 architecture/crates.py --write .` regenerates 22 §2.1 and `CRATES.md`, and
  `cargo xtask deps` must still pass. Likewise an **API** trailer's REST routes go to
  `registry/routes.toml` and its Action API modules to `action-api.toml`, a **Permissions** trailer to `permissions.toml` (and `groups.toml` for who
  holds it), a **Log events** trailer to `log-events.toml`; `python3 architecture/catalogues.py
  --write .` regenerates 18 §3.2, 09 §2.2 and 16 §2.3.
- Then run `python3 architecture/pointers.py NNNN` from `docs/`. It writes every section's
  pointer line from the Sources lines and reports any section no chapter cites: write that text
  into a chapter and run it again.
- Then Alternatives considered (optional), Consequences, Open questions (`**Q1. Title.**`) and
  References.
- Read the result as a reviewer will: `python3 tools/adr.py show NNNN --full` prints the ADR with
  every pointer replaced by its chapter text; CI posts the same on the pull request.
- Citations: link the first mention of another ADR in each section,
  `[0008](0008-namespaces-and-document-pages.md) §2` from an ADR,
  `[0008](../decisions/0008-namespaces-and-document-pages.md) §2` from a chapter. Name both ends
  of a relation outside a heading ("0049 §2 amends 0008 §2"). Never use dated forms ("as amended
  2026-09-27") or placeholders (`§…`).

## Changing an existing ADR: fold, log, mark, answer (24 §4)

For each target ADR *Y*:

1. **Fold.** Follow *Y*'s pointer to the chapter section and rewrite the affected text there so
   it states the current decision, citing the source in the sentence. Add the changing ADR's
   section to that chapter section's Sources line. Every change is folded, extensions included.
   Never use a blockquote callout such as `> **Amended by …**`.
2. **Log.** Append `### An. Title` to *Y*'s Amendment log, with Date, Source, Change (verbs plus
   *Y*'s sections) and Summary. Quote removed or contradicted chapter text verbatim as
   `Replaced text ([03](../architecture/03-….md) §4.5, as it stood):`. IDs are contiguous and
   never edited.
3. **Mark.** Add `An` to the `*Changed by …*` provenance line of each changed *Y* section,
   creating the line if needed. Strike any Consequence the change falsifies
   (`~~**Claim.** …~~ *Why (An).*`). Set *Y*'s `Updated: YYYY-MM-DD (An)`. If *Y* is Accepted,
   its status becomes `Accepted with proposed amendment (An)`.
4. **Answer.** Strike any open question the change settles, and add *Settled by NNNN §N: answer.*
   This needs no log entry.

Nothing is deleted. A section that no longer holds becomes a stub, *Superseded by NNNN §M (An).*,
with its old text in the log entry. If a chapter section is split or renamed, rerun
`pointers.py` for the ADRs it cites: refreshing a pointer is not logged.

## Chapters

- A chapter describes; it decides nothing. A disagreement found in a chapter is a decision for
  an ADR.
- Every `##` and `###` heading is followed by a `*Sources:*` line. Those lines are the only map
  between ADRs and chapters: there is no `MAP.md`, no `Chapters` header, and nothing to register.
  `map.py` fails when an ADR section is cited by no chapter.
- New chapters take the next number and a row in `architecture/README.md`'s table; chapters and
  their sections are never renumbered.
- Catalogue tables (crate map, `view` schema, routes, permissions, log events) are registry data
  (24 §11): when one has a registry file, edit the TOML and regenerate the table; until then the
  chapter table is the authority.

## Registry files

- A registry change is a commit. Codes, numbers and slugs are never reused or changed: retire
  an entry with `retired = true`.
- The chapter states the rule and names the registry file as the authority. A table copied into
  a chapter is generated from the file, never edited by hand.
- The crates embed the files, and their parsers reject unknown fields. A field the parser does
  not accept yet is written as a **comment** beside the entry and listed under *Pending
  allocations* in `docs/registry/README.md`, until the crate learns it.
- Add or update the file's row in the registry README table (*Defined in*).
- `settings.toml` drives chapter 23's catalogue: run `python3 architecture/settings.py --write .`.
- Run `cargo build` for the embedding crate: a malformed TOML file fails the build.

## When the code is done

Finishing a section is part of the change that finishes it (24 §15): add `implements NNNN §N (An)`
in a comment at the completing src code (the entry is the last one in the section's provenance
line; none if it has no provenance line; `§N.M` or `§N rule K` for a part) and make sure a test
cites the section (`// 0013 §7` in the test is enough). Then `docs/check.sh --write` recomputes
the ADR's Status and `INDEX.md`. When a change implements a log entry on an Accepted ADR, move
the claim's entry ID. `tools/coverage.py --todo` lists what is still not done.

## Validating

```sh
docs/check.sh --write    # regenerate INDEX.md and chapter 23's settings tables, then check all
docs/check.sh            # check only (what CI runs); exits non-zero on any failure
```

It runs `check_adrs.py`, `map.py`, `settings.py` and the log test vectors, and names the ones that
failed. Each checker also runs alone from `docs/`, e.g. `python3 decisions/check_adrs.py .`, which
exits 1 when it lists any problem.

`check_adrs.py` covers: links and § citations; header order and `Changes` ↔ the targets' logs;
heading verbs; log IDs and Replaced text; provenance lines matching the log; every Decision section
a pointer or a stub, and each pointer's chapter section citing it back; `NNNN An` and `Qn`
citations; crate names against chapter 22 §2.1; `Special:` names against `special-pages.toml`;
agent names against `version.toml`; `INDEX.md` being current. `map.py` covers the chapters: every
heading has a Sources line, every citation resolves, every ADR section is cited somewhere, and
README's table matches the files.

Never hand-edit `INDEX.md`. Regenerate it.

A merge or rebase conflict in a generated file (`INDEX.md`, `OPEN.md`, `CRATES.md`, or a generated
table in chapters 09, 16, 18, 22 or 23) is resolved the same way: take either side
(`git checkout --theirs -- <file>`), run `docs/check.sh --write`, and `git add` the result; in a
rebase, at each commit that stops on it. Hand-merging conflict markers in these files is never right,
even when it looks trivial.

## Bulk edits

To touch many ADRs at once, use a Python script with exact-match replacement that fails
loudly, rather than `sed`:

```python
import pathlib
def sub(path, old, new, count=1):
    p = pathlib.Path(path); t = p.read_text()
    assert t.count(old) == count, f"{path}: expected {count} of {old[:60]!r}, found {t.count(old)}"
    p.write_text(t.replace(old, new))
```

Then run the checkers and read the diff (`git diff --stat`, then the diff itself) before
reporting.

## Reporting back

Give James:
- the ADR number and title
- every chapter section and ADR changed, with the new log IDs
- the registry edits and pending allocations
- the checker results
- the assumptions and open questions that need his decision
