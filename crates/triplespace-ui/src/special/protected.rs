//! `Special:ProtectedPages` (09 §4.5; 0016 §7; 0023 §9; 0056 §13; 19 §6.3): the
//! restrictions in force, each only to a viewer who may read its target, so a
//! confidential page is listed to its group alone.
//!
//! - **Pages**: every page and entity restriction (`list=protectedpages`, `read`
//!   included, with a **Read** column), and the reserved titles (`list=protectedtitles`).
//! - **Sets**: the sets the viewer may read (`list=protectedsets`), each with its
//!   restrictions and members, and for a holder of `protect` the forms that manage them:
//!   create a set, rename it or change what it restricts, add a member by title, remove
//!   one, retire the set.
//! - **Empty groups**: for a holder of `userrights`, each group a confidential restriction
//!   in force names that has no current member, with how many restrictions name it and no
//!   titles (`GET /acl/empty-groups`), so an orphaned group is found before anyone needs
//!   it.
//!
//! Every form posts back here; the API's refusal is shown above the section it came from.
//!
//! [`titles`] serves `Special:ProtectedTitles`, the reserved titles alone.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use serde_json::json;
use triplespace_client::settings::Saved;
use triplespace_client::{Incoming, Lookup, ProtectedRow, SetInfo};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, Tab, title_url};
use crate::i18n::Messages;
use crate::moderation::link_to;
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{Control, FieldRow, Kind};

const TITLE: &str = "Special:ProtectedPages";

fn expiry_text(m: &Messages, expiry: &str) -> String {
    if expiry == "infinity" || expiry == "infinite" {
        m.get("ts-protect-expiry-never")
    } else {
        expiry.to_string()
    }
}

fn kind_label(m: &Messages, kind: &str) -> String {
    let key = format!("ts-protect-action-{kind}");
    if m.has(&key) {
        m.get(&key)
    } else {
        kind.to_string()
    }
}

/// `Special:ProtectedPages` as Plain's `special/protectedpages.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProtectedPagesPage {
    /// The notice after a form, or its refusal.
    pub notices: Vec<MessageModel>,
    /// The restrictions on pages and titles.
    pub pages: PagesSection,
    /// The sets.
    pub sets: SetsSection,
    /// The groups restrictions name that have no member, for a holder of `userrights`.
    pub empty: Option<EmptySection>,
}

/// The Pages section.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PagesSection {
    /// The heading.
    pub heading: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// One per restriction.
    pub rows: Vec<RestrictionRow>,
}

/// One restriction.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RestrictionRow {
    /// The title, to its Protect form (or, reserved, its page).
    pub page: Choice,
    /// The action restricted, or none for a reserved title's list.
    pub action: Option<String>,
    /// The group.
    pub group: String,
    /// Until when.
    pub expiry: String,
}

/// The Sets section.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetsSection {
    /// The heading.
    pub heading: String,
    /// What a set is.
    pub help: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The CSRF token, for a holder of `protect`: every form needs it.
    pub token: Option<String>,
    /// The sets.
    pub sets: Vec<SetModel>,
    /// The form that creates one.
    pub new: Option<NewSet>,
}

/// One set.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetModel {
    /// Its ID.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// What it restricts.
    pub restrictions: String,
    /// Said where it has no members.
    pub no_members: Option<String>,
    /// Its members.
    pub members: Vec<SetMember>,
    /// The forms that manage it.
    pub manage: Option<ManageSet>,
}

/// A member of a set.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SetMember {
    /// Its page.
    pub page: Choice,
    /// Its page ID, which **Remove** posts.
    pub page_id: u64,
    /// **Remove**.
    pub remove: Option<ButtonModel>,
}

/// The forms of a set, for a holder of `protect`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ManageSet {
    /// The title to add.
    pub member: FieldModel,
    /// **Add**.
    pub add: ButtonModel,
    /// The heading of the change form.
    pub change_heading: String,
    /// The set's name.
    pub name: FieldModel,
    /// Who may read its members.
    pub read: FieldRow,
    /// Who may edit them.
    pub edit: FieldRow,
    /// **Change**.
    pub change: ButtonModel,
    /// **Retire**.
    pub retire: ButtonModel,
}

