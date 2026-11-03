//! `Special:Version` (ADR 0077): the credit line and licence notice, installed software and
//! services, features, entry points, components, wikitext, the extensions that inspired
//! features and the AI agents used, all from `GET /version`; and its subpages:
//!
//! - `Special:Version/Credits`: every contributor and funder, and the AI agents (0077 §2).
//! - `Special:Version/License`: the GNU GPL, verbatim and in English only, from the
//!   `LICENSE` this build embeds (0077 §3).
//! - `Special:Version/License/{component}`: a component's licence files, from
//!   `GET /version/licenses/{id}`.
//!
//! The page names no ADRs (0077 A1).

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::Value;
use triplespace_client::version::{Build, Component, FeatureSwitch, Version};

use crate::Site;
use crate::codex::{CellModel, ColumnModel, Message, MessageKind, MessageModel, TableModel};
use crate::features::Feature;
use crate::frame::Page;
use crate::i18n::Messages;
use crate::inline::{self, Inline};
use crate::pages::{self, Context, Peer};

/// The GNU General Public License, version 3, as the repository carries it: every build
/// serves its own copy (0077 §3).
pub const GPL: &str = include_str!("../../../../LICENSE");

/// How many contributors the credit line names before "and others" (0077 §2).
pub const CREDIT_LINE_NAMES: usize = 10;

/// A lineage list longer than this opens on demand.
pub const LONG_LIST: usize = 20;

/// The page's own title as shown: `Special:Version/License/vue@3.5.43` → `License/vue@3.5.43`.
fn subpage(title: &str) -> &str {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map_or("", |(_, rest)| rest)
}

/// `Special:Version` and its subpages as Plain draws them: a list of blocks.
#[derive(Debug, Clone, serde::Serialize)]
pub struct VersionPage {
    /// The blocks, in order.
    pub blocks: Vec<Block>,
}

/// One block of the page.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Block {
    /// A heading.
    Heading {
        /// Its anchor.
        id: Option<String>,
        /// Its text.
        text: String,
    },
    /// A paragraph.
    Para {
        /// Its parts.
        parts: Vec<Inline>,
    },
    /// Paragraphs inside a `div` of their own (the credit line and notice).
    Div {
        /// The `div`'s class.
        class: String,
        /// Its paragraphs.
        paras: Vec<Vec<Inline>>,
    },
    /// A table.
    Table {
        /// The table.
        table: TableModel,
    },
    /// A table that opens on demand.
    Accordion {
        /// What it is, and how many.
        title: String,
        /// The table.
        table: TableModel,
    },
    /// A notice.
    Message {
        /// The notice.
        message: MessageModel,
    },
    /// Preformatted text: a licence.
    Pre {
        /// Its language, where it is not the page's.
        lang: Option<String>,
        /// The text.
        text: String,
    },
}

fn heading(id: &str, text: String) -> Block {
    Block::Heading {
        id: Some(id.to_string()),
        text,
    }
}

fn table_model(
    caption: String,
    header: bool,
    columns: &[String],
    rows: Vec<Vec<CellModel>>,
) -> TableModel {
    TableModel {
        header: header.then(|| caption.clone()),
        caption,
        columns: columns
            .iter()
            .map(|c| ColumnModel::new(c.clone(), crate::codex::Align::Start))
            .collect(),
        rows,
        empty: String::new(),
    }
}

fn table_block(caption: String, columns: &[String], rows: Vec<Vec<CellModel>>) -> Block {
    Block::Table {
        table: table_model(caption, false, columns, rows),
    }
}

/// A name, linked where it has a URL the site may link.
fn named(name: &str, url: Option<&str>) -> Vec<Inline> {
    match url.filter(|u| crate::html::linkable(u)) {
        Some(u) => vec![Inline::external(u, name)],
        None => inline::text(name),
    }
}

