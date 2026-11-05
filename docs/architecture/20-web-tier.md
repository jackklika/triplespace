# 20. Web tier

The web tier is the process that renders the site's pages: either `triplespace-server` itself, with the site embedded, or the separate `triplespace-web` binary. This chapter covers its principles, the two ways to run it, the routing table the edge proxy splits requests by, how it talks to the API, forms that work without JavaScript, fragments fetched with `action=render`, deploying it independently of the API, and the trust the API places in forwarded headers, which is where the web tier sits in the deployment boundary of the security model. It assumes the API of [18](18-api.md), the pages and components of [19](19-site-ui.md) and the security model of [09](09-security-and-moderation.md). The web tier's response cache and a page's own cacheability are in [03](03-storage-caches-and-search.md) §10.2 and §10.3; its health routes and crates are in [22](22-crates-and-stack.md); the OAuth pages it renders are in [07](07-actors-and-accounts.md); purging is in [11](11-rendering-templates-and-modules.md); the `web.*` settings appear in the settings catalogue of [23](23-configuration-and-registry.md).

## 1. Principles, and two ways to run the site

*Sources: [0057](../decisions/0057-web-tier.md) §1, §2.*

### 1.1 Principles

*Sources: [0057](../decisions/0057-web-tier.md) §1.*

1. **The web tier holds no state.** It has no database connection, no disk it writes and no instance key. Its only secrets are an optional forwarder key (§5.2) and the password of the Valkey its optional response cache may use ([03](03-storage-caches-and-search.md) §10.2), which holds public responses only. Any replica can serve any request; a replica can be stopped at any moment and loses only its requests in flight.
2. **One origin.** The site and the API share each tenant host. The edge proxy splits requests by path (§2.1); the browser sees one origin, its cookies work for both, and the Vue components of [0034](../decisions/0034-frontend-stack.md) §4 call `api.php` and `rest.php` directly with no CORS.
3. **The renderer is an API client.** It reaches the instance only through the public HTTP API, with the viewer's own credentials, so a page carries exactly the redaction the API applied ([0012](../decisions/0012-api-requirements.md) §1.4–1.5). It never links an API crate's handlers and acts as no account of its own.
4. **One code path, two transports.** The same `triplespace-ui` router serves pages from `triplespace-web`, over HTTP, and from `triplespace-server`, over an in-process call into the API's own router (§1.2). Both send the same requests through the same middleware, with two exceptions that keep a page from paying for its own sub-requests: the in-process transport resolves the viewer's session once per page and hands the resolved principal to every sub-request, and its sub-requests are not counted against the `read` rate class (§2.2).

### 1.2 Two ways to run the site

*Sources: [0057](../decisions/0057-web-tier.md) §2; [0097](../decisions/0097-versioning-and-skins.md) §8.*

- **`triplespace-server` embeds the site.** `server.ui` is `embedded` (the default) or `off`. Embedded, the server serves the web paths of §2.1 itself through `triplespace-ui`, calling its own API router as a `tower::Service` with no network hop. The embedded call carries the original request's headers and connection address unchanged, so the API sees the real client and the in-process path adds no `X-Forwarded-*`. A small instance stays one binary and Postgres ([0033](../decisions/0033-backend-stack.md) §1). This is runtime configuration, not a Cargo feature, so the binary's behaviour is not chosen at build time ([0005](../decisions/0005-crate-organization.md) §3 rule 6).
- **`triplespace-web` serves the site on its own.** A second binary from the same workspace, configured by the settings below. An HA deployment runs `triplespace-server` with `server.ui = off` behind the edge and `triplespace-web` beside it, each pool sized on its own.
- **Assets.** `ui/dist` is embedded in both binaries ([0034](../decisions/0034-frontend-stack.md) §8), so either serves the assets its pages name.
- **Node stays a build-time dependency only.**

`triplespace-web`'s settings:

| Setting | Meaning |
|---|---|
| `web.api` | The API's base URL on the internal network, one address or a load balancer's; `https` unless the address is internal (§2.2) |
| `web.listen`, `web.admin_listen` | The public listener; health and metrics, on a separate listener ([0056](../decisions/0056-security-model.md) §10 line 9) |
| `web.mode` | `production` or `development`, as `server.mode` (§5.4) |
| `web.forwarder_key_file` | Optional: the web tier's forwarder key (§5.2) |
| `web.cache` | `memory`, `valkey`, or `off` ([03](03-storage-caches-and-search.md) §10.2); `web.cache.valkey_url` and `web.cache.valkey_password_file` for the second; `web.cache.max_ttl` |
| `web.upstream_host` | `forward` (the default) or, in `development` mode only, `fixed` (§2.2) |
| `web.assets_from` | Optional: a base URL that serves the hashed assets instead of the binary (§4) |
| `web.skins` | Optional: a directory of installed skins beside the shipped ones, as `server.skins` ([19](19-site-ui.md) §5.10; [23](23-configuration-and-registry.md) §3.6) |

Secrets come from files, never the command line ([0033](../decisions/0033-backend-stack.md) §12).

## 2. The routing table, and talking to the API

*Sources: [0057](../decisions/0057-web-tier.md) §3, §4.*

### 2.1 The routing table

*Sources: [0057](../decisions/0057-web-tier.md) §3; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1.*

The edge proxy routes by path prefix, longest prefix first. The authoritative list is `triplespace_ui::routes`; `triplespace-web routes --format nginx|haproxy|caddy` prints it as proxy configuration, and the embedded mode dispatches by the same list.

| Prefix | Served by |
|---|---|
| `/w/api.php`, `/api.php` | API |
| `/w/rest.php/`, `/rest.php/` | API |
| `/wiki/Special:EntityData/`, `/entity/` | API |
| `/.well-known/`, `/oauth/`, `/dumps/` | API |
| `/w/styles/`: the site styles and TemplateStyles sheets ([11](11-rendering-templates-and-modules.md) §6.3–6.4), from when the first is served | API |
| The media routes of [0039](../decisions/0039-files-and-media.md) §7, where the media base shares the host | API |
| Everything else: `/`, `/wiki/`, `/w/index.php`, `/index.php`, `/ui/assets/`, `/ui/skins/` | Web tier |

The web tier is the default route, so an unknown path gets a 404 page in the site's frame. **At the auth base** ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1; [07](07-actors-and-accounts.md) §2.8) the same table routes, but the API answers only its authentication modules at `/w/api.php` and the `/auth/` routes under `/w/rest.php/triplespace/v0/`, the web tier only `Special:UserLogin`, `Special:CreateAccount`, `Special:Account` and `Special:UserLogout`, and `/ui/assets/`; every other path is a 404 in a plain frame, since the auth host holds no wiki. Special pages ([0047](../decisions/0047-special-pages.md), [21](21-special-pages.md)), the OAuth pages ([0057](../decisions/0057-web-tier.md) §13, in [07](07-actors-and-accounts.md)) and a private tenant's landing page ([0056](../decisions/0056-security-model.md) §13) are pages like any other, rendered by `triplespace-ui`.

### 2.2 Talking to the API

*Sources: [0057](../decisions/0057-web-tier.md) §4.*

- **Forwarded to the API:** `Cookie`, `Authorization`, `Accept-Language`, `User-Agent` and `X-Request-Id`; the `Host` the browser sent, as `X-Forwarded-Host`, with `X-Forwarded-Proto`; `X-Forwarded-For` as received, with the address of the peer that connected to the web tier appended; and `Triplespace-Forwarder` as received, with the web tier's own key appended when it has one. The web tier judges none of these: only the API decides what to believe (§5.1). Hop-by-hop headers are not forwarded.
- **Relayed back:** `Set-Cookie` from the API on form posts (§3.1), verbatim. The cookie names the tenant host, which is the web tier's host too.
- **Never logged:** cookies, bearer credentials, tokens, forwarder keys.
- **TLS unless the API is internal.** `web.api` may be plain `http` only when its host is a loopback, link-local or private address (RFC 1918, RFC 4193), or a name every resolved address of which is one. The web tier checks at startup and whenever it re-resolves the name, and refuses plain HTTP to anything else. A name such as `api.svc` or `api.internal` passes because of where it resolves, not because of how it is spelled.
- **Transport.** Pooled HTTP/2 to `web.api`, with timeouts; idempotent `GET`s may be retried once, writes never.
- **The in-process transport.** With `server.ui = embedded`, a page's sub-requests enter the API router as `tower` calls. The session is resolved once, when the page request arrives, and the principal is passed to each sub-request rather than looked up again; the sub-requests are exempt from the `read` rate counter ([07](07-actors-and-accounts.md) §6), which counts the page itself, so that a page of four API calls ([19](19-site-ui.md) §5.1) costs one count and a reading room behind one address is not limited by a budget written for API clients. Visibility, ACLs and redaction run on every sub-request as they would over HTTP. `triplespace-web` has no such exemption: its requests arrive over the network like any client's, which is why the page budget is met by the routes of [18](18-api.md) §3.2 and not by the transport.
- **Development.** In `development` mode only, `web.upstream_host = fixed` sends `web.api`'s own host instead of the browser's, so the web tier can be pointed at www.wikidata.org or the MediaWiki 1.43 reference install, the prototyping path of [0034](../decisions/0034-frontend-stack.md) §13; the features those lack are hidden by capability discovery ([0012](../decisions/0012-api-requirements.md) §1.4).

