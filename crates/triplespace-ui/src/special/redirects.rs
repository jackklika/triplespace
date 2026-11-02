//! MediaWiki's redirecting special pages (0047 §9; 21 §6.1): each sends the viewer on to
//! a page or an action the site already serves, with a `302`.
//!
//! - **The viewer's own:** `MyPage[/sub]` and `MyTalk[/sub]` to their user and user talk
//!   pages, `MyContributions` (`MyContribs`) to `Special:Contributions/{name}`, and
//!   `MyLog[/type]` to `Special:Log` filtered to them. A viewer not signed in is sent to
//!   `Special:UserLogin`, returning here.
//! - **An action on a page:** `EditPage` (`Edit`), `PageHistory` (`History`), `DeletePage`
//!   (`Delete`) and `ProtectPage` (`Protect`), each `/{title}`, to `?action=edit|history|
//!   delete|protect`. The action's own page decides who may use it.
//! - **A page's discussion:** `TalkPage/{title}` to its talk page (a talk title is its own),
//!   from the namespace registry's `talk`, and `NewSection/{title}` to the talk page's
//!   new-thread form (`?action=addtopic`).
//! - **The viewer's language:** `MyLanguage/{title}` to `{title}/{lang}` where that page
//!   exists and the interface language is not the site's, else to `{title}`.
//! - **By number:** `PermanentLink/{revid}` (`PermaLink`) to `?oldid=`, `Diff/{id}[/{id}]`
//!   to `?diff=` as MediaWiki's `Special:Diff` reads it, and `Redirect/revision/{id}` and
//!   `Redirect/page/{id}` (or `wptype` and `wpvalue`), each by asking the API whose title
//!   the number is (`prop=info` with `revids` or `pageids`). `Redirect`'s `user`, `file`
//!   and `logid` forms are not served: accounts have no numeric IDs, there are no files
//!   yet (0039), and `Special:Log` takes no log ID.
//! - **An entity:** `EntityPage/{id}` to the entity's page (0008 §3).
//!
//! Without its subpage, a page takes `page` or `target` from the query, and with neither
//! shows a one-field form. Every other query parameter is passed on, as MediaWiki does
//! (`Special:EditPage/Foo?section=2`).

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use scatter_titles::{Kind as NsKind, Namespace, Namespaces};
use scatter_wikibase_model::id::EntityId;
use serde::Serialize;
use triplespace_client::{Lookup, PageRef};

use crate::Site;
use crate::codex::{Message, MessageKind, MessageModel};
use crate::entity;
use crate::features::Feature;
use crate::frame::Page;
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};
use crate::special::changes::FormItem;

// ---------------------------------------------------------------------------------------
// The view model (19 §5.10): what `special/redirect.html` draws. Every other path of these
// pages is a `302` and draws nothing.
// ---------------------------------------------------------------------------------------

/// The form a redirecting page shows when it needs a parameter, or could not follow one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedirectForm {
    /// Why the request could not be followed.
    pub error: Option<MessageModel>,
    /// The page it asks.
    pub title: String,
    /// The controls, as the changes form draws them: `Special:Redirect`'s kind, then the
    /// field.
    pub items: Vec<FormItem>,
    /// **Go**.
    pub go: String,
}

/// The redirecting pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:MyPage`.
    MyPage,
    /// `Special:MyTalk`.
    MyTalk,
    /// `Special:MyContributions`.
    MyContributions,
    /// `Special:MyLog`.
    MyLog,
    /// `Special:EditPage`.
    EditPage,
    /// `Special:PageHistory`.
    PageHistory,
    /// `Special:DeletePage`.
    DeletePage,
    /// `Special:ProtectPage`.
    ProtectPage,
    /// `Special:TalkPage`.
    TalkPage,
    /// `Special:NewSection`.
    NewSection,
    /// `Special:MyLanguage`.
    MyLanguage,
    /// `Special:PermanentLink`.
    PermanentLink,
    /// `Special:Diff`.
    Diff,
    /// `Special:Redirect`.
    Redirect,
    /// `Special:EntityPage`.
    EntityPage,
}