/// The credit line, the funders' sentence and the licence notice, as [`credit_and_notice`].
fn credits_block(m: &Messages, v: &Version) -> Block {
    let product = vec![Inline::external(&v.developer.source, &v.product)];
    let developer = inline::text(&v.developer.name);
    let place = inline::text(&v.developer.place);
    let mut names: Vec<Vec<Inline>> = v
        .contributors
        .iter()
        .take(CREDIT_LINE_NAMES)
        .map(|c| named(&c.name, c.url.as_deref()))
        .collect();
    if v.contributors.len() > CREDIT_LINE_NAMES {
        names.push(vec![Inline::link(
            "/wiki/Special:Version/Credits",
            m.get("ts-version-poweredby-others"),
        )]);
    }
    let mut paras = vec![if names.is_empty() {
        inline::message(m, "ts-version-poweredby", vec![product, developer, place])
    } else {
        let joined = inline::list(m, names);
        inline::message(
            m,
            "ts-version-poweredby-contributors",
            vec![product, developer, place, joined],
        )
    }];
    if !v.funders.is_empty() {
        let funders = v
            .funders
            .iter()
            .map(|f| named(&f.name, f.url.as_deref()))
            .collect();
        paras.push(inline::message(
            m,
            "ts-version-funders",
            vec![inline::list(m, funders)],
        ));
    }
    let name = || inline::text(&v.product);
    paras.push(inline::message(m, "ts-version-license-info", vec![name()]));
    paras.push(inline::message(
        m,
        "ts-version-license-warranty",
        vec![name()],
    ));
    paras.push(inline::message(
        m,
        "ts-version-license-copy",
        vec![
            vec![Inline::link(
                "/wiki/Special:Version/License",
                m.get("ts-version-license-copy-link"),
            )],
            vec![Inline::external(
                "https://www.gnu.org/licenses/",
                "https://www.gnu.org/licenses/",
            )],
        ],
    ));
    paras.push(inline::text(
        m.with("ts-version-dual-and-cc0", &[&v.developer.name]),
    ));
    paras.push(vec![Inline::link(
        "/wiki/Special:Version/Credits",
        m.get("ts-version-credits-link"),
    )]);
    Block::Div {
        class: "ts-version__credits".into(),
        paras,
    }
}

fn build_parts(m: &Messages, b: &Build) -> Vec<Vec<Inline>> {
    let mut parts = Vec::new();
    if !b.commit.is_empty() {
        parts.push(vec![Inline::code(
            b.commit.chars().take(12).collect::<String>(),
        )]);
    }
    if !b.commit_date.is_empty() {
        parts.push(inline::text(&b.commit_date));
    }
    if b.modified {
        parts.push(inline::text(m.get("ts-version-build-modified")));
    }
    parts
}

fn details_parts(b: &Build) -> Vec<Inline> {
    let mut parts = Vec::new();
    for p in [&b.rustc, &b.target] {
        if !p.is_empty() {
            parts.push(inline::text(p.as_str()));
        }
    }
    if !b.features.is_empty() {
        parts.push(inline::text(b.features.join(", ")));
    }
    inline::join(parts, " · ")
}

fn version_cell(m: &Messages, version: &str, b: &Build) -> CellModel {
    let mut parts = vec![inline::text(version)];
    parts.extend(build_parts(m, b));
    let mut joined = inline::join(parts, " · ");
    if build_parts(m, b).is_empty() {
        joined.push(Inline::text(" · "));
    }
    CellModel::of(joined)
}

