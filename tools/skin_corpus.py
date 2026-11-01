#!/usr/bin/env python3
"""Seeds a test instance with the states the site's pages draw, then records the API
exchanges of every page as three viewers, for the skin migration's parity corpus
(`crates/triplespace-ui/tests/corpus/`; 19 §5.10). `tools/skin_corpus.sh` brings the
instance up and runs this.

The seed is real activity through the API and the site's own forms, on the server's test
fixtures (the e2e fixture entities, Librarybase's own wikitext pages, the internetdomains
sample): accounts registered, pages edited, moved and deleted, a redirect, a markdown page,
threads opened and answered, watches with notifications, a protection, a block, a group
granted, a reader's preferences (German, dark mode). What a real wiki holds after a
week, not a day.

Three viewers: anonymous; Bob, a user reading in German with dark mode; Alice, the owner.
Each gets one recording (`exchanges/<set>.json`) and its cases are written to
`cases.recorded.json`, which the harness reads beside `cases.json`.
"""

import argparse
import html
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path
from urllib.parse import quote

import requests

ARGS = None


def cli(*args, check=True):
    env = dict(os.environ)
    env["TRIPLESPACE_DATABASE_URL"] = ARGS.db
    r = subprocess.run([ARGS.cli, *args], env=env, capture_output=True, text=True, check=False)
    if check and r.returncode != 0:
        raise RuntimeError(f"{' '.join(args)}: {r.stderr}")
    return r.stdout


class C:
    """A client: one session, optionally logged in."""

    def __init__(self, name="anon"):
        self.s = requests.Session()
        self.s.headers["Host"] = ARGS.host
        self.name = name

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

    def create(self, name, pw, **more):
        """`createaccount`, solving the challenge where one is asked; the API's answer."""
        v = self.api(action="query", meta="authmanagerinfo", amirequestsfor="create",
                     amimergerequestfields=1)
        extra = dict(more)
        fields = v["query"]["authmanagerinfo"].get("fields", {})
        if "tschallengeid" in fields:
            cid = fields["tschallengeid"]["value"]
            sol = cli("challenge", "solve", cid).strip()
            extra.update({"tschallengeid": cid, "tschallengesolution": sol})
        t = self.token("createaccount")
        return self.post(action="createaccount", username=name, password=pw, retype=pw,
                         createtoken=t, createreturnurl=f"{ARGS.api}/", **extra)

    def register(self, name, pw):
        r = self.create(name, pw)
        if r.get("createaccount", {}).get("status") != "PASS":
            raise RuntimeError(f"register {name}: {r}")
        return self.login(name, pw)

    def request(self, name, pw, statement):
        """Files an account request where the wiki reviews new accounts (0087 §6)."""
        r = self.create(name, pw, tsstatement=statement)
        if r.get("createaccount", {}).get("messagecode") != "ts-account-requested":
            raise RuntimeError(f"request {name}: {r}")
        return self

    def cookie(self):
        return "; ".join(f"{k}={v}" for k, v in self.s.cookies.items())

    def edit(self, title, text, **kw):
        return must(f"edit {title}", self.post(action="edit", title=title, text=text, token=self.token(), **kw))

    def page_id(self, title):
        r = self.api(action="query", prop="info", titles=title)
        return r["query"]["pages"][0]["pageid"]

    def form(self, title, action, fields):
        """Posts one of the site's forms, as a browser would."""
        data = {"wpEditToken": self.token(), **fields}
        return self.s.post(f"{ARGS.api}/w/index.php", params={"title": title, "action": action},
                           data=data, allow_redirects=False)


def must(what, r):
    if isinstance(r, tuple):
        st, body = r
        if st >= 400:
            raise RuntimeError(f"{what}: {st} {body}")
        return body
    if "error" in r:
        raise RuntimeError(f"{what}: {r['error']}")
    return r


