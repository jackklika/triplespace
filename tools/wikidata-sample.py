#!/usr/bin/env python3
"""A random sample of Wikidata items, written in the shape of the JSON dump.

The classifier audit of ADR 0003 §10 (step 4) needs Wikidata's real statement shapes, not
all of Wikidata. This draws item IDs uniformly at random below the newest item's number,
fetches them with `wbgetentities` 50 at a time, keeps the ones that exist (deleted items
and redirects are skipped, so the sample is uniform over the items that exist today), and
adds every property, so that each statement's property and data type is present. It
writes what `latest-all.json` holds: a JSON array, one entity per line, gzip-compressed;
and a manifest beside it with the seed, the time and the counts, so a sample can be
described and, as far as Wikidata has not changed meanwhile, drawn again.

Entities the sampled items refer to are mostly not in the sample. That is expected: the
classifier works from the statements themselves (values, qualifiers, ranks, data types),
and a page rendered from the sample shows an ID where a label is missing.

    python3 tools/wikidata-sample.py --contact you@example.org --items 50000 \\
        --out wikidata-sample.json.gz

The sample is too large for git: keep it in `docs/samples/wikidata/big/`, which is ignored,
with its manifest beside it, and share it out of band.

Wikimedia's API etiquette is kept: one request at a time, a User-Agent with your contact
(required), `maxlag=5` with the server's `Retry-After` honoured, and a pause between
requests. 50,000 items take about 1,500 requests: roughly half an hour. Standard library
only. `--self-test` checks the logic offline.
"""

import argparse
import gzip
import json
import random
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone

API = 'https://www.wikidata.org/w/api.php'
BATCH = 50


class Api:
    """Wikidata's Action API, politely."""

    def __init__(self, contact, pause, endpoint=API):
        self.agent = f'TriplespaceSample/0.1 (https://github.com/scatterbase/triplespace; {contact})'
        self.pause = pause
        self.endpoint = endpoint
        self.requests = 0

    def get(self, **params):
        params = {'format': 'json', 'formatversion': '2', 'maxlag': '5', **params}
        url = self.endpoint + '?' + urllib.parse.urlencode(params)
        for attempt in range(8):
            time.sleep(self.pause)
            req = urllib.request.Request(url, headers={'User-Agent': self.agent, 'Accept-Encoding': 'gzip'})
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    body = r.read()
                    if r.headers.get('Content-Encoding') == 'gzip':
                        body = gzip.decompress(body)
                    data = json.loads(body)
            except urllib.error.HTTPError as e:
                if e.code in (429, 503):
                    wait = int(e.headers.get('Retry-After') or 5 * (attempt + 1))
                    print(f'  HTTP {e.code}; waiting {wait}s', file=sys.stderr)
                    time.sleep(wait)
                    continue
                raise
            except (urllib.error.URLError, TimeoutError) as e:
                print(f'  {e}; retrying', file=sys.stderr)
                time.sleep(5 * (attempt + 1))
                continue
            self.requests += 1
            err = data.get('error')
            if err and err.get('code') == 'maxlag':
                wait = 5 * (attempt + 1)
                print(f'  replication lag; waiting {wait}s', file=sys.stderr)
                time.sleep(wait)
                continue
            if err:
                raise RuntimeError(f"API error {err.get('code')}: {err.get('info')}")
            return data
        raise RuntimeError(f'gave up on {url}')


def newest_item(api):
    """The number of the most recently created item."""
    data = api.get(action='query', list='recentchanges', rctype='new', rcnamespace='0',
                   rclimit='1', rcprop='title')
    return int(data['query']['recentchanges'][0]['title'].lstrip('Q'))


def entities(api, ids):
    """The entities that exist among `ids` (redirects and deleted ones are left out)."""
    data = api.get(action='wbgetentities', ids='|'.join(ids), redirects='no')
    found = []
    for key, e in data.get('entities', {}).items():
        if 'missing' in e or e.get('id') != key:
            continue
        found.append(e)
    return found


def property_ids(api):
    """Every property's ID, from the Property namespace (120 on Wikidata)."""
    out, cont = [], {}
    while True:
        data = api.get(action='query', list='allpages', apnamespace='120', aplimit='max', **cont)
        out += [p['title'].split(':', 1)[1] for p in data['query']['allpages']]
        if 'continue' not in data:
            return out
        cont = {'apcontinue': data['continue']['apcontinue']}


def sample_items(api, n, newest, rng, log=print):
    """`n` distinct existing items, drawn uniformly from Q1…Q{newest}."""
    kept, tried = {}, set()
    while len(kept) < n:
        batch = []
        while len(batch) < BATCH and len(tried) < newest:
            q = rng.randint(1, newest)
            if q not in tried:
                tried.add(q)
                batch.append(f'Q{q}')
        if not batch:
            break
        for e in entities(api, batch):
            if len(kept) < n:
                kept[e['id']] = e
        log(f'  {len(kept):>7} items kept of {len(tried):>8} IDs tried ({api.requests} requests)')
    return list(kept.values()), len(tried)