/// The query parameters a page reads its target from, and so does not pass on.
const ITS_OWN: &[&str] = &["title", "page", "target", "wptype", "wpvalue"];

impl Which {
    /// The page a special page's name (or one of its registry aliases) names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        Some(match name {
            "MyPage" => Self::MyPage,
            "MyTalk" => Self::MyTalk,
            "MyContributions" | "MyContribs" => Self::MyContributions,
            "MyLog" => Self::MyLog,
            "EditPage" | "Edit" => Self::EditPage,
            "PageHistory" | "History" => Self::PageHistory,
            "DeletePage" | "Delete" => Self::DeletePage,
            "ProtectPage" | "Protect" => Self::ProtectPage,
            "TalkPage" => Self::TalkPage,
            "NewSection" => Self::NewSection,
            "MyLanguage" => Self::MyLanguage,
            "PermanentLink" | "PermaLink" => Self::PermanentLink,
            "Diff" => Self::Diff,
            "Redirect" => Self::Redirect,
            "EntityPage" => Self::EntityPage,
            _ => return None,
        })
    }

    /// The canonical name.
    fn name(self) -> &'static str {
        match self {
            Self::MyPage => "MyPage",
            Self::MyTalk => "MyTalk",
            Self::MyContributions => "MyContributions",
            Self::MyLog => "MyLog",
            Self::EditPage => "EditPage",
            Self::PageHistory => "PageHistory",
            Self::DeletePage => "DeletePage",
            Self::ProtectPage => "ProtectPage",
            Self::TalkPage => "TalkPage",
            Self::NewSection => "NewSection",
            Self::MyLanguage => "MyLanguage",
            Self::PermanentLink => "PermanentLink",
            Self::Diff => "Diff",
            Self::Redirect => "Redirect",
            Self::EntityPage => "EntityPage",
        }
    }

    /// The feature the page leads to, without which it is not served.
    fn feature(self) -> Option<Feature> {
        match self {
            Self::MyPage | Self::MyTalk | Self::TalkPage | Self::NewSection | Self::MyLanguage => {
                Some(Feature::Pages)
            }
            Self::MyContributions | Self::MyLog => Some(Feature::RecentChanges),
            Self::PageHistory | Self::PermanentLink | Self::Diff | Self::Redirect => {
                Some(Feature::History)
            }
            Self::DeletePage => Some(Feature::Delete),
            Self::ProtectPage => Some(Feature::Protect),
            Self::EditPage | Self::EntityPage => None,
        }
    }

    /// The action an action page leads to.
    fn action(self) -> Option<&'static str> {
        match self {
            Self::EditPage => Some("edit"),
            Self::PageHistory => Some("history"),
            Self::DeletePage => Some("delete"),
            Self::ProtectPage => Some("protect"),
            _ => None,
        }
    }

    /// The form's title and its field's label and name.
    fn form(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::EditPage => ("ts-editpage-title", "ts-redirects-page", "page"),
            Self::PageHistory => ("ts-pagehistory-title", "ts-redirects-page", "page"),
            Self::DeletePage => ("ts-deletepage-title", "ts-redirects-page", "page"),
            Self::ProtectPage => ("ts-protectpage-title", "ts-redirects-page", "page"),
            Self::NewSection => ("ts-newsection-title", "ts-redirects-page", "target"),
            Self::MyLanguage => ("ts-mylanguage-title", "ts-redirects-page", "target"),
            Self::PermanentLink => ("ts-permanentlink-title", "ts-redirects-revision", "target"),
            Self::Diff => ("ts-diff-page-title", "ts-redirects-diff", "target"),
            Self::Redirect => ("ts-redirect-title", "ts-redirect-value", "wpvalue"),
            Self::EntityPage => ("ts-entitypage-title", "ts-redirects-entity", "target"),
            Self::TalkPage | Self::MyPage | Self::MyTalk | Self::MyContributions | Self::MyLog => {
                ("ts-talkpage-title", "ts-redirects-page", "target")
            }
        }
    }
}