def seed():
    """The states the pages draw. Returns what the paths need: revision IDs, thread IDs."""
    alice = C().login("Alice", ARGS.owner_password)
    bob = C().register("Bob", "bob-secret-1234")
    carol = C().register("Carol", "carol-secret-1234")
    facts = {}

    # Pages: a project page edited twice (history, diff), a sandbox, a markdown page, a
    # redirect, a page moved, a page deleted.
    home = alice.edit("Project:Home", "Welcome to '''Librarybase''', a catalogue of sources.\n\n== About ==\nSee [[Project:About]].")
    alice.edit("Project:Home", "Welcome to '''Librarybase''', a catalogue of sources and the people who cite them.\n\n== About ==\nSee [[Project:About]] and [[Project:Analytics]].\n\n== Lists ==\n* [[Special:ListProperties|Properties]]\n* [[Special:RecentChanges|Recent changes]]")
    bob.edit("Project:Sandbox", "Testing, testing. [[Item:Q6]] is a number; {{PAGENAME}}.\n\n[[Category:Tests]] [[Category:Drafts]]")
    alice.edit("Project:Notes", "# Notes\n\nA *markdown* page with a [link](Project:Home) and a list:\n\n- one\n- two\n", contentmodel="markdown")
    alice.edit("Project:Shortcut", "#REDIRECT [[Project:Home]]")
    # For the reports (group 8): red links and a wanted category, a redirect to a page
    # that does not exist, a redirect to a redirect, a category page in no category and
    # one with no members; and, since the index-backed reports (ShortPages, LongPages,
    # MostRevisions, FewestRevisions, AncientPages, DeadendPages, LonelyPages,
    # UncategorizedPages, MostCategories) read the main namespace only, articles: one long,
    # edited twice and in two categories, a short dead end it links to, and an orphan.
    alice.edit("Project:Wishlist", "Pages we want: [[Project:Style guide]], [[Project:Citing sources]] and "
               "[[Project:Style guide#Dates|dates]].\n\n[[Category:Wishlists]] [[Category:Guides]]")
    alice.edit("Citing sources", "How to cite a source in Librarybase.\n\nSee [[Stub]].\n\n[[Category:Guides]]")
    bob.edit("Citing sources", "How to cite a source in Librarybase: cite the edition you held, with its "
             "publisher and year, and the page.\n\nSee [[Stub]] and [[Item:Q3]].\n\n[[Category:Guides]] [[Category:Help]]")
    bob.edit("Stub", "Short.")
    alice.edit("Orphan", "Nobody links here. See [[Citing sources]].")
    alice.edit("Category:Guides", "Guides to the catalogue.")
    alice.edit("Category:Unused", "A category with no members.")
    alice.edit("Project:Old name", "#REDIRECT [[Project:Never written]]")
    alice.edit("Project:Shortcut/Again", "#REDIRECT [[Project:Shortcut]]")
    must("move", alice.post(action="move", **{"from": "Project:Analytics/Orb", "to": "Project:Analytics/Orbit",
                                              "reason": "Renamed upstream", "token": alice.token()}))
    must("delete", alice.post(action="delete", title="Project:Consensus", reason="Superseded", token=alice.token()))
    facts["home_revs"] = [r["revid"] for r in alice.api(action="query", prop="revisions", titles="Project:Home",
                                                        rvprop="ids", rvlimit=5)["query"]["pages"][0]["revisions"]]

    # Entities: Bob labels an item (a notification for its watcher), Alice adds a statement.
    must("watch", alice.post(action="watch", titles="Item:Q6", tsnotify="1", token=alice.token("watch")))
    must("watch", bob.post(action="watch", titles="Project:Home|Item:Q5", token=bob.token("watch")))
    must("label", bob.post(action="wbsetlabel", id="Q6", language="de", value="Sechs und mehr", token=bob.token()))
    must("description", bob.post(action="wbsetdescription", id="Q6", language="de", value="eine Zahl", token=bob.token()))
    must("claim", alice.post(action="wbcreateclaim", entity="Q6", property="P2", snaktype="value",
                             value=json.dumps({"entity-type": "item", "id": "Q5"}), token=alice.token()))
    must("claim", alice.post(action="wbcreateclaim", entity="Q5", property="P3", snaktype="value",
                             value=json.dumps({"amount": "+3898747", "unit": "1"}), token=alice.token()))
    # A wider edit on Q6, for the diff page: a value changed with its qualifiers and
    # references kept, a statement removed, one added with a rank and a reference, the
    # aliases and a label changed.
    census = {"snaktype": "value", "property": "P3", "datatype": "quantity",
              "datavalue": {"value": {"amount": "+3898900", "unit": "https://librarybase.org/entity/Q5",
                                      "upperBound": "+3898910", "lowerBound": "+3898890"}, "type": "quantity"}}
    when = {"snaktype": "value", "property": "P9", "datatype": "time",
            "datavalue": {"value": {"time": "+2024-00-00T00:00:00Z", "timezone": 0, "before": 0, "after": 0,
                                    "precision": 9, "calendarmodel": "http://www.wikidata.org/entity/Q1985727"},
                          "type": "time"}}
    cite = {"snaks": {"P11": [{"snaktype": "value", "property": "P11", "datatype": "url",
                               "datavalue": {"value": "https://example.org/census/2024", "type": "string"}}]},
            "snaks-order": ["P11"]}
    must("edit", alice.post(action="wbeditentity", id="Q6", summary="Census update", token=alice.token(), data=json.dumps({
        "labels": {"de": {"language": "de", "value": "Sechs"}},
        "aliases": {"en": [{"language": "en", "value": "6"}, {"language": "en", "value": "VI"}]},
        "claims": [
            {"id": "Q6$00000005-0000-0000-0000-000000000000", "type": "statement", "rank": "normal",
             "mainsnak": census, "qualifiers": {"P9": [when]}, "qualifiers-order": ["P9"], "references": [cite]},
            {"id": "Q6$00000006-0000-0000-0000-000000000000", "remove": ""},
            {"type": "statement", "rank": "preferred", "references": [cite],
             "mainsnak": {"snaktype": "value", "property": "P8", "datatype": "monolingualtext",
                          "datavalue": {"value": {"text": "sechs", "language": "de"}, "type": "monolingualtext"}}},
        ]})))
    facts["q6_revs"] = [r["revid"] for r in alice.api(action="query", prop="revisions", titles="Item:Q6",
                                                      rvprop="ids", rvlimit=10)["query"]["pages"][0]["revisions"]]
    facts["q5_rev"] = alice.api(action="query", prop="revisions", titles="Item:Q5", rvprop="ids",
                                rvlimit=1)["query"]["pages"][0]["revisions"][0]["revid"]
    st, rows = alice.rest("GET", "/entity/domain:wikipedia.org/history")
    sync = next(r["record"] for r in must("history", (st, rows))["rows"] if r["kind"] == "sync")
    facts["sync"] = f'{sync["partition"]}:{sync["offset"]}'

    # Threads: a topic on an item's talk page with a reply, a topic on the project talk page.
    q5 = alice.page_id("Item:Q5")
    st, r = alice.rest("POST", "/thread", {"target": {"kind": "page", "id": q5}, "subject": "Is five a city?",
                                          "text": "The label says so, the sources do not. --~~~~", "token": alice.token()})
    t1 = must("thread", (st, r))["title"]
    t1_id = r["thread"]
    r1 = must("reply", bob.rest("POST", f"/thread/{t1_id}/post", {"text": "Checked: it is not. ~~~~", "token": bob.token()}))
    r2 = must("reply", alice.rest("POST", f"/thread/{t1_id}/post", {"text": "Then let us fix the label.", "token": alice.token()}))
    # A reply to a reply, one that sets a status, a range collapsed under a hat, and a
    # reply nested past the depth the page indents.
    r3 = must("reply", bob.rest("POST", f"/thread/{t1_id}/post",
                                {"text": "Agreed; I will do it.", "inReplyTo": r2["revid"], "token": bob.token()}))
    must("reply", alice.rest("POST", f"/thread/{t1_id}/post",
                             {"text": "Marking this as in progress.", "status": "in-progress", "token": alice.token()}))
    r5 = must("reply", carol.rest("POST", f"/thread/{t1_id}/post",
                                  {"text": "What about seven?", "inReplyTo": r3["revid"], "token": carol.token()}))
    must("reply", alice.rest("POST", f"/thread/{t1_id}/post",
                             {"text": "Off topic: sevens are another thread.", "collapse": {"first": r5["revid"], "last": r5["revid"]},
                              "token": alice.token()}))
    deep = r3["revid"]
    for i in range(7):
        who = (bob, alice)[i % 2]
        deep = must("reply", who.rest("POST", f"/thread/{t1_id}/post",
                                      {"text": f"Nested reply {i + 1}.", "inReplyTo": deep, "token": who.token()}))["revid"]
    facts["reply_to"] = r1["revid"]
    home_id = alice.page_id("Project:Home")
    st, r = bob.rest("POST", "/thread", {"target": {"kind": "page", "id": home_id}, "subject": "Welcome text",
                                        "text": "Could the welcome mention how to cite a source?", "token": bob.token()})
    t2 = must("thread", (st, r))["title"]
    t2_id = r["thread"]
    must("reply", alice.rest("POST", f"/thread/{t2_id}/post",
                             {"text": "Done, see the new paragraph.", "status": "resolved", "token": alice.token()}))
    st, r = alice.rest("POST", "/thread", {"target": {"kind": "page", "id": home_id}, "subject": "House style",
                                          "text": "Cite the edition you held.", "token": alice.token()})
    must("thread", (st, r))
    facts["threads"] = [t1 if t1.startswith("Thread:") else f"Thread:{t1}",
                        t2 if t2.startswith("Thread:") else f"Thread:{t2}",
                        "Thread:2025-08-31/Welcome to Librarybase!"]

    # Moderation of records: Carol's sandbox edit has its user and comment hidden, and a
    # second one is erased, so the feeds show a hidden actor and an erased row.
    carol_edit = must("edit", carol.post(action="edit", title="Project:Sandbox", summary="Rude words",
                                         text="Testing, testing. [[Item:Q6]] is a number.\n\nSpam link.", token=carol.token()))
    carol_rev = carol_edit["edit"]["newrevid"]
    carol_edit2 = must("edit", carol.post(action="edit", title="Project:Sandbox", summary="More",
                                          text="Testing, testing. [[Item:Q6]] is a number.", token=carol.token()))
    must("hide", alice.post(action="revisiondelete", type="revision", ids=str(carol_rev), hide="user|comment",
                            reason="Harassment", token=alice.token()))
    must("erase", alice.post(action="revisiondelete", type="revision", ids=str(carol_edit2["edit"]["newrevid"]),
                             hide="content|user|comment", suppress="yes", reason="Private data", token=alice.token()))
    # Moderation: a protection, a block, a group.
    must("protect", alice.post(action="protect", title="Item:Q6", protections="edit=sysop|move=sysop",
                               reason="Vandalism", token=alice.token()))
    must("block", alice.post(action="block", user="Carol", reason="Spam", expiry="1 week", token=alice.token()))
    must("rights", alice.post(action="userrights", user="Bob", add="autopatrolled", reason="Trusted",
                              token=alice.token("userrights")))

    # Bob reads in German, in dark mode, with more contrast.
    must("options", bob.post(action="options", change="language=de|ui.mode=dark|ui.contrast=more", token=bob.token()))
    # Alice looks at her notifications once, so the bell has seen some.
    must("seen", alice.post(action="echomarkseen", type="all", token=alice.token()))

    # Registration reviewed (group 9): Dave's request waits on Alice, with a statement.
    # Last, so that the accounts above were created at once.
    cli("settings", "set", "--tenant", "librarybase", "account.approval", "true")
    C().request("Dave", "dave-secret-1234", "I catalogue my library's sources.")

    st, r = alice.rest("GET", "/activity?limit=3&lang=en")
    facts["rc_next"] = must("activity", (st, r)).get("next")
    rc = alice.api(action="query", list="recentchanges", rcprop="ids|title", rclimit=50)["query"]["recentchanges"]
    facts["rc"] = [(c["title"], c.get("revid"), c.get("old_revid")) for c in rc if c.get("revid")]
    return {"anon": C(), "bob": bob, "alice": alice}, facts


