#!/usr/bin/env python3
"""Attack battery for the permission model (09 §4–§5; 0056): real HTTP against a running
server, no test helpers, every route that could leak or widen.

Two halves, both run by default:

- **negative** — what nobody outside a group should be able to see or do, but can: content
  of a confidential page or entity through every read route (Action API, REST, the site,
  Special:EntityData, RDF), write-side oracles that distinguish hidden from missing,
  privilege escalation (self-grants, group definitions, ACL writes, lock-out bypass by a
  sysop, scoped-group escape, delegation ceilings, stale delegation, inclusion chains), a
  hidden user's name, tokens, a private tenant, threads on a confidential page's talk page,
  talk pages and threads through every surface (listings, composite histories, rendering,
  departures, hidden posts, the site's pages and forms, a protected talk page),
  notifications of mentions in a thread the account may not read or in a post since hidden,
  a watch on a confidential page and its talk page (watchlist, raw list, `prop=info`,
  notifications, the site),
  deletion fallout, a bot of a member, and the owner outside `suppress`.
- **positive** — what legitimate actors should be able to do, but can't: the owner's setup,
  a group member's full rights on a confidential page, entity and set, delegated and scoped
  administration, bureaucrat and sysop work, notifications, the owner with `suppress`, a
  private tenant for its users, a bot with grants, and the CLI's rights commands.

Each probe is a check; a check that fails is a *finding*, printed with `!!` and listed at
the end, and the exit status is 1 when there is any. Run through `tools/acl_check.sh`,
which sets the server up, or by hand against a fresh instance whose owner is `Alice`:

    tools/acl_check.py --api http://127.0.0.1:18290 --owner-password-file owner.pw \\
        --cli target/debug/triplespace --db postgres://…/tsacl

The run registers users (Mallory, Bob, Carol, Dave, Erin, Frank, Grace), defines groups and
writes pages and entities on the tenant, so point it at a throwaway database whose tenant
allows seven registrations a day from one address (`account.creation_limit`). Needs
`requests`. `--sections` limits the run (`--sections leaks,positive`); `--list` names them.
"""

import argparse
import json
import os
import subprocess
import sys

import requests

FINDINGS = []
CHECKS = [0]
ARGS = None


def finding(title, detail):
    FINDINGS.append((title, detail))
    CHECKS[0] += 1
    print(f"!! {title}\n   {detail[:600]}")


def ok(title):
    CHECKS[0] += 1
    print(f"ok {title}")


def text_of(r):
    if isinstance(r, str):
        return r
    if isinstance(r, tuple):
        return json.dumps(r[1])
    return json.dumps(r)


def contains(obj, needle):
    return needle in text_of(obj)


def deny(title, r):
    """`r` must be a refusal: an Action API `error`, or a REST status ≥ 400."""
    if isinstance(r, tuple):
        if r[0] < 400:
            finding(f"ALLOWED: {title}", f"{r[0]} {json.dumps(r[1])[:300]}")
            return False
    elif "error" not in r:
        finding(f"ALLOWED: {title}", json.dumps(r)[:300])
        return False
    ok(title)
    return True


def must(title, r, want=None):
    """`r` must be a success, carrying `want` if given."""
    if isinstance(r, tuple):
        st, body = r
        if st >= 400:
            finding(f"DENIED: {title}", f"{st} {json.dumps(body)[:300]}")
            return False
        if want and not contains(body, want):
            finding(f"WRONG: {title}", json.dumps(body)[:300])
            return False
        ok(title)
        return True
    if "error" in r:
        finding(f"DENIED: {title}", json.dumps(r)[:300])
        return False
    if want and not contains(r, want):
        finding(f"WRONG: {title}", json.dumps(r)[:300])
        return False
    ok(title)
    return True


def absent(title, r, needles):
    """None of `needles` may appear in `r`."""
    t = text_of(r)
    hits = [n for n in needles if n in t]
    if hits:
        i = max(t.find(hits[0]) - 150, 0)
        finding(f"LEAK: {title}", f"{hits}: …{t[i:i + 400]}…")
        return False
    ok(title)
    return True


def cli(*args, check=False):
    env = dict(os.environ)
    if ARGS.db:
        env["TRIPLESPACE_DATABASE_URL"] = ARGS.db
    r = subprocess.run([ARGS.cli, *args], env=env, capture_output=True, text=True, check=False)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {r.stderr}")
    return r


def cli_result(r):
    """A subprocess result as an Action API answer for `must`."""
    return {} if r.returncode == 0 else {"error": (r.stderr or r.stdout)[:300]}


class C:
    """A client: one session, optionally logged in."""

    def __init__(self, name=None):
        self.s = requests.Session()
        if ARGS.host:
            self.s.headers["Host"] = ARGS.host
        self.name = name or "anon"

    def api(self, **params):
        params.setdefault("format", "json")
        params.setdefault("formatversion", "2")
        return self.s.get(f"{ARGS.api}/w/api.php", params=params).json()

    def post(self, **params):
        params.setdefault("format", "json")
        params.setdefault("formatversion", "2")
        return self.s.post(f"{ARGS.api}/w/api.php", data=params).json()

    def rest(self, method, path, body=None):
        r = self.s.request(method, f"{ARGS.api}/w/rest.php/triplespace/v0{path}", json=body)
        try:
            return r.status_code, r.json()
        except ValueError:
            return r.status_code, r.text

    def raw(self, path, **kw):
        return self.s.get(f"{ARGS.api}{path}", **kw).text

    def token(self, kind="csrf"):
        return self.api(action="query", meta="tokens", type=kind)["query"]["tokens"][f"{kind}token"]

    def login(self, name, pw):
        t = self.token("login")
        r = self.post(action="clientlogin", username=name, password=pw, logintoken=t,
                      loginreturnurl=f"{ARGS.api}/")
        if r.get("clientlogin", {}).get("status") != "PASS":
            raise RuntimeError(f"login {name}: {r}")
        self.name = name
        return self

    def register(self, name, pw):
        v = self.api(action="query", meta="authmanagerinfo", amirequestsfor="create",
                     amimergerequestfields=1)
        extra = {}
        fields = v["query"]["authmanagerinfo"].get("fields", {})
        if "tschallengeid" in fields:
            cid = fields["tschallengeid"]["value"]
            sol = cli("challenge", "solve", cid, check=True).stdout.strip()
            extra = {"tschallengeid": cid, "tschallengesolution": sol}
        t = self.token("createaccount")
        r = self.post(action="createaccount", username=name, password=pw, retype=pw,
                      createtoken=t, createreturnurl=f"{ARGS.api}/", **extra)
        if r.get("createaccount", {}).get("status") != "PASS":
            raise RuntimeError(f"register {name}: {r}")
        return self.login(name, pw)

    def userrights(self, **kw):
        return self.post(action="userrights", token=self.token("userrights"), **kw)

    def edit(self, title, text):
        return self.post(action="edit", title=title, text=text, token=self.token())

    def protect(self, title, protections, **kw):
        return self.post(action="protect", title=title, protections=protections,
                         token=self.token(), **kw)

    def page_id(self, title):
        return self.api(action="query", prop="info", titles=title)["query"]["pages"][0]["pageid"]

    def new_item(self, label, description=None):
        data = {"labels": {"en": {"language": "en", "value": label}}}
        if description:
            data["descriptions"] = {"en": {"language": "en", "value": description}}
        r = self.post(action="wbeditentity", new="item", token=self.token(), data=json.dumps(data))
        if "entity" not in r:
            raise RuntimeError(f"new item: {r}")
        return r["entity"]["id"]


class Cast:
    """The actors and fixtures the sections share."""

    def __init__(self):
        pw = ARGS.owner_password
        self.alice = C().login("Alice", pw)
        self.mallory = C().register("Mallory", "mallory-secret-1234")
        self.bob = C().register("Bob", "bob-secret-1234")
        self.carol = C().register("Carol", "carol-secret-1234")
        self.anon = C()
        a = self.alice
        # The site probes read real pages only if the site is served (`--ui embedded`);
        # against a bare 404 every `absent` would pass.
        must("the site is served", (200, self.anon.raw("/wiki/Special:SpecialPages")), "ts-header")
        must("owner defines board", a.rest("PUT", "/group/board", {"permissions": [], "token": a.token()}))
        must("owner makes Bob a sysop", a.userrights(user="Bob", add="sysop"))
        must("owner makes Carol board, bureaucrat and sysop",
             a.userrights(user="Carol", add="board|bureaucrat|sysop"))
        must("owner joins board", a.userrights(user="Alice", add="board"))
        at = a.token()
        a.edit("Project:Secret", "Hush. The password is swordfish.")
        a.edit("Project:Secret/Sub", "Subpage hush.")
        a.edit("Project:Open", "Public text linking [[Project:Secret]] and {{Project:Secret}}.")
        self.secret = a.page_id("Project:Secret")
        self.gem = a.new_item("Hidden Gem", "secretdesc")
        self.plain = a.new_item("Plain Item")
        self.plain_page = a.page_id(f"Item:{self.plain}")
        # A protect event before the page is confidential: it must vanish with the page.
        must("owner protects edit before restricting", a.protect("Project:Secret", "edit=sysop"))
        self.gem_rev = a.post(action="wbsetlabel", id=self.gem, language="de", value="Geheim",
                              token=at)["entity"]["lastrevid"]
        must("owner restricts page to board (with its talk page)",
             a.protect("Project:Secret", "read=board", tstalk=1))
        must("owner restricts entity to board",
             a.rest("PUT", f"/acl/entity/{self.gem}",
                    {"restrictions": {"read": {"group": "board"}}, "token": at}))
        self.outsiders = (self.anon, self.mallory, self.bob)