/// The form that creates a set.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NewSet {
    /// The heading.
    pub heading: String,
    /// The name.
    pub name: FieldModel,
    /// Who may read its members.
    pub read: FieldRow,
    /// Who may edit them.
    pub edit: FieldRow,
    /// **Create**.
    pub create: ButtonModel,
}

/// The empty-group report.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EmptySection {
    /// The heading.
    pub heading: String,
    /// What it reports.
    pub help: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// One line per group.
    pub rows: Vec<String>,
}

/// `Special:ProtectedTitles` as Plain's `special/protectedtitles.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProtectedTitlesPage {
    /// What the page lists.
    pub intro: String,
    /// Said where there are none.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// The reserved titles.
    pub rows: Vec<RestrictionRow>,
    /// To every restriction.
    pub all: Choice,
}

fn button_model(label: &str, action: Action, weight: Weight) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action,
        weight,
    }
    .model()
}

fn text_field(
    id: &str,
    name: &str,
    value: &str,
    label: String,
    help: Option<String>,
    required: bool,
) -> FieldModel {
    FieldModel {
        id: id.into(),
        name: name.into(),
        kind: "text".into(),
        value: value.into(),
        label,
        description: None,
        help,
        required,
        autocomplete: Some("off".into()),
    }
}

/// A group picker for a set's restriction on `action`, as [`picker`] draws it.
fn picker_row(
    m: &Messages,
    id: &str,
    action: &str,
    groups: &[String],
    current: Option<&str>,
    help: Option<String>,
) -> FieldRow {
    let value = current.map(|c| serde_json::Value::String(c.to_string()));
    let all = m.get("ts-protect-level-all");
    FieldRow {
        id: id.to_string(),
        label: kind_label(m, action),
        class: None,
        control: Control {
            id,
            name: action,
            kind: Kind::Choice,
            choices: groups,
            value: value.as_ref(),
            empty_choice: Some(&all),
            described_by: None,
            choice_label: None,
        }
        .model(),
        help,
        help_id: None,
    }
}