/// `GET Special:{which}[/{target}]`.
#[allow(clippy::too_many_lines)]
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if which.feature().is_some_and(|f| !cx.features.has(f)) {
        return pages::no_page(&cx, headers, title.to_string());
    }
    let subpage = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty());
    let arg = subpage.clone().or_else(|| {
        ["page", "target"]
            .iter()
            .filter_map(|k| params.map.get(*k))
            .map(|v| v.replace('_', " ").trim().to_string())
            .find(|v| !v.is_empty())
    });
    let mut keep: BTreeMap<String, String> = params
        .map
        .iter()
        .filter(|(k, _)| !ITS_OWN.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    match which {
        Which::MyPage | Which::MyTalk | Which::MyContributions | Which::MyLog => {
            let Some(user) = cx.user.clone() else {
                let mut q = BTreeMap::new();
                q.insert("returnto".to_string(), title.replace('_', " "));
                return pages::redirect_to_with(&cx, "Special:UserLogin", &q);
            };
            let sub = arg.map(|a| format!("/{a}")).unwrap_or_default();
            let target = match which {
                Which::MyPage => format!("User:{user}{sub}"),
                Which::MyTalk => format!("User talk:{user}{sub}"),
                Which::MyContributions => format!("Special:Contributions/{user}"),
                _ => {
                    // `Special:MyLog/delete`: the viewer's deletions.
                    if let Some(t) = sub.strip_prefix('/') {
                        keep.insert("type".into(), t.to_string());
                    }
                    keep.insert("user".into(), user);
                    "Special:Log".to_string()
                }
            };
            pages::redirect_to_with(&cx, &target, &keep)
        }
        Which::EditPage | Which::PageHistory | Which::DeletePage | Which::ProtectPage => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let Some(target) = page_title(&cx, &arg) else {
                let e = cx.m.with("ts-redirects-bad-title", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            keep.insert("action".into(), which.action().unwrap_or("view").into());
            pages::redirect_to_with(&cx, &target, &keep)
        }
        Which::TalkPage | Which::NewSection => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let talk = page_title(&cx, &arg).and_then(|t| talk_of(&t));
            let Some(talk) = talk else {
                let e = cx.m.with("ts-redirects-no-talk", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            if which == Which::NewSection {
                keep.insert("action".into(), "addtopic".into());
            }
            pages::redirect_to_with(&cx, &talk, &keep)
        }
        Which::MyLanguage => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let Some(base) = page_title(&cx, &arg) else {
                let e = cx.m.with("ts-redirects-bad-title", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            let lang = cx.m.lang().to_string();
            if lang == cx.site.lang {
                return pages::redirect_to_with(&cx, &base, &keep);
            }
            let translated = format!("{base}/{lang}");
            match site.client().page_info(&incoming, &translated).await {
                Ok(found) => {
                    cx.inputs.push(found.cache);
                    let to = if matches!(found.value, Lookup::Found(_)) {
                        translated
                    } else {
                        base
                    };
                    pages::redirect_to_with(&cx, &to, &keep)
                }
                Err(e) => pages::api_failed(&cx, headers, &e),
            }
        }
        Which::PermanentLink => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let Ok(revid) = arg.parse::<u64>() else {
                let e = cx.m.with("ts-redirects-not-a-number", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            keep.insert("oldid".into(), revid.to_string());
            by_number(
                site,
                &incoming,
                cx,
                headers,
                which,
                PageRef::Revision(revid),
                &arg,
                &keep,
            )
            .await
        }
        Which::Diff => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let Some((oldid, diff, of)) = diff_of(&arg) else {
                let e = cx.m.with("ts-redirects-bad-diff", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            if let Some(o) = oldid {
                keep.insert("oldid".into(), o.to_string());
            }
            keep.insert("diff".into(), diff);
            by_number(
                site,
                &incoming,
                cx,
                headers,
                which,
                PageRef::Revision(of),
                &arg,
                &keep,
            )
            .await
        }
        Which::Redirect => {
            let asked = subpage
                .as_deref()
                .and_then(|s| s.split_once('/'))
                .map(|(t, v)| (t.trim().to_lowercase(), v.trim().to_string()))
                .or_else(|| {
                    let t = params.map.get("wptype")?.trim().to_lowercase();
                    let v = params.map.get("wpvalue")?.trim().to_string();
                    Some((t, v))
                })
                .filter(|(_, v)| !v.is_empty());
            let Some((kind, value)) = asked else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let Ok(n) = value.parse::<u64>() else {
                let e = cx.m.with("ts-redirects-not-a-number", &[&value]);
                return form(
                    &cx,
                    headers,
                    which,
                    StatusCode::BAD_REQUEST,
                    Some(&e),
                    &value,
                );
            };
            let id = match kind.as_str() {
                "revision" => {
                    keep.insert("oldid".into(), n.to_string());
                    PageRef::Revision(n)
                }
                "page" => PageRef::Page(n),
                _ => {
                    let e = cx.m.get("ts-redirect-unsupported");
                    return form(
                        &cx,
                        headers,
                        which,
                        StatusCode::BAD_REQUEST,
                        Some(&e),
                        &value,
                    );
                }
            };
            by_number(site, &incoming, cx, headers, which, id, &value, &keep).await
        }
        Which::EntityPage => {
            let Some(arg) = arg else {
                return form(&cx, headers, which, StatusCode::OK, None, "");
            };
            let id = EntityId::parse(&arg)
                .ok()
                .filter(|_| !arg.contains(char::is_whitespace));
            let Some(id) = id else {
                let e = cx.m.with("ts-redirects-bad-entity", &[&arg]);
                return form(&cx, headers, which, StatusCode::BAD_REQUEST, Some(&e), &arg);
            };
            let id = id.as_str();
            let target = entity::page_title(id, &entity::entity_type_of(id));
            pages::redirect_to_with(&cx, &target, &keep)
        }
    }
}

/// On to the page a page or revision ID belongs to, or the form saying there is none.
#[allow(clippy::too_many_arguments)]
async fn by_number(
    site: &Site,
    incoming: &triplespace_client::Incoming,
    mut cx: Context,
    headers: &HeaderMap,
    which: Which,
    id: PageRef,
    asked: &str,
    keep: &BTreeMap<String, String>,
) -> Response {
    match site.client().page_info_by(incoming, id).await {
        Ok(found) => {
            cx.inputs.push(found.cache);
            match found.value {
                Lookup::Found(page) if !page.title.is_empty() => {
                    pages::redirect_to_with(&cx, &page.title, keep)
                }
                _ => {
                    let key = match id {
                        PageRef::Page(_) => "ts-redirects-no-page",
                        PageRef::Revision(_) => "ts-redirects-no-revision",
                    };
                    let n = match id {
                        PageRef::Page(n) | PageRef::Revision(n) => n.to_string(),
                    };
                    let e = cx.m.with(key, &[&n]);
                    form(&cx, headers, which, StatusCode::NOT_FOUND, Some(&e), asked)
                }
            }
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `Special:Diff`'s subpage as MediaWiki reads it: `{diff}` alone, or `{oldid}/{diff}`
/// where `{diff}` may be `prev`, `next` or `cur`. Gives `oldid`, `diff`, and the revision
/// whose page is the title.
fn diff_of(arg: &str) -> Option<(Option<u64>, String, u64)> {
    let parts: Vec<&str> = arg.split('/').map(str::trim).collect();
    match parts.as_slice() {
        [d] => {
            let d = d.parse::<u64>().ok()?;
            Some((None, d.to_string(), d))
        }
        [o, d] => {
            let o = o.parse::<u64>().ok()?;
            if let Ok(d) = d.parse::<u64>() {
                return Some((Some(o), d.to_string(), d));
            }
            matches!(*d, "prev" | "next" | "cur").then(|| (Some(o), (*d).to_string(), o))
        }
        _ => None,
    }
}

/// A namespace by its prefix: the canonical name or the local one (`Interface`).
fn namespace_named(prefix: &str) -> Option<&'static Namespace> {
    let p = prefix.replace('_', " ");
    let p = p.trim();
    Namespaces::embedded().iter().find(|n| {
        !n.canonical.is_empty()
            && (n.canonical.eq_ignore_ascii_case(p)
                || n.name.as_deref().is_some_and(|l| l.eq_ignore_ascii_case(p)))
    })
}

/// A title's namespace and the rest of it; the main namespace where the prefix names none.
fn split(title: &str) -> Option<(&'static Namespace, String)> {
    if let Some((prefix, rest)) = title.split_once(':')
        && let Some(ns) = namespace_named(prefix)
    {
        return Some((ns, rest.trim().to_string()));
    }
    Some((Namespaces::embedded().get(0)?, title.trim().to_string()))
}

/// A title a page could have: written canonically, not empty, and not in `Special:` or
/// `Media:`.
fn page_title(cx: &Context, raw: &str) -> Option<String> {
    let t = pages::canonical(raw, &cx.site.sitename);
    let (ns, rest) = split(&t)?;
    (ns.kind != NsKind::Virtual && !rest.is_empty()).then_some(t)
}

/// The talk page of a title, from the namespace registry; a talk title is its own. `None`
/// where the namespace has no talk pages (`Thread:`, `DOI:`).
fn talk_of(title: &str) -> Option<String> {
    let (ns, rest) = split(title)?;
    if ns.default_model.as_deref() == Some("triplespace-talk") {
        return Some(title.to_string());
    }
    let talk = Namespaces::embedded().get(ns.talk?)?;
    (talk.kind == NsKind::Pages).then(|| format!("{}:{rest}", talk.canonical))
}

/// The page's form, with an error above it where the request could not be followed.
fn form(
    cx: &Context,
    headers: &HeaderMap,
    which: Which,
    status: StatusCode,
    error: Option<&str>,
    value: &str,
) -> Response {
    let (title_key, _, _) = which.form();
    let name = which.name();
    let body = cx.draw(
        "special/redirect.html",
        form_model(&cx.m, which, error, value),
    );
    pages::respond(cx, headers, status, |m| Page {
        title: m.get(title_key),
        returnto: Some(format!("Special:{name}")),
        body,
        ..Page::default()
    })
}

/// The form's model.
fn form_model(m: &Messages, which: Which, error: Option<&str>, value: &str) -> RedirectForm {
    let (_, label, field) = which.form();
    let mut items = Vec::new();
    if which == Which::Redirect {
        items.push(FormItem::select(
            m.get("ts-redirect-type"),
            "wptype",
            vec![
                ("revision".into(), m.get("ts-redirect-type-revision"), false),
                ("page".into(), m.get("ts-redirect-type-page"), false),
            ],
        ));
    }
    items.push(FormItem::text(m.get(label), field, value));
    RedirectForm {
        error: error.map(|e| Message::block(MessageKind::Error, e).model()),
        title: format!("Special:{}", which.name()),
        items,
        go: m.get("ts-redirects-go"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form's model (19 §5.10): `Special:Redirect` asks the kind first, every page
    /// keeps what was typed, an error goes above it.
    #[test]
    fn the_form_is_a_model() {
        let m = Messages::for_language("en");
        let f = form_model(&m, Which::Redirect, Some("Not a number"), "abc");
        assert_eq!(f.title, "Special:Redirect");
        assert_eq!(f.items[0].name(), Some("wptype"));
        assert_eq!(f.items[1].name(), Some("wpvalue"));
        assert!(matches!(
            &f.items[1],
            FormItem::Field { control: crate::special::controls::ControlModel::Input { value, .. }, .. } if value == "abc"
        ));
        assert!(f.error.unwrap().classes.contains("cdx-message--error"));
        let f = form_model(&m, Which::EditPage, None, "");
        assert_eq!(f.items.len(), 1);
        assert_eq!(f.items[0].name(), Some("page"));
        assert!(f.error.is_none());
    }
}