# ---------------------------------------------------------------------------------------
# Negative: shouldn't be able to, but can.

CONTENT = ["swordfish", "Hush", "Hidden Gem", "secretdesc", "Subpage hush", "Geheim"]
# The title itself is a leak from a listing; bare "Secret" is not, since a fixture item's
# label may hold it.
LISTING = CONTENT + ["Project:Secret"]


def leaks(x):
    """Every route that could carry the confidential page's or entity's content or name."""
    gem, pid = x.gem, x.secret
    # (name, probe, needles): a listing route must not name the page either; a route whose
    # request names the page is judged on content, since the echo is not a leak.
    probes = [
        ("prop=info", lambda c: c.api(action="query", prop="info", titles="Project:Secret"), CONTENT),
        ("prop=revisions content", lambda c: c.api(action="query", prop="revisions", rvprop="content|user|comment", titles="Project:Secret"), CONTENT),
        ("prop=info by pageid", lambda c: c.api(action="query", prop="info", pageids=pid), LISTING),
        ("revisions by revids", lambda c: c.api(action="query", prop="revisions", rvprop="content", revids="|".join(str(i) for i in range(1, 40))), LISTING),
        ("parse page", lambda c: c.api(action="parse", page="Project:Secret", prop="text|wikitext"), CONTENT),
        ("parse transcluding page", lambda c: c.api(action="parse", page="Project:Open", prop="text"), CONTENT),
        ("parse text transclusion", lambda c: c.api(action="parse", text="{{Project:Secret}}", prop="text", contentmodel="wikitext", title="X"), CONTENT),
        ("compare", lambda c: c.api(action="compare", fromtitle="Project:Secret", totitle="Project:Open"), CONTENT),
        ("compare by rev", lambda c: c.api(action="compare", fromrev=1, torev=2), CONTENT),
        ("allpages", lambda c: c.api(action="query", list="allpages", apnamespace=4, aplimit=500), LISTING),
        ("allpages prefix", lambda c: c.api(action="query", list="allpages", apnamespace=4, apprefix="Sec"), LISTING),
        ("recentchanges", lambda c: c.api(action="query", list="recentchanges", rcprop="title|comment|user", rclimit=500), LISTING),
        ("logevents", lambda c: c.api(action="query", list="logevents", lelimit=500), LISTING),
        ("logevents protect", lambda c: c.api(action="query", list="logevents", letype="protect", lelimit=500), LISTING),
        ("usercontribs Alice", lambda c: c.api(action="query", list="usercontribs", ucuser="Alice", uclimit=500, ucprop="title|comment"), LISTING),
        ("subpage", lambda c: c.api(action="query", prop="info", titles="Project:Secret/Sub"), CONTENT),
        ("wbgetentities", lambda c: c.api(action="wbgetentities", ids=gem), CONTENT),
        ("wbsearchentities label", lambda c: c.api(action="wbsearchentities", search="Hidden", language="en"), [gem, "Hidden Gem", "secretdesc"]),
        ("wbsearchentities description", lambda c: c.api(action="wbsearchentities", search="secretdesc", language="en"), [gem, "Hidden Gem"]),
        ("prop=info item title", lambda c: c.api(action="query", prop="info", titles=f"Item:{gem}"), CONTENT + ["lastrevid"]),
        ("templates", lambda c: c.api(action="query", prop="templates", titles="Project:Open"), CONTENT),
        ("REST activity", lambda c: c.rest("GET", "/activity?limit=500"), LISTING + [gem]),
        ("REST entity history", lambda c: c.rest("GET", f"/entity/{gem}/history"), CONTENT),
        ("REST provenance", lambda c: c.rest("GET", f"/entity/{gem}/provenance"), CONTENT),
        ("REST acl page", lambda c: c.rest("GET", f"/acl/page/{pid}"), LISTING + ["board"]),
        ("REST acl entity", lambda c: c.rest("GET", f"/acl/entity/{gem}"), LISTING + ["board"]),
        ("REST threads of page", lambda c: c.rest("GET", f"/page/{pid}/threads"), LISTING),
        ("REST page history", lambda c: c.rest("GET", f"/page/{pid}/history"), LISTING),
        ("Special:EntityData", lambda c: c.raw(f"/wiki/Special:EntityData/{gem}.json"), CONTENT),
        ("site page", lambda c: c.raw("/wiki/Project:Secret"), CONTENT),
        ("site item", lambda c: c.raw(f"/wiki/Item:{gem}"), CONTENT),
        ("site history", lambda c: c.raw("/w/index.php?title=Project:Secret&action=history"), CONTENT),
        ("site raw", lambda c: c.raw("/w/index.php?title=Project:Secret&action=raw"), CONTENT),
        ("site recentchanges", lambda c: c.raw("/wiki/Special:RecentChanges"), LISTING),
        ("site log", lambda c: c.raw("/wiki/Special:Log"), LISTING),
        ("site search label", lambda c: c.raw("/w/index.php?search=Hidden+Gem&title=Special:Search"), [gem, "secretdesc"]),
        ("site search content", lambda c: c.raw("/w/index.php?search=swordfish&title=Special:Search"), ["Hush", "Project:Secret"]),
        ("site allpages", lambda c: c.raw("/wiki/Special:AllPages?namespace=4"), LISTING),
        ("site contributions", lambda c: c.raw("/wiki/Special:Contributions/Alice"), LISTING),
        ("site whatlinkshere", lambda c: c.raw("/wiki/Special:WhatLinksHere/Project:Secret"), CONTENT),
        ("site protectedpages", lambda c: c.raw("/wiki/Special:ProtectedPages"), LISTING),
        ("list=protectedpages", lambda c: c.api(action="query", list="protectedpages", prtype="read|edit", prlimit=500), LISTING),
        ("entity turtle", lambda c: c.raw(f"/entity/{gem}", headers={"Accept": "text/turtle"}), CONTENT),
        ("entity json", lambda c: c.raw(f"/entity/{gem}", headers={"Accept": "application/json"}), CONTENT),
    ]
    for who in x.outsiders:
        for name, probe, needles in probes:
            try:
                r = probe(who)
            except Exception as e:  # noqa: BLE001 — any failure is a finding, not a crash
                r = {"exception": str(e)}
            absent(f"{name} as {who.name}", r, needles)
    must("board member reads the page",
         x.carol.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), "swordfish")
    must("board member reads the entity", x.carol.api(action="wbgetentities", ids=x.gem), "Hidden Gem")


def oracles(x):
    """Does any write distinguish hidden from missing? (09 §5.3: an outsider may learn that a
    title is taken, nothing more.)"""
    m, b = x.mallory, x.bob
    r = m.post(action="wbsetlabel", id=x.gem, language="en", value="x", token=m.token())
    r2 = m.post(action="wbsetlabel", id="Q999999", language="en", value="x", token=m.token())
    if r.get("error", {}).get("code") != r2.get("error", {}).get("code"):
        finding("entity write oracle", f"hidden: {r} vs missing: {r2}")
    else:
        ok("entity write: hidden == missing")
    # `protect` is left out: a reserved title answers differently from a free one by design
    # (09 §5.3), and a confidential page's title is reserved for its group.
    bt = b.token()
    for action in ("delete", "undelete"):
        r = b.post(action=action, title="Project:Secret", token=bt)
        r2 = b.post(action=action, title="Project:Nonexistent123", token=bt)
        c1, c2 = r.get("error", {}).get("code"), r2.get("error", {}).get("code")
        if c1 != c2:
            finding(f"{action} oracle", f"hidden: {c1} {r} vs missing: {c2} {r2}")
        else:
            ok(f"{action}: hidden == missing ({c1})")
    r = b.post(action="delete", title=x.gem, token=bt)
    r2 = b.post(action="delete", title="Q999999", token=bt)
    if r.get("error", {}).get("code") != r2.get("error", {}).get("code"):
        finding("entity delete oracle", f"hidden: {r} vs missing: {r2}")
    else:
        ok("entity delete: hidden == missing")
    r = b.post(action="revisiondelete", type="revision", ids=str(x.gem_rev), hide="content", token=bt)
    items = r.get("revisiondelete", {}).get("items", [])
    if any(i.get("status") == "Success" for i in items):
        finding("revdel on a hidden entity's revision", json.dumps(r)[:300])
    else:
        ok("revdel of a hidden entity's revision refused")
    absent("earlier protect event hidden with the page",
           x.anon.api(action="query", list="logevents", letype="protect", lelimit=500), ["Project:Secret"])


