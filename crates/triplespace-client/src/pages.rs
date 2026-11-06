//! Document pages through the Action API (0008; 19 §3.1–3.2): a page rendered by
//! `action=parse`, its source by `prop=revisions`, what it is by `prop=info`, a preview by
//! `action=parse` with `pst`, and a save by `action=edit` with the viewer's CSRF token.

use serde_json::Value;

use crate::api::{CacheInfo, Fetched, check};
use crate::transport::ClientError;
use crate::{Client, Incoming};

/// The query value of a title or other text.
fn q(text: &str) -> String {
    url::form_urlencoded::byte_serialize(text.as_bytes()).collect()
}

/// A category a page is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageCategory {
    /// The name, without the namespace, spaces for underscores.
    pub name: String,
    /// Whether the category is hidden (`__HIDDENCAT__`).
    pub hidden: bool,
}

/// A target's visibility, as `ts-visibility` reports it: the groups a reader must be in,
/// and the sets it is restricted through (19 §1.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Visibility {
    /// The groups.
    pub groups: Vec<String>,
    /// The sets' names.
    pub sets: Vec<String>,
}

impl Visibility {
    /// From the API's object; `None` where there is none (public).
    #[must_use]
    pub fn of(v: &Value) -> Option<Self> {
        let list = |k: &str| -> Vec<String> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        v.is_object().then(|| Self {
            groups: list("groups"),
            sets: list("sets"),
        })
    }
}

/// A page as `action=parse` renders it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The title, prefixed, as the API writes it.
    pub title: String,
    /// The page ID; 0 for text parsed under a title that has no page.
    pub page_id: u64,
    /// The revision rendered.
    pub revid: u64,
    /// The rendered HTML, sanitized by the API.
    pub html: String,
    /// The categories, in the order the page names them.
    pub categories: Vec<PageCategory>,
    /// The title a redirect was followed from, where one was.
    pub redirected_from: Option<String>,
    /// The templates the page transcludes, prefixed.
    pub templates: Vec<String>,
    /// The page's visibility, where not public (`ts-visibility`; 19 §1.3).
    pub visibility: Option<Visibility>,
}

fn parsed(v: &Value) -> Parsed {
    let p = &v["parse"];
    let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
    let list = |k: &str| p[k].as_array().cloned().unwrap_or_default();
    Parsed {
        title: s("title"),
        page_id: p["pageid"].as_u64().unwrap_or(0),
        revid: p["revid"].as_u64().unwrap_or(0),
        html: s("text"),
        categories: list("categories")
            .iter()
            .filter_map(|c| {
                Some(PageCategory {
                    name: c["category"].as_str()?.replace('_', " "),
                    hidden: c.get("hidden").is_some_and(|h| h != &Value::Bool(false)),
                })
            })
            .collect(),
        redirected_from: p["redirects"][0]["from"].as_str().map(str::to_string),
        visibility: Visibility::of(&p["ts-visibility"]),
        templates: list("templates")
            .iter()
            .filter_map(|t| t["title"].as_str().map(str::to_string))
            .collect(),
    }
}

/// A page's latest source, or one revision's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Source {
    /// The title, prefixed.
    pub title: String,
    /// The page ID.
    pub page_id: u64,
    /// The namespace.
    pub ns: i64,
    /// The revision.
    pub revid: u64,
    /// When it was made, ISO 8601: the edit form's `basetimestamp`.
    pub timestamp: String,
    /// The content model.
    pub model: String,
    /// The text; empty where it is withheld from the viewer.
    pub text: String,
}

/// What `prop=revisions` says of a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup<T> {
    /// The page, as asked for.
    Found(T),
    /// No page has the title; `ns` and `title` as the API normalized them.
    Missing {
        /// The namespace.
        ns: i64,
        /// The title, prefixed.
        title: String,
    },
    /// The title cannot name a page.
    Invalid,
}

/// A page named by a number rather than a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRef {
    /// A page ID.
    Page(u64),
    /// A revision ID: the page the revision belongs to.
    Revision(u64),
}

