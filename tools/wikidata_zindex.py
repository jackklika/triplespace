#!/usr/bin/env python3
"""An indexed, seekable zstd copy of a Wikibase JSON dump, for fetching entities by ID.

The full Wikidata dump (`latest-all.json.bz2`, ~100 GB) is not in ID order (it is written in
shards by page ID: it opens Q31, Q8, Q23), and bzip2 cannot seek, so every question about a
few entities, or about the first N items, means another hours-long pass over it. This turns
the dump once into a JSON Lines file of independent zstd frames and an SQLite index from each
entity ID to its frame, after which any entity comes back in about a millisecond and a slice
of any size is a list of IDs (real-dumps.md §3.2).

    # once per dump: ~2 h for Wikidata on 16 cores, bound by bzip2 decompression
    wikidata_zindex.py build OUTDIR --input wikidata-20261026-all.json.bz2 --sha1 e463…
    wikidata_zindex.py verify OUTDIR --sample 20000   # random IDs fetched and checked
    wikidata_zindex.py info OUTDIR                    # source, sha1, settings, counts

    wikidata_zindex.py get OUTDIR Q42 P31             # one JSON entity per line, in the given order
    wikidata_zindex.py ids OUTDIR --kind Q --limit 1000000 > first-1m-items.txt
    wikidata_zindex.py ids OUTDIR --kind P > properties.txt
    wikidata_zindex.py ids OUTDIR --instance-of Q13442814 --exclude-file first-1m-items.txt \\
        --sample 1000000 --seed 1 > scholarly.txt      # P31 scan of the archive: ~20 min on 16 cores
    cat first-1m-items.txt properties.txt | \\
        wikidata_zindex.py get OUTDIR --ids-file - | gzip > slice.json.gz   # `sync --dump` reads it

`--input` takes the dump plain or compressed (.bz2 through lbzip2 when installed, else bzip2;
.gz through pigz or gzip; .zst through zstd), or `-` for standard input. A dump has only
entities that exist: redirects and deleted IDs are absent, so they are absent here too.

OUTDIR holds two files:

entities.jsonl.zst
    The dump as JSON Lines: one entity per line, in dump order, with the array's brackets and
    the trailing commas removed. It is a sequence of independent zstd frames, each holding
    whole lines (about --block-bytes of them), followed by a seek table in the zstd seekable
    format (a skippable frame), so `zstd -dc entities.jsonl.zst` gives the whole JSONL and
    seekable-zstd readers (indexed_zstd, t2sz, the zstd contrib library) can seek in it.

index.sqlite
    frame(frame, offset, csize, usize): where each frame starts in the .zst and its sizes.
    entity(key, frame, off, len): key = number * 4 + kind (Q=0, P=1, L=2, other=3); off and
    len locate the entity's JSON (without newline) in the frame's decompressed bytes.
    meta(name, value): source file, sha1, settings, counts.

Fetching one entity reads one frame (about --block-bytes uncompressed) and decompresses it;
`get` decompresses each frame once however many requested IDs it holds. Defaults (1 MiB
frames, level 9) were chosen on the first 867 MB of the 2026-10-26 dump: ratio 9.3 (bzip2:
8.7), 0.7 ms per lookup, 500 MB/s on 12 workers. Level 19 gains 8% at a sixteenth of the
speed; 4 MiB frames gain 5% at 2.5 ms per lookup. `Store` can be imported for use from Python.

Needs the `zstandard` module (Debian: python3-zstandard); otherwise the standard library.
"""

import argparse
import collections
import concurrent.futures as cf
import json
import os
import random
import re
import shutil
import sqlite3
import struct
import subprocess
import sys
import time

import zstandard

ID = re.compile(rb'"id":"([QPL])(\d+)"')
KINDS = {b"Q": 0, b"P": 1, b"L": 2}
LETTERS = "QPL?"
SEEK_MAGIC = 0x8F92EAB1
SKIPPABLE_MAGIC = 0x184D2A5E