fn installed_blocks(m: &Messages, v: &Version, local: Option<&Build>) -> Vec<Block> {
    let source = match v.license.source.as_deref() {
        Some(u) => vec![Inline::external(u, m.get("ts-version-source"))],
        None => inline::text(m.get("ts-version-modified-unsourced")),
    };
    let mut details = details_parts(&v.build);
    details.push(Inline::Break);
    details.extend(source);
    let mut rows = vec![vec![
        CellModel::heading(inline::text(&v.product)),
        version_cell(m, &v.build.version, &v.build),
        CellModel::of(details),
    ]];
    if let Some(w) = local {
        rows.push(vec![
            CellModel::heading(inline::text(m.with("ts-version-webtier", &[&v.product]))),
            version_cell(m, &w.version, w),
            CellModel::of(details_parts(w)),
        ]);
    }
    for s in &v.services {
        let state = m.get(if s.state == "connected" {
            "ts-version-state-connected"
        } else {
            "ts-version-state-unreachable"
        });
        let mut what = Vec::new();
        if let Some(r) = s.role.as_deref() {
            what.push(Inline::text(format!("{r} · ")));
        }
        what.push(Inline::text(state));
        rows.push(vec![
            CellModel::heading(inline::text(&s.name)),
            CellModel::text(s.version.as_deref().unwrap_or("")),
            CellModel::of(inline::join(vec![what], "")),
        ]);
    }
    vec![
        heading("ts-version-installed", m.get("ts-version-installed")),
        table_block(
            m.get("ts-version-installed"),
            &[
                m.get("ts-version-col-product"),
                m.get("ts-version-col-version"),
                m.get("ts-version-col-details"),
            ],
            rows,
        ),
    ]
}

fn value_parts(m: &Messages, f: &FeatureSwitch) -> Vec<Inline> {
    let shown = match &f.value {
        None => return inline::text(m.get("ts-version-not-set")),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => (if *b { "true" } else { "false" }).to_string(),
        Some(Value::Array(a)) if a.is_empty() => return inline::text(m.get("ts-version-none")),
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_string))
            .collect::<Vec<_>>()
            .join(", "),
        Some(other) => other.to_string(),
    };
    let mut parts = vec![Inline::code(shown)];
    if !f.set {
        parts.push(Inline::text(format!(" {}", m.get("ts-version-default"))));
    }
    parts
}

fn feature_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    let mut out = Vec::new();
    let mut section = |caption: String, list: &[FeatureSwitch], extra: Vec<Vec<CellModel>>| {
        let mut rows: Vec<Vec<CellModel>> = list
            .iter()
            .map(|f| {
                vec![
                    CellModel::heading(vec![Inline::code(&f.key)]),
                    CellModel::of(value_parts(m, f)),
                ]
            })
            .collect();
        rows.extend(extra);
        out.push(Block::Table {
            table: table_model(
                caption,
                true,
                &[
                    m.get("ts-version-col-setting"),
                    m.get("ts-version-col-value"),
                ],
                rows,
            ),
        });
    };
    if let Some(t) = &v.features.tenant {
        let providers = if v.features.providers.is_empty() {
            m.get("ts-version-none")
        } else {
            v.features.providers.join(", ")
        };
        section(
            m.get("ts-version-features-tenant"),
            t,
            vec![vec![
                CellModel::heading(inline::text(m.get("ts-version-providers"))),
                CellModel::text(&providers),
            ]],
        );
    }
    if let Some(i) = &v.features.instance {
        section(m.get("ts-version-features-instance"), i, Vec::new());
    }
    if out.is_empty() {
        return out;
    }
    out.insert(
        0,
        heading("ts-version-features", m.get("ts-version-features")),
    );
    out
}

fn entry_point_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    let rows = v
        .entry_points
        .iter()
        .map(|e| {
            vec![
                CellModel::heading(inline::text(m.get(&format!(
                    "ts-version-entry-{}",
                    e.name.replace(['.', '/'], "-")
                )))),
                CellModel::of(vec![Inline::code(&e.url)]),
            ]
        })
        .collect();
    vec![
        heading("ts-version-entrypoints", m.get("ts-version-entrypoints")),
        table_block(
            m.get("ts-version-entrypoints"),
            &[
                m.get("ts-version-col-entrypoint"),
                m.get("ts-version-col-url"),
            ],
            rows,
        ),
    ]
}

fn component_cells(list: &[Component]) -> Vec<Vec<CellModel>> {
    list.iter()
        .map(|c| {
            vec![
                CellModel::heading(named(&c.name, c.repository.as_deref())),
                CellModel::text(&c.version),
                CellModel::of(vec![Inline::link(
                    format!(
                        "/wiki/Special:Version/License/{}",
                        crate::frame::title_url(&c.id)
                    ),
                    &c.license,
                )]),
            ]
        })
        .collect()
}