## 3. Forms without JavaScript, and fragments

*Sources: [0057](../decisions/0057-web-tier.md) §7, §8; [0034](../decisions/0034-frontend-stack.md) §5.*

### 3.1 Forms without JavaScript

*Sources: [0057](../decisions/0057-web-tier.md) §7; [0087](../decisions/0087-account-registration-and-challenges.md) §5.*

Reading, logging in and out, creating an account, the account pages, page source editing and search work without JavaScript ([0034](../decisions/0034-frontend-stack.md) §4), except an act that a challenge guards ([07](07-actors-and-accounts.md) §2.6; [0087](../decisions/0087-account-registration-and-challenges.md) §5): creating an account or filing a request with the `password` issuer, an anonymous save that would create a temporary account, and a login after repeated failures. Such an act is refused with an explanation, never queued: the page says what the wiki asks of the browser and why, and what the person can do instead ([19](19-site-ui.md) §2.3).

- **A form** carries the token the web tier fetched with the viewer's cookie (`logintoken`, `csrftoken`).
- **Its submission** goes to the web tier, which calls the matching API module and answers `303` to the result, or renders the form again with the API's error.
- **Sign-in through an issuer** (`POST /auth/{issuer}/start` and `GET /auth/{issuer}/callback`, [0012](../decisions/0012-api-requirements.md) §5; [0088](../decisions/0088-external-identity-providers.md) §5) stays on the API: the browser posts the issuer's button straight to the API, the provider returns to the API's callback, and the API ends with a redirect to the `returnto` the web tier passed, so the provider's code never reaches the web tier. A page carrying an issuer's button widens the policy's `form-action 'self'` to that issuer's authorization origin, which `siprop=triplespace` reports, because a browser applies `form-action` to the redirect that follows a form. `action=clientlogin`, `action=createaccount` and the auth routes accept only a `returnto` on the request's own origin, and ignore any other.

### 3.2 Fragments: `action=render` on the page's own URL

*Sources: [0057](../decisions/0057-web-tier.md) §8; [0034](../decisions/0034-frontend-stack.md) §5; [0097](../decisions/0097-versioning-and-skins.md) §15.*

**There are no fragment routes.** MediaWiki's `index.php?title={title}&action=render` returns a page's content without the skin. Triplespace keeps that meaning, at `/w/index.php`, `/index.php` and `/wiki/{title}?action=render`, which the routing table already sends to the web tier, and adds one parameter, `region`, for part of the content. After a component saves, it fetches the server's rendering of the changed region from the page's own URL and swaps it in, such as `index.php?title=Item:Q42&action=render&region=statements/P1082` or `region=terms`. The web tier serves it like the page, so there are no fragment routes in the API.

| Request | Returns |
|---|---|
| `action=render` on a document page or thread | The rendered body, as MediaWiki returns it |
| `action=render` on an entity | The content of the Statements tab, without the frame |
| `region=terms` | The term box |
| `region=statements/{P}` | One statement group in its layout ([0003](../decisions/0003-statement-ui.md) §3) |
| `region=identifiers`, `region=sitelinks` | Those tabs' content |
| `region=post/{id}` | One post of a thread |
| `region=row/{id}` | One row of a `Table` page's grid ([0045](../decisions/0045-table-content-model.md) §11) |

