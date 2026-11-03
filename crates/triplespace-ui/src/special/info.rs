//! Read-only pages about the wiki itself (0047 §9; 21 §6.1), each drawn from what the API
//! reports or the site knows:
//!
//! - `Special:NamespaceInfo`: the namespaces (`siprop=namespaces|namespacealiases`), with
//!   their names, other names and properties.
//! - `Special:ListGroupRights` (`UserGroupRights`): each group and its permissions
//!   (`siprop=usergroups`).
//! - `Special:ListDatatypes`: the data types the repository registers
//!   (`general.wikibase-propertytypes`), with their value types, in the order
//!   `Special:NewProperty` offers the same types.
//! - `Special:MyLanguageFallbackChain`: the languages a label, description or alias is
//!   looked up in for the viewer, in order (0009 §5).

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

use crate::Site;
use crate::frame::Page;
use crate::pages::{self, Context, Params, Peer};

/// The pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:NamespaceInfo`.
    NamespaceInfo,
    /// `Special:ListGroupRights`.
    ListGroupRights,
    /// `Special:ListDatatypes`.
    ListDatatypes,
    /// `Special:MyLanguageFallbackChain`.
    MyLanguageFallbackChain,
}

impl Which {
    /// The page a special page's name (or a registry alias) names.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        Some(match name {
            "NamespaceInfo" => Self::NamespaceInfo,
            "ListGroupRights" | "UserGroupRights" => Self::ListGroupRights,
            "ListDatatypes" => Self::ListDatatypes,
            "MyLanguageFallbackChain" => Self::MyLanguageFallbackChain,
            _ => return None,
        })
    }

    fn title(self) -> &'static str {
        match self {
            Self::NamespaceInfo => "Special:NamespaceInfo",
            Self::ListGroupRights => "Special:ListGroupRights",
            Self::ListDatatypes => "Special:ListDatatypes",
            Self::MyLanguageFallbackChain => "Special:MyLanguageFallbackChain",
        }
    }

    fn title_key(self) -> &'static str {
        match self {
            Self::NamespaceInfo => "ts-namespaceinfo-title",
            Self::ListGroupRights => "ts-listgrouprights-title",
            Self::ListDatatypes => "ts-listdatatypes-title",
            Self::MyLanguageFallbackChain => "ts-fallbackchain-title",
        }
    }
}

/// `GET Special:{which}`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let model = match which {
        Which::NamespaceInfo => match site.client().namespaces(&incoming).await {
            Ok(found) => {
                cx.inputs.push(found.cache);
                namespaces_model(&cx, &found.value)
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        Which::ListGroupRights => match site.client().groups(&incoming).await {
            Ok(found) => {
                cx.inputs.push(found.cache);
                groups_model(&cx, &found.value)
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        },
        Which::ListDatatypes => datatypes_model(&cx),
        Which::MyLanguageFallbackChain => chain_model(&cx),
    };
    let body = cx.draw("special/info.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get(which.title_key()),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    })
}

/// One of the pages as Plain's `special/info.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InfoPage {
    /// What the page lists; none where it lists nothing.
    pub intro: Option<String>,
    /// Said where there is nothing to list.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// What it lists.
    pub body: InfoBody,
    /// A paragraph after the list.
    pub outro: Option<String>,
}

/// What a page lists.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InfoBody {
    /// `Special:NamespaceInfo`.
    Namespaces {
        /// One row per namespace.
        rows: Vec<NamespaceLine>,
    },
    /// `Special:ListGroupRights`.
    Groups {
        /// One row per group.
        rows: Vec<GroupLine>,
    },
    /// `Special:ListDatatypes`.
    Datatypes {
        /// `(name, ID, value type)`, in the order `NewProperty` offers them.
        rows: Vec<(String, String, String)>,
    },
    /// `Special:MyLanguageFallbackChain`.
    Chain {
        /// `(language, note)`, in order.
        langs: Vec<(String, Option<String>)>,
    },
}

/// A namespace.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NamespaceLine {
    /// Its number.
    pub id: i64,
    /// Its name; for the main namespace, what it is called instead.
    pub name: String,
    /// Whether it is the main namespace, which has no name.
    pub main: bool,
    /// Its canonical name, where it differs.
    pub canonical: String,
    /// Its other names, joined.
    pub aliases: String,
    /// What is notable about it.
    pub notes: Vec<String>,
}

/// A group.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupLine {
    /// Its name.
    pub name: String,
    /// "(everyone)" or "(implicit)".
    pub note: Option<String>,
    /// Its permissions, sorted.
    pub rights: Vec<String>,
    /// Said where it holds none.
    pub no_rights: Option<String>,
}

fn columns(cx: &Context, keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| cx.m.get(k)).collect()
}