fn component_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    let mut out = vec![heading("ts-version-crates", m.get("ts-version-crates"))];
    if !v.components_listed {
        out.push(Block::Message {
            message: Message::block(
                MessageKind::Notice,
                &m.get("ts-version-components-unlisted"),
            )
            .model(),
        });
        return out;
    }
    let ws_rows = v
        .workspace
        .iter()
        .map(|c| {
            let licence = if c.commercial {
                format!("{} {}", c.license, m.get("ts-version-commercial-mark"))
            } else {
                c.license.clone()
            };
            vec![
                CellModel::heading(inline::text(&c.name)),
                CellModel::text(&c.version),
                CellModel::text(&licence),
            ]
        })
        .collect();
    let cols = [
        m.get("ts-version-col-name"),
        m.get("ts-version-col-version"),
        m.get("ts-version-col-license"),
    ];
    out.push(table_block(m.get("ts-version-crates"), &cols, ws_rows));
    out.push(heading(
        "ts-version-components",
        m.get("ts-version-components"),
    ));
    for (key, list) in [
        ("ts-version-components-crates", &v.crates),
        ("ts-version-components-packages", &v.packages),
        ("ts-version-components-vendored", &v.vendored),
    ] {
        if list.is_empty() {
            continue;
        }
        out.push(Block::Accordion {
            title: m.with(&format!("{key}-count"), &[&list.len().to_string()]),
            table: table_model(m.get(key), false, &cols, component_cells(list)),
        });
    }
    out
}

fn wikitext_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    if v.wikitext.is_empty() {
        return Vec::new();
    }
    let rows = v
        .wikitext
        .iter()
        .map(|w| {
            let shown = if w.written.is_empty() {
                &w.name
            } else {
                &w.written
            };
            vec![
                CellModel::heading(vec![Inline::code(shown)]),
                CellModel::text(w.origin.as_deref().unwrap_or("")),
                CellModel::text(&m.get(&format!("ts-version-wikitext-{}", w.status))),
                CellModel::text(&m.get(if w.active {
                    "ts-version-active"
                } else {
                    "ts-version-inactive"
                })),
            ]
        })
        .collect();
    vec![
        heading("ts-version-wikitext", m.get("ts-version-wikitext")),
        Block::Accordion {
            title: m.with(
                "ts-version-wikitext-count",
                &[&v.wikitext.len().to_string()],
            ),
            table: table_model(
                m.get("ts-version-wikitext"),
                false,
                &[
                    m.get("ts-version-col-name"),
                    m.get("ts-version-col-origin"),
                    m.get("ts-version-col-status"),
                    m.get("ts-version-col-active"),
                ],
                rows,
            ),
        },
    ]
}

fn lineage_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    let rows = v
        .inspired_by
        .iter()
        .map(|l| {
            let mut what: Vec<Vec<Inline>> = Vec::new();
            let mut part = |key: &str, items: &[String]| {
                if items.len() > LONG_LIST {
                    what.push(vec![Inline::Details {
                        title: m.with(&format!("{key}-count"), &[&items.len().to_string()]),
                        text: items.join(", "),
                    }]);
                } else if !items.is_empty() {
                    what.push(inline::text(format!("{} {}", m.get(key), items.join(", "))));
                }
            };
            part("ts-version-from-special-pages", &l.special_pages);
            part("ts-version-from-content-models", &l.content_models);
            part("ts-version-from-wikitext", &l.wikitext.implemented);
            part("ts-version-from-recognized", &l.wikitext.recognized);
            for i in &l.inspired_by {
                what.push(inline::text(i.as_str()));
            }
            let mut cell = Vec::new();
            for (i, w) in what.into_iter().enumerate() {
                if i > 0 {
                    cell.push(Inline::Break);
                }
                cell.extend(w);
            }
            vec![
                CellModel::heading(named(&l.name, l.url.as_deref())),
                CellModel::of(cell),
            ]
        })
        .collect();
    vec![
        heading("ts-version-inspiredby", m.get("ts-version-inspiredby")),
        Block::Para {
            parts: inline::text(m.with("ts-version-inspiredby-intro", &[&v.product])),
        },
        table_block(
            m.get("ts-version-inspiredby"),
            &[m.get("ts-version-col-name"), m.get("ts-version-col-what")],
            rows,
        ),
    ]
}