/// What `list=allpages` is asked (`Special:AllPages`, `Special:PrefixIndex`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageListQuery {
    /// `apnamespace`.
    pub namespace: i64,
    /// `apfrom`: the first title, without the namespace.
    pub from: String,
    /// `apto`: the last title, without the namespace.
    pub to: String,
    /// `apprefix`.
    pub prefix: String,
    /// `apfilterredir`: `redirects`, `nonredirects`, or all when empty.
    pub filter_redirects: String,
    /// `apdir=descending`.
    pub descending: bool,
    /// `aplimit`.
    pub limit: u32,
}

/// A page of `list=allpages`: the titles, prefixed as the API writes them, and the
/// title the next page starts at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageList {
    /// The titles.
    pub titles: Vec<String>,
    /// `apcontinue`.
    pub next: Option<String>,
}

/// A page of `list=backlinks`: the pages linking to a title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backlinks {
    /// Each linking page's title, prefixed, and whether it is a redirect.
    pub pages: Vec<(String, bool)>,
    /// `blcontinue`, where there are more.
    pub next: Option<String>,
}

/// What the **About this page** panel shows of a page (19 §3.1): `prop=info` with its
/// protection, and its first revision (`prop=revisions`, `rvdir=newer`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct About {
    /// The title, prefixed.
    pub title: String,
    /// The content model.
    pub model: String,
    /// The latest text's size in bytes.
    pub length: u64,
    /// When it last changed, ISO 8601.
    pub touched: Option<String>,
    /// The latest revision.
    pub lastrevid: u64,
    /// When the first revision was saved, ISO 8601.
    pub created: Option<String>,
    /// Who saved it; `None` where hidden.
    pub creator: Option<String>,
    /// Its protection, one entry per restricted action.
    pub protection: Vec<crate::rights::Restriction>,
}

/// What `prop=info` says of a page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageInfo {
    /// The title, prefixed.
    pub title: String,
    /// The page ID.
    pub page_id: u64,
    /// The namespace.
    pub ns: i64,
    /// The content model.
    pub model: String,
    /// Whether the page is a redirect.
    pub redirect: bool,
}

/// One side of a page diff.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffSide {
    /// The revision.
    pub revid: u64,
    /// The page's title, prefixed.
    pub title: String,
    /// When, ISO 8601.
    pub timestamp: String,
    /// Who; `None` where hidden.
    pub user: Option<String>,
    /// The summary; `None` where hidden.
    pub comment: Option<String>,
    /// The size in bytes.
    pub size: u64,
}

/// A diff between two revisions of a document page (`action=compare`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageDiff {
    /// The old side; `None` where the new side created the page.
    pub from: Option<DiffSide>,
    /// The new side.
    pub to: DiffSide,
    /// MediaWiki's table rows; empty where the two texts are equal.
    pub body: String,
    /// The same diff in MediaWiki's inline layout, one column, for a narrow screen.
    pub inline: String,
    /// The revision before the old side.
    pub prev: Option<u64>,
    /// The revision after the new side.
    pub next: Option<u64>,
}

/// Which two revisions to compare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffAsk {
    /// One revision against another: `fromrev`, `torev`.
    Revisions(u64, u64),
    /// A revision and its neighbour: `fromrev`, `torelative=prev|next|cur`.
    Relative(u64, &'static str),
    /// A page's latest revision against the one before it.
    Latest(String),
    /// Any two pages or revisions (`Special:ComparePages`): `fromtitle` or `fromrev`,
    /// `totitle` or `torev`.
    Between(DiffEnd, DiffEnd),
}

/// One side of [`DiffAsk::Between`]: a page's latest revision, or one revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffEnd {
    /// A page, by title: its latest revision.
    Title(String),
    /// A revision, by ID.
    Revision(u64),
}

impl DiffEnd {
    fn param(&self, side: &str) -> String {
        match self {
            Self::Title(t) => format!("{side}title={}", q(t)),
            Self::Revision(r) => format!("{side}rev={r}"),
        }
    }
}

fn diff_side(c: &Value, prefix: &str) -> Option<DiffSide> {
    let key = |k: &str| format!("{prefix}{k}");
    let revid = c[key("revid")].as_u64()?;
    Some(DiffSide {
        revid,
        title: c[key("title")].as_str().unwrap_or_default().to_string(),
        timestamp: c[key("timestamp")].as_str().unwrap_or_default().to_string(),
        user: c[key("user")].as_str().map(str::to_string),
        comment: c[key("comment")].as_str().map(str::to_string),
        size: c[key("size")].as_u64().unwrap_or(0),
    })
}

