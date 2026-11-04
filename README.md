# Triplespace

Wikibase reimagined as an append-only log, in Rust. Bulk ingest is a first-class path, and
foreign entities (Wikidata's `WDQ42`, OpenAlex's `OAW123`, a domain name as `domain:example.org`)
sit alongside local ones without being reified.

The design is described in [`docs/architecture/`](docs/architecture/), twenty-four chapters by
subject, and decided in [`docs/decisions/`](docs/decisions/), the architecture decision records,
which hold why and when and point into the chapters for the current text (0050 §14); the names
and codes the code embeds live in [`docs/registry/`](docs/registry/); the compatibility contracts
with Wikibase and MediaWiki are in [`docs/api/`](docs/api/). Start with
[chapter 00](docs/architecture/00-overview.md), then [22](docs/architecture/22-crates-and-stack.md)
for the crate map and build order.

## Layout

| Path | Holds |
|---|---|
| `crates/` | One Cargo workspace. `scatter-*` crates are shared with Scatterbase and know nothing about Triplespace; `triplespace-*` crates are the product (0005 §1). |
| `tools/` | Scripts that are not part of the build (`tools/README.md`): `wikidata-sample.py` samples a Wikidata dump for the classifier audit (0003 §10); `wikidata_zindex.py` makes a Wikidata JSON dump an indexed zstd archive, to fetch entities by ID and cut slices; `dump_slice.py` cuts a smaller XML dump; `api_check.py`, `pwb_check.py` and `wdi_check.py` exercise a running server through WikibaseIntegrator, Pywikibot and WikidataIntegrator, and `client_compat.sh` runs all three from scratch. |
| `xtask/` | Repository tasks: `cargo xtask deps` checks the workspace dependency graph against the crate table of 0005 §2 (chapter 22 §2.1); `cargo xtask wasm` builds the crates 0005 rule 7 requires to build for `wasm32-unknown-unknown`. |
| `docs/` | Architecture chapters, ADRs, registry, API contracts and test vectors. CC0-1.0 (`docs/LICENSE`), so other implementations can embed them. |
| `ui/` | The site's front end (0034): Codex, its design tokens and the default theme's fonts, built by Vite into `ui/dist`, which `triplespace-ui` embeds. |
| `i18n/` | The site's interface messages, banana JSON for translatewiki.net (0034 §9). |

## Building

Stable Rust as pinned in `rust-toolchain.toml`, edition 2024. A small instance needs Postgres and
nothing else (0033 §1); Valkey, OpenSearch and QLever are optional services a larger one adds.

```
cargo build --workspace
cargo nextest run --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check
cargo xtask deps
```

The site's styles and script come from `ui/` (Node 22):

```
cd ui && npm ci && npm run build   # writes ui/dist; rebuild the Rust afterwards to embed it
npm run lint && npm test           # ESLint, Stylelint, banana-checker; Vitest
npm run e2e                        # Playwright and axe, JavaScript off, two web replicas
```

`npm run e2e` loads the server's test fixtures into a fresh database (it needs
`TRIPLESPACE_TEST_DATABASE_URL` and the three binaries built), starts the API with its site
off, two `triplespace-web` replicas and an edge that alternates them, and runs the suite in
`ui/e2e/` against it.

A Rust build without `ui/dist` still compiles and serves pages, unstyled.

The Wikibase client libraries are checked the same way (`docs/clients.md`):

```
python3 -m venv .venv && .venv/bin/pip install -r tools/requirements-compat.txt
PYTHON=.venv/bin/python tools/client_compat.sh   # WikibaseIntegrator, Pywikibot, WikidataIntegrator
```

It adopts the Librarybase sample fixture into a fresh database, starts the API with its site
off, issues a bot key, and runs `tools/{api,pwb,wdi}_check.py` against it; CI runs it on every
push.