fn agents_block(m: &Messages, v: &Version) -> Block {
    let rows = v
        .agents
        .iter()
        .map(|a| {
            vec![
                CellModel::heading(inline::text(&a.name)),
                CellModel::text(&a.maker),
                CellModel::text(&a.versions.join(", ")),
            ]
        })
        .collect();
    table_block(
        m.get("ts-version-agents"),
        &[
            m.get("ts-version-col-name"),
            m.get("ts-version-col-maker"),
            m.get("ts-version-col-versions"),
        ],
        rows,
    )
}

fn agent_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    if v.agents.is_empty() {
        return Vec::new();
    }
    vec![
        heading("ts-version-agents", m.get("ts-version-agents")),
        Block::Para {
            parts: inline::text(m.with("ts-version-agents-intro", &[&v.product])),
        },
        agents_block(m, v),
    ]
}

fn instance_blocks(m: &Messages, v: &Version) -> Vec<Block> {
    let mut rows = vec![vec![
        CellModel::heading(inline::text(m.get("ts-version-instance-name"))),
        CellModel::text(&v.instance.name),
    ]];
    if let Some(owner) = &v.instance.owner_of_record {
        let shown = owner
            .as_str()
            .map_or_else(|| owner.to_string(), str::to_string);
        rows.push(vec![
            CellModel::heading(inline::text(m.get("ts-version-owner"))),
            CellModel::text(&shown),
        ]);
    }
    vec![
        heading("ts-version-instance", m.get("ts-version-instance")),
        table_block(
            m.get("ts-version-instance"),
            &[
                m.get("ts-version-col-setting"),
                m.get("ts-version-col-value"),
            ],
            rows,
        ),
    ]
}

/// The page, in the order of 0077 §1, as `special/version.html` draws it.
#[must_use]
pub fn page_model(m: &Messages, v: &Version, local: Option<&Build>) -> VersionPage {
    let mut blocks = vec![credits_block(m, v)];
    blocks.extend(installed_blocks(m, v, local));
    blocks.extend(feature_blocks(m, v));
    blocks.extend(entry_point_blocks(m, v));
    blocks.extend(component_blocks(m, v));
    blocks.extend(wikitext_blocks(m, v));
    blocks.extend(lineage_blocks(m, v));
    blocks.extend(agent_blocks(m, v));
    blocks.extend(instance_blocks(m, v));
    VersionPage { blocks }
}

/// The back link to the page or one of its sections.
fn back_to(m: &Messages, href: &str) -> Block {
    Block::Para {
        parts: vec![Inline::link(href, m.get("ts-version-back"))],
    }
}