def key_of(eid):
    """'Q42' → 168."""
    m = re.fullmatch(r"([QPL])(\d+)", eid.strip().upper())
    if not m:
        raise ValueError(f"not an entity ID: {eid!r}")
    return int(m.group(2)) * 4 + "QPL".index(m.group(1))


def id_of(key):
    return f"{LETTERS[key & 3]}{key >> 2}"


# --- build -------------------------------------------------------------------------------

_compressor = None


def _compress(args):
    level, data = args
    global _compressor
    if _compressor is None:
        _compressor = zstandard.ZstdCompressor(level=level, write_checksum=True, write_content_size=True)
    return _compressor.compress(data)


def blocks(stream, block_bytes, stats):
    """Yields (data, [(key, off, len)]) for consecutive runs of whole lines."""
    lines, entries, size = [], [], 0
    for raw in stream:
        line = raw.rstrip(b"\r\n")
        if line.endswith(b","):
            line = line[:-1]
        if line in (b"[", b"]", b""):
            continue
        m = ID.search(line, 0, 300)
        if m:
            key = int(m.group(2)) * 4 + KINDS[m.group(1)]
            stats[m.group(1).decode()] += 1
        else:
            raise ValueError(f"no entity ID near the start of line: {line[:200]!r}")
        entries.append((key, size, len(line)))
        lines.append(line)
        lines.append(b"\n")
        size += len(line) + 1
        if size >= block_bytes:
            yield b"".join(lines), entries
            lines, entries, size = [], [], 0
    if lines:
        yield b"".join(lines), entries


def open_input(path, threads):
    """The dump as a binary stream, decompressed by a child process when compressed."""
    if path == "-":
        return open(0, "rb", buffering=16 << 20), None
    tools = {
        ".bz2": [["lbzip2", "-dc", "-n", str(threads)], ["bzip2", "-dc"]],
        ".gz": [["pigz", "-dc", "-p", str(threads)], ["gzip", "-dc"]],
        ".zst": [["zstd", "-dc", "-T" + str(threads)]],
    }
    ext = os.path.splitext(path)[1]
    if ext not in tools:
        return open(path, "rb", buffering=16 << 20), None
    for cmd in tools[ext]:
        if shutil.which(cmd[0]):
            proc = subprocess.Popen(cmd + [path], stdout=subprocess.PIPE, bufsize=16 << 20)
            return proc.stdout, proc
    sys.exit(f"no decompressor for {ext} found (tried {', '.join(c[0] for c in tools[ext])})")