The tests that need a database read `TRIPLESPACE_TEST_DATABASE_URL` (a Postgres URL whose role
may create databases) and are vacuous without it. `docs/check.sh` runs every documentation checker
(`--write` regenerates the ADR index, the chapter map and the settings tables first), and
`tools/adr.py` looks up ADR and chapter sections. In a fresh Linux sandbox, `eval
"$(tools/dev_env.sh)"` starts Postgres and sets the test database URL.

## Setting up an instance

Three binaries come out of the workspace: `triplespace`, the command line that creates and loads
an instance; `triplespace-server`, which serves the API and, unless `--ui off`, the site; and
`triplespace-web`, the stateless web tier that serves the site from the API (below). The first
two take the database as `--database` or `TRIPLESPACE_DATABASE_URL`.

Every flag that names a `TRIPLESPACE_*` variable can instead be set in a `.env` file in the
working directory, which all three binaries read when they start (23 §5.1). A flag wins over
the environment, and the environment over the file. `TRIPLESPACE_ENV_FILE` names a different
file, which must then exist. `.env` is gitignored.

```sh
# .env
TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace
TRIPLESPACE_LISTEN=127.0.0.1:8080
TRIPLESPACE_AUTH_BASE=https://auth.example.org
```

```sh
createdb triplespace
export TRIPLESPACE_DATABASE_URL=postgres://triplespace@localhost/triplespace

# The instance and its primary tenant: the schema, the instance signing key (written to
# triplespace.key, mode 0600), the instance and tenant partitions, the owner account.
echo 'a long passphrase' > owner.pw
triplespace instance create \
    --farm-slug scatter \
    --tenant example --base https://example.wiki \
    --provider internetdomains \
    --owner 1 --owner-name Alice --owner-password-file owner.pw
```