def paths(facts, viewer):
    """The paths one viewer's recording holds, with each case's migration group."""
    r1, r0 = facts["home_revs"][0], facts["home_revs"][-1]
    q6 = facts["q6_revs"]
    t1, t2, t3 = facts["threads"]
    everyone = [
        # Group 1: the frame on pages that are only the frame.
        (1, "/wiki/Nothing_here"),
        (1, "/wiki/Project:Shortcut"),
        # Group 3: entities with the seeded states (a protection, a German label).
        (3, "/wiki/Item:Q6"),
        (3, "/wiki/Item:Q5"),
        (3, "/wiki/Property:P3"),
        (3, "/wiki/Domain:wikipedia.org"),
        (3, "/wiki/Item:Q6?tab=labels"),
        (3, "/wiki/Item:Q6?tab=identifiers"),
        (3, "/wiki/Item:Q6?action=render&region=terms"),
        (3, "/w/index.php?title=Item:Q6&action=render&region=statements/P3"),
        # Group 4: entity history and diffs.
        (4, "/wiki/Item:Q6?action=history"),
        (4, "/wiki/Item:Q5?action=history"),
        (4, "/wiki/Domain:wikipedia.org?action=history"),
        (4, f"/wiki/Item:Q6?diff={q6[0]}"),
        (4, f"/wiki/Item:Q6?diff={q6[0]}&view=changeset"),
        (4, f"/wiki/Item:Q6?diff={q6[1]}"),
        (4, f"/wiki/Item:Q6?oldid={q6[-1]}&diff={q6[0]}"),
        (4, f"/wiki/Item:Q6?oldid={q6[1]}"),
        (4, f"/wiki/Item:Q5?diff={facts['q5_rev']}"),
        (4, f"/wiki/Domain:wikipedia.org?diff={facts['sync']}"),
        (4, "/wiki/Domain:wikipedia.org?action=history&source=local"),
        # Group 5: document pages.
        (5, "/wiki/Project:Home"),
        (5, "/wiki/Project:About"),
        (5, "/wiki/Project:Notes"),
        (5, "/wiki/Project:Sandbox"),
        (5, "/wiki/Project:Analytics/Orbit"),
        (5, "/wiki/Project:Analytics/Orb"),
        (5, "/wiki/Project:Consensus"),
        (5, "/wiki/Project:Home?action=history"),
        (5, f"/wiki/Project:Home?oldid={r0}"),
        (5, f"/wiki/Project:Home?diff={r1}&oldid={r0}"),
        (5, "/wiki/Project:Home?action=edit"),
        (5, "/wiki/Project:Never_written?action=edit"),
        (5, "/wiki/Project:Home?action=info"),
        (5, "/wiki/Project:Home?action=render"),
        (5, "/wiki/User:Harej"),
        (5, "/wiki/Project:Shortcut?redirect=no"),
        (5, f"/wiki/Project:Home?diff={r1}&oldid={r0}&diffonly=1"),
        (5, "/wiki/Project:Home?diff=cur"),
        (5, f"/wiki/Project:Home?diff=prev&oldid={r0}"),
        (5, "/wiki/Project:Home?diff=x"),
        (5, "/wiki/Project:Home?oldid=1"),
        (5, "/wiki/Project:Notes?action=edit"),
        (5, "/w/index.php?action=edit"),
        (5, "/wiki/Project:Sandbox?action=render"),
        # Group 6: discussions.
        (6, "/wiki/Item_talk:Q5"),
        (6, "/wiki/Project_talk:Home"),
        (6, "/wiki/User_talk:Harej"),
        (6, "/wiki/Item_talk:Q6"),
        (6, f"/wiki/{quote(t1, safe='/:')}"),
        (6, f"/wiki/{quote(t2, safe='/:')}"),
        (6, f"/wiki/{quote(t3, safe='/:')}"),
        (6, "/wiki/Item_talk:Q5?action=addtopic"),
        (6, "/wiki/Item_talk:Q5?action=history"),
        (6, "/wiki/Item_talk:Q5?sort=activity"),
        (6, f"/wiki/{quote(t1, safe='/:')}?replyto={facts['reply_to']}"),
        (6, f"/wiki/{quote(t1, safe='/:')}?action=history"),
        (6, "/wiki/Project_talk:Home?action=addtopic"),
        (6, "/wiki/Item_talk:Q404"),
        # Group 7: changes.
        (7, "/wiki/Special:RecentChanges"),
        (7, "/wiki/Special:RecentChanges?filtered=1&hidebots=1&limit=10"),
        (7, "/wiki/Special:NewPages"),
        (7, "/wiki/Special:Contributions/Alice"),
        (7, "/wiki/Special:Contributions/Bob"),
        (7, "/wiki/Special:Contributions/Nobody"),
        (7, "/wiki/Special:Log"),
        (7, "/wiki/Special:Log?type=block"),
        (7, "/wiki/Special:Log/Alice"),
        (7, "/wiki/Special:RecentChanges?filtered=1&kind=edit&kind=sync&kind=log&kind=job&bots=1"),
        (7, "/wiki/Special:RecentChanges?filtered=1&kind=edit&limit=50&namespace=120&days=7"),
        (7, "/wiki/Special:RecentChanges?limit=3"),
        (7, f"/wiki/Special:RecentChanges?limit=3&continue={quote(facts['rc_next'] or '', safe='')}"),
        (7, "/wiki/Special:Log/move"),
        (7, "/wiki/Special:Log?type=delete&page=Project:Consensus"),
        (7, "/wiki/Special:Log?page=Item:Q6"),
        (7, "/wiki/Special:Contributions?target=Carol"),
        (7, "/wiki/Special:Contributions"),
        (7, "/wiki/Special:NewPages?username=Bob&bots=1"),
        (7, "/wiki/Special:Watchlist"),
        (7, "/wiki/Special:Notifications"),
        # Group 8: search, indexes, reports.
        (8, "/wiki/Special:Search"),
        (8, "/w/index.php?title=Special:Search&search=Librarybase&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=six"),
        # 8a: no hits, several groups, one group, its pages, an alias, an ID (Go to, and
        # the 302 the header's box gets), an ID that names nothing, a domain, right to left.
        (8, "/w/index.php?title=Special:Search&search=zzzz&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=zzzz&fulltext=1&ns210=1"),
        (8, "/w/index.php?title=Special:Search&search=n&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=e&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=n&fulltext=1&ns122=1"),
        (8, "/w/index.php?title=Special:Search&search=n&fulltext=1&ns122=1&limit=1"),
        (8, "/w/index.php?title=Special:Search&search=n&fulltext=1&ns122=1&limit=1&offset=1"),
        (8, "/w/index.php?title=Special:Search&search=half&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=Q6&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=Q6"),
        (8, "/w/index.php?title=Special:Search&search=Q404&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=wikipedia.org&fulltext=1"),
        (8, "/w/index.php?title=Special:Search&search=six&fulltext=1&uselang=ar"),
        (8, "/wiki/Special:SpecialPages"),
        (8, "/wiki/Special:AllPages"),
        (8, "/wiki/Special:AllPages?namespace=4"),
        (8, "/wiki/Special:PrefixIndex/Project:Analytics/"),
        # 8b: each namespace that holds pages, an empty one, one that holds entities,
        # `from`, `to`, `hideredirects`; a prefix with `stripprefix`, one that matches nothing.
        (8, "/wiki/Special:AllPages?namespace=2"),
        (8, "/wiki/Special:AllPages?namespace=10"),
        (8, "/wiki/Special:AllPages?namespace=14"),
        (8, "/wiki/Special:AllPages?namespace=120"),
        (8, "/wiki/Special:AllPages?namespace=4&from=N&to=Sh"),
        (8, "/wiki/Special:AllPages?namespace=4&hideredirects=1"),
        (8, "/wiki/Special:AllPages/Project:S"),
        (8, "/wiki/Special:PrefixIndex?prefix=Analytics/&namespace=4&stripprefix=1"),
        (8, "/wiki/Special:PrefixIndex?prefix=Zz&namespace=4"),
        (8, "/wiki/Special:PrefixIndex"),
        (8, "/wiki/Special:ListRedirects"),
        (8, "/wiki/Special:ListProperties"),
        (8, "/wiki/Special:WantedPages"),
        (8, "/wiki/Special:ShortPages"),
        (8, "/wiki/Special:MostRevisions"),
        (8, "/wiki/Special:LonelyPages"),
        (8, "/wiki/Special:DoubleRedirects"),
        # 8c: every other report once, by MediaWiki's names, and paged.
        (8, "/wiki/Special:WantedCategories"),
        (8, "/wiki/Special:MostLinkedPages"),
        (8, "/wiki/Special:MostLinkedCategories"),
        (8, "/wiki/Special:MostCategories"),
        (8, "/wiki/Special:FewestRevisions"),
        (8, "/wiki/Special:DeadendPages"),
        (8, "/wiki/Special:UncategorizedPages"),
        (8, "/wiki/Special:UncategorizedCategories"),
        (8, "/wiki/Special:UncategorizedTemplates"),
        (8, "/wiki/Special:UnusedCategories"),
        (8, "/wiki/Special:LongPages"),
        (8, "/wiki/Special:AncientPages"),
        (8, "/wiki/Special:BrokenRedirects"),
        (8, "/wiki/Special:Wantedpages"),
        (8, "/wiki/Special:BrokenLinks"),
        (8, "/wiki/Special:Mostrevisions"),
        (8, "/w/index.php?title=Special:WantedPages&limit=5"),
        (8, "/w/index.php?title=Special:WantedPages&limit=5&offset=5"),
        (8, "/w/index.php?title=Special:WantedPages&offset=500"),
        (8, "/wiki/Special:ListProperties?uselang=ar"),
        # 8c: the redirecting pages' forms: no parameter, and one they cannot follow.
        (8, "/wiki/Special:EditPage"),
        (8, "/wiki/Special:Redirect"),
        (8, "/wiki/Special:PermanentLink"),
        (8, "/wiki/Special:PermanentLink/abc"),
        (8, "/wiki/Special:Diff/x"),
        (8, "/wiki/Special:EntityPage/not an id"),
        (8, "/wiki/Special:Redirect/file/3"),
        (8, "/wiki/Special:TalkPage"),
        (8, "/wiki/Special:MyContributions"),
        (8, "/wiki/Special:ProtectedPages"),
        (8, "/wiki/Special:ProtectedTitles"),
        (8, "/wiki/Special:BlockList"),
        # Group 9: accounts.
        (9, "/wiki/Special:UserLogin"),
        (9, "/wiki/Special:UserLogin?returnto=Item:Q6"),
        (9, "/wiki/Special:CreateAccount"),
        (9, "/wiki/Special:Preferences"),
        (9, "/wiki/Special:Account"),
        # 9b–9d: logging out, the review page to a reader who cannot review, a creation
        # form returning somewhere, and the Appearance menu without stylesheets.
        (9, "/wiki/Special:UserLogout"),
        (9, "/wiki/Special:UserLogout?returnto=Item:Q6"),
        (9, "/wiki/Special:ConfirmAccounts"),
        (9, "/wiki/Special:CreateAccount?returnto=Item:Q6"),
        (9, "/wiki/Project:Home?useskin=plain"),
        (9, "/wiki/Special:Preferences?useskin=plain"),
        # Group 10: settings.
        (10, "/wiki/Special:WikiSettings"),
        (10, "/wiki/Special:InstanceSettings"),
        (10, "/wiki/Special:Tenancy"),
        # Group 11: moderation.
        (11, "/wiki/Special:Block"),
        (11, "/wiki/Special:Block/Carol"),
        (11, "/wiki/Special:Unblock/Carol"),
        (11, "/wiki/Special:UserRights/Bob"),
        (11, "/wiki/Special:Undelete"),
        (11, "/wiki/Special:Undelete/Project:Consensus"),
        (11, "/wiki/Special:MovePage/Project:About"),
        (11, "/wiki/Item:Q6?action=protect"),
        (11, "/wiki/Project:About?action=delete"),
        (11, "/wiki/Project:About?action=protect"),
        # Group 12: the rest.
        (12, "/wiki/Special:Version"),
        (12, "/wiki/Special:NewItem"),
        (12, "/wiki/Special:NewProperty"),
        (12, "/wiki/Special:SetLabel/Q6"),
        (12, "/wiki/Special:SetDescription/Q6/de"),
        (12, "/wiki/Special:SetLabelDescriptionAliases/Q6"),
        (12, "/wiki/Special:AllMessages"),
        (12, "/wiki/Special:AllMessages?prefix=ts-watch"),
        (12, "/wiki/Special:Providers"),
        (12, "/wiki/Special:ComparePages"),
        (12, "/wiki/Special:ComparePages?page1=Project:Home&page2=Project:About"),
        (12, "/wiki/Special:ListDatatypes"),
        (12, "/wiki/Special:ListGroupRights"),
        (12, "/wiki/Special:NamespaceInfo"),
        (12, "/wiki/Special:MyLanguageFallbackChain"),
        (12, "/wiki/Special:BlankPage"),
        (12, "/wiki/Special:MyPage"),
        (12, "/wiki/Special:MyTalk"),
        (12, "/wiki/Special:EditPage/Project:Home"),
        (12, "/wiki/Special:Nonsense"),
        # Languages: right to left, and a fallback chain, on a page of each kind.
        (3, "/wiki/Item:Q6?uselang=ar"),
        (5, "/wiki/Project:Home?uselang=ar"),
        (7, "/wiki/Special:RecentChanges?uselang=ar"),
        (8, "/wiki/Special:SpecialPages?uselang=he"),
        (9, "/wiki/Special:UserLogin?uselang=ar"),
        (3, "/wiki/Item:Q6?uselang=pt-br"),
        (12, "/wiki/Special:Version?uselang=de"),
    ]
    signed_in = [
        (7, "/wiki/Special:Watchlist"),
        (7, "/wiki/Special:EditWatchlist"),
        (7, "/wiki/Special:Notifications"),
        (7, "/wiki/Special:Notifications?read=1"),
        (7, "/wiki/Special:Notifications?unread=1"),
        (9, "/wiki/Special:Preferences?tab=appearance"),
        (3, "/wiki/Item:Q6?action=render&region=terms"),
    ]
    owner = [
        (9, "/wiki/Special:ConfirmAccounts"),
        (10, "/wiki/Special:WikiSettings?tab=visibility"),
        (10, "/wiki/Special:InstanceSettings?tab=tenancy"),
    ]
    out = list(everyone)
    if viewer != "anon":
        out += signed_in
    if viewer == "alice":
        out += owner
    return out