def escalation(x):
    m, b, c, a = x.mallory, x.bob, x.carol, x.alice
    mt = m.token()
    for g in ["sysop", "board", "owner", "bureaucrat", "read-all", "suppress"]:
        deny(f"self-grant of {g}", m.userrights(user="Mallory", add=g))
    deny("user defines a group", m.rest("PUT", "/group/evil", {"permissions": ["*"], "token": mt}))
    for path, body in [("/acl/tenant/librarybase", {"restrictions": {"read": {"group": "user"}}}),
                       ("/acl/namespace/4", {"restrictions": {"edit": {"group": "user"}}}),
                       (f"/acl/page/{x.secret}", {"restrictions": {"edit": {"group": "user"}}})]:
        deny(f"user writes {path}", m.rest("PUT", path, dict(body, token=mt)))
    deny("user lists empty groups", m.rest("GET", "/acl/empty-groups"))
    deny("user blocks the owner", m.post(action="block", user="Alice", token=mt))
    deny("bureaucrat appoints an owner", c.userrights(user="Mallory", add="owner"))
    bt = b.token()
    deny("sysop outside the group lifts the ACL", b.rest("DELETE", f"/acl/page/{x.secret}", {"token": bt}))
    deny("sysop outside the group re-points the ACL",
         b.rest("PUT", f"/acl/page/{x.secret}", {"restrictions": {"read": {"group": "sysop"}}, "token": bt}))
    deny("sysop outside the group unprotects", b.protect("Project:Secret", "read=all"))
    deny("sysop puts the hidden page in a set",
         b.rest("POST", "/acl/set", {"name": "s", "restrictions": {"edit": {"group": "sysop"}},
                                     "members": [{"kind": "page", "id": x.secret}], "token": bt}))
    deny("sysop defines a delegating group",
         b.rest("PUT", "/group/helpers", {"permissions": [], "adds": ["sysop"], "token": bt}))
    # Scoped group: delete within namespace 4 only.
    must("bureaucrat defines a scoped group",
         c.rest("PUT", "/group/hr-admins", {"permissions": ["delete", "protect", "edit"],
                                            "within": ["namespace:4"], "token": c.token()}))
    must("bureaucrat adds to it", c.userrights(user="Mallory", add="hr-admins"))
    a.edit("Mainspace page", "main")
    a.edit("Project:Departmental", "dept")
    must("user edits a page", m.edit("Project:Scratchpad", "by Mallory"))
    deny("scoped delete outside the scope", m.post(action="delete", title="Mainspace page", token=m.token()))
    must("scoped delete inside the scope", m.post(action="delete", title="Project:Departmental", token=m.token()))
    must("owner undeletes it", a.post(action="undelete", title="Project:Departmental", token=a.token()))
    must("scoped protect inside the scope", m.protect("Project:Departmental", "read=hr-admins"))
    must("scoped member reads it", m.api(action="query", prop="revisions", rvprop="content",
                                         titles="Project:Departmental"), "dept")
    must("scoped member lifts it", m.protect("Project:Departmental", "read=all"))


def delegation(x):
    """hr-admins may add to hr and no further; a delegation that reaches `userrights` is
    refused when written and when used."""
    a, m = x.alice, x.mallory
    at = a.token()
    must("owner defines hr", a.rest("PUT", "/group/hr", {"permissions": [], "token": at}))
    must("owner delegates hr to hr-admins",
         a.rest("PUT", "/group/hr-admins", {"permissions": [], "adds": ["hr"], "removes": ["hr"], "token": at}))
    for g in ["hr-admins", "sysop", "board"]:
        deny(f"delegate adds to {g}", m.userrights(user="Bob", add=g))
    deny("delegate adds self without adds_self", m.userrights(user="Mallory", add="hr"))
    must("delegate adds to hr", m.userrights(user="Bob", add="hr"))
    must("owner gives hr userrights", a.rest("PUT", "/group/hr", {"permissions": ["userrights"], "token": at}))
    deny("delegating to a group holding userrights",
         a.rest("PUT", "/group/hr-admins", {"permissions": [], "adds": ["hr"], "token": at}))
    deny("stale delegation to a group now holding userrights", m.userrights(user="Carol", add="hr"))
    must("owner takes userrights back from hr", a.rest("PUT", "/group/hr", {"permissions": [], "token": at}))
    must("owner removes Bob from hr", a.userrights(user="Bob", remove="hr"))


def includes(x):
    a, m = x.alice, x.mallory
    at = a.token()
    must("owner defines peek including owner",
         a.rest("PUT", "/group/peek", {"permissions": [], "includes": ["owner"], "token": at}))
    must("owner adds Mallory to peek", a.userrights(user="Mallory", add="peek"))
    absent("including owner reads nothing of board (inclusion is one level)",
           m.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), ["Hush"])
    must("owner redefines peek as includes=*",
         a.rest("PUT", "/group/peek", {"permissions": [], "includes": ["*", "suppress"], "token": at}))
    must("includes=* reads board",
         m.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), "Hush")
    deny("an included reader lifts the restriction", m.protect("Project:Secret", "read=all"))
    deny("an included reader edits the page", m.edit("Project:Secret", "x"))
    must("owner removes Mallory from peek", a.userrights(user="Mallory", remove="peek"))
    absent("removed from peek, reads nothing",
           m.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), ["Hush"])


def threads(x):
    """A thread on the confidential page's talk page: `tstalk=1` took the talk page along."""
    a, at = x.alice, x.alice.token()
    st, r = a.rest("POST", "/thread", {"target": {"kind": "page", "id": x.secret},
                                       "subject": "Secret talk", "text": "The code is 4711.", "token": at})
    must("board member opens a thread on the talk page", (st, r))
    thread = r.get("thread")
    talk = a.rest("GET", f"/thread/{thread}")[1]["home"]["talk_page_id"]
    needles = ["4711", "Secret talk"]
    for who in x.outsiders:
        for name, probe in [
            ("thread", lambda c: c.rest("GET", f"/thread/{thread}")),
            ("page threads", lambda c: c.rest("GET", f"/page/{x.secret}/threads")),
            ("talk page threads", lambda c: c.rest("GET", f"/page/{talk}/threads")),
            ("talk page info", lambda c: c.api(action="query", prop="info", titles="Project talk:Secret")),
            ("thread page info", lambda c: c.api(action="query", prop="info", pageids=thread)),
            ("thread history", lambda c: c.rest("GET", f"/page/{thread}/history")),
            ("talk history", lambda c: c.rest("GET", f"/page/{talk}/history")),
            ("recentchanges", lambda c: c.api(action="query", list="recentchanges", rcprop="title|comment", rclimit=500)),
            ("site thread", lambda c: c.raw(f"/wiki/Thread:{thread}")),
        ]:
            absent(f"thread via {name} as {who.name}", probe(who), needles)
        tok = who.token() if who.name != "anon" else "+\\"
        deny(f"post into the hidden thread as {who.name}",
             who.rest("POST", f"/thread/{thread}/post", {"text": "hi", "token": tok}))
    must("board member reads the thread", x.carol.rest("GET", f"/thread/{thread}"), "4711")


def _site_form(who, title, action, fields):
    """Posts one of the site's discussion forms, with the viewer's token, as a browser would."""
    data = {"wpEditToken": who.token() if who.name != "anon" else "+\\", **fields}
    return who.s.post(f"{ARGS.api}/w/index.php", params={"title": title, "action": action},
                      data=data, allow_redirects=False).text