fn pages_model(m: &Messages, pages: &[ProtectedRow], titles: &[ProtectedRow]) -> PagesSection {
    PagesSection {
        heading: m.get("ts-protectedpages-pages"),
        nothing: (pages.is_empty() && titles.is_empty()).then(|| m.get("ts-protectedpages-none")),
        columns: [
            "ts-protectedpages-col-title",
            "ts-protectedpages-col-action",
            "ts-protectedpages-col-group",
            "ts-protectedpages-col-expiry",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: pages
            .iter()
            .chain(titles)
            .map(|r| {
                let kind = if r.kind == "create" || r.kind == "createpage" {
                    "create"
                } else {
                    r.kind.as_str()
                };
                RestrictionRow {
                    page: link_to(
                        format!("/w/index.php?title={}&action=protect", title_url(&r.title)),
                        r.title.clone(),
                    ),
                    action: Some(kind_label(m, kind)),
                    group: r.level.clone(),
                    expiry: expiry_text(m, &r.expiry),
                }
            })
            .collect(),
    }
}

fn set_model(m: &Messages, set: &SetInfo, groups: &[String], token: Option<&str>) -> SetModel {
    let id = set.id;
    let manager = token.is_some();
    let level = |action: &str| -> String {
        set.protection
            .iter()
            .find(|(k, _, _)| k == action)
            .map(|(_, g, _)| g.clone())
            .unwrap_or_default()
    };
    SetModel {
        id,
        name: if set.name.is_empty() {
            m.with("ts-protectedpages-set-unnamed", &[&id.to_string()])
        } else {
            set.name.clone()
        },
        restrictions: set
            .protection
            .iter()
            .map(|(k, g, e)| {
                m.with(
                    "ts-protectedpages-set-restriction",
                    &[&kind_label(m, k), g, &expiry_text(m, e)],
                )
            })
            .collect::<Vec<_>>()
            .join(" · "),
        no_members: set
            .members
            .is_empty()
            .then(|| m.get("ts-protectedpages-set-no-members")),
        members: set
            .members
            .iter()
            .map(|(page_id, _, title)| SetMember {
                page: link_to(format!("/wiki/{}", title_url(title)), title.clone()),
                page_id: *page_id,
                remove: manager.then(|| {
                    button_model(
                        &m.get("ts-protectedpages-remove-member"),
                        Action::Destructive,
                        Weight::Quiet,
                    )
                }),
            })
            .collect(),
        manage: manager.then(|| ManageSet {
            member: text_field(
                &format!("ts-set-{id}-title"),
                "member",
                "",
                m.get("ts-protectedpages-add-member"),
                Some(m.get("ts-protectedpages-add-member-help")),
                true,
            ),
            add: button_model(
                &m.get("ts-protectedpages-add"),
                Action::Progressive,
                Weight::Normal,
            ),
            change_heading: m.get("ts-protectedpages-change-set"),
            name: text_field(
                &format!("ts-set-{id}-name"),
                "name",
                &set.name,
                m.get("ts-protectedpages-set-name"),
                None,
                false,
            ),
            read: picker_row(
                m,
                &format!("ts-set-{id}-read"),
                "read",
                groups,
                Some(&level("read")),
                None,
            ),
            edit: picker_row(
                m,
                &format!("ts-set-{id}-edit"),
                "edit",
                groups,
                Some(&level("edit")),
                None,
            ),
            change: button_model(
                &m.get("ts-protectedpages-change"),
                Action::Progressive,
                Weight::Normal,
            ),
            retire: button_model(
                &m.get("ts-protectedpages-retire"),
                Action::Destructive,
                Weight::Normal,
            ),
        }),
    }
}

fn sets_model(
    m: &Messages,
    sets: &[SetInfo],
    groups: &[String],
    token: Option<&str>,
) -> SetsSection {
    SetsSection {
        heading: m.get("ts-protectedpages-sets"),
        help: m.get("ts-protectedpages-sets-help"),
        nothing: sets.is_empty().then(|| m.get("ts-protectedpages-no-sets")),
        token: token.map(str::to_string),
        sets: sets
            .iter()
            .map(|s| set_model(m, s, groups, token))
            .collect(),
        new: token.map(|_| NewSet {
            heading: m.get("ts-protectedpages-new-set"),
            name: text_field(
                "ts-set-new-name",
                "name",
                "",
                m.get("ts-protectedpages-set-name"),
                None,
                true,
            ),
            read: picker_row(
                m,
                "ts-set-new-read",
                "read",
                groups,
                None,
                Some(m.get("ts-protectedpages-set-read-help")),
            ),
            edit: picker_row(m, "ts-set-new-edit", "edit", groups, None, None),
            create: button_model(
                &m.get("ts-protectedpages-create"),
                Action::Progressive,
                Weight::Primary,
            ),
        }),
    }
}

fn empty_model(m: &Messages, groups: &[triplespace_client::EmptyGroup]) -> EmptySection {
    EmptySection {
        heading: m.get("ts-protectedpages-empty-groups"),
        help: m.get("ts-protectedpages-empty-groups-help"),
        nothing: groups
            .is_empty()
            .then(|| m.get("ts-protectedpages-empty-groups-none")),
        rows: groups
            .iter()
            .map(|g| {
                m.with(
                    "ts-protectedpages-empty-group",
                    &[&g.group, &g.restrictions.to_string()],
                )
            })
            .collect(),
    }
}

/// The page.
async fn render(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    notice: Option<(MessageKind, String)>,
) -> Response {
    let may_manage = cx.holds("protect") && cx.user.is_some();
    let (pages, sets, empty, groups, token) = tokio::join!(
        site.client().protected_pages(incoming),
        site.client().protected_sets(incoming),
        async {
            if cx.holds("userrights") {
                site.client().empty_groups(incoming).await
            } else {
                Ok(None)
            }
        },
        site.client().groups(incoming),
        async {
            if may_manage {
                site.client().token(incoming, "csrf").await.map(Some)
            } else {
                Ok(None)
            }
        }
    );
    let (pages, sets, empty, groups, token) = match (pages, sets, empty, groups, token) {
        (Ok(pages), Ok(sets), Ok(empty), Ok(groups), Ok(token)) => {
            (pages, sets, empty, groups, token)
        }
        (Err(e), ..)
        | (_, Err(e), ..)
        | (_, _, Err(e), ..)
        | (_, _, _, Err(e), _)
        | (.., Err(e)) => {
            return pages::api_failed(cx, headers, &e);
        }
    };
    cx.inputs.push(pages.cache);
    cx.inputs.push(sets.cache);
    cx.inputs.push(groups.cache);
    let group_names: Vec<String> = groups
        .value
        .into_iter()
        .filter(|g| !g.implicit && g.name != "*")
        .map(|g| g.name)
        .collect();
    let m = &cx.m;
    let token_value = token.as_ref().map(|t| t.value.value.as_str());
    let model = ProtectedPagesPage {
        notices: notice
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        pages: pages_model(m, &pages.value.0, &pages.value.1),
        sets: sets_model(m, &sets.value, &group_names, token_value),
        empty: empty.as_ref().map(|g| empty_model(m, g)),
    };
    let mut tabs = vec![
        Tab {
            label: m.get("ts-protectedpages-pages"),
            href: "#pages".into(),
            current: false,
        },
        Tab {
            label: m.get("ts-protectedpages-sets"),
            href: "#sets".into(),
            current: false,
        },
    ];
    if empty.is_some() {
        tabs.push(Tab {
            label: m.get("ts-protectedpages-empty-groups"),
            href: "#empty-groups".into(),
            current: false,
        });
    }
    let body = cx.draw("special/protectedpages.html", &model);
    let r = pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-protectedpages-title"),
        returnto: Some(TITLE.to_string()),
        tabs,
        body,
        ..Page::default()
    });
    match token {
        Some(t) => pages::with_cookies(r, &t.set_cookies),
        None => r,
    }
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Protect) {
        return Err(pages::no_page(&cx, headers, TITLE.to_string()));
    }
    Ok(cx)
}