def build(args):
    os.makedirs(args.outdir, exist_ok=True)
    zpath = os.path.join(args.outdir, "entities.jsonl.zst")
    dbpath = os.path.join(args.outdir, "index.sqlite")
    for p in (zpath, dbpath, dbpath + "-journal"):
        if os.path.exists(p):
            sys.exit(f"{p} exists; remove it or choose another OUTDIR")

    os.environ["SQLITE_TMPDIR"] = os.path.abspath(args.outdir)  # the index sort needs GBs
    db = sqlite3.connect(dbpath)
    db.executescript("""
        PRAGMA journal_mode=OFF; PRAGMA synchronous=OFF; PRAGMA cache_size=-2000000;
        CREATE TABLE meta(name TEXT PRIMARY KEY, value TEXT);
        CREATE TABLE frame(frame INTEGER PRIMARY KEY, offset INTEGER, csize INTEGER, usize INTEGER);
        CREATE TABLE staging(key INTEGER, frame INTEGER, off INTEGER, len INTEGER);
    """)
    stats = collections.Counter()
    inp, proc = open_input(args.input, args.decompress_threads)
    out = open(zpath, "wb")
    seek = []  # (csize, usize) per frame
    offset = 0
    t0 = last = time.time()
    usize_total = 0
    pending = collections.deque()

    def drain():
        nonlocal offset, usize_total, last
        fut, frame_no, entries, usize = pending.popleft()
        comp = fut.result()
        out.write(comp)
        db.execute("INSERT INTO frame VALUES (?,?,?,?)", (frame_no, offset, len(comp), usize))
        db.executemany("INSERT INTO staging VALUES (?,?,?,?)", ((k, frame_no, o, n) for k, o, n in entries))
        seek.append((len(comp), usize))
        offset += len(comp)
        usize_total += usize
        now = time.time()
        if now - last >= 60:
            last = now
            el = now - t0
            print(f"[{el / 60:6.1f} min] {sum(stats.values()):,} entities, {usize_total / 1e9:,.1f} GB in "
                  f"({usize_total / el / 1e6:,.0f} MB/s), {offset / 1e9:,.1f} GB out, ratio "
                  f"{usize_total / max(offset, 1):.2f}", file=sys.stderr, flush=True)

    with cf.ProcessPoolExecutor(args.workers) as pool:
        for frame_no, (data, entries) in enumerate(blocks(inp, args.block_bytes, stats)):
            pending.append((pool.submit(_compress, (args.level, data)), frame_no, entries, len(data)))
            while len(pending) > args.workers * 3:
                drain()
        while pending:
            drain()
    if proc is not None and proc.wait() != 0:
        sys.exit(f"{proc.args[0]} exited {proc.returncode}: the dump is truncated or corrupt; "
                 f"{os.path.abspath(args.outdir)} is incomplete")

    # Seek table (zstd seekable format: entries of csize u32, dsize u32; no checksums).
    table = b"".join(struct.pack("<II", c, u) for c, u in seek)
    footer = struct.pack("<IBI", len(seek), 0, SEEK_MAGIC)
    out.write(struct.pack("<II", SKIPPABLE_MAGIC, len(table) + len(footer)) + table + footer)
    out.close()

    print("sorting the index…", file=sys.stderr, flush=True)
    db.executescript("""
        CREATE TABLE entity(key INTEGER PRIMARY KEY, frame INTEGER, off INTEGER, len INTEGER);
        INSERT OR IGNORE INTO entity SELECT key, frame, off, len FROM staging ORDER BY key;
        DROP TABLE staging;
    """)
    n_index = db.execute("SELECT count(*) FROM entity").fetchone()[0]
    meta = {
        "source": args.source or os.path.basename(args.input),
        "source_sha1": args.sha1 or "",
        "block_bytes": args.block_bytes,
        "level": args.level,
        "frames": len(seek),
        "entities": sum(stats.values()),
        "items": stats["Q"], "properties": stats["P"], "lexemes": stats["L"],
        "uncompressed_bytes": usize_total,
        "compressed_bytes": offset,
        "zstandard": zstandard.__version__,
        "built": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "build_seconds": round(time.time() - t0),
    }
    db.executemany("INSERT INTO meta VALUES (?,?)", ((k, str(v)) for k, v in meta.items()))
    db.commit()
    db.execute("VACUUM")
    db.close()
    if n_index != meta["entities"]:
        sys.exit(f"duplicate IDs: {meta['entities']:,} lines but {n_index:,} index rows")
    print(json.dumps(meta, indent=2), file=sys.stderr)


# --- read --------------------------------------------------------------------------------

class Store:
    """Random access to a built OUTDIR."""

    def __init__(self, outdir):
        self.db = sqlite3.connect(f"file:{os.path.join(outdir, 'index.sqlite')}?mode=ro", uri=True)
        self.f = open(os.path.join(outdir, "entities.jsonl.zst"), "rb")
        self.d = zstandard.ZstdDecompressor()

    def frame(self, n):
        offset, csize = self.db.execute("SELECT offset, csize FROM frame WHERE frame=?", (n,)).fetchone()
        self.f.seek(offset)
        return self.d.decompress(self.f.read(csize))

    def get_many(self, ids):
        """Yields (id, json bytes or None) for each ID, decompressing each frame once."""
        keys = [key_of(i) for i in ids]
        where = {}
        for k in set(keys):
            row = self.db.execute("SELECT frame, off, len FROM entity WHERE key=?", (k,)).fetchone()
            if row:
                where[k] = row
        found = {}
        by_frame = collections.defaultdict(list)
        for k, (fr, off, ln) in where.items():
            by_frame[fr].append((k, off, ln))
        for fr in sorted(by_frame):
            data = self.frame(fr)
            for k, off, ln in by_frame[fr]:
                found[k] = data[off:off + ln]
        for i, k in zip(ids, keys):
            yield id_of(k), found.get(k)

    def get(self, eid):
        return next(self.get_many([eid]))[1]