def discussions(x):
    """Talk pages and threads beyond the one confidential thread (14 §1–§4; 16 §6.1; 19 §6.1):
    a confidential thread listed on a public talk page, rendering, departures, hidden posts,
    the site's own pages and forms, and a protected talk page."""
    a = x.alice
    at = a.token()
    open_id = a.page_id("Project:Open")
    base = a.rest("GET", "/settings")[1].get("base")
    must("owner makes posts wikitext",
         a.rest("PATCH", "/settings", {"set": {"thread.markup": "wikitext"}, "base": base, "token": at}))

    # A public thread whose wikitext post transcludes and links the confidential page.
    st, r = a.rest("POST", "/thread", {"target": {"kind": "page", "id": open_id}, "subject": "Open questions",
                                       "text": "Quoting {{Project:Secret}} and [[Project:Secret]].", "token": at})
    must("owner opens a public thread", (st, r))
    pub, pub_title = r.get("thread"), r.get("title")
    open_talk = a.rest("GET", f"/thread/{pub}")[1]["home"]["talk_page_id"]
    # A confidential thread: homed on the confidential page's talk page, listed on the public one.
    c = x.carol
    st, r = c.rest("POST", "/thread", {"target": {"kind": "page", "id": x.secret}, "subject": "Board only",
                                       "text": "Ballot 9931.", "also": [{"kind": "page", "id": open_id}],
                                       "token": c.token()})
    must("board member lists a confidential thread on the public talk page", (st, r))
    hid, hid_title, hid_rev = r.get("thread"), r.get("title"), r.get("revid")
    hidden = ["9931", "Board only"]
    # A post whose text and author are hidden afterwards.
    bt = x.bob.token()
    st, r = x.bob.rest("POST", f"/thread/{pub}/post", {"text": "Draft 2468, [[User:Mallory]].", "token": bt})
    must("sysop replies in the public thread", (st, r))
    hidden_reply = r.get("revid")
    must("the reply's mention reaches Mallory",
         x.mallory.api(action="query", meta="notifications", notprop="list"), f'"revid": {hidden_reply}')
    must("owner hides the reply's text and author",
         a.post(action="revisiondelete", type="revision", ids=str(r.get("revid")), hide="content|user",
                token=a.token()), "Success")
    must("board member mentions outsiders and the owner in the confidential thread",
         c.rest("POST", f"/thread/{hid}/post",
                {"text": "Ask [[User:Mallory]], [[User:Bob]] and [[User:Alice]]. Ballot 9931.", "token": c.token()}))
    posts_before = len(a.rest("GET", f"/thread/{hid}")[1].get("posts", []))

    for who in x.outsiders:
        for name, probe, needles in [
            ("listing", lambda c: c.rest("GET", f"/page/{open_talk}/threads"), hidden),
            ("composite history", lambda c: c.rest("GET", f"/page/{open_talk}/history?limit=500"), hidden),
            ("rendered public thread", lambda c: c.rest("GET", f"/thread/{pub}?html=1"), CONTENT),
            ("rendered hidden thread", lambda c: c.rest("GET", f"/thread/{hid}?html=1"), hidden),
            ("site talk page", lambda c: c.raw("/wiki/Project_talk:Open"), hidden + CONTENT),
            ("site talk history", lambda c: c.raw("/wiki/Project_talk:Open?action=history"), hidden),
            ("site public thread", lambda c: c.raw(f"/wiki/Thread:{pub_title}"), CONTENT),
            # The request names the hidden thread's subject; judged on its content.
            ("site hidden thread", lambda c: c.raw(f"/wiki/Thread:{hid_title}"), ["9931"]),
            ("site hidden thread history", lambda c: c.raw(f"/wiki/Thread:{hid_title}?action=history"),
             ["9931", "started the thread"]),
            # The talk page's revisions and diffs are its threads' (0019 A29).
            ("talk revisions", lambda c: c.api(action="query", prop="revisions", pageids=open_talk,
                                                 rvprop="ids|content|comment", rvslots="main", rvlimit=50), hidden),
            ("talk diff", lambda c: c.api(action="compare", fromid=open_talk, torelative="prev", prop="diff|title|comment"),
             hidden),
            ("talk diff at the hidden thread's revision",
             lambda c: c.api(action="compare", fromid=open_talk, fromrev=hid_rev, torelative="prev", prop="diff"), hidden),
            ("hidden thread's revision alone", lambda c: c.api(action="compare", fromrev=hid_rev, torelative="prev",
                                                               prop="diff|title"), hidden),
        ]:
            absent(f"discussion via {name} as {who.name}", probe(who), needles)
        if who is not x.bob:
            for name, probe in [
                ("rendered post", lambda c: c.rest("GET", f"/thread/{pub}?html=1")),
                ("site thread", lambda c: c.raw(f"/wiki/Thread:{pub_title}")),
                ("REST history", lambda c: c.rest("GET", f"/page/{open_talk}/history?limit=500")),
            ]:
                absent(f"hidden post's text and author via {name} as {who.name}", probe(who), ["2468", "Bob"])
        # Notifications (16 §5.2): a mention in a thread the account may not read addresses
        # nobody, and a hidden post leaves the inbox it reached.
        for name, probe in [
            ("notifications", lambda c: c.api(action="query", meta="notifications", notprop="list|count",
                                              notgroupbysection=1, notlimit=50)),
            ("Special:Notifications", lambda c: c.raw("/wiki/Special:Notifications")),
        ]:
            absent(f"mention in the hidden thread via {name} as {who.name}", probe(who),
                   hidden + [f"Thread:{hid_title}"])
        absent(f"the hidden reply's mention as {who.name}",
               who.api(action="query", meta="notifications", notprop="list"), [f'"revid": {hidden_reply}'])
        absent(f"site reply to the hidden thread as {who.name}",
               _site_form(who, f"Thread:{hid_title}", "reply", {"wpTextbox1": "Me too. ~~~~"}), ["9931"])
        tok = who.token() if who.name != "anon" else "+\\"
        deny(f"list the hidden thread elsewhere as {who.name}",
             who.rest("POST", f"/thread/{hid}/attach", {"target": {"kind": "page", "id": x.plain_page}, "base": 0,
                                                       "token": tok}))
        deny(f"detach the hidden thread from the public page as {who.name}",
             who.rest("POST", f"/thread/{hid}/detach", {"target": {"kind": "page", "id": open_id}, "base": 0,
                                                       "token": tok}))
    after = len(a.rest("GET", f"/thread/{hid}")[1].get("posts", []))
    if after == posts_before:
        ok("nothing was posted to the hidden thread through the site")
    else:
        finding("ALLOWED: a post reached the hidden thread through the site", f"{posts_before} → {after}")
    must("the owner, in board, is notified of the mention",
         a.api(action="query", meta="notifications", notprop="list", notlimit=50), "Board only")
    must("board member sees the listed thread", x.carol.rest("GET", f"/page/{open_talk}/threads"), "Board only")
    must("board member sees its rows in the composite history",
         x.carol.rest("GET", f"/page/{open_talk}/history?limit=500"), str(hid))
    must("board member reads it on the site", (200, x.carol.raw("/wiki/Project_talk:Open")), "9931")
    must("board member reads it in the talk page's revisions",
         x.carol.api(action="query", prop="revisions", pageids=open_talk, rvprop="content", rvslots="main"), "9931")

    # Departures: the detach of a confidential thread, and a public thread that moves into
    # the confidential talk page and is renamed there, leave rows on the public page.
    b = a.rest("GET", f"/thread/{hid}")[1]["base"]
    must("owner detaches the confidential thread from the public page",
         a.rest("POST", f"/thread/{hid}/detach", {"target": {"kind": "page", "id": open_id}, "base": b, "token": at}))
    st, r = a.rest("POST", "/thread", {"target": {"kind": "page", "id": open_id}, "subject": "Moving out",
                                       "text": "Soon.", "token": at})
    mover = r.get("thread")
    b = a.rest("GET", f"/thread/{mover}")[1]["base"]
    must("board member moves a public thread into the confidential talk page",
         c.rest("POST", f"/thread/{mover}/move", {"target": {"kind": "page", "id": x.secret}, "base": b,
                                                 "token": c.token()}))
    b = c.rest("GET", f"/thread/{mover}")[1]["base"]
    must("and renames it there",
         c.rest("POST", f"/thread/{mover}/rename", {"subject": "Plans 5150", "base": b, "token": c.token()}))
    for who in x.outsiders:
        for name, probe in [
            ("REST history", lambda c: c.rest("GET", f"/page/{open_talk}/history?limit=500")),
            ("site history", lambda c: c.raw("/wiki/Project_talk:Open?action=history")),
            ("recentchanges", lambda c: c.api(action="query", list="recentchanges", rcprop="title|comment", rclimit=500)),
        ]:
            absent(f"departures via {name} as {who.name}", probe(who), hidden + ["5150"])
    must("board member sees the move on the public page's history",
         x.carol.rest("GET", f"/page/{open_talk}/history?limit=500"), "from_talk")

    # A protected talk page: its threads take the protection (a talk page encloses its threads).
    must("owner protects the public talk page", a.protect("Project talk:Open", "edit=sysop"))
    mt = x.mallory.token()
    deny("a user starts a thread on the protected talk page",
         x.mallory.rest("POST", "/thread", {"target": {"kind": "page", "id": open_id}, "subject": "Let me in",
                                            "text": "x", "token": mt}))
    deny("a user replies in a thread homed there",
         x.mallory.rest("POST", f"/thread/{pub}/post", {"text": "x", "token": mt}))
    # The refusal page keeps the user's own text in the form; the listing must not have it.
    _site_form(x.mallory, "Project talk:Open", "addtopic", {"wpSummary": "Site 7777", "wpTextbox1": "x"})
    absent("nor through the site", x.anon.rest("GET", f"/page/{open_talk}/threads"), ["Site 7777"])
    must("a sysop replies there", x.bob.rest("POST", f"/thread/{pub}/post", {"text": "Sysop here.", "token": bt}))
    must("owner lifts the protection", a.protect("Project talk:Open", "edit=all"))
    base = a.rest("GET", "/settings")[1].get("base")
    must("owner makes posts markdown again",
         a.rest("PATCH", "/settings", {"set": {"thread.markup": "markdown"}, "base": base, "token": a.token()}))