def private_paths():
    """What a private tenant shows an outsider and a member (18 §2.2; 09 §4.7)."""
    return [
        (1, "/wiki/Project:Home"),
        (3, "/wiki/Item:Q6"),
        (5, "/wiki/Project:About"),
        (7, "/wiki/Special:RecentChanges"),
        (8, "/wiki/Special:SpecialPages"),
        (8, "/w/index.php?title=Special:Search&search=six"),
        (9, "/wiki/Special:UserLogin"),
        (9, "/wiki/Special:CreateAccount"),
        (12, "/wiki/Special:Version"),
        (1, "/wiki/Nothing_here"),
    ]


def record(clients, facts, out_dir):
    cases = []
    differing = []
    public = [("anon", "lb", paths(facts, "anon")), ("bob", "lb-bob", paths(facts, "bob")),
              ("alice", "lb-alice", paths(facts, "alice"))]
    # Then the tenant private: an outsider sees the login page and the wiki's name, a
    # member everything.
    private = [("anon", "lb-private", private_paths()), ("bob", "lb-private-bob", private_paths())]
    # Between them, Alice approves Dave's request: the review page with none open, and the
    # notice it shows after a decision (group 9).
    done = [("alice", "lb-alice-done", [(9, "/wiki/Special:ConfirmAccounts"),
                                         (9, "/wiki/Special:ConfirmAccounts?done=approved")])]

    def approve():
        alice = clients["alice"]
        reqs = must("requests", alice.rest("GET", "/account/requests"))["requests"]
        dave = next(r for r in reqs if r["name"] == "Dave")
        must("approve", alice.rest("POST", f"/account/requests/{dave['id']}/approve",
                                   {"token": alice.token()}))

    def make_private():
        alice = clients["alice"]
        must("private", alice.rest("PUT", "/acl/tenant/librarybase",
                                   {"restrictions": {"read": {"group": "user"}}, "token": alice.token()}))

    for i, (viewer, set_name, ps) in enumerate(public + done + private):
        if i == len(public):
            approve()
        if i == len(public) + len(done):
            make_private()
        c = clients[viewer]
        cmd = [ARGS.web, "record", "--api", ARGS.api, "--host", ARGS.host,
               "--out", str(out_dir / "exchanges" / f"{set_name}.json")]
        if viewer != "anon":
            cmd += ["--cookie", c.cookie()]
        # As a browser sends them: spaces and punctuation in a title percent-encoded.
        ps = [(g, quote(p, safe="/?=&:%+|")) for g, p in ps]
        cmd += [p for _, p in ps]
        r = subprocess.run(cmd, capture_output=True, text=True, check=False)
        if r.returncode != 0:
            raise RuntimeError(f"record {set_name}: {r.stderr}")
        # Under TRIPLESPACE_WEB_RENDERER=compare the recording is also the soak's crawl
        # (19 §5.10): every route whose renderings differ is a line of the web tier's log.
        for line in r.stderr.splitlines():
            if "compare:" in line:
                differing.append(f"{set_name}: {line}")
        statuses = {}
        for line in r.stderr.splitlines():
            m = re.match(r"(\d{3}) [A-Za-z ]+ (/\S+)$", line)
            if m:
                statuses[m.group(2)] = int(m.group(1))
        for group, p in ps:
            name = re.sub(r"[^a-z0-9]+", "-", p.lower().replace("/wiki/", "").replace("/w/index.php?title=", "")).strip("-")
            case = {"name": f"{set_name}-{name}"[:80], "set": set_name, "path": p, "group": group}
            if viewer != "anon":
                case["viewer"] = "user"
            status = statuses.get(p, 200)
            if status != 200:
                case["status"] = status
            cases.append(case)
        print(f"{set_name}: {len(ps)} pages recorded", file=sys.stderr)
    (out_dir / "cases.recorded.json").write_text(json.dumps({
        "_": ["Written by tools/skin_corpus.py from a seeded test instance; do not edit by hand.",
              "The sets are recordings under exchanges/: lb (anonymous), lb-bob (a user reading in",
              "German with dark mode), lb-alice (the owner), lb-alice-done (the owner after",
              "approving the request the seed files for review), lb-private and lb-private-bob",
              "(the tenant made private)."],
        "cases": cases,
    }, indent=1) + "\n")
    print(f"{len(cases)} cases written", file=sys.stderr)
    if os.environ.get("TRIPLESPACE_WEB_RENDERER") == "compare":
        for line in differing:
            print(line, file=sys.stderr)
        print(f"compare: {len(differing)} routes differ", file=sys.stderr)


def main():
    global ARGS
    p = argparse.ArgumentParser()
    p.add_argument("--api", required=True)
    p.add_argument("--host", default="librarybase.org")
    p.add_argument("--db", required=True)
    p.add_argument("--cli", required=True)
    p.add_argument("--web", required=True)
    p.add_argument("--owner-password-file", required=True)
    p.add_argument("--out", required=True, help="crates/triplespace-ui/tests/corpus")
    ARGS = p.parse_args()
    ARGS.owner_password = Path(ARGS.owner_password_file).read_text().strip()
    clients, facts = seed()
    time.sleep(1)
    record(clients, facts, Path(ARGS.out))


if __name__ == "__main__":
    main()