def get(args):
    ids = list(args.ids)
    if args.ids_file:
        with (open(0) if args.ids_file == "-" else open(args.ids_file)) as f:
            ids += [l.strip() for l in f if l.strip()]
    store = Store(args.outdir)
    missing = 0
    out = sys.stdout.buffer
    for start in range(0, len(ids), 100_000):
        for eid, data in store.get_many(ids[start:start + 100_000]):
            if data is None:
                missing += 1
                print(f"missing: {eid}", file=sys.stderr)
            else:
                out.write(data + b"\n")
    if missing:
        sys.exit(1 if missing == len(ids) else 0)


def _scan(task):
    """Keys of the entities in some frames whose P31 main value includes `qid`."""
    path, frames, qid = task
    needle = f'"id":"{qid}"'.encode()
    d = zstandard.ZstdDecompressor()
    found = []
    with open(path, "rb") as f:
        for offset, csize in frames:
            f.seek(offset)
            data = d.decompress(f.read(csize))
            if needle not in data:
                continue
            for line in data.split(b"\n"):
                if needle not in line:
                    continue
                e = json.loads(line)
                for st in e.get("claims", {}).get("P31", ()):
                    v = st["mainsnak"].get("datavalue", {}).get("value")
                    if isinstance(v, dict) and v.get("id") == qid:
                        found.append(key_of(e["id"]))
                        break
    return found


def ids(args):
    """IDs of one kind in numeric order, filtered, then the first N or a seeded sample of N.

    --instance-of scans the whole archive on --workers processes (the index does not hold
    statements): about 1.85 TB decompressed for Wikidata.
    """
    db = sqlite3.connect(f"file:{os.path.join(args.outdir, 'index.sqlite')}?mode=ro", uri=True)
    kind = "QPL".index(args.kind)
    if args.instance_of:
        qid = args.instance_of.strip().upper()
        key_of(qid)  # validates
        frames = db.execute("SELECT offset, csize FROM frame ORDER BY frame").fetchall()
        path = os.path.join(args.outdir, "entities.jsonl.zst")
        tasks = [(path, frames[i:i + 2000], qid) for i in range(0, len(frames), 2000)]
        keys, t0, last = [], time.time(), time.time()
        with cf.ProcessPoolExecutor(args.workers) as pool:
            for i, found in enumerate(pool.map(_scan, tasks)):
                keys += found
                if time.time() - last >= 60:
                    last = time.time()
                    print(f"[{(last - t0) / 60:5.1f} min] {i + 1:,}/{len(tasks):,} chunks, "
                          f"{len(keys):,} instances of {qid}", file=sys.stderr, flush=True)
        keys = sorted(k for k in keys if k & 3 == kind)
        print(f"{len(keys):,} {args.kind} IDs are instances of {qid} "
              f"({(time.time() - t0) / 60:.1f} min)", file=sys.stderr)
    else:
        # key = number * 4 + kind, so key order is numeric order within a kind.
        sql = "SELECT key FROM entity WHERE (key & 3) = ? ORDER BY key"
        if args.limit and not args.exclude_file and not args.sample:
            sql += f" LIMIT {int(args.limit)}"
        keys = [k for (k,) in db.execute(sql, (kind,))]
    if args.exclude_file:
        with (open(0) if args.exclude_file == "-" else open(args.exclude_file)) as f:
            excluded = {key_of(l) for l in f if l.strip()}
        before = len(keys)
        keys = [k for k in keys if k not in excluded]
        print(f"excluded {before - len(keys):,}", file=sys.stderr)
    if args.sample:
        if args.sample > len(keys):
            sys.exit(f"--sample {args.sample:,} but only {len(keys):,} IDs to draw from")
        keys = sorted(random.Random(args.seed).sample(keys, args.sample))
    elif args.limit:
        if len(keys) < args.limit:
            print(f"only {len(keys):,} {args.kind} IDs exist", file=sys.stderr)
        keys = keys[:args.limit]
    sys.stdout.writelines(f"{args.kind}{k >> 2}\n" for k in keys)