/// `Special:Version/Credits`, as [`credits_page`] draws it.
fn credits_model(m: &Messages, v: &Version) -> VersionPage {
    let mut blocks = vec![
        back_to(m, "/wiki/Special:Version"),
        heading(
            "ts-version-contributors",
            m.get("ts-version-credits-contributors"),
        ),
    ];
    if v.contributors.is_empty() {
        blocks.push(Block::Para {
            parts: inline::text(m.with(
                "ts-version-credits-developer-only",
                &[&v.product, &v.developer.name],
            )),
        });
    } else {
        let rows = v
            .contributors
            .iter()
            .map(|c| {
                vec![
                    CellModel::heading(inline::text(&c.name)),
                    CellModel::text(c.since.as_deref().unwrap_or("")),
                ]
            })
            .collect();
        blocks.push(table_block(
            m.get("ts-version-credits-contributors"),
            &[m.get("ts-version-col-name"), m.get("ts-version-col-since")],
            rows,
        ));
    }
    if !v.funders.is_empty() {
        blocks.push(heading(
            "ts-version-funders",
            m.get("ts-version-credits-funders"),
        ));
        let rows = v
            .funders
            .iter()
            .map(|f| {
                let when = match (&f.since, &f.until) {
                    (Some(s), Some(u)) => format!("{s} – {u}"),
                    (Some(s), None) => s.clone(),
                    _ => String::new(),
                };
                vec![
                    CellModel::heading(inline::text(&f.name)),
                    CellModel::text(f.funded.as_deref().unwrap_or("")),
                    CellModel::text(&when),
                ]
            })
            .collect();
        blocks.push(table_block(
            m.get("ts-version-credits-funders"),
            &[
                m.get("ts-version-col-name"),
                m.get("ts-version-col-funded"),
                m.get("ts-version-col-since"),
            ],
            rows,
        ));
    }
    if !v.agents.is_empty() {
        blocks.push(heading("ts-version-agents", m.get("ts-version-agents")));
        blocks.push(agents_block(m, v));
    }
    VersionPage { blocks }
}

/// `Special:Version/License`: the GPL, in English.
fn license_model(m: &Messages) -> VersionPage {
    VersionPage {
        blocks: vec![
            back_to(m, "/wiki/Special:Version"),
            Block::Pre {
                lang: Some("en".into()),
                text: GPL.to_string(),
            },
        ],
    }
}

/// `Special:Version/License/{component}`.
fn component_license_model(
    m: &Messages,
    c: &triplespace_client::version::ComponentLicense,
) -> VersionPage {
    let mut blocks = vec![
        back_to(m, "/wiki/Special:Version#ts-version-components"),
        Block::Para {
            parts: inline::text(
                m.with("ts-version-license-of", &[&c.name, &c.version, &c.license]),
            ),
        },
    ];
    if let Some(r) = c.repository.as_deref().filter(|r| crate::html::linkable(r)) {
        blocks.push(Block::Para {
            parts: vec![Inline::external(r, r)],
        });
    }
    if c.texts.is_empty() {
        blocks.push(Block::Message {
            message: Message::block(MessageKind::Notice, &m.get("ts-version-license-no-files"))
                .model(),
        });
    }
    for t in &c.texts {
        blocks.push(Block::Heading {
            id: None,
            text: t.file.clone(),
        });
        blocks.push(Block::Pre {
            lang: None,
            text: t.text.clone().unwrap_or_default(),
        });
    }
    VersionPage { blocks }
}

/// A page that is a notice alone.
fn notice_model(kind: MessageKind, text: &str) -> VersionPage {
    VersionPage {
        blocks: vec![Block::Message {
            message: Message::block(kind, text).model(),
        }],
    }
}

/// `Special:Version` and its subpages.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, query).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if !cx.features.has(Feature::Version) {
        return pages::no_page(&cx, headers, "Special:Version".into());
    }
    let sub = subpage(title);
    let lower = sub.to_ascii_lowercase();
    if lower == "license" || lower == "licence" {
        let body = cx.draw("special/version.html", license_model(&cx.m));
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-version-license-title"),
            returnto: Some("Special:Version/License".into()),
            body,
            ..Page::default()
        });
    }
    if let Some(id) = sub
        .strip_prefix("License/")
        .or_else(|| sub.strip_prefix("license/"))
        .or_else(|| sub.strip_prefix("Licence/"))
    {
        return component_license(site, &mut cx, headers, &incoming, id).await;
    }
    let fetched = match site.client().version(&incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(fetched.cache);
    let v = fetched.value;
    let local = site.build().cloned();
    if lower == "credits" {
        let body = cx.draw("special/version.html", credits_model(&cx.m, &v));
        return pages::respond(&cx, headers, StatusCode::OK, |m| Page {
            title: m.get("ts-version-credits-title"),
            returnto: Some("Special:Version/Credits".into()),
            body,
            ..Page::default()
        });
    }
    if !sub.is_empty() {
        let text = cx.m.get("ts-not-found");
        let body = cx.draw(
            "special/version.html",
            notice_model(MessageKind::Warning, &text),
        );
        return pages::respond(&cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-not-found-title"),
            body,
            ..Page::default()
        });
    }
    let body = cx.draw(
        "special/version.html",
        page_model(&cx.m, &v, local.as_ref()),
    );
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-version-title"),
        returnto: Some("Special:Version".into()),
        body,
        ..Page::default()
    })
}