`instance create` is where the shape of the instance is fixed (0015 §3, 0018, 0046): the farm
slug, the instance's own name, which differs from every tenant slug and never changes; the farm
base (`--farm-base`, the primary tenant's base by default); the primary tenant's slug and base
URL; the providers that get mirror partitions; and the owner, who holds every permission (0016
§3). It refuses to run against a database that already holds an instance. A slug may not be a
name the registry has (`wikidatawiki`, `internetdomains`), except for a wiki the registry
already knows, adopted under its own name (`--tenant librarybase --adopt
https://librarybase.org/`; see below).

Each tenant's actors are written `{issuer code}:{n}`, where the issuer code is derived from the
tenant's founding record and printed by `instance create` (0079): a slug is a name on this
instance, and the code is what travels with the tenant's history. `--tenancy` takes the tenancy
preset (0028 §1); `isolated`, the default, is the only one this version honours.
Everything it writes is a log record, so `triplespace status` shows the partitions it made and
`triplespace rebuild` reproduces the serving tables from them at any time.

Bots and tools log in as **subsidiary accounts** of a person (0024), never as the person:

```sh
triplespace subsidiary create --tenant example --name "Alice bot" --operator Alice --group bot
triplespace subsidiary key --tenant example --name "Alice bot" --label laptop \
    --grant editentity --grant highvolume
```

The second command prints the key once (`--json` prints it as one object, for scripts):
`lgname=Alice bot@laptop` with the secret is a MediaWiki bot password, and `Authorization:
Bearer {key ID}.{secret}` is the same credential without a session. WikibaseIntegrator,
Pywikibot and WikidataIntegrator log in, read and edit with it unchanged; `docs/clients.md` has
what each needs to know (Pywikibot's family file, WikidataIntegrator's SPARQL backoff and
`setuptools<80`) and CI runs all three against every push. `subsidiary keys` lists a
subsidiary's keys and `subsidiary revoke --key-id` ends one together with every session it
opened. `triplespace password set` sets or replaces a person's password.

Then serve it:

```sh
triplespace-server --listen 0.0.0.0:8080 --key-file triplespace.key
```

The server answers for the hosts of its registered tenant bases, the farm base, which serves the
primary tenant, and the auth base if there is one (0056 §10; 0089 §1); anything else is 421, so
it expects to sit behind a proxy that terminates TLS for those hosts and forwards `Host`. For a
laptop, `--mode development --dev-tenant example` serves any host as that
tenant over plain HTTP and marks the API as insecure in `meta=siteinfo&siprop=triplespace`.
`/healthz` answers on the public listener and on `--admin-listen` if given.

**Proxies.** The server reads `X-Forwarded-For` from the right and believes one more entry for
each hop it trusts (0057 §10), so the client address behind your proxies is the one blocks and
rate limits see. A hop is trusted by its address, `--trusted-proxy 10.0.0.2,10.0.1.0/24`
(addresses and CIDR ranges), which suits a network only the instance's services share. Where
other workloads share the network, give each proxy a forwarder key instead:

```sh
triplespace instance forwarder create --label edge     # prints the key once
```

The proxy appends it to `Triplespace-Forwarder` on every request it forwards (Caddy:
`header_up Triplespace-Forwarder <key>`). `forwarder list` and `forwarder revoke --key-id` manage
the keys; a revoked key stops being believed within 30 seconds. `X-Forwarded-Host` and
`X-Forwarded-Proto` are believed only from a trusted peer.

**The site.** `triplespace-server` serves the site itself by default (`--ui embedded`): whatever
path the API does not route, the site answers, calling the API in process. To scale the site
apart from the API, or run several replicas of it, start the server with `--ui off` and run the
web tier beside it:

```sh
triplespace-web --api http://api.svc:8080 --listen 0.0.0.0:8081 --admin-listen 127.0.0.1:9091
triplespace-web routes --format caddy    # or nginx, haproxy: which paths go where
```

The site serves entity pages (`/wiki/Item:Q6`, `/wiki/Property:P12`, `/wiki/Item:WDQ65`,
`/wiki/Domain:wikipedia.org`) with their Statements, Identifiers, Sitelinks and Labels tabs
(`?tab=identifiers` and so on), in the language of `uselang` where given. With
`action=render` a page answers with one region instead of the whole page: `terms`,
`statements/{P}`, `identifiers` or `sitelinks` (0057 §8). Other titles show the frame and a
notice that they are not served yet.

The web tier keeps no state, holds no database credentials and reaches the instance only through
its public API (0057). Plain `http` is refused unless the API's host resolves only to internal
addresses; otherwise give it `https`. The edge sends the API's paths to the API and everything
else to the web tier; `routes` prints that split for your proxy, from the same table the site
uses. Add the web tier to `--trusted-proxy`, or give it a forwarder key with
`--forwarder-key-file`, so the API sees the browser's address rather than the web tier's.
`/readyz` on the admin listener answers once the API does.

A client then uses the usual Wikibase surface at `/w/api.php` (or `/api.php`): `meta=siteinfo`,
`meta=tokens`, `action=login` with the bot password, `wbgetentities`, `wbsearchentities`, and the
edit modules from `wbeditentity` to `wbremovereferences`; `/wiki/Special:EntityData/Q6.json` and
`/entity/Q6` serve the entity documents. The owner logs in with `action=clientlogin`. Everything
not yet served answers with a MediaWiki-shaped error rather than silence, so a client's failure
names the gap.

## More tenants

An instance hosts any number of wikis (0018). Each has its own base URL, partitions, numbering,
accounts and configuration; an account on one is nothing on another. A second tenant needs the
instance's **auth host** (0089 §1), a host of its own that is neither a tenant's nor the farm's:
the server and `tenant create` read it as `--auth-base` or `TRIPLESPACE_AUTH_BASE`.

```sh
export TRIPLESPACE_AUTH_BASE=https://auth.example.org
triplespace tenant create --tenant other --base https://other.example \
    --name "Other wiki" --owner-name Bob --owner-password-file bob.pw
triplespace tenant list
triplespace-server --listen 0.0.0.0:8080 --key-file triplespace.key   # with the same variable
```

`tenant create` needs the instance key and is done by an `owner` of the primary tenant
(`--operator`, the lowest-numbered owner by default), who attests the new tenant's registry
record (0046 §4). At the farm base, `GET /w/rest.php/triplespace/v0/tenancy` lists the
tenants with their issuer codes, the primary tenant and the tenancy policy. The auth host's
login pages are not built yet: until they are, the auth base answers 404, and each tenant's
accounts log in on its own host with a password of their own.

## Loading a Wikibase

There are two ways a Wikibase's entities enter an instance, and they are not interchangeable.

**Adopting** (0035) makes an existing wiki *become* a tenant: its items and properties keep
their `Q` and `P` numbers, page IDs and user IDs, its accounts are recreated under their own
numbers, and every ID counter is floored above what the wiki used, so nothing is ever reissued.
It is a one-way door: the source must be frozen first, because after adoption the tenant mints
the next IDs. The input is the wiki's **MediaWiki XML dump**, not its JSON dump, because only
the XML carries page IDs, revision IDs, timestamps and contributors together:

```sh
# On the frozen source wiki:
php maintenance/dumpBackup.php --full --include-files=no | gzip > librarybase-20261003.xml.gz
mysql -e 'SELECT * FROM wb_id_counters'          # the counters, not the highest IDs in the dump

# Here. --adopt tells the tenant where it came from; --owner is the owner's user ID *on the source*.
triplespace instance create --farm-slug scatter \
    --tenant librarybase --base https://librarybase.org \
    --provider internetdomains --adopt https://librarybase.org/ \
    --owner 7 --owner-name Alice --owner-password-file owner.pw

triplespace adopt --tenant librarybase --dump librarybase-20261003.xml.gz \
    --source https://librarybase.org/ --version librarybase-20261003 --frozen \
    --counters item=350000,property=1200 \
    --entity-source wikidata=WD           # only if the source used federated properties
```

`adopt` reads the dump twice: a survey for the floors and the accounts, then the job, which
writes every revision of every entity page as an `adopt` record under the source's own revision
ID, timestamp, author and edit summary, so `lastrevid` and every `oldid=` match the source (0035
A12). The dump must carry history; `--current-only` adopts a current-state export instead, one
record per entity under a fresh revision ID. Re-running it over the same dump is a no-op; a
changed dump is a reject per differing revision, never an overwrite. Pages the model cannot
parse are skipped and counted; the job's
first hundred rejects are on its `job/finish` record and in `view.job_reject`. Pass
`--counters` from `wb_id_counters` whenever you can: without it the floors come from the highest
IDs in the dump, and a number the source allocated and deleted could be reused (0035 §4).

**Syncing** (0002 §8.4) mirrors another Wikibase as a *provider*: its entities appear under the
provider's prefix (`XDQ20`) or, for a key-mapped provider, under the key they identify
(`domain:wikipedia.org`, 0009 §9), beside the tenant's own entities, and local assertions about
them live in the tenant's graph. The input is the provider's entity JSON dump (`dumpJson.php`)
or its XML dump; the adapter decides what each entity becomes:

```sh
triplespace sync --provider internetdomains --dump internetdomains-20261003.json.gz --version 20261003
```

For internetdomains.wiki the adapter maps every item with exactly one valid `P1` (domain name)
onto `domain:{key}`, rewrites values that point at mapped items into domain values, drops the
identity property from the mapped state, and reports as rejects the items whose `P1` is not a
valid domain name or is shared by two items (those stay `XDQ` items, their `P1` kept as data).
`--snapshot` declares the dump complete, so entities it no longer carries are tombstoned
afterwards, up to `--threshold`. Re-running a sync writes only what changed upstream.

After either, `triplespace status` shows every partition's head and whether each projection is
current, and the serving tables are there to inspect: `view.entity`, `view.term`,
`view.identifier`, `view.actor`, `view.job`, `view.job_reject`, `view.keyed_map`.

## Licence

Triplespace is GPL-3.0-or-later (`LICENSE`). The shared `scatter-*` crates are also available
under a commercial licence from Scatter LLC. `docs/` is CC0-1.0. See chapter 22 §3.3 and §3.4.