class Writer:
    """`latest-all.json`'s shape: `[`, one entity per line separated by `,`, `]`."""

    def __init__(self, path):
        self.f = gzip.open(path, 'wt', encoding='utf-8') if path.endswith('.gz') else open(path, 'w', encoding='utf-8')
        self.f.write('[\n')
        self.first = True

    def write(self, entity):
        # The dump carries no page metadata beside the entity.
        e = {k: v for k, v in entity.items() if k not in ('pageid', 'ns', 'title')}
        if not self.first:
            self.f.write(',\n')
        self.f.write(json.dumps(e, ensure_ascii=False, separators=(',', ':')))
        self.first = False

    def close(self):
        self.f.write('\n]\n')
        self.f.close()


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    p.add_argument('--contact', help='your email or user page, for the User-Agent (required by Wikimedia)')
    p.add_argument('--items', type=int, default=50000, help='how many items (default 50000)')
    p.add_argument('--out', default='wikidata-sample.json.gz', help='output file (.json or .json.gz)')
    p.add_argument('--seed', type=int, help='random seed (default: from the clock; recorded in the manifest)')
    p.add_argument('--no-properties', action='store_true', help='leave the properties out')
    p.add_argument('--pause', type=float, default=1.0, help='seconds between requests (default 1)')
    p.add_argument('--self-test', action='store_true', help='check the logic offline and exit')
    a = p.parse_args(argv)
    if a.self_test:
        return self_test()
    if not a.contact:
        p.error('--contact is required: Wikimedia asks every client to say who runs it')
    seed = a.seed if a.seed is not None else int(time.time())
    rng = random.Random(seed)
    api = Api(a.contact, a.pause)
    started = datetime.now(timezone.utc).isoformat(timespec='seconds')
    newest = newest_item(api)
    print(f'newest item Q{newest}; sampling {a.items} items with seed {seed}')
    items, tried = sample_items(api, a.items, newest, rng)
    props = []
    if not a.no_properties:
        ids = property_ids(api)
        print(f'{len(ids)} properties')
        for i in range(0, len(ids), BATCH):
            props += entities(api, ids[i:i + BATCH])
    w = Writer(a.out)
    for e in props + sorted(items, key=lambda e: int(e['id'][1:])):
        w.write(e)
    w.close()
    manifest = {
        'source': API, 'started': started,
        'finished': datetime.now(timezone.utc).isoformat(timespec='seconds'),
        'seed': seed, 'newest_item': f'Q{newest}', 'ids_tried': tried,
        'items': len(items), 'properties': len(props), 'requests': api.requests,
        'method': 'uniform over existing items below the newest; every property added',
    }
    with open(a.out.removesuffix('.gz').removesuffix('.json') + '.manifest.json', 'w') as f:
        json.dump(manifest, f, indent=2)
        f.write('\n')
    print(f'wrote {len(items)} items and {len(props)} properties to {a.out}')
    return 0


def self_test():
    """The sampling, filtering and writing logic against a fake API."""
    import os
    import tempfile

    class Fake:
        requests = 0

        def get(self, **q):
            self.requests += 1
            if q.get('list') == 'recentchanges':
                return {'query': {'recentchanges': [{'title': 'Q300'}]}}
            if q.get('list') == 'allpages':
                if 'apcontinue' in q:
                    return {'query': {'allpages': [{'title': 'Property:P2'}]}}
                return {'query': {'allpages': [{'title': 'Property:P1'}]}, 'continue': {'apcontinue': 'P2'}}
            out = {}
            for i in q['ids'].split('|'):
                n = int(i[1:])
                if i.startswith('Q') and n % 3 == 0:
                    out[i] = {'id': i, 'missing': ''}          # deleted
                elif i.startswith('Q') and n % 3 == 1 and n > 200:
                    out[i] = {'id': 'Q1', 'type': 'item'}      # a redirect would not match
                else:
                    out[i] = {'id': i, 'type': 'item' if i[0] == 'Q' else 'property', 'pageid': n}
            return {'entities': out}

    api = Fake()
    assert newest_item(api) == 300
    items, tried = sample_items(api, 120, 300, random.Random(1), log=lambda *_: None)
    ids = [e['id'] for e in items]
    assert len(ids) == 120 == len(set(ids)), 'distinct and as many as asked'
    assert all(int(i[1:]) % 3 != 0 for i in ids), 'deleted items skipped'
    assert all(not (int(i[1:]) % 3 == 1 and int(i[1:]) > 200) for i in ids), 'redirects skipped'
    assert tried <= 300
    # Asking for more than exist stops when the ID space is used up.
    all_items, tried = sample_items(api, 10000, 300, random.Random(2), log=lambda *_: None)
    assert tried == 300 and len(all_items) < 300
    assert property_ids(api) == ['P1', 'P2']
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, 's.json.gz')
        w = Writer(path)
        for e in items[:3]:
            w.write(e)
        w.close()
        with gzip.open(path, 'rt') as f:
            text = f.read()
        lines = text.split('\n')
        assert lines[0] == '[' and lines[-2] == ']', 'the dump framing'
        assert all(line.endswith(',') for line in lines[1:3]) and not lines[3].endswith(',')
        parsed = json.loads(text)
        assert len(parsed) == 3 and 'pageid' not in parsed[0], 'one entity per line, no page metadata'
    print('self-test passed')
    return 0


if __name__ == '__main__':
    sys.exit(main())