def watching(x):
    """The watch set (16 §4.2–4.3; 0021 A21): an outsider who watches a confidential page and
    its talk page, by title and with notifications, learns nothing of their changes through
    the watchlist, its raw list, `prop=info`, notifications or the site; a member who
    watches it is told."""
    a, c = x.alice, x.carol
    hidden = ["Hush", "swordfish", "Watched 4242"]
    for who in (x.mallory, x.bob):
        wt = who.token("watch")
        for title in ["Project:Secret", "Project talk:Secret"]:
            r = who.post(action="watch", titles=title, tsnotify="1", token=wt)
            must(f"{who.name} may watch {title} (a title is theirs to watch)", r)
            absent(f"watching {title} says nothing more as {who.name}", r, ["pageid", "missing"] + hidden)
    # A watched title the outsider may not see is listed as a title with no page is, and a
    # page ID they may not see answers as one that does not exist: neither is an oracle.
    m = x.mallory
    must("Mallory watches a title with no page", m.post(action="watch", titles="Project:Nothing here 77",
                                                         token=m.token("watch")))
    raw = m.api(action="query", list="watchlistraw", wrlimit=500)
    titles = sorted(w.get("title", "") for w in raw.get("watchlistraw", []))
    if any(t.endswith(":Secret") for t in titles) and any(t.endswith(":Nothing here 77") for t in titles):
        ok("the confidential title is listed as a title with no page is")
    else:
        finding("ORACLE: the raw list tells a confidential page from a missing one", json.dumps(raw)[:300])
    gone = m.rest("PUT", "/watchlist/page/999999999", {"token": m.token()})
    hid = m.rest("PUT", f"/watchlist/page/{x.secret}", {"token": m.token()})
    if (gone[0], gone[1].get("code")) == (hid[0], hid[1].get("code")):
        ok("watching a hidden page ID answers as a missing one")
    else:
        finding("ORACLE: watching a hidden page ID differs from a missing one", f"{gone} vs {hid}")
    must("the owner, in board, watches it with notifications",
         a.post(action="watch", titles="Project:Secret", tsnotify="1", token=a.token("watch")))
    must("a board member who is a sysop changes it", c.edit("Project:Secret", "Hush. Watched 4242."))
    st, r = c.rest("POST", "/thread", {"target": {"kind": "page", "id": x.secret}, "subject": "Watched 4242",
                                       "text": "Watched 4242 in the talk.", "token": c.token()})
    must("and starts a thread on its talk page", (st, r))
    for who in x.outsiders:
        if who.name == "anon":
            continue
        for name, probe in [
            ("notifications", lambda c: c.api(action="query", meta="notifications", notprop="list|count", notlimit=50)),
            ("watchlist", lambda c: c.api(action="query", list="watchlist", wlprop="title|comment|user|notificationtimestamp",
                                          wllimit=500, wlallrev=1)),
            ("watchlistraw", lambda c: c.api(action="query", list="watchlistraw", wrprop="changed", wrlimit=500)),
            ("REST watchlist feed", lambda c: c.rest("GET", "/activity?set=watchlist&limit=500")),
            ("REST watch set", lambda c: c.rest("GET", "/watchlist")),
            ("prop=info", lambda c: c.api(action="query", prop="info", titles="Project:Secret|Project talk:Secret",
                                          inprop="watched|notificationtimestamp")),
            ("Special:Watchlist", lambda c: c.raw("/wiki/Special:Watchlist")),
            ("Special:EditWatchlist", lambda c: c.raw("/wiki/Special:EditWatchlist")),
            ("Special:Notifications", lambda c: c.raw("/wiki/Special:Notifications")),
        ]:
            absent(f"watched confidential page via {name} as {who.name}", probe(who), hidden + ["Secret talk"])
        info = who.api(action="query", prop="info", titles="Project:Secret", inprop="notificationtimestamp")
        stamps = [p.get("notificationtimestamp", "") for p in info.get("query", {}).get("pages", [])]
        if any(stamps):
            finding(f"LEAK: an unseen change to the confidential page via notificationtimestamp as {who.name}",
                    json.dumps(info)[:300])
        else:
            ok(f"no unseen change to the confidential page via notificationtimestamp as {who.name}")
    must("the owner is notified of the change",
         a.api(action="query", meta="notifications", notprop="list", notlimit=50), "ts-watch")
    must("and sees it on the watchlist",
         a.api(action="query", list="watchlist", wlprop="title|comment", wllimit=50), "Secret")
    for who in (x.mallory, x.bob, a):
        who.post(action="watch", titles="Project:Secret", unwatch="1", token=who.token("watch"))


def deletion(x):
    a, b, anon = x.alice, x.bob, x.anon
    a.edit("Project:Open/Sub", "opensub")
    must("sysop deletes a public page", b.post(action="delete", title="Project:Open", token=b.token()))
    absent("its subpage survives", anon.api(action="query", prop="info", titles="Project:Open/Sub"), ["missing"])
    must("owner undeletes it", a.post(action="undelete", title="Project:Open", token=a.token()))
    must("owner (in board) deletes the confidential page",
         a.post(action="delete", title="Project:Secret", token=a.token()))
    deny("sysop outside the group undeletes it", b.post(action="undelete", title="Project:Secret", token=b.token()))
    absent("deleted and confidential: absent from the sysop's delete log",
           b.api(action="query", list="logevents", letype="delete", lelimit=500), ["Project:Secret"])
    # The deletion's own event cannot be reached by log ID from outside the group either.
    ev = a.api(action="query", list="logevents", letype="delete", letitle="Project:Secret", lelimit=1)
    logid = ev["query"]["logevents"][0]["logid"]
    r = b.post(action="revisiondelete", type="logging", ids=str(logid), hide="comment", token=b.token())
    item = (r.get("revisiondelete", {}).get("items") or [{}])[0]
    if item.get("status") == "Fail" and item.get("errors", [{}])[0].get("code") == "nosuchlogid":
        ok("sysop outside the group cannot hide the confidential deletion's event")
    else:
        finding("ALLOWED: sysop outside the group hides the confidential deletion's event", json.dumps(r)[:300])
    absent("the refusal names nothing", r, ["Project:Secret", "board"])
    # Partial undeletion keeps the unrestored revisions hidden from readers.
    a.edit("Project:Partial", "first text")
    a.edit("Project:Partial", "second text")
    must("sysop deletes it", b.post(action="delete", title="Project:Partial", token=b.token()))
    latest = a.api(action="query", prop="deletedrevisions", drvprop="ids", titles="Project:Partial")
    revs = [rv["revid"] for p in latest["query"]["pages"] for rv in p.get("deletedrevisions", [])]
    must("sysop restores the latest revision only",
         b.post(action="undelete", title="Project:Partial", revisions=str(max(revs)), token=b.token()))
    absent("the unrestored revision's text is hidden",
           anon.api(action="query", prop="revisions", rvprop="content", rvlimit=10, titles="Project:Partial"),
           ["first text"])
    must("the restored one is read", anon.api(action="query", prop="revisions", rvprop="content",
                                              titles="Project:Partial"), "second text")
    must("owner undeletes it", a.post(action="undelete", title="Project:Secret", token=a.token()))
    absent("still confidential after undelete",
           b.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), ["Hush"])


def moves(x):
    """`action=move` is checked at both ends (10 §2.5; 09 §3.4)."""
    a, b, m, anon = x.alice, x.bob, x.mallory, x.anon
    a.edit("Project:Movable", "movable text")
    a.edit("Project:Movable/Sub", "movable sub")
    m.edit("User:Mallory/Mine", "mine")
    must("owner reserves a title", a.protect("Project:Taken", "create=sysop"))
    # The confidential page is a title taken, never a hint; the reservation is restricted.
    for who in (m, b):
        deny(f"{who.name} moves a page onto the confidential title",
             who.post(action="move", **{"from": "Project:Movable"}, to="Project:Secret", token=who.token()))
        r = who.post(action="move", **{"from": "Project:Movable"}, to="Project:Secret", token=who.token())
        absent(f"{who.name}'s refusal names nothing confidential", r, ["board", "confidential", "Hush"])
    deny("user moves onto a reserved title",
         m.post(action="move", **{"from": "User:Mallory/Mine"}, to="Project:Taken", token=m.token()))
    # The confidential page itself is missing to outsiders at the source end.
    for who in (m, b):
        r = who.post(action="move", **{"from": "Project:Secret"}, to="Project:Leaked", token=who.token())
        deny(f"{who.name} moves the confidential page", r)
        if r.get("error", {}).get("code") == "missingtitle":
            ok(f"{who.name} is told the page is missing, not restricted")
        else:
            finding(f"ORACLE: {who.name}'s refusal tells the confidential page apart", json.dumps(r)[:300])
    absent("no page appeared at the leaked title",
           anon.api(action="query", prop="info", titles="Project:Leaked"), ["pageid"])
    # A move restriction holds; a protected page's redirect does not carry its protection.
    must("owner restricts move on the movable page", a.protect("Project:Movable", "move=sysop"))
    deny("user moves a move-protected page",
         m.post(action="move", **{"from": "Project:Movable"}, to="Project:Moved", token=m.token()))
    deny("user suppresses the redirect",
         m.post(action="move", **{"from": "User:Mallory/Mine"}, to="User:Mallory/Mine2", noredirect=1,
                token=m.token()))
    deny("anon moves anything",
         anon.post(action="move", **{"from": "User:Mallory/Mine"}, to="User:Mallory/Mine3", token="+\\"))
    deny("user moves an entity", m.post(action="move", **{"from": f"Item:{x.plain}"}, to="Project:Q", token=m.token()))
    deny("user moves a page into the entity namespace",
         m.post(action="move", **{"from": "User:Mallory/Mine"}, to="Item:Q9999", token=m.token()))
    must("sysop moves the move-protected page with its subpages",
         b.post(action="move", **{"from": "Project:Movable"}, to="Project:Moved", movesubpages=1, token=b.token()))
    must("the old title is a redirect", anon.api(action="query", prop="info", titles="Project:Movable"), "redirect")
    must("the subpage moved", anon.api(action="query", prop="info", titles="Project:Moved/Sub"), "pageid")
    must("the move is logged under the old title",
         anon.api(action="query", list="logevents", letype="move", lelimit=5), "Project:Movable")
    # The group moves the confidential page; its old title stays absent for outsiders
    # when no redirect is left, and the move event is theirs alone.
    c = x.carol  # in board, and a sysop: the page's edit protection admits her
    must("board sysop moves the confidential page without a redirect",
         c.post(action="move", **{"from": "Project:Secret"}, to="Project:Secret2", noredirect=1, token=c.token()))
    absent("the confidential move is absent from the public log",
           anon.api(action="query", list="logevents", letype="move", lelimit=500), ["Secret2", "Project:Secret"])
    absent("the new title is absent for outsiders",
           b.api(action="query", prop="info", titles="Project:Secret2"), ["pageid"])
    must("board sysop moves it back", c.post(action="move", **{"from": "Project:Secret2"}, to="Project:Secret",
                                             noredirect=1, token=c.token()))