async fn component_license(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &triplespace_client::Incoming,
    id: &str,
) -> Response {
    let fetched = match site.client().component_license(incoming, id).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(fetched.cache);
    let Some(c) = fetched.value else {
        let text = cx.m.with("ts-version-license-no-component", &[id]);
        let body = cx.draw(
            "special/version.html",
            notice_model(MessageKind::Warning, &text),
        );
        return pages::respond(cx, headers, StatusCode::NOT_FOUND, |m| Page {
            title: m.get("ts-not-found-title"),
            body,
            ..Page::default()
        });
    };
    let model = component_license_model(&cx.m, &c);
    let body = cx.draw("special/version.html", &model);
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.with("ts-version-license-component-title", &[&c.id]),
        returnto: Some(format!("Special:Version/License/{}", c.id)),
        body,
        ..Page::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_client::version::{Credit, Developer, License};

    /// A page as Plain draws it.
    fn drawn(m: &Messages, page: &VersionPage) -> String {
        let skins = crate::skin::Skins::load(None);
        skins
            .get("plain")
            .expect("plain")
            .render("special/version.html", m, page)
            .unwrap()
    }

    fn credit_and_notice(m: &Messages, v: &Version) -> String {
        drawn(
            m,
            &VersionPage {
                blocks: vec![credits_block(m, v)],
            },
        )
    }

    fn body(m: &Messages, v: &Version, local: Option<&Build>) -> String {
        drawn(m, &page_model(m, v, local))
    }

    fn credits_page(m: &Messages, v: &Version) -> String {
        drawn(m, &credits_model(m, v))
    }

    fn sample() -> Version {
        serde_json::from_value(serde_json::json!({
            "product": "Triplespace",
            "developer": {"name": "Scatter LLC", "place": "Portland, Oregon", "source": "https://github.com/scatterbase/triplespace"},
            "contributors": [], "funders": [],
            "license": {"spdx": "GPL-3.0-or-later", "source": "https://github.com/scatterbase/triplespace/tree/abc", "modified": false},
            "build": {"version": "0.0.1", "commit": "abcdef0123456789", "modified": false, "rustc": "rustc 1.95.0", "target": "x86_64-unknown-linux-gnu"},
            "services": [{"name": "PostgreSQL", "role": "Log store and serving model", "state": "connected", "version": "16.4"}],
            "components_listed": true,
            "workspace": [{"name": "scatter-log", "version": "0.0.1", "license": "GPL-3.0-or-later", "commercial": true}],
            "crates": [{"id": "url@2.5.4", "name": "url", "version": "2.5.4", "license": "MIT OR Apache-2.0", "repository": "https://github.com/servo/rust-url"}],
            "packages": [{"id": "@wikimedia/codex@2.7.0", "name": "@wikimedia/codex", "version": "2.7.0", "license": "GPL-2.0+"}],
            "vendored": [],
            "features": {"tenant": [{"key": "wikitext.lua", "set": false, "value": "off"}, {"key": "content.licence", "set": false}], "providers": ["wikidata"]},
            "entry_points": [{"name": "api.php", "url": "https://x.example/w/api.php"}],
            "wikitext": [],
            "inspired_by": [{"name": "MediaWiki", "url": "https://www.mediawiki.org/wiki/MediaWiki", "special_pages": ["Search", "Version"], "content_models": [], "wikitext": {"implemented": [], "recognized": []}, "inspired_by": []}],
            "agents": [{"name": "Claude Opus", "maker": "Anthropic", "versions": ["5.5"]}],
            "instance": {"name": "scatter"}
        }))
        .unwrap()
    }

    #[test]
    fn the_credit_line_without_contributors() {
        let m = Messages::for_language("en");
        let html = credit_and_notice(&m, &sample());
        assert!(
            html.contains(
                "This wiki is powered by <a href=\"https://github.com/scatterbase/triplespace\""
            ),
            "{html}"
        );
        assert!(
            html.contains(">Triplespace</a>, developed by Scatter LLC in Portland, Oregon.</p>"),
            "{html}"
        );
        assert!(!html.contains("with contributions"));
        assert!(
            html.contains("either version 3 of the License, or (at your option) any later version")
        );
        assert!(html.contains("<a href=\"/wiki/Special:Version/License\""));
        assert!(!html.contains("Development has been supported"));
    }

    #[test]
    fn the_credit_line_with_contributors_and_funders() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.contributors = ["Alice", "Bob"]
            .iter()
            .map(|n| Credit {
                name: (*n).into(),
                ..Credit::default()
            })
            .collect();
        v.funders = vec![Credit {
            name: "A Foundation".into(),
            ..Credit::default()
        }];
        let html = credit_and_notice(&m, &v);
        assert!(html.contains("developed by Scatter LLC in Portland, Oregon, with contributions from Alice and Bob."), "{html}");
        assert!(
            html.contains("Development has been supported by A Foundation."),
            "{html}"
        );
        v.contributors = (0..12)
            .map(|i| Credit {
                name: format!("C{i}"),
                ..Credit::default()
            })
            .collect();
        let html = credit_and_notice(&m, &v);
        assert!(
            html.contains("C9 and <a href=\"/wiki/Special:Version/Credits\">others</a>."),
            "{html}"
        );
        assert!(!html.contains("C10"));
    }

    #[test]
    fn names_are_escaped() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.developer = Developer {
            name: "<b>X</b>".into(),
            ..v.developer
        };
        let html = credit_and_notice(&m, &v);
        assert!(html.contains("&lt;b&gt;X&lt;/b&gt;") && !html.contains("<b>X"));
    }

    #[test]
    fn the_body_in_order() {
        let m = Messages::for_language("en");
        let html = body(&m, &sample(), None);
        let order = [
            "This wiki is powered by",
            "Installed software",
            "PostgreSQL",
            "Features",
            "Entry point URLs",
            "Triplespace crates",
            "Third-party components",
            "Inspired by",
            "AI agents",
            "Instance",
        ];
        let mut at = 0;
        for o in order {
            let i = html[at..]
                .find(o)
                .unwrap_or_else(|| panic!("{o} missing or out of order"));
            at += i;
        }
        let href = format!(
            "/wiki/Special:Version/License/{}",
            crate::frame::title_url("@wikimedia/codex@2.7.0")
        );
        assert!(html.contains(&href), "{html}");
        assert!(html.contains("also available under a commercial licence"));
        assert!(html.contains("not set"));
        assert!(!html.contains("⧼"), "a message is missing: {html}");
        assert!(!html.contains("ADR"));
    }

    #[test]
    fn an_unlisted_build_and_a_modified_one() {
        let m = Messages::for_language("en");
        let mut v = sample();
        v.components_listed = false;
        v.license = License {
            source: None,
            modified: true,
            ..v.license
        };
        let html = body(&m, &v, None);
        assert!(
            html.contains("Development build: components are not listed."),
            "{html}"
        );
        assert!(html.contains("source location not configured"));
    }

    #[test]
    fn the_credits_page() {
        let m = Messages::for_language("en");
        let html = credits_page(&m, &sample());
        assert!(html.contains("Claude Opus"));
        assert!(!html.contains("⧼"), "{html}");
    }

    #[test]
    fn subpages() {
        assert_eq!(subpage("Special:Version"), "");
        assert_eq!(
            subpage("Special:Version/License/@wikimedia/codex@2.7.0"),
            "License/@wikimedia/codex@2.7.0"
        );
        assert!(GPL.contains("GNU GENERAL PUBLIC LICENSE"));
    }
}