/// `GET Special:ProtectedTitles` (0023 §9; 09 §4.8): the reserved titles, each with the
/// group that may create it and when the reservation ends, from `list=protectedtitles`, so
/// a confidential reservation is listed only to those who may read it.
pub async fn titles(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Protect) {
        return pages::no_page(&cx, headers, "Special:ProtectedTitles".to_string());
    }
    let found = match site.client().protected_titles(&incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(found.cache);
    let m = &cx.m;
    let model = ProtectedTitlesPage {
        intro: m.get("ts-protectedtitles-intro"),
        nothing: found
            .value
            .is_empty()
            .then(|| m.get("ts-protectedtitles-none")),
        columns: [
            "ts-protectedpages-col-title",
            "ts-protectedpages-col-group",
            "ts-protectedpages-col-expiry",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        rows: found
            .value
            .iter()
            .map(|r| RestrictionRow {
                page: link_to(format!("/wiki/{}", title_url(&r.title)), r.title.clone()),
                action: None,
                group: r.level.clone(),
                expiry: expiry_text(m, &r.expiry),
            })
            .collect(),
        all: link_to(
            "/wiki/Special:ProtectedPages".into(),
            m.get("ts-protectedtitles-all"),
        ),
    };
    let body = cx.draw("special/protectedtitles.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-protectedtitles-title"),
        returnto: Some("Special:ProtectedTitles".to_string()),
        body,
        ..Page::default()
    })
}