/// What a save did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Saved {
    /// A new revision.
    Saved {
        /// The title, prefixed.
        title: String,
        /// The revision.
        revid: u64,
    },
    /// The text was the page's already.
    NoChange {
        /// The title, prefixed.
        title: String,
    },
}

/// The fields of an edit form.
#[derive(Debug, Clone, Default)]
pub struct EditForm<'a> {
    /// The title.
    pub title: &'a str,
    /// The text.
    pub text: &'a str,
    /// The summary.
    pub summary: &'a str,
    /// A minor edit.
    pub minor: bool,
    /// The revision the edit started from; none for a new page.
    pub baserevid: Option<u64>,
    /// When the edit started from it, for the API's own check.
    pub basetimestamp: Option<&'a str>,
    /// The page did not exist when the edit started: refuse if it does now.
    pub createonly: bool,
    /// The content model of a page being created (`contentmodel`; 0041 §10); `None`
    /// keeps the page's, or the namespace's default.
    pub content_model: Option<&'a str>,
    /// The CSRF token.
    pub token: &'a str,
}

fn page_lookup<T>(
    v: &Value,
    found: impl FnOnce(&Value) -> Result<T, ClientError>,
) -> Result<Lookup<T>, ClientError> {
    let page = &v["query"]["pages"][0];
    if page.is_null() {
        return Err(ClientError::Unexpected("no page in the answer".into()));
    }
    if page.get("invalid").is_some() {
        return Ok(Lookup::Invalid);
    }
    if page.get("missing").is_some() {
        return Ok(Lookup::Missing {
            ns: page["ns"].as_i64().unwrap_or(0),
            title: page["title"].as_str().unwrap_or_default().to_string(),
        });
    }
    found(page).map(Lookup::Found)
}

