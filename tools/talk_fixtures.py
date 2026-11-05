#!/usr/bin/env python3
"""Turn the samples `tools/talk_dumps.py` wrote into `scatter-talk`'s committed test fixtures
(0090 §13).

    tools/talk_fixtures.py data/talk-samples docs/samples/enwiki-talk

The fixtures keep the samples' layout and names, so the golden tests read either:

  manifest.json                    copied as is
  current.xml.bz2                  the current-revision dump, bzip2-compressed
  history.xml.bz2                  the full-history dump, recompressed from gzip to bzip2,
                                   which is about half the size on a page's history
  discussiontools/*.json.bz2       each DiscussionTools response without its rendered `html`
                                   fields, which the tests never read and which are most of
                                   its size
  README.md                        where the text came from and its licence, page by page

The output is deterministic: the same samples give the same bytes.

Standard library only.
"""

import argparse
import bz2
import gzip
import json
import pathlib
import urllib.parse

LICENCE = """\
# `scatter-talk` test fixtures

Talk pages from {sitename}, fetched by `tools/talk_dumps.py` from `{api}` and converted by
`tools/talk_fixtures.py`, for the golden tests in `crates/scatter-talk/tests/samples.rs` (ADR 0090 §13).

**The text is by {sitename}'s contributors** and is available under the
[Creative Commons Attribution-ShareAlike 4.0 License](https://creativecommons.org/licenses/by-sa/4.0/)
(older revisions also under the GFDL). The authors of each page are listed in its history,
linked below; the full-history dump also names the author of every revision. The DiscussionTools
responses are the extension's reading of the same revisions. The fixtures are test data: they
are read by the tests at run time and are not compiled into any crate.

**Changes:** the dumps are recompressed with bzip2, and the DiscussionTools responses have their
rendered `html` fields removed. Nothing else is altered.

| Page | Revision | Revisions in the dump | History |
|---|---|---|---|
{rows}
"""


def strip_html(v):
    if isinstance(v, dict):
        return {k: strip_html(x) for k, x in v.items() if k != "html"}
    if isinstance(v, list):
        return [strip_html(x) for x in v]
    return v


def write_bz2(path: pathlib.Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bz2.compress(data, 9))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("samples", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    args = ap.parse_args()
    src, out = args.samples, args.out

    raw = (src / "manifest.json").read_bytes()
    manifest = json.loads(raw)
    out.mkdir(parents=True, exist_ok=True)
    (out / "manifest.json").write_bytes(raw)

    for dump in manifest["dumps"].values():
        name = dump["file"]
        data = (src / name).read_bytes()
        if name.endswith(".gz"):
            data = gzip.decompress(data)
            name = name[: -len(".gz")]
        write_bz2(out / f"{name}.bz2", data)

    base = manifest["api"].rsplit("/", 1)[0] + "/index.php"
    rows = []
    for page in manifest["pages"]:
        dt = page.get("discussiontools", {}).get("file")
        if dt:
            v = json.loads((src / dt).read_text(encoding="utf-8"))
            data = json.dumps(strip_html(v), ensure_ascii=False, separators=(",", ":")).encode()
            write_bz2(out / f"{dt}.bz2", data)
        title = urllib.parse.quote(page["title"].replace(" ", "_"), safe="/:")
        rows.append(
            f"| {page['title']} | [{page['revid']}]({base}?oldid={page['revid']}) "
            f"| {page['revisions']} | [history]({base}?title={title}&action=history) |"
        )

    (out / "README.md").write_text(
        LICENCE.format(
            sitename=manifest.get("sitename", "the wiki"),
            api=manifest["api"],
            rows="\n".join(rows),
        ),
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