/// `GET Special:ProtectedPages`.
pub async fn serve(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let notice = params.map.get("done").map(|d| {
        (
            MessageKind::Success,
            cx.m.get(match d.as_str() {
                "created" => "ts-protectedpages-done-created",
                "changed" => "ts-protectedpages-done-changed",
                "added" => "ts-protectedpages-done-added",
                "removed" => "ts-protectedpages-done-removed",
                "retired" => "ts-protectedpages-done-retired",
                _ => "ts-protectedpages-done",
            }),
        )
    });
    render(site, &mut cx, headers, &incoming, notice).await
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "ts-locked-out" => m.get("ts-protect-locked-out"),
        "ts-policy" => m.get("ts-protect-policy"),
        "ts-unknown-group" => m.get("ts-protect-unknown-group"),
        "ts-no-target" | "missingtitle" => m.get("ts-protectedpages-no-page"),
        "ts-already-member" => m.get("ts-protectedpages-already-member"),
        "badbody" => m.get("ts-protectedpages-set-needs-restriction"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" => m.get("ts-protect-denied"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `POST Special:ProtectedPages`: one of the Sets tab's forms.
#[allow(clippy::too_many_lines)] // one match over the forms, read top to bottom
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let token = field("token");
    let set = field("set");
    let (done, result) = match field("do") {
        "create" => {
            let mut restrictions = serde_json::Map::new();
            for action in ["read", "edit"] {
                let g = field(action).trim();
                if !g.is_empty() {
                    restrictions.insert(action.into(), json!({"group": g}));
                }
            }
            let body = json!({
                "name": field("name").trim(),
                "restrictions": restrictions,
                "members": [],
                "token": token,
            });
            (
                "created",
                site.client()
                    .set_write(&incoming, Method::POST, "", &body)
                    .await,
            )
        }
        "change" => {
            let mut restrictions = serde_json::Map::new();
            for action in ["read", "edit"] {
                let g = field(action).trim();
                if !g.is_empty() {
                    restrictions.insert(action.into(), json!({"group": g}));
                }
            }
            let body = json!({
                "name": field("name").trim(),
                "restrictions": restrictions,
                "token": token,
            });
            (
                "changed",
                site.client()
                    .set_write(&incoming, Method::PUT, &format!("/{set}"), &body)
                    .await,
            )
        }
        "add-member" => {
            // The member is named by title; the API takes its page ID.
            let title = field("member").trim();
            let info = match site.client().page_info(&incoming, title).await {
                Ok(i) => i,
                Err(e) => return pages::api_failed(&cx, headers, &e),
            };
            let Lookup::Found(p) = info.value else {
                let notice = Some((MessageKind::Error, cx.m.get("ts-protectedpages-no-page")));
                return render(site, &mut cx, headers, &incoming, notice).await;
            };
            (
                "added",
                site.client()
                    .set_write(
                        &incoming,
                        Method::POST,
                        &format!("/{set}/members"),
                        &json!({"kind": "page", "id": p.page_id, "token": token}),
                    )
                    .await,
            )
        }
        "remove-member" => (
            "removed",
            site.client()
                .set_write(
                    &incoming,
                    Method::DELETE,
                    &format!("/{set}/members/page/{}", field("page")),
                    &json!({"token": token}),
                )
                .await,
        ),
        "retire" => (
            "retired",
            site.client()
                .set_write(
                    &incoming,
                    Method::DELETE,
                    &format!("/{set}"),
                    &json!({"token": token}),
                )
                .await,
        ),
        _ => return render(site, &mut cx, headers, &incoming, None).await,
    };
    match result {
        Ok(Saved::Ok(_)) => {
            pages::see_other(&format!("/wiki/{}?done={done}", title_url(TITLE)), &[])
        }
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            render(
                site,
                &mut cx,
                headers,
                &incoming,
                Some((MessageKind::Error, text)),
            )
            .await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_lists_its_members_and_offers_forms_to_a_manager() {
        let m = Messages::for_language("en");
        let set = SetInfo {
            id: 4,
            name: "Board docs".into(),
            protection: vec![("read".into(), "board".into(), "infinity".into())],
            members: vec![(9, 4, "Project:Minutes".into())],
        };
        let groups = vec!["board".to_string(), "sysop".to_string()];
        let reader = set_model(&m, &set, &groups, None);
        assert_eq!(reader.name, "Board docs");
        assert_eq!(reader.members[0].page.href, "/wiki/Project:Minutes");
        assert!(reader.manage.is_none() && reader.members[0].remove.is_none());
        let manager = set_model(&m, &set, &groups, Some("t"));
        let manage = manager.manage.expect("the forms");
        assert!(matches!(
            &manage.read.control,
            crate::special::controls::ControlModel::Select { options, .. }
                if options.iter().any(|o| o.value == "board" && o.selected)
        ));
        assert_eq!(manager.members[0].page_id, 9);
        assert!(manager.members[0].remove.is_some());
    }
}