def bearer(x):
    """A bot of a board member: credentials never widen beyond the subsidiary's own groups."""
    if not ARGS.cli:
        print("-- bearer: skipped (no --cli)")
        return
    cli("subsidiary", "create", "--tenant", ARGS.tenant, "--name", "AliceBot", "--operator", "Alice",
        "--group", "bot", check=True)
    key = json.loads(cli("subsidiary", "key", "--tenant", ARGS.tenant, "--name", "AliceBot",
                         "--label", "x", "--grant", "editentity", "--json", check=True).stdout)
    bot = C("AliceBot")
    bot.s.headers["Authorization"] = f"Bearer {key['bearer']}"
    absent("operator's groups do not reach the bot",
           bot.api(action="query", prop="revisions", rvprop="content", titles="Project:Secret"), ["Hush"])
    deny("bot grants itself a group", bot.post(action="userrights", user="AliceBot", add="board",
                                             token=bot.token("userrights")))
    deny("bot writes an ACL", bot.rest("PUT", f"/acl/page/{x.secret}",
                                       {"restrictions": {"edit": {"group": "bot"}}, "token": bot.token()}))
    deny("bot edits a page without the grant",
         bot.post(action="edit", title="Project:BotScratch", text="x", token=bot.token()))


def owner(x):
    """The owner reads everything except what is restricted to `suppress`, and lifts that
    only after joining — in the public rights log."""
    a, anon = x.alice, x.anon
    a.userrights(user="Alice", remove="suppress")
    a.edit("Project:Oversight", "oversight text")
    must("owner joins suppress", a.userrights(user="Alice", add="suppress"))
    must("owner restricts a page to suppress", a.protect("Project:Oversight", "read=suppress"))
    must("owner leaves suppress", a.userrights(user="Alice", remove="suppress"))
    absent("owner outside suppress cannot read it",
           a.api(action="query", prop="revisions", rvprop="content", titles="Project:Oversight"), ["oversight text"])
    deny("owner outside suppress cannot lift it", a.protect("Project:Oversight", "read=all"))
    must("owner rejoins suppress", a.userrights(user="Alice", add="suppress"))
    must("owner in suppress lifts it", a.protect("Project:Oversight", "read=all"))
    must("the self-grant is in the public rights log",
         anon.api(action="query", list="logevents", letype="rights", lelimit=50), "suppress")
    must("owner leaves suppress", a.userrights(user="Alice", remove="suppress"))


def hidden_names(x):
    """A hidden user's name reaches only `suppress`."""
    a, b, c, anon = x.alice, x.bob, x.carol, x.anon
    deny("sysop without hideuser hides a name", b.post(action="block", user="Mallory", hidename=1, token=b.token()))
    # A topic on Mallory's own talk page that mentions Carol: once the name is hidden, the
    # notification must not carry it, by agent or by talk page.
    st, r = x.mallory.rest("POST", "/thread", {"target": {"kind": "actor", "name": "Mallory"}, "subject": "Note 6060",
                                               "text": "See [[User:Carol]].", "token": x.mallory.token()})
    must("Mallory mentions Carol on her own talk page", (st, r))
    note, note_title = r.get("thread"), r.get("title")
    must("Carol is notified", c.api(action="query", meta="notifications", notprop="list", notlimit=50), "Note 6060")
    must("owner joins suppress", a.userrights(user="Alice", add="suppress"))
    must("owner hides Mallory", a.post(action="block", user="Mallory", hidename=1, token=a.token()))
    # Routes whose request does not carry the name; a request echo is not a leak.
    for name, probe in [
        ("recentchanges", lambda c: c.api(action="query", list="recentchanges", rcprop="user|title", rclimit=500)),
        ("logevents", lambda c: c.api(action="query", list="logevents", lelimit=500)),
        ("revisions user", lambda c: c.api(action="query", prop="revisions", rvprop="user", titles="Project:Departmental")),
        ("site log", lambda c: c.raw("/wiki/Special:Log")),
        ("site recentchanges", lambda c: c.raw("/wiki/Special:RecentChanges")),
        ("REST activity", lambda c: c.rest("GET", "/activity?limit=500")),
        ("usergroups", lambda c: c.api(action="query", meta="siteinfo", siprop="usergroups")),
        ("allusers", lambda c: c.api(action="query", list="allusers", aulimit=500)),
        ("notifications", lambda c: c.api(action="query", meta="notifications", notprop="list", notlimit=50)),
        ("site notifications", lambda c: c.raw("/wiki/Special:Notifications")),
        ("a thread on her talk page", lambda c: c.rest("GET", f"/thread/{note}")),
        ("a thread on her talk page, rendered", lambda c: c.rest("GET", f"/thread/{note}?html=1")),
        ("a thread on her talk page, on the site", lambda c: c.raw(f"/wiki/Thread:{note_title}")),
    ]:
        for who in (anon, c):
            absent(f"hidden name via {name} as {who.name}", probe(who), ["Mallory"])
    for name, probe in [
        ("usercontribs", lambda c: c.api(action="query", list="usercontribs", ucuser="Mallory", ucprop="title")),
        ("site contributions", lambda c: c.raw("/wiki/Special:Contributions/Mallory")),
    ]:
        for who in (anon, c):
            absent(f"hidden user's edits via {name} as {who.name}", probe(who), ["Scratchpad"])
    must("suppress still sees the name",
         a.api(action="query", list="usercontribs", ucuser="Mallory", ucprop="title"), "Scratchpad")
    must("owner unblocks, which unhides", a.post(action="unblock", user="Mallory", token=a.token()))
    must("owner leaves suppress", a.userrights(user="Alice", remove="suppress"))
    must("the name is back for everyone",
         anon.api(action="query", list="usercontribs", ucuser="Mallory", ucprop="title"), "Scratchpad")
    must("and may be acted on by name", a.userrights(user="Mallory", add="peek"))
    must("owner removes Mallory from peek", a.userrights(user="Mallory", remove="peek"))


def tokens(x):
    m = x.mallory
    deny("anon edits with the anonymous token",
         x.anon.post(action="edit", title="Project:AnonPage", text="x", token="+\\"))
    deny("userrights with a csrf token",
         m.post(action="userrights", user="Mallory", add="sysop", token=m.token("csrf")))
    r = m.s.get(f"{ARGS.api}/w/api.php", params={"action": "userrights", "user": "Mallory", "add": "sysop",
                                                 "token": m.token("userrights"), "format": "json"}).json()
    deny("userrights over GET", r)
    deny("edit with a stale token", m.post(action="edit", title="Project:X", text="x", token="deadbeef+\\"))


def private(x):
    a, anon = x.alice, x.anon
    must("owner makes the tenant private",
         a.rest("PUT", "/acl/tenant/librarybase", {"restrictions": {"read": {"group": "user"}}, "token": a.token()}))
    needles = ["Plain Item", "Public text", "Project:Open", "swordfish", "Hush"]
    for name, probe in [
        ("siteinfo", lambda c: c.api(action="query", meta="siteinfo", siprop="general|namespaces|statistics|usergroups|triplespace")),
        ("wbgetentities", lambda c: c.api(action="wbgetentities", ids=x.plain)),
        ("wbsearchentities", lambda c: c.api(action="wbsearchentities", search="Plain", language="en")),
        ("allpages", lambda c: c.api(action="query", list="allpages", aplimit=50)),
        ("recentchanges", lambda c: c.api(action="query", list="recentchanges", rclimit=50)),
        ("Special:EntityData", lambda c: c.raw(f"/wiki/Special:EntityData/{x.plain}.json")),
        # The request names the page: answered as missing, which echoes the title.
        ("site page", lambda c: c.raw("/wiki/Project:Open").replace("Project:Open", "")),
        ("site item", lambda c: c.raw(f"/wiki/Item:{x.plain}")),
        ("site recentchanges", lambda c: c.raw("/wiki/Special:RecentChanges")),
        ("site version", lambda c: c.raw("/wiki/Special:Version")),
        ("REST version", lambda c: c.rest("GET", "/version")),
        ("REST settings", lambda c: c.rest("GET", "/settings")),
        ("REST tenancy", lambda c: c.rest("GET", "/tenancy")),
        ("REST activity", lambda c: c.rest("GET", "/activity")),
        ("entity turtle", lambda c: c.raw(f"/entity/{x.plain}", headers={"Accept": "text/turtle"})),
        ("parse", lambda c: c.api(action="parse", page="Project:Open", prop="text")),
        ("well-known", lambda c: c.raw("/.well-known/apibase")),
    ]:
        absent(f"private tenant via {name}", probe(anon), needles)
    must("a user reads the private tenant",
         x.carol.api(action="wbgetentities", ids=x.plain), "Plain Item")
    must("owner makes the tenant public", a.rest("DELETE", "/acl/tenant/librarybase", {"token": a.token()}))
    must("anon reads again", anon.api(action="wbgetentities", ids=x.plain), "Plain Item")