fn nothing(cx: &Context, key: &str, body: InfoBody) -> InfoPage {
    InfoPage {
        intro: None,
        nothing: Some(cx.m.get(key)),
        columns: Vec::new(),
        body,
        outro: None,
    }
}

fn namespaces_model(cx: &Context, rows: &[triplespace_client::NamespaceRow]) -> InfoPage {
    let m = &cx.m;
    if rows.is_empty() {
        return nothing(
            cx,
            "ts-namespaceinfo-none",
            InfoBody::Namespaces { rows: Vec::new() },
        );
    }
    InfoPage {
        intro: Some(m.get("ts-namespaceinfo-intro")),
        nothing: None,
        columns: columns(
            cx,
            &[
                "ts-namespaceinfo-col-id",
                "ts-namespaceinfo-col-name",
                "ts-namespaceinfo-col-canonical",
                "ts-namespaceinfo-col-aliases",
                "ts-namespaceinfo-col-notes",
            ],
        ),
        body: InfoBody::Namespaces {
            rows: rows
                .iter()
                .map(|n| {
                    let mut notes = Vec::new();
                    if n.content {
                        notes.push(m.get("ts-namespaceinfo-content"));
                    }
                    if n.subpages {
                        notes.push(m.get("ts-namespaceinfo-subpages"));
                    }
                    if n.case == "case-sensitive" {
                        notes.push(m.get("ts-namespaceinfo-case-sensitive"));
                    }
                    if let Some(model) = &n.model {
                        notes.push(m.with("ts-namespaceinfo-model", &[model]));
                    }
                    NamespaceLine {
                        id: n.id,
                        main: n.id == 0,
                        name: if n.id == 0 {
                            m.get("ts-namespaceinfo-main")
                        } else {
                            n.name.clone()
                        },
                        canonical: n
                            .canonical
                            .clone()
                            .filter(|c| *c != n.name)
                            .unwrap_or_default(),
                        aliases: n.aliases.join(", "),
                        notes,
                    }
                })
                .collect(),
        },
        outro: None,
    }
}

fn groups_model(cx: &Context, groups: &[triplespace_client::rights::GroupInfo]) -> InfoPage {
    let m = &cx.m;
    if groups.is_empty() {
        return nothing(
            cx,
            "ts-listgrouprights-none",
            InfoBody::Groups { rows: Vec::new() },
        );
    }
    InfoPage {
        intro: Some(m.get("ts-listgrouprights-intro")),
        nothing: None,
        columns: columns(
            cx,
            &[
                "ts-listgrouprights-col-group",
                "ts-listgrouprights-col-rights",
            ],
        ),
        body: InfoBody::Groups {
            rows: groups
                .iter()
                .map(|g| {
                    let mut rights = g.rights.clone();
                    rights.sort();
                    GroupLine {
                        name: g.name.clone(),
                        note: if g.name == "*" {
                            Some(m.get("ts-listgrouprights-everyone"))
                        } else if g.implicit {
                            Some(m.get("ts-listgrouprights-implicit"))
                        } else {
                            None
                        },
                        no_rights: rights
                            .is_empty()
                            .then(|| m.get("ts-listgrouprights-no-rights")),
                        rights,
                    }
                })
                .collect(),
        },
        outro: None,
    }
}

fn datatypes_model(cx: &Context) -> InfoPage {
    let m = &cx.m;
    let types = &cx.site.property_types;
    if types.is_empty() {
        return nothing(
            cx,
            "ts-listdatatypes-none",
            InfoBody::Datatypes { rows: Vec::new() },
        );
    }
    InfoPage {
        intro: Some(m.get("ts-listdatatypes-intro")),
        nothing: None,
        columns: columns(
            cx,
            &[
                "ts-listdatatypes-col-name",
                "ts-listdatatypes-col-id",
                "ts-listdatatypes-col-valuetype",
            ],
        ),
        body: InfoBody::Datatypes {
            rows: super::new_entity::offered(&cx.site)
                .iter()
                .filter_map(|id| types.iter().find(|(t, _)| t == id))
                .map(|(id, vt)| {
                    (
                        super::new_entity::datatype_name(m, id),
                        id.clone(),
                        vt.clone(),
                    )
                })
                .collect(),
        },
        outro: None,
    }
}

fn chain_model(cx: &Context) -> InfoPage {
    let m = &cx.m;
    InfoPage {
        intro: Some(m.get("ts-fallbackchain-intro")),
        nothing: None,
        columns: Vec::new(),
        body: InfoBody::Chain {
            langs: m
                .term_chain()
                .into_iter()
                .map(|l| {
                    let note = (l == "mul").then(|| m.get("ts-fallbackchain-mul"));
                    (l, note)
                })
                .collect(),
        },
        outro: Some(m.get("ts-fallbackchain-uselang")),
    }
}