def info(args):
    db = sqlite3.connect(f"file:{os.path.join(args.outdir, 'index.sqlite')}?mode=ro", uri=True)
    for name, value in db.execute("SELECT name, value FROM meta"):
        print(f"{name:20} {value}")


def verify(args):
    """Fetch a random sample by ID and check each comes back as that entity."""
    store = Store(args.outdir)
    lo, hi = store.db.execute("SELECT min(key), max(key) FROM entity").fetchone()
    rng = random.Random(args.seed)
    keys = [store.db.execute("SELECT key FROM entity WHERE key >= ? ORDER BY key LIMIT 1",
                             (rng.randint(lo, hi),)).fetchone()[0] for _ in range(args.sample)]
    t = time.time()
    bad = 0
    for eid, data in store.get_many([id_of(k) for k in keys]):
        if data is None or json.loads(data).get("id") != eid:
            bad += 1
            print(f"BAD {eid}", file=sys.stderr)
    dt = time.time() - t
    print(f"{args.sample - bad}/{args.sample} entities fetched by ID and matched ({dt / args.sample * 1000:.2f} ms each)")
    # single-ID latency, cold-ish
    t = time.time()
    for k in keys[:200]:
        store.get(id_of(k))
    print(f"one-at-a-time get: {(time.time() - t) / min(200, len(keys)) * 1000:.2f} ms each")
    sys.exit(1 if bad else 0)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    b = sub.add_parser("build")
    b.add_argument("outdir")
    b.add_argument("--input", default="-",
                   help="the JSON dump: .json, .bz2, .gz or .zst, or - for stdin (default)")
    b.add_argument("--decompress-threads", type=int, default=6,
                   help="threads for lbzip2/pigz/zstd (default 6)")
    b.add_argument("--source", help="name of the original dump file, for the manifest "
                   "(default: --input's file name)")
    b.add_argument("--sha1", help="sha1 of the original dump file, for the manifest")
    b.add_argument("--block-bytes", type=int, default=1 << 20,
                   help="uncompressed bytes per zstd frame (default 1 MiB)")
    b.add_argument("--level", type=int, default=9, help="zstd level (default 9)")
    b.add_argument("--workers", type=int, default=max(1, (os.cpu_count() or 2) - 6),
                   help="compression processes (default: CPUs - 6)")
    g = sub.add_parser("get")
    g.add_argument("outdir")
    g.add_argument("ids", nargs="*")
    g.add_argument("--ids-file", help="one ID per line; - for stdin")
    d = sub.add_parser("ids")
    d.add_argument("outdir")
    d.add_argument("--kind", choices=["Q", "P", "L"], default="Q")
    d.add_argument("--limit", type=int, help="the first N in numeric order")
    d.add_argument("--instance-of", metavar="QID",
                   help="only entities whose P31 main value includes QID (scans the archive)")
    d.add_argument("--exclude-file", help="IDs to leave out, one per line; - for stdin")
    d.add_argument("--sample", type=int, help="a uniform random sample of N, in numeric order")
    d.add_argument("--seed", type=int, default=1, help="seed for --sample (default 1)")
    d.add_argument("--workers", type=int, default=os.cpu_count() or 1,
                   help="processes for --instance-of (default: all CPUs)")
    i = sub.add_parser("info")
    i.add_argument("outdir")
    v = sub.add_parser("verify")
    v.add_argument("outdir")
    v.add_argument("--sample", type=int, default=10000)
    v.add_argument("--seed", type=int, default=1)
    args = ap.parse_args()
    {"build": build, "get": get, "ids": ids, "info": info, "verify": verify}[args.cmd](args)


if __name__ == "__main__":
    main()