- **A region is part of its page.** Full pages are assembled from the region functions, and a region exists for `action=render` only once the page draws it, so the two cannot drift.
- **The region names are a contract; their markup is not.** The names in the table are documented with the site's URLs and kept until a log entry in [0057](../decisions/0057-web-tier.md) retires one. The HTML inside a region changes with the site, as the HTML of MediaWiki's `action=render` does.
- **Errors.** An unknown region is `400` with a MediaWiki-shaped error body. A region with no content yet (a property the entity does not use) is `200` with the empty group's markup, so a component can add the first value. A missing or confidential target answers exactly as its page would ([0056](../decisions/0056-security-model.md) §5).
- **Caching** follows [03](03-storage-caches-and-search.md) §10.3: a region carries the same `ETag`, `Cache-Tag` and redaction as its page ([0014](../decisions/0014-caches-and-search.md) §3), so purging the page purges its regions. The tags are two-tiered: a composed page carries exact tags for its subjects (the entity or page, and a talk page's threads) and for every page in its render manifest, and nothing else; the entities whose labels it drew are not tagged, since a large item draws labels from thousands of them and tag headers are capped by every CDN and proxy. Label staleness is bounded by the page's `s-maxage` instead, as MediaWiki bounds its label cache, and the tag list has a stated cap.
- **Links are full URLs,** as MediaWiki's `action=render` makes them.
- **Build skew.** A component sends `X-Triplespace-UI-Build`, the build ID of the page it lives in. A web tier on another build answers `409` with no body, and the component reloads the page rather than swap in markup its loaded styles and scripts do not match; the build ID includes the skin that drew the page and its version, so a change of skin or a skin upgrade is build skew too ([0097](../decisions/0097-versioning-and-skins.md) §15); the write has already gone through the API, so the reload shows it. A request without the header, as from any third party, is always served.

## 4. Independent deployment

*Sources: [0057](../decisions/0057-web-tier.md) §9; [0097](../decisions/0097-versioning-and-skins.md) §5.*

The web tier and the API are deployed separately, so each must tolerate the other's version.

- **Assets** have hashed names and are served `immutable`. A web release also carries the previous release's assets, or `web.assets_from` names a base that only ever accumulates, so a page rendered by the old release keeps loading during a rolling deploy.
- **The API's version.** `meta=siteinfo&siprop=triplespace` reports `api_version`, beside the capability list it already carries ([18](18-api.md)): a semantic version `MAJOR.MINOR.PATCH` of the REST routes and the `triplespace` siteinfo property, the integer reported before being its minor at major 0 ([0097](../decisions/0097-versioning-and-skins.md) §5). The web tier hides what the API lacks ([0012](../decisions/0012-api-requirements.md) §1.4) and reports itself not ready unless the API's major is the one it was built against and its minor at least the one it was built against. The REST base path carries the major, `triplespace/v0`, so a new major is served at a new base path beside the old one while clients move.
- **The editor's renderer.** `action=parse` reports its renderer version ([0012](../decisions/0012-api-requirements.md) §4); where it differs from the build of `scatter-wasm` in the page, the editor previews through `action=parse` instead.

## 5. Proxy trust and the deployment boundary

*Sources: [0057](../decisions/0057-web-tier.md) §10; [0056](../decisions/0056-security-model.md) §10.*

### 5.1 Only the API judges forwarded headers

*Sources: [0057](../decisions/0057-web-tier.md) §10; [0056](../decisions/0056-security-model.md) §10.*

The edge and the web tier append what they see (§2.2). The API reads `X-Forwarded-For` from the right, believing one more entry for each hop it trusts and stopping at the first it does not; the client is the last entry believed, or the peer itself when the peer is not trusted. `X-Forwarded-Host` and `X-Forwarded-Proto` are believed only from a trusted peer.

**A hop is trusted by its address, or by a forwarder key.** Read from the right, a hop is trusted if its address (the peer's, or the entry the next hop appended) is in `server.trusted_proxies`, a list of addresses and CIDR ranges, as MediaWiki's `$wgCdnServers` and Caddy's `trusted_proxies` are. **Address trust is the default** and suits a deployment where only the edge, the web tier and the API share the network. Otherwise the hop is trusted if the next unused entry from the right of `Triplespace-Forwarder` is a valid **forwarder key**: the option for networks shared with workloads that are not trusted, such as a Kubernetes cluster where the web tier's addresses are the whole pod range and any pod there could otherwise forge a client address. The two mix freely.

```
client C → edge E → web W → API
X-Forwarded-For:       F, C, E      (F is forged by the client)
Triplespace-Forwarder: kE, kW
The API: W is trusted by kW, so believe E; E is trusted by kE, so believe C; no more → the client is C
```

The right-to-left reading is the API's defence when [0056](../decisions/0056-security-model.md) §10 line 6 does not hold: a conforming edge strips inbound `X-Forwarded-*` from clients before setting its own and produces `C, E`; the example shows defence in depth.

### 5.2 Forwarder keys

*Sources: [0057](../decisions/0057-web-tier.md) §10.*

A forwarder key means only "believe what this hop says about the client". It acts as no account, reads nothing and grants nothing. `triplespace instance forwarder create --label {label}` prints a key once, as `{key id}.{secret}`. Its hash is kept in `private` by `triplespace-accounts`; a `forwarder` record in the instance `config` partition holds its ID, label and dates, never the secret or its hash, so issuing and revoking keys is in the log ([23](23-configuration-and-registry.md)). `forwarder revoke` retires a key with a null record, and two keys may be valid at once for rotation. The edge adds its key as a fixed request header (Caddy's `header_up`); the web tier reads its own from `web.forwarder_key_file`. Over plain HTTP on an internal network (§2.2) a key is as exposed as the session cookies on the same link, which are worth more.

### 5.3 The deployment boundary

*Sources: [0056](../decisions/0056-security-model.md) §10; [0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1, §12.*

The evaluator runs in `triplespace-server`. Everything behind it holds restricted data in the clear: Postgres `log` and `view`, OpenSearch ([0056](../decisions/0056-security-model.md) §8), Valkey (0056 §7), the blob store, the render and job queues, the backups, the access logs. **The model holds only inside a deployment where the Triplespace services are the only readers of those stores.** That is a property of how the instance is set up, and Triplespace checks what it can and says what it cannot.

**Requirements.** Each is a line in `triplespace-cli instance check`, which reports `pass`, `fail` or `attest`: the last for a requirement the server cannot verify from inside, which the operator confirms with `instance check --attest {name}` and which is recorded in the instance `config` with the operator and the date.

| # | Requirement | Why | Check |
|---|---|---|---|
| 1 | The database roles are those of [0013](../decisions/0013-postgres-storage.md) §4: the public role has no privilege on `private`, no role Triplespace uses is a superuser, and no other application connects to `view` or `log` | Every row in `view` is readable to a database client; the evaluator is in the server, not the database | `pass`/`fail` on grants, read from `pg_catalog`; `attest` that nothing else holds a connection |
| 2 | OpenSearch is reachable only from the servers and requires credentials; no dashboard or other client is configured against it | The index holds restricted text ([0056](../decisions/0056-security-model.md) §8) | `fail` if the configured endpoint is a public address or accepts an unauthenticated request; `attest` for other clients |
| 3 | Valkey requires `AUTH` or runs on a loopback or private address, and persists, if at all, to a volume with the same protection as the database | Sessions and restricted forms live there ([0056](../decisions/0056-security-model.md) §7) | `fail` on a public address or an unauthenticated connection |
| 4 | The blob store is private. File bytes and thumbnails are served by the binary after `read` is evaluated, and a version only some may read only from a signed URL valid for minutes, exactly as [0039](../decisions/0039-files-and-media.md) §7 has it; the media origin is separate from the wiki origin | A public bucket is a public dump of every tenant's files | `fail` if an object URL in the store answers 200 without a signature; `attest` for a media base that is same-origin with the wiki, which 0039 §7 recommends against |
| 5 | The server rejects a request whose `Host` is not a registered tenant base, the farm base or the auth base ([0089](../decisions/0089-the-auth-host-and-instance-logins.md) §1), with 421, and believes `X-Forwarded-For`, `X-Forwarded-Host` and `X-Forwarded-Proto` only from hops it trusts: by address in `server.trusted_proxies` (the default), or by a forwarder key, read from the right (§5.1) | The host selects the tenant ([0018](../decisions/0018-tenants.md) §11); IP blocks, rate limits and filter IP rows depend on the client address ([0016](../decisions/0016-permissions-and-access-control.md) §3, [0024](../decisions/0024-subsidiary-accounts.md) §5, [0030](../decisions/0030-edit-filters.md) §11) | `pass`/`fail` by sending a request with an unregistered host, and one with a forged `X-Forwarded-For` from an untrusted address; `fail` if the auth base is a tenant base or the farm base, or if the instance has two tenants and no auth base; `fail` if the server is behind a proxy and trusts no hop; `attest`, where trust is by address, that only trusted hops can reach the server's listener |
| 6 | The reverse proxy or CDN forwards `Host` unchanged, caches only responses with public `Cache-Control` and never one carrying `Set-Cookie` or answering a request with `Authorization` or a session cookie, strips inbound `X-Forwarded-*` from clients before setting its own, terminates TLS and sends HSTS | L2 may hold public form only ([0056](../decisions/0056-security-model.md) §7); a proxy that caches an authenticated page serves it to the next visitor | `attest`; the privacy test of [0056](../decisions/0056-security-model.md) §15 exercises the first two through the proxy when `instance check --through {url}` is given |
| 7 | A SPARQL endpoint, QLever or any other external index, and the embedded query store ([0059](../decisions/0059-query-service.md) §2), is loaded from the public dump or the update stream, or from `view.rdf_delta`, which carries the same rows, never from `view` or `log` otherwise | Those carry ∅ form by construction ([0032](../decisions/0032-sparql-update-stream.md) §1); a direct load bypasses the evaluator | `attest` |
| 8 | The Parsoid service and any other helper process reach content only through the server's internal endpoint, with a service credential, on a private address | A renderer that reads the database reads everything | `fail` if the configured Parsoid endpoint is a public address |
| 9 | `/metrics`, health and debug routes are served on `server.admin_listen`, a separate listener, never on the public one | They expose names, counts and timings | `fail` if the admin listener is the public one |
| 10 | Backups of Postgres, the blob store and Valkey's persistence are encrypted at rest and held with access equal to the database's | They are the whole instance, `private` included | `attest` |
| 11 | Secrets come from files or the environment, never from the command line ([0033](../decisions/0033-backend-stack.md) §12), and the instance key is in a file readable by the server alone | A command line is visible to every process | `fail` on a world-readable key file |
| 12 | A web tier holds no credential of the instance's beyond an optional forwarder key and the password of the Valkey its response cache uses, which may be the instance's L1 Valkey, reads no other store of it, and reaches the API over TLS unless the API's address resolves only to internal addresses; it appends to forwarded headers and judges none of them (line 5), and the Valkey of its response cache is held to line 3 | It sees every viewer's cookie in transit | `fail` if the web tier's cache names a Valkey that fails line 3, or its `web.api` is plain HTTP to an address that is not internal |
| 13 | The query service's remote store ([0059](../decisions/0059-query-service.md) §4) is reachable only from the servers and requires credentials, and every query against it passes through the service, which fixes the tenant's dataset; the embedded store's directory is readable by the server alone | A shared store holds every public tenant's graphs; a query that reached it directly would choose its own dataset | `fail` if `query.endpoint` is a public address or accepts an unauthenticated request; `fail` on a world-readable `query.path`; `attest` for other clients |

### 5.4 Modes, and what the boundary is not

*Sources: [0056](../decisions/0056-security-model.md) §10.*

**Modes.** `server.mode` is `production` or `development`. In `production`, the server refuses to start while any `fail` stands and logs every `attest` not yet given; `development` starts anyway and marks `siprop=triplespace` with `insecure: true`. `instance create` writes `production`.

**What the boundary is not.** It is not an encryption scheme: restricted content is plaintext in the stores, because rendering, searching and diffing need it, and because key management per group would move the problem rather than solve it. It is not a guarantee against the operator ([0056](../decisions/0056-security-model.md) §5). It is the statement that the server is the reference monitor, and the list of what has to be true for that to mean anything.

### 5.5 The web tier in the boundary

*Sources: [0057](../decisions/0057-web-tier.md) §10; [0056](../decisions/0056-security-model.md) §10.*

Line 12 of §5.3 is the web tier's line: it holds no credential of the instance's beyond an optional forwarder key and the password of the Valkey its cache uses, which may be the instance's L1 Valkey ([03](03-storage-caches-and-search.md) §10.2), and reads no other store of it, so lines 1, 2 and 4 do not apply to it and line 3 applies only to its cache's Valkey. It sees every viewer's cookie in transit, so it is a hop like the edge.

**`instance check`** sends a forged `X-Forwarded-For` from an untrusted address and fails if it is believed; it records an `attest` that only trusted hops can reach the API's listener, where trust is by address; and it fails a web tier configured with plain HTTP to an address that is not internal.

**The privacy test runs through the web tier.** `instance check --through {url}` and the cases of [0056](../decisions/0056-security-model.md) §15 ([09](09-security-and-moderation.md)) are run against the site's pages as well as the API, so a confidential target's page and its `action=render` are byte-equal to a missing one's.
