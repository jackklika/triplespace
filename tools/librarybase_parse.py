#!/usr/bin/env python3
"""Download a wiki's own `action=parse` renders of its wikitext pages, as golden fixtures
for the built-in renderer (architecture 11 §1.2; claude/librarybase-wikitext-subset.md).

    tools/librarybase_parse.py docs/samples/librarybase/parse
    tools/librarybase_parse.py OUT --history          # every revision, not just the latest
    tools/librarybase_parse.py OUT --api https://example.org/w/api.php --namespaces 4

Pages are every page whose content model is `wikitext`, in every namespace whose default
model is not a Wikibase one (or only in --namespaces). Each revision is parsed by `oldid`,
so a render is pinned to the revision it came from. For each one OUT gets:

    pages/<pageid>/<revid>.wiki    the source the wiki parsed
    pages/<pageid>/<revid>.html    the `text` of the parse, unchanged
    pages/<pageid>/<revid>.json    everything else the parse returned (links, sections, ...)

and `manifest.json` records the API, the wiki's generator, the parse parameters, when it
ran, and each page's ID, namespace, title, redirect flag and revisions. Files already
there are kept, so a second run only fetches what is missing; --force fetches again.

Section edit links are turned off (`disableeditsection`) and the limit report is left
out, since neither is part of what a page reads as; the TOC, the `mw-parser-output`
wrapper and everything else are as the wiki serves them.

Standard library only. Requests are sequential, `--delay` seconds apart (default 0.2),
with `maxlag=5` and a few retries on network errors and lag.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

DEFAULT_API = "https://librarybase.org/w/api.php"
USER_AGENT = "triplespace-tools/librarybase_parse.py (https://github.com/scatterbase/triplespace)"

PARSE_PROPS = (
    "text|wikitext|revid|displaytitle|sections|links|iwlinks|langlinks|templates"
    "|images|externallinks|categories|properties|parsewarnings"
)
PARSE_PARAMS = {
    "action": "parse",
    "prop": PARSE_PROPS,
    "disableeditsection": "1",
    "disablelimitreport": "1",
    "formatversion": "2",
}


class Api:
    def __init__(self, url, delay, retries=5):
        self.url = url
        self.delay = delay
        self.retries = retries
        self.last = 0.0

    def get(self, **params):
        params = {"format": "json", "formatversion": "2", "maxlag": "5", **params}
        url = self.url + "?" + urllib.parse.urlencode(params)
        for attempt in range(self.retries + 1):
            wait = self.last + self.delay - time.monotonic()
            if wait > 0:
                time.sleep(wait)
            self.last = time.monotonic()
            try:
                req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(req, timeout=60) as r:
                    data = json.load(r)
            except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
                if attempt == self.retries:
                    raise
                print(f"  retrying after {e}", file=sys.stderr)
                time.sleep(2 ** attempt)
                continue
            err = data.get("error")
            if err and err.get("code") == "maxlag" and attempt < self.retries:
                time.sleep(5)
                continue
            return data
        raise RuntimeError("unreachable")

    def query_all(self, **params):
        """Every result of a `query`, following `continue`."""
        cont = {}
        while True:
            data = self.get(action="query", **params, **cont)
            if "error" in data:
                raise RuntimeError(f"{params}: {data['error']}")
            yield data.get("query", {})
            if "continue" not in data:
                return
            cont = data["continue"]


def content_namespaces(api):
    data = api.get(action="query", meta="siteinfo", siprop="general|namespaces")
    if "error" in data:
        raise RuntimeError(data["error"])
    q = data["query"]
    keep = []
    for ns in q["namespaces"].values():
        if ns["id"] < 0:
            continue
        if ns.get("defaultcontentmodel", "wikitext").startswith("wikibase"):
            continue
        keep.append(ns["id"])
    return q["general"], sorted(keep)


def wikitext_pages(api, namespaces):
    for ns in namespaces:
        for q in api.query_all(generator="allpages", gapnamespace=ns, gaplimit="max", prop="info"):
            for p in q.get("pages", []):
                if p.get("contentmodel") == "wikitext":
                    yield p


def revisions(api, pageid):
    revs = []
    for q in api.query_all(prop="revisions", pageids=pageid, rvprop="ids|timestamp|size",
                           rvlimit="max", rvdir="newer"):
        for p in q.get("pages", []):
            revs.extend(p.get("revisions", []))
    return revs


def fetch(api, out, pageid, revid, force):
    base = out / "pages" / str(pageid)
    html, meta, wiki = (base / f"{revid}{ext}" for ext in (".html", ".json", ".wiki"))
    if not force and html.exists() and meta.exists() and wiki.exists():
        return None
    data = api.get(**PARSE_PARAMS, oldid=revid)
    if "error" in data:
        return data["error"]
    parse = data["parse"]
    base.mkdir(parents=True, exist_ok=True)
    html.write_text(parse.pop("text"), encoding="utf-8")
    wiki.write_text(parse.pop("wikitext"), encoding="utf-8")
    meta.write_text(json.dumps(parse, ensure_ascii=False, indent=1, sort_keys=True) + "\n",
                    encoding="utf-8")
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out", type=Path, help="directory to write the fixtures into")
    ap.add_argument("--api", default=DEFAULT_API, help=f"the wiki's api.php (default {DEFAULT_API})")
    ap.add_argument("--namespaces", help="comma-separated namespace numbers (default: every non-Wikibase one)")
    ap.add_argument("--history", action="store_true", help="parse every revision, not just the latest")
    ap.add_argument("--delay", type=float, default=0.2, help="seconds between requests (default 0.2)")
    ap.add_argument("--force", action="store_true", help="fetch again even where files exist")
    a = ap.parse_args()

    api = Api(a.api, a.delay)
    general, namespaces = content_namespaces(api)
    if a.namespaces:
        namespaces = [int(n) for n in a.namespaces.split(",")]
    print(f"{general.get('sitename')} ({general.get('generator')}), namespaces {namespaces}", file=sys.stderr)

    a.out.mkdir(parents=True, exist_ok=True)
    pages, errors = [], []
    for p in wikitext_pages(api, namespaces):
        if a.history:
            revs = revisions(api, p["pageid"])
        else:
            revs = [{"revid": p["lastrevid"]}]
        print(f"{p['title']}: {len(revs)} revision(s)", file=sys.stderr)
        for r in revs:
            err = fetch(api, a.out, p["pageid"], r["revid"], a.force)
            if err:
                print(f"  {r['revid']}: {err.get('code')}: {err.get('info')}", file=sys.stderr)
                errors.append({"pageid": p["pageid"], "revid": r["revid"], "error": err})
        pages.append({
            "pageid": p["pageid"],
            "ns": p["ns"],
            "title": p["title"],
            "redirect": bool(p.get("redirect")),
            "lastrevid": p["lastrevid"],
            "revisions": revs,
        })

    manifest = {
        "api": a.api,
        "sitename": general.get("sitename"),
        "generator": general.get("generator"),
        "fetched": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "parse_params": PARSE_PARAMS,
        "history": a.history,
        "namespaces": namespaces,
        "pages": pages,
        "errors": errors,
    }
    (a.out / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    n = sum(len(p["revisions"]) for p in pages)
    print(f"{len(pages)} pages, {n} revisions, {len(errors)} errors -> {a.out}", file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
