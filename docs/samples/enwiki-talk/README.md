# `scatter-talk` test fixtures

Talk pages from Wikipedia, fetched by `tools/talk_dumps.py` from `https://en.wikipedia.org/w/api.php` and converted by
`tools/talk_fixtures.py`, for the golden tests in `crates/scatter-talk/tests/samples.rs` (ADR 0090 §13).

**The text is by Wikipedia's contributors** and is available under the
[Creative Commons Attribution-ShareAlike 4.0 License](https://creativecommons.org/licenses/by-sa/4.0/)
(older revisions also under the GFDL). The authors of each page are listed in its history,
linked below; the full-history dump also names the author of every revision. The DiscussionTools
responses are the extension's reading of the same revisions. The fixtures are test data: they
are read by the tests at run time and are not compiled into any crate.

**Changes:** the dumps are recompressed with bzip2, and the DiscussionTools responses have their
rendered `html` fields removed. Nothing else is altered.

| Page | Revision | Revisions in the dump | History |
|---|---|---|---|
| Talk:Barack Obama/Archive 1 | [1060719815](https://en.wikipedia.org/w/index.php?oldid=1060719815) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Talk:Barack_Obama/Archive_1&action=history) |
| Talk:Barack Obama/Archive 84 | [1334286385](https://en.wikipedia.org/w/index.php?oldid=1334286385) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Talk:Barack_Obama/Archive_84&action=history) |
| Talk:Gdańsk/Vote | [1089047270](https://en.wikipedia.org/w/index.php?oldid=1089047270) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Talk:Gda%C5%84sk/Vote&action=history) |
| Talk:Jesus/Archive 1 | [1037710864](https://en.wikipedia.org/w/index.php?oldid=1037710864) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Talk:Jesus/Archive_1&action=history) |
| Talk:Jesus/Archive 139 | [1379906783](https://en.wikipedia.org/w/index.php?oldid=1379906783) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Talk:Jesus/Archive_139&action=history) |
| User talk:Jimbo Wales/Archive 256 | [1379920767](https://en.wikipedia.org/w/index.php?oldid=1379920767) | 1 | [history](https://en.wikipedia.org/w/index.php?title=User_talk:Jimbo_Wales/Archive_256&action=history) |
| Wikipedia:Administrators' noticeboard/Incidents | [1381262876](https://en.wikipedia.org/w/index.php?oldid=1381262876) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Wikipedia:Administrators%27_noticeboard/Incidents&action=history) |
| Wikipedia:Reference desk/Science | [1381256424](https://en.wikipedia.org/w/index.php?oldid=1381256424) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Wikipedia:Reference_desk/Science&action=history) |
| Wikipedia:Requests for adminship/Vermont | [1379264812](https://en.wikipedia.org/w/index.php?oldid=1379264812) | 1 | [history](https://en.wikipedia.org/w/index.php?title=Wikipedia:Requests_for_adminship/Vermont&action=history) |
| User talk:Harej | [1380056661](https://en.wikipedia.org/w/index.php?oldid=1380056661) | 4677 | [history](https://en.wikipedia.org/w/index.php?title=User_talk:Harej&action=history) |