impl Client {
    /// `action=parse` of a page's latest revision (`page=`, redirects followed where
    /// `follow`) or of one
    /// revision (`oldid=`). A page that does not exist is [`ClientError::Api`] with
    /// `missingtitle`; a revision that does not, `nosuchrevid`.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn parse_page(
        &self,
        incoming: &Incoming,
        title: &str,
        oldid: Option<u64>,
        follow: bool,
    ) -> Result<Fetched<Parsed>, ClientError> {
        self.parse_page_marked(incoming, title, oldid, follow, false)
            .await
    }

    /// As [`Self::parse_page`], with `templates` asking the API to mark the blocks each
    /// of the page's template calls drew (`tstemplates`; 0042 §4).
    ///
    /// # Errors
    ///
    /// As [`Self::parse_page`].
    pub async fn parse_page_marked(
        &self,
        incoming: &Incoming,
        title: &str,
        oldid: Option<u64>,
        follow: bool,
        templates: bool,
    ) -> Result<Fetched<Parsed>, ClientError> {
        let marked = if templates { "&tstemplates=1" } else { "" };
        let which = match oldid {
            Some(r) => format!("oldid={r}"),
            None if follow => format!("page={}&redirects=1", q(title)),
            None => format!("page={}", q(title)),
        };
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=parse&{which}&prop=text%7Ccategories%7Ctemplates{marked}&format=json&formatversion=2"
                ),
            )
            .await?;
        let v = check(&r)?;
        Ok(Fetched {
            value: parsed(&v),
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=parse` of text under a title, with the pre-save transform: what a save
    /// would show, in a content model (`contentmodel`; a page that exists keeps its own).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn preview(
        &self,
        incoming: &Incoming,
        title: &str,
        text: &str,
        model: &str,
    ) -> Result<Parsed, ClientError> {
        let r = self
            .post_form(
                incoming,
                "/w/api.php",
                &[
                    ("action", "parse"),
                    ("format", "json"),
                    ("formatversion", "2"),
                    ("title", title),
                    ("text", text),
                    ("pst", "1"),
                    ("preview", "1"),
                    ("contentmodel", model),
                    ("prop", "text|categories|templates"),
                ],
            )
            .await?;
        Ok(parsed(&check(&r)?))
    }

    /// The latest source of a page, for its edit form.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn page_source(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Lookup<Source>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=revisions&titles={}&rvprop=ids%7Ctimestamp%7Ccontent&rvslots=main&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = page_lookup(&v, |page| {
            let rev = &page["revisions"][0];
            let slot = &rev["slots"]["main"];
            Ok(Source {
                title: page["title"].as_str().unwrap_or_default().to_string(),
                page_id: page["pageid"].as_u64().unwrap_or(0),
                ns: page["ns"].as_i64().unwrap_or(0),
                revid: rev["revid"].as_u64().unwrap_or(0),
                timestamp: rev["timestamp"].as_str().unwrap_or_default().to_string(),
                model: slot["contentmodel"]
                    .as_str()
                    .unwrap_or("wikitext")
                    .to_string(),
                text: slot["content"].as_str().unwrap_or_default().to_string(),
            })
        })?;
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `prop=info` of a title, redirects not followed: whether a page has it, and what
    /// it is.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn page_info(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Lookup<PageInfo>>, ClientError> {
        self.info_of(incoming, &format!("titles={}", q(title)))
            .await
    }

    /// What the **About this page** panel shows of a title (19 §3.1), in one request:
    /// `prop=info|revisions` with `inprop=protection` and the first revision.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn page_about(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Lookup<About>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=info%7Crevisions&inprop=protection&rvprop=ids%7Ctimestamp%7Cuser&rvdir=newer&rvlimit=1&titles={}&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = page_lookup(&v, |page| {
            let first = &page["revisions"][0];
            Ok(About {
                title: page["title"].as_str().unwrap_or_default().to_string(),
                model: page["contentmodel"]
                    .as_str()
                    .unwrap_or("wikitext")
                    .to_string(),
                length: page["length"].as_u64().unwrap_or(0),
                touched: page["touched"].as_str().map(str::to_string),
                lastrevid: page["lastrevid"].as_u64().unwrap_or(0),
                created: first["timestamp"].as_str().map(str::to_string),
                creator: first["user"].as_str().map(str::to_string),
                protection: page["protection"]
                    .as_array()
                    .map(|a| a.iter().map(crate::rights::Restriction::of).collect())
                    .unwrap_or_default(),
            })
        })?;
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=backlinks` of a title: the pages linking to it, a page at a time, from
    /// `blcontinue` where given.
    ///
    /// # Errors
    ///
    /// The API's refusal (`invalidtitle`), or the API away or unreadable.
    pub async fn backlinks(
        &self,
        incoming: &Incoming,
        title: &str,
        limit: u32,
        from: Option<&str>,
    ) -> Result<Fetched<Backlinks>, ClientError> {
        let mut path = format!(
            "/w/api.php?action=query&list=backlinks&bltitle={}&bllimit={limit}&format=json&formatversion=2",
            q(title)
        );
        if let Some(f) = from.filter(|f| !f.is_empty()) {
            path.push_str("&blcontinue=");
            path.push_str(&q(f));
        }
        let r = self.get(incoming, &path).await?;
        let v = check(&r)?;
        let pages = v["query"]["backlinks"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some((
                            p["title"].as_str()?.to_string(),
                            p.get("redirect").is_some_and(|r| r != &Value::Bool(false)),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Fetched {
            value: Backlinks {
                pages,
                next: v["continue"]["blcontinue"].as_str().map(str::to_string),
            },
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=allpages`, as asked.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn all_pages(
        &self,
        incoming: &Incoming,
        ask: &PageListQuery,
    ) -> Result<Fetched<PageList>, ClientError> {
        let mut path = format!(
            "/w/api.php?action=query&list=allpages&apnamespace={}&aplimit={}&format=json&formatversion=2",
            ask.namespace, ask.limit
        );
        for (k, v) in [
            ("apfrom", &ask.from),
            ("apto", &ask.to),
            ("apprefix", &ask.prefix),
            ("apfilterredir", &ask.filter_redirects),
        ] {
            if !v.is_empty() {
                path.push('&');
                path.push_str(k);
                path.push('=');
                path.push_str(&q(v));
            }
        }
        if ask.descending {
            path.push_str("&apdir=descending");
        }
        let r = self.get(incoming, &path).await?;
        let v = check(&r)?;
        let titles = v["query"]["allpages"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| p["title"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let next = v["continue"]["apcontinue"]
            .as_str()
            .map(|c| c.replace('_', " "));
        Ok(Fetched {
            value: PageList { titles, next },
            cache: CacheInfo::of(&r),
        })
    }

    /// `prop=info` of the page with a page ID (`pageids=`) or of the page a revision
    /// belongs to (`revids=`): what `Special:Redirect` and `Special:PermanentLink` need
    /// to find the title. An ID that names nothing the viewer may see is
    /// [`Lookup::Missing`], with an empty title.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn page_info_by(
        &self,
        incoming: &Incoming,
        id: PageRef,
    ) -> Result<Fetched<Lookup<PageInfo>>, ClientError> {
        let selector = match id {
            PageRef::Page(n) => format!("pageids={n}"),
            PageRef::Revision(n) => format!("revids={n}"),
        };
        self.info_of(incoming, &selector).await
    }

    async fn info_of(
        &self,
        incoming: &Incoming,
        selector: &str,
    ) -> Result<Fetched<Lookup<PageInfo>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=info&{selector}&format=json&formatversion=2"
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = page_lookup(&v, |page| {
            Ok(PageInfo {
                title: page["title"].as_str().unwrap_or_default().to_string(),
                page_id: page["pageid"].as_u64().unwrap_or(0),
                ns: page["ns"].as_i64().unwrap_or(0),
                model: page["contentmodel"]
                    .as_str()
                    .unwrap_or("wikitext")
                    .to_string(),
                redirect: page
                    .get("redirect")
                    .is_some_and(|r| r != &Value::Bool(false)),
            })
        })?;
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=compare` of two revisions of a document page, in both of MediaWiki's
    /// layouts, the table and the inline column, asked for together. A refusal is
    /// [`ClientError::Api`] with the API's code (`nosuchrevid`, `missingtitle`,
    /// `missingcontent`, …).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn page_diff(
        &self,
        incoming: &Incoming,
        ask: &DiffAsk,
    ) -> Result<Fetched<PageDiff>, ClientError> {
        self.compare_pages(incoming, ask, true).await
    }

    /// `action=compare` for the two sides only: their headers and neighbours, with
    /// `body` and `inline` empty, in one request. The diff page's other views draw their
    /// own diff (0010 §6).
    ///
    /// # Errors
    ///
    /// As [`Self::page_diff`].
    pub async fn page_diff_sides(
        &self,
        incoming: &Incoming,
        ask: &DiffAsk,
    ) -> Result<Fetched<PageDiff>, ClientError> {
        self.compare_pages(incoming, ask, false).await
    }

    async fn compare_pages(
        &self,
        incoming: &Incoming,
        ask: &DiffAsk,
        with_diff: bool,
    ) -> Result<Fetched<PageDiff>, ClientError> {
        let which = match ask {
            DiffAsk::Revisions(from, to) => format!("fromrev={from}&torev={to}"),
            DiffAsk::Relative(from, rel) => format!("fromrev={from}&torelative={rel}"),
            DiffAsk::Latest(title) => format!("fromtitle={}&torelative=prev", q(title)),
            DiffAsk::Between(from, to) => format!("{}&{}", from.param("from"), to.param("to")),
        };
        if !with_diff {
            let r = self
                .get(
                    incoming,
                    &format!(
                        "/w/api.php?action=compare&{which}&prop=ids%7Ctitle%7Cuser%7Ccomment%7Csize%7Ctimestamp%7Crel&format=json&formatversion=2"
                    ),
                )
                .await?;
            let v = check(&r)?;
            let c = &v["compare"];
            let to = diff_side(c, "to")
                .ok_or_else(|| ClientError::Unexpected(format!("compare answered {c}")))?;
            return Ok(Fetched {
                value: PageDiff {
                    from: diff_side(c, "from"),
                    to,
                    body: String::new(),
                    inline: String::new(),
                    prev: c["prev"].as_u64(),
                    next: c["next"].as_u64(),
                },
                cache: CacheInfo::of(&r),
            });
        }
        let table = format!(
            "/w/api.php?action=compare&{which}&prop=diff%7Cids%7Ctitle%7Cuser%7Ccomment%7Csize%7Ctimestamp%7Crel&format=json&formatversion=2"
        );
        let inline = format!(
            "/w/api.php?action=compare&{which}&prop=diff&difftype=inline&format=json&formatversion=2"
        );
        let (r, ri) = tokio::join!(self.get(incoming, &table), self.get(incoming, &inline));
        let (r, ri) = (r?, ri?);
        let v = check(&r)?;
        let vi = check(&ri)?;
        let c = &v["compare"];
        let to = diff_side(c, "to")
            .ok_or_else(|| ClientError::Unexpected(format!("compare answered {c}")))?;
        Ok(Fetched {
            value: PageDiff {
                from: diff_side(c, "from"),
                to,
                body: c["body"].as_str().unwrap_or_default().to_string(),
                inline: vi["compare"]["body"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                prev: c["prev"].as_u64(),
                next: c["next"].as_u64(),
            },
            cache: CacheInfo::combine(&[CacheInfo::of(&r), CacheInfo::of(&ri)]),
        })
    }

    /// The source text of one revision of a document page, `prop=revisions&revids=`;
    /// `None` when the revision is not found or its text is hidden from the viewer.
    ///
    /// # Errors
    ///
    /// The API away or unreadable.
    pub async fn revision_text(
        &self,
        incoming: &Incoming,
        revid: u64,
    ) -> Result<Fetched<Option<String>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=revisions&revids={revid}&rvprop=ids%7Ccontent&rvslots=main&format=json&formatversion=2"
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["pages"].as_array().and_then(|pages| {
            pages.iter().find_map(|p| {
                p["revisions"].as_array()?.iter().find_map(|rev| {
                    (rev["revid"].as_u64() == Some(revid))
                        .then(|| rev["slots"]["main"]["content"].as_str())
                        .flatten()
                        .map(str::to_string)
                })
            })
        });
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=edit`. A refusal is [`ClientError::Api`] with the API's code
    /// (`editconflict`, `articleexists`, `badtoken`, `permissiondenied`, …).
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn save_page(
        &self,
        incoming: &Incoming,
        form: &EditForm<'_>,
    ) -> Result<Saved, ClientError> {
        let base = form.baserevid.map(|r| r.to_string());
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "edit"),
            ("format", "json"),
            ("formatversion", "2"),
            ("title", form.title),
            ("text", form.text),
            ("summary", form.summary),
            ("token", form.token),
        ];
        if form.minor {
            fields.push(("minor", "1"));
        }
        if let Some(b) = &base {
            fields.push(("baserevid", b));
        }
        if let Some(t) = form.basetimestamp {
            fields.push(("basetimestamp", t));
        }
        if form.createonly {
            fields.push(("createonly", "1"));
        }
        if let Some(m) = form.content_model {
            fields.push(("contentmodel", m));
        }
        let r = self.post_form(incoming, "/w/api.php", &fields).await?;
        let v = check(&r)?;
        let e = &v["edit"];
        let title = e["title"].as_str().unwrap_or(form.title).to_string();
        if e.get("nochange").is_some() {
            return Ok(Saved::NoChange { title });
        }
        match e["result"].as_str() {
            Some("Success") => Ok(Saved::Saved {
                title,
                revid: e["newrevid"].as_u64().unwrap_or(0),
            }),
            _ => Err(ClientError::Unexpected(format!("edit answered {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_parse_answer() {
        let p = parsed(&json!({"parse": {
            "title": "Project:Scope", "pageid": 10, "revid": 7, "text": "<p>x</p>",
            "categories": [{"sortkey": "", "category": "Policy_pages"},
                           {"sortkey": "", "category": "Hidden", "hidden": true}],
            "redirects": [{"from": "Project:Notability", "to": "Project:Scope"}],
            "templates": [{"ns": 10, "title": "Template:Cwbn", "exists": true}]
        }}));
        assert_eq!(p.page_id, 10);
        assert_eq!(p.categories[0].name, "Policy pages");
        assert!(!p.categories[0].hidden);
        assert!(p.categories[1].hidden);
        assert_eq!(p.redirected_from.as_deref(), Some("Project:Notability"));
        assert_eq!(p.templates, ["Template:Cwbn"]);
    }

    #[test]
    fn lookups() {
        let missing =
            json!({"query": {"pages": [{"ns": 4, "title": "Project:X", "missing": true}]}});
        let m: Lookup<()> = page_lookup(&missing, |_| Ok(())).unwrap();
        assert_eq!(
            m,
            Lookup::Missing {
                ns: 4,
                title: "Project:X".into()
            }
        );
        let invalid = json!({"query": {"pages": [{"title": "Project:", "invalid": true}]}});
        assert_eq!(page_lookup(&invalid, |_| Ok(())).unwrap(), Lookup::Invalid);
    }
}