# ---------------------------------------------------------------------------------------
# Positive: should be able to, but can't.

def positive(x):
    a, b, c, anon = x.alice, x.bob, x.carol, x.anon
    dave = C().register("Dave", "dave-secret-1234")
    erin = C().register("Erin", "erin-secret-1234")
    frank = C().register("Frank", "frank-secret-1234")
    at = a.token()

    # Setup by the owner.
    must("owner defines hr", a.rest("PUT", "/group/hr", {"permissions": [], "token": at}))
    must("owner defines scoped, delegating hr-admins", a.rest("PUT", "/group/hr-admins", {
        "permissions": ["delete", "undelete", "protect", "edit", "createpage", "deleterevision"],
        "within": ["namespace:4"], "adds": ["hr"], "removes": ["hr"], "adds_self": ["hr"],
        "removes_self": ["hr"], "token": at}))
    must("owner grants hr-admins", a.userrights(user="Dave", add="hr-admins"))
    must("owner grants read-all", a.userrights(user="Frank", add="read-all"))
    must("owner grants with expiry", a.userrights(user="Erin", add="autopatrolled", expiry="2099-01-01T00:00:00Z"))
    must("owner reads a group", a.rest("GET", "/group/hr-admins"), "namespace:4")
    must("owner reads usergroups", a.api(action="query", meta="siteinfo", siprop="usergroups"), "hr-admins")
    must("anon reads usergroups", anon.api(action="query", meta="siteinfo", siprop="usergroups"), "hr-admins")

    # Content.
    must("owner edits", a.edit("Project:Handbook", "handbook v1"))
    must("owner edits a subpage", a.edit("Project:Handbook/Sub", "sub v1"))
    must("owner edits a public page", a.edit("Project:Public", "public"))
    thing = a.new_item("Thing")
    vault = a.new_item("Vaulted")
    must("user creates an item", erin.post(action="wbeditentity", new="item", token=erin.token(),
                                          data=json.dumps({"labels": {"en": {"language": "en", "value": "Erins"}}})))
    must("user edits an item", erin.post(action="wbsetlabel", id=thing, language="de", value="Ding", token=erin.token()))
    must("user edits a page", erin.edit("Project:Public", "public v2"))
    must("user creates a page", erin.edit("Project:ErinPage", "mine"))
    must("anon reads", anon.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), "public v2")

    # Confidential page: a member's full rights.
    must("board member restricts a page", c.protect("Project:Handbook", "read=board"))
    must("board member reads", c.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), "handbook v1")
    must("board member reads the subpage", c.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook/Sub"), "sub v1")
    must("board member edits", c.edit("Project:Handbook", "handbook v2"))
    must("board member edits the subpage", c.edit("Project:Handbook/Sub", "sub v2"))
    must("board member creates a subpage", c.edit("Project:Handbook/New", "new"))
    must("board member lists it", c.api(action="query", list="allpages", apnamespace=4, aplimit=500), "Handbook/New")
    must("board member sees it in recentchanges", c.api(action="query", list="recentchanges", rcprop="title", rclimit=500), "Handbook/New")
    must("board member sees own contributions", c.api(action="query", list="usercontribs", ucuser="Carol", uclimit=50), "Handbook")
    must("board member parses", c.api(action="parse", page="Project:Handbook", prop="text"), "handbook v2")
    must("board member compares", c.api(action="compare", fromtitle="Project:Handbook", totitle="Project:Public"))
    must("board member reads the ACL", c.rest("GET", f"/acl/page/{c.page_id('Project:Handbook')}"), "board")
    must("board member lists it as protected", c.api(action="query", list="protectedpages", prtype="read", prlimit=500), "Handbook")
    must("board member protects edit", c.protect("Project:Handbook", "edit=board"))
    must("board member sets an expiry", c.protect("Project:Handbook", "edit=board", expiry="2099-01-01T00:00:00Z"))
    must("board member lifts edit protection", c.protect("Project:Handbook", "edit=all"))
    must("read-all reads a confidential page", frank.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), "handbook v2")
    must("read-all lists it", frank.api(action="query", list="allpages", apnamespace=4, aplimit=500), "Handbook")
    must("owner reads it by includes=*", a.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), "handbook v2")
    must("owner lists it", a.api(action="query", list="allpages", apnamespace=4, aplimit=500), "Handbook")
    must("owner sees its log", a.api(action="query", list="logevents", letype="protect", lelimit=50), "Handbook")
    must("bureaucrat adds a sysop to board", c.userrights(user="Bob", add="board"))
    must("sysop in board deletes it", b.post(action="delete", title="Project:Handbook", token=b.token()))
    must("sysop in board sees the deletion logged", b.api(action="query", list="logevents", letype="delete", lelimit=50), "Handbook")
    must("sysop in board undeletes it", b.post(action="undelete", title="Project:Handbook", token=b.token()))
    must("board member reads it after undelete", c.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), "handbook v2")
    absent("still confidential after undelete", erin.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), ["handbook"])
    must("board member lifts confidentiality", c.protect("Project:Handbook", "read=all"))
    must("anon reads it then", anon.api(action="query", prop="revisions", rvprop="content", titles="Project:Handbook"), "handbook v2")
    must("bureaucrat removes the sysop from board", c.userrights(user="Bob", remove="board"))

    # Entity.
    must("board member restricts an entity", c.rest("PUT", f"/acl/entity/{vault}", {"restrictions": {"read": {"group": "board"}}, "token": c.token()}))
    must("board member reads the entity", c.api(action="wbgetentities", ids=vault), "Vaulted")
    must("board member edits the entity", c.post(action="wbsetlabel", id=vault, language="de", value="Tresor", token=c.token()))
    must("board member finds the entity", c.api(action="wbsearchentities", search="Vault", language="en"), vault)
    must("board member reads its history", c.rest("GET", f"/entity/{vault}/history"))
    must("board member reads its provenance", c.rest("GET", f"/entity/{vault}/provenance"), vault)
    must("board member reads its page info", c.api(action="query", prop="info", titles=f"Item:{vault}"), "lastrevid")
    must("owner reads the entity by includes", a.api(action="wbgetentities", ids=vault), "Vaulted")
    must("sysop in board deletes the entity", c.post(action="delete", title=f"Item:{vault}", token=c.token()))
    must("sysop in board undeletes the entity", c.post(action="undelete", title=f"Item:{vault}", token=c.token()))
    must("board member lifts the entity restriction", c.rest("DELETE", f"/acl/entity/{vault}", {"token": c.token()}))
    must("anon reads the entity then", anon.api(action="wbgetentities", ids=vault), "Vaulted")

    # Set.
    r = c.rest("POST", "/acl/set", {"name": "Board docs", "restrictions": {"read": {"group": "board"}},
                                    "members": [{"kind": "entity", "id": vault}], "token": c.token()})
    must("board member creates a set", r)
    set_id = r[1].get("id") if r[0] < 400 else None
    if set_id:
        must("board member adds a page member", c.rest("POST", f"/acl/set/{set_id}/members",
             {"kind": "page", "id": c.page_id("Project:Public"), "token": c.token()}))
        must("board member reads the set", c.rest("GET", f"/acl/set/{set_id}"), "Board docs")
        must("board member lists it", c.api(action="query", list="protectedsets"), "Board docs")
        must("board member reads a set member", c.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), "public v2")
        must("board member edits a set member", c.edit("Project:Public", "public v3"))
        absent("outsider cannot read a set member", erin.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), ["public v"])
        must("board member removes a member", c.rest("DELETE", f"/acl/set/{set_id}/members/entity/{vault}", {"token": c.token()}))
        must("board member retires the set", c.rest("DELETE", f"/acl/set/{set_id}", {"token": c.token()}))
        must("anon reads the page after", anon.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), "public v3")

    # Delegated and scoped administration.
    must("delegate adds to hr", dave.userrights(user="Erin", add="hr"))
    must("delegate removes from hr", dave.userrights(user="Erin", remove="hr"))
    must("delegate joins hr (adds_self)", dave.userrights(user="Dave", add="hr"))
    must("delegate leaves hr (removes_self)", dave.userrights(user="Dave", remove="hr"))
    must("delegate sees changeablegroups", dave.api(action="query", meta="userinfo", uiprop="changeablegroups"), "hr")
    must("scoped member sees ts-scopedrights", dave.api(action="query", meta="userinfo", uiprop="rights"), "ts-scopedrights")
    must("scoped admin edits in scope", dave.edit("Project:Dept", "dept"))
    must("scoped admin creates in scope", dave.edit("Project:Dept/Plan", "plan"))
    must("scoped admin protects in scope", dave.protect("Project:Dept", "edit=sysop"))
    must("scoped admin restricts in scope", dave.protect("Project:Dept", "read=hr-admins"))
    must("scoped admin reads it", dave.api(action="query", prop="revisions", rvprop="content", titles="Project:Dept"), "dept")
    must("scoped admin lifts it", dave.protect("Project:Dept", "read=all"))
    must("scoped admin deletes in scope", dave.post(action="delete", title="Project:Dept/Plan", token=dave.token()))
    must("scoped admin undeletes in scope", dave.post(action="undelete", title="Project:Dept/Plan", token=dave.token()))
    dept = dave.page_id("Project:Dept")
    must("scoped admin opens a thread in scope", dave.rest("POST", "/thread", {"target": {"kind": "page", "id": dept}, "subject": "Dept talk", "text": "hi", "token": dave.token()}))
    rev = dave.api(action="query", prop="revisions", rvprop="ids", titles="Project:Dept")["query"]["pages"][0]["revisions"][0]["revid"]
    must("scoped admin revdels in scope", dave.post(action="revisiondelete", type="revision", ids=str(rev), hide="comment", token=dave.token()), "Success")
    must("scoped admin writes a page ACL in scope", dave.rest("PUT", f"/acl/page/{dept}", {"restrictions": {"edit": {"group": "hr-admins"}}, "token": dave.token()}))

    # Bureaucrat, sysop, notifications.
    must("bureaucrat defines a group", c.rest("PUT", "/group/newbies", {"permissions": [], "token": c.token()}))
    must("bureaucrat defines an including group", c.rest("PUT", "/group/auditors", {"permissions": [], "includes": ["board"], "token": c.token()}))
    must("bureaucrat retires a group", c.rest("DELETE", "/group/newbies", {"token": c.token()}))
    must("bureaucrat lists empty groups", c.rest("GET", "/acl/empty-groups"))
    must("sysop blocks", b.post(action="block", user="Erin", expiry="2099-01-01T00:00:00Z", token=b.token()))
    must("sysop unblocks", b.post(action="unblock", user="Erin", token=b.token()))
    must("sysop blocks with nocreate", b.post(action="block", user="Erin", nocreate=1, token=b.token()))
    must("sysop reblocks", b.post(action="block", user="Erin", reblock=1, token=b.token()))
    logid = b.api(action="query", list="logevents", letype="block", lelimit=1)["query"]["logevents"][0]["logid"]
    must("sysop unblocks by id", b.post(action="unblock", id=logid, token=b.token()))
    must("user edits after unblock", erin.edit("Project:ErinPage", "mine again"))
    must("bureaucrat removes a group", c.userrights(user="Erin", remove="autopatrolled"))
    must("user reads own notifications", erin.api(action="query", meta="notifications", notprop="list|count"), "user-rights")
    must("user marks read", erin.post(action="echomarkread", all=1, token=erin.token()))
    must("user marks seen", erin.post(action="echomarkseen", type="all", token=erin.token()))

    # The owner with suppress; a private tenant for its users.
    must("owner joins suppress", a.userrights(user="Alice", add="suppress"))
    must("owner hides a user", a.post(action="block", user="Erin", hidename=1, token=at))
    must("suppress sees the hidden user's contributions", a.api(action="query", list="usercontribs", ucuser="Erin", uclimit=50), "ErinPage")
    must("suppress sees the name in recentchanges", a.api(action="query", list="recentchanges", rcprop="user|title", rclimit=500), "\"Erin\"")
    must("owner unblocks the hidden user by name", a.post(action="unblock", user="Erin", token=at))
    must("owner suppresses a revision", a.post(action="revisiondelete", type="revision", ids=str(rev), hide="content", suppress="yes", token=at), "Success")
    must("owner unsuppresses it", a.post(action="revisiondelete", type="revision", ids=str(rev), show="content|comment", suppress="no", token=at), "Success")
    must("owner makes the tenant private", a.rest("PUT", "/acl/tenant/librarybase", {"restrictions": {"read": {"group": "user"}}, "token": at}))
    must("user reads the private tenant", erin.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), "public v3")
    must("user edits the private tenant", erin.edit("Project:ErinPage", "private edit"))
    must("user edits an entity on it", erin.post(action="wbsetlabel", id=thing, language="fr", value="Chose", token=erin.token()))
    must("user searches it", erin.api(action="wbsearchentities", search="Thing", language="en"), thing)
    must("user reads full siteinfo", erin.api(action="query", meta="siteinfo", siprop="general|namespaces"), "namespaces")
    must("login on the private tenant", C().login("Carol", "carol-secret-1234").api(action="query", meta="userinfo"), "Carol")
    must("registration on the private tenant", C().register("Grace", "grace-secret-1234").api(action="query", meta="userinfo"), "Grace")
    must("owner makes the tenant public", a.rest("DELETE", "/acl/tenant/librarybase", {"token": at}))
    must("owner leaves suppress", a.userrights(user="Alice", remove="suppress"))

    if not ARGS.cli:
        print("-- bot and CLI: skipped (no --cli)")
        return
    # A bot with grants.
    cli("subsidiary", "create", "--tenant", ARGS.tenant, "--name", "CarolBot", "--operator", "Carol", "--group", "bot", check=True)
    key = json.loads(cli("subsidiary", "key", "--tenant", ARGS.tenant, "--name", "CarolBot", "--label", "x",
                         "--grant", "editentity", "--grant", "createeditmovepage", "--json", check=True).stdout)
    bot = C("CarolBot")
    bot.s.headers["Authorization"] = f"Bearer {key['bearer']}"
    must("bot edits an entity", bot.post(action="wbsetlabel", id=thing, language="es", value="Cosa", token=bot.token()))
    must("bot edits a page", bot.post(action="edit", title="Project:BotPage", text="bot", token=bot.token()))
    must("bureaucrat adds the bot to board", c.userrights(user="CarolBot", add="board"))
    must("board member restricts a page", c.protect("Project:Public", "read=board"))
    must("bot in board reads it", bot.api(action="query", prop="revisions", rvprop="content", titles="Project:Public"), "public v3")
    must("bot in board edits it", bot.post(action="edit", title="Project:Public", text="bot v4", token=bot.token()))
    must("board member lifts it", c.protect("Project:Public", "read=all"))

    # The CLI's rights commands.
    t = ("--tenant", ARGS.tenant)
    must("CLI appoints an owner", cli_result(cli("member", "add", *t, "--user", "Erin", "--group", "owner", "--reason", "cli")))
    must("the new owner holds owner", erin.api(action="query", meta="userinfo", uiprop="groups"), "owner")
    must("CLI sets a namespace ACL", cli_result(cli("acl", "set", *t, "namespace:4", '{"edit": {"group": "sysop"}}')))
    must("CLI shows it", {} if "sysop" in cli("acl", "show", *t, "namespace:4").stdout else {"error": "not shown"})
    must("sysop edits under the namespace ACL", b.edit("Project:Public", "sysop v5"))
    deny("user cannot edit under the namespace ACL", C().login("Grace", "grace-secret-1234").edit("Project:Public", "x"))
    must("CLI unsets it", cli_result(cli("acl", "unset", *t, "namespace:4")))
    must("CLI defines a group", cli_result(cli("group", "set", *t, "ops", '{"permissions": ["delete"], "notify_joins": true}')))
    must("CLI shows it", {} if "delete" in cli("group", "show", *t, "ops").stdout else {"error": "not shown"})
    must("CLI removes the owner", cli_result(cli("member", "remove", *t, "--user", "Erin", "--group", "owner")))
    absent("the former owner no longer holds owner", erin.api(action="query", meta="userinfo", uiprop="groups"), ["\"owner\""])


