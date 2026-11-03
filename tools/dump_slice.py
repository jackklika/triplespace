#!/usr/bin/env python3
"""Cut a smaller, valid MediaWiki XML dump out of a Wikibase dump.

Two ways to choose pages:

    # the first 10,000 entities, in dump order (the pages `triplespace adopt` sees first)
    dump_slice.py librarybase-20261005.xml.gz slice-10k.xml.gz --limit 10000

    # a reproducible random sample of 200 items, plus every property page, plus Q1
    dump_slice.py librarybase-20261005.xml.gz sample.xml.gz --sample 200 --seed 1 \\
        --all-properties --include Q1

    # the same, plus 10 items whose pages have more than one revision
    dump_slice.py librarybase-20261101.xml.gz sample.xml.gz --sample 200 --seed 1 \\
        --history 10 --all-properties --include Q1

An entity is a page whose <model> is wikibase-* (item, property, lexeme). With --limit
the slice is a prefix of the dump: non-entity pages before the Nth entity are kept too.
With --sample the slice holds only entity pages (and, with --keep-other, the non-entity
pages as well), written in their original order; sampling is a reservoir over the
non-property entities, so the same --seed on the same dump gives the same slice.

A page is copied whole, with every revision a full-history dump gives it. On a real wiki
few pages have more than one revision, so a random sample rarely holds any; --history N
adds N items drawn from those that do, by a second reservoir on the same seed, so the
--sample N part is the same with or without it.

--all-properties copies every wikibase-property page wherever it sits in the dump, so
each snak in the slice has the datatype its property declares (`adopt` reports snaks
on properties the dump does not define as untyped). It works with either mode.
--defined-only then drops sampled items that still use a property the slice does not
define — which only happens when the input is itself a slice — since Wikibase clients
fetch a statement's property and fail when it is missing.

Streams the input; only the selected pages of a sample are held in memory. Input and
output may be .gz or plain .xml.
"""

import argparse
import gzip
import random
import re
import sys

MODEL = re.compile(r"<model>(wikibase-[a-z]+)</model>")
TITLE = re.compile(r"<title>(?:[A-Za-z ]+:)?([QPL]\d+)</title>")
PROP_USE = re.compile(r'&quot;property&quot;:&quot;(P\d+)&quot;|"property":"(P\d+)"')


def open_in(path):
    return gzip.open(path, "rt", encoding="utf-8") if path.endswith(".gz") else open(path, encoding="utf-8")


def open_out(path):
    return gzip.open(path, "wt", encoding="utf-8", compresslevel=6) if path.endswith(".gz") else open(path, "w", encoding="utf-8")


def pages(inp, out):
    """Yields (index, model, entity_id, text) per <page>, copying the header to out."""
    page = None  # lines of the page being read, or None between pages
    i = 0
    for line in inp:
        s = line.strip()
        if page is None:
            if s == "<page>":
                page = [line]
            elif s == "</mediawiki>":
                return
            else:
                out.write(line)  # the <siteinfo> header, whitespace
            continue
        page.append(line)
        if s == "</page>":
            text = "".join(page)
            page = None
            m = MODEL.search(text)
            t = TITLE.search(text)
            yield i, (m.group(1) if m else None), (t.group(1) if t else None), text
            i += 1


def prefix(stream, out, limit, all_properties):
    entities = written = 0
    for _, model, _, text in stream:
        if entities >= limit:
            if not all_properties:
                break
            if model != "wikibase-property":
                continue
        out.write(text)
        written += 1
        if model:
            entities += 1
    return written, entities


def sample(stream, out, n, seed, all_properties, include, keep_other, defined_only, history=0):
    rng = random.Random(seed)
    chosen = []  # reservoir of (index, text)
    seen = 0
    hrng = random.Random(f"history-{seed}")
    hchosen = []  # reservoir of (index, text) over pages with more than one revision
    hseen = 0
    kept = []  # (index, text) copied unconditionally
    for i, model, eid, text in stream:
        if model is None:
            if keep_other:
                kept.append((i, text))
            continue
        if eid in include or (all_properties and model == "wikibase-property"):
            kept.append((i, text))
            continue
        if model == "wikibase-property":
            continue  # properties are all-or-nothing; items and lexemes are sampled
        seen += 1
        if len(chosen) < n:
            chosen.append((i, text))
        else:
            j = rng.randrange(seen)
            if j < n:
                chosen[j] = (i, text)
        if history and text.count("<revision>") > 1:
            hseen += 1
            if len(hchosen) < history:
                hchosen.append((i, text))
            else:
                j = hrng.randrange(hseen)
                if j < history:
                    hchosen[j] = (i, text)
    if history:
        print(f"{len(hchosen)} of {hseen} items with more than one revision", file=sys.stderr)
    selected = sorted(dict(kept + chosen + hchosen).items())
    if defined_only:
        defined = {TITLE.search(t).group(1) for _, t in selected if MODEL.search(t) and TITLE.search(t)}
        defined = {d for d in defined if d.startswith("P")}
        before = len(selected)
        selected = [
            (i, t) for i, t in selected
            if not MODEL.search(t) or {a or b for a, b in PROP_USE.findall(t)} <= defined
        ]
        if before != len(selected):
            print(f"dropped {before - len(selected)} entities using properties the slice does not define", file=sys.stderr)
    for _, text in selected:
        out.write(text)
    entities = sum(1 for _, t in selected if MODEL.search(t))
    return len(selected), entities, seen


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("src")
    ap.add_argument("dst")
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--limit", type=int, help="the first N entities, in dump order")
    mode.add_argument("--sample", type=int, help="a random sample of N items (reservoir)")
    ap.add_argument("--seed", type=int, default=0, help="RNG seed for --sample (default 0)")
    ap.add_argument("--all-properties", action="store_true", help="copy every property page")
    ap.add_argument("--include", default="", help="entity IDs to copy regardless, comma-separated (Q1,P5)")
    ap.add_argument("--keep-other", action="store_true", help="with --sample, keep non-entity pages too")
    ap.add_argument("--history", type=int, default=0,
                    help="with --sample, also N items whose pages have more than one revision")
    ap.add_argument("--defined-only", action="store_true",
                    help="with --sample, drop items using properties the slice does not define")
    a = ap.parse_args()
    include = {s.strip() for s in a.include.split(",") if s.strip()}

    with open_in(a.src) as inp, open_out(a.dst) as out:
        stream = pages(inp, out)
        if a.limit is not None:
            written, entities = prefix(stream, out, a.limit, a.all_properties)
            note = ""
        else:
            written, entities, seen = sample(stream, out, a.sample, a.seed, a.all_properties, include, a.keep_other, a.defined_only, a.history)
            note = f" (sampled from {seen}, seed {a.seed})"
        out.write("</mediawiki>\n")
    print(f"{written} pages, {entities} entities -> {a.dst}{note}", file=sys.stderr)


if __name__ == "__main__":
    main()