SECTIONS = [
    ("leaks", leaks), ("oracles", oracles), ("escalation", escalation), ("delegation", delegation),
    ("includes", includes), ("threads", threads), ("discussions", discussions), ("watching", watching),
    ("deletion", deletion),
    ("moves", moves),
    ("bearer", bearer),
    ("owner", owner), ("hidden-names", hidden_names), ("tokens", tokens), ("private", private),
    ("positive", positive),
]


def main():
    global ARGS  # noqa: PLW0603
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--api", default="http://127.0.0.1:18290", help="server base URL")
    p.add_argument("--host", help="Host header, for a server without --dev-tenant")
    p.add_argument("--tenant", default="librarybase")
    p.add_argument("--owner-password", help="Alice's password (or --owner-password-file)")
    p.add_argument("--owner-password-file")
    p.add_argument("--cli", help="the triplespace binary, for registration challenges, bots and the CLI checks")
    p.add_argument("--db", help="TRIPLESPACE_DATABASE_URL for the CLI")
    p.add_argument("--sections", help="comma-separated subset; `--list` names them")
    p.add_argument("--list", action="store_true")
    ARGS = p.parse_args()
    if ARGS.list:
        print("\n".join(n for n, _ in SECTIONS))
        return 0
    if ARGS.owner_password_file:
        with open(ARGS.owner_password_file, encoding="utf-8") as f:
            ARGS.owner_password = f.read().strip()
    if not ARGS.owner_password:
        p.error("--owner-password or --owner-password-file")
    wanted = set(ARGS.sections.split(",")) if ARGS.sections else {n for n, _ in SECTIONS}
    unknown = wanted - {n for n, _ in SECTIONS}
    if unknown:
        p.error(f"unknown sections: {', '.join(sorted(unknown))}")

    cast = Cast()
    for name, section in SECTIONS:
        if name not in wanted:
            continue
        print(f"\n== {name}")
        try:
            section(cast)
        except Exception as e:  # noqa: BLE001 — a section that crashes is a finding too
            line = e.__traceback__
            while line.tb_next:
                line = line.tb_next
            finding(f"CRASH: {name}", f"{type(e).__name__}: {e} (line {line.tb_lineno})")

    print(f"\n== {CHECKS[0]} checks, {len(FINDINGS)} findings")
    for title, _ in FINDINGS:
        print(f"- {title}")
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    sys.exit(main())
