//! `Special:Block` and `Special:Unblock` (09 §3.3, §6.3; 0016 §7; 19 §6.3), MediaWiki's
//! addresses, with `Special:Block/{name}` prefilled.
//!
//! - **Block**: the account, an expiry (`infinite` by default, a time, or a span such as
//!   `1 week`), a reason, and the options the API serves: **also prevent account
//!   creation** (`nocreate`), **block everything but reading** (`tsallbutread`), and for a
//!   holder of `hideuser` **hide the username** (`hidename`), which the page says needs an
//!   indefinite block and is undone by an unblock from a holder of that right (0056 A25).
//!   A block in force is replaced when **replace the current block** is on (`reblock`).
//! - **Unblock**: the account and a reason. An administrator without `hideuser` is told
//!   that unblocking a hidden user leaves the name hidden.
//! - **`Special:BlockList`**: the blocks in force (`list=blocks`): who, by whom, when,
//!   until when, why, what the block removes; a hidden name shows as such to those outside
//!   the suppression group and carries a **Hidden** mark for those in it (19 §6.3); an
//!   **Unblock** link for a holder of `block`.
//!
//! The API's refusal comes back on the form: `alreadyblocked`, `ipb_expiry_temp`,
//! `ts-cantblock-self`, `cantunblock`, `nosuchuser`, `permissiondenied`, `badtoken`.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use triplespace_client::settings::Saved;
use triplespace_client::{BlockForm, Incoming};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::moderation::{GateModel, link_to};
use crate::pages::{self, Context, Params, Peer};
use crate::special::controls::{CheckRow, ControlModel};

/// Which form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// `Special:Block`.
    Block,
    /// `Special:Unblock`.
    Unblock,
}

impl Which {
    fn title(self) -> &'static str {
        match self {
            Self::Block => "Special:Block",
            Self::Unblock => "Special:Unblock",
        }
    }
}

/// The name a title carries: `Special:Block/Mallory`.
fn subject_of(title: &str) -> Option<String> {
    title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest.replace('_', " ").trim().to_string())
        .filter(|s| !s.is_empty())
}

/// What the form shows when it comes back.
#[derive(Debug, Default)]
#[allow(clippy::struct_excessive_bools, reason = "the form's four checkboxes")]
struct Returned {
    notice: Option<(MessageKind, String)>,
    user: String,
    expiry: String,
    reason: String,
    nocreate: bool,
    all_but_read: bool,
    hidename: bool,
    reblock: bool,
}

fn refusal_text(m: &Messages, code: &str, info: &str) -> String {
    match code {
        "alreadyblocked" => m.get("ts-block-already"),
        "ipb_expiry_temp" => m.get("ts-block-hide-indefinite"),
        "ts-cantblock-self" => m.get("ts-block-self"),
        "cantunblock" => m.get("ts-block-not-blocked"),
        "nosuchuser" | "nosuchuserid" => m.get("ts-userrights-nosuchuser"),
        "invalidexpiry" => m.get("ts-protect-invalid-expiry"),
        "badtoken" => m.get("ts-login-expired"),
        "permissiondenied" => m.get("ts-block-denied"),
        _ => {
            if info.is_empty() {
                code.to_string()
            } else {
                info.to_string()
            }
        }
    }
}

/// `Special:Block` or `Special:Unblock` as Plain's `special/block.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockPage {
    /// The notice it came back with.
    pub notices: Vec<MessageModel>,
    /// Where it posts.
    pub action: String,
    /// The CSRF token.
    pub token: String,
    /// The account.
    pub user: FieldModel,
    /// The expiry, for a block.
    pub expiry: Option<FieldModel>,
    /// The reason.
    pub reason: FieldModel,
    /// The block's options.
    pub options: Vec<CheckRow>,
    /// Said on Unblock to an administrator without `hideuser`.
    pub hidden_note: Option<MessageModel>,
    /// **Block** or **Unblock**.
    pub submit: ButtonModel,
    /// The other form.
    pub other: Choice,
}

/// `Special:BlockList` as Plain's `special/blocklist.html` draws it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockListPage {
    /// Said where there are no blocks.
    pub nothing: Option<String>,
    /// The column headings.
    pub columns: Vec<String>,
    /// Whether rows end in an **Unblock** cell.
    pub unblock_column: bool,
    /// The blocks.
    pub rows: Vec<BlockRow>,
}

/// One block.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockRow {
    /// The account's contributions, unless its name is hidden from the viewer.
    pub user: Option<Choice>,
    /// **Hidden** beside a hidden name, or the placeholder for it.
    pub hidden: Option<String>,
    /// Who blocked.
    pub by: String,
    /// When.
    pub when: String,
    /// Until when.
    pub expiry: String,
    /// Why.
    pub reason: String,
    /// What it removes.
    pub removes: String,
    /// **Unblock**.
    pub unblock: Option<Choice>,
}

fn option(id: &str, name: &str, label: String, help: String, on: bool) -> CheckRow {
    CheckRow {
        class: "ts-block__option".into(),
        control: ControlModel::check(id, name, on, label),
        help,
    }
}

#[allow(clippy::too_many_lines)] // one form's model, read top to bottom
fn block_model(cx: &Context, which: Which, token: &str, back: &Returned) -> BlockPage {
    let m = &cx.m;
    let field =
        |id: &str, name: &str, value: &str, label: &str, help: Option<String>, required: bool| {
            FieldModel {
                id: id.into(),
                name: name.into(),
                kind: "text".into(),
                value: value.into(),
                label: m.get(label),
                description: None,
                help,
                required,
                autocomplete: Some("off".into()),
            }
        };
    let mut options = Vec::new();
    if which == Which::Block {
        options.push(option(
            "ts-block-nocreate",
            "nocreate",
            m.get("ts-block-nocreate"),
            m.get("ts-block-nocreate-help"),
            back.nocreate,
        ));
        options.push(option(
            "ts-block-allbutread",
            "allbutread",
            m.get("ts-block-allbutread"),
            m.get("ts-block-allbutread-help"),
            back.all_but_read,
        ));
        if cx.holds("hideuser") {
            options.push(option(
                "ts-block-hidename",
                "hidename",
                m.get("ts-block-hidename"),
                m.get("ts-block-hidename-help"),
                back.hidename,
            ));
        }
        options.push(option(
            "ts-block-reblock",
            "reblock",
            m.get("ts-block-reblock"),
            m.get("ts-block-reblock-help"),
            back.reblock,
        ));
    }
    let other = match which {
        Which::Block => Which::Unblock,
        Which::Unblock => Which::Block,
    };
    BlockPage {
        notices: back
            .notice
            .iter()
            .map(|(k, t)| Message::block(*k, t).model())
            .collect(),
        action: format!("/wiki/{}", title_url(which.title())),
        token: token.to_string(),
        user: field(
            "ts-block-user",
            "user",
            &back.user,
            "ts-block-user",
            None,
            true,
        ),
        expiry: (which == Which::Block).then(|| {
            field(
                "ts-block-expiry",
                "expiry",
                if back.expiry.is_empty() {
                    "infinite"
                } else {
                    &back.expiry
                },
                "ts-block-expiry",
                Some(m.get("ts-userrights-expiry-help")),
                false,
            )
        }),
        reason: field(
            "ts-block-reason",
            "reason",
            &back.reason,
            "ts-block-reason",
            Some(m.get("ts-block-reason-help")),
            false,
        ),
        options,
        hidden_note: (which == Which::Unblock && !cx.holds("hideuser"))
            .then(|| Message::block(MessageKind::Notice, &m.get("ts-unblock-hidden-note")).model()),
        submit: Button {
            label: &m.get(match which {
                Which::Block => "ts-block-submit",
                Which::Unblock => "ts-unblock-submit",
            }),
            kind: "submit",
            action: match which {
                Which::Block => Action::Destructive,
                Which::Unblock => Action::Progressive,
            },
            weight: Weight::Primary,
        }
        .model(),
        other: link_to(
            format!("/wiki/{}", title_url(other.title())),
            m.get(match other {
                Which::Block => "ts-block-title",
                Which::Unblock => "ts-unblock-title",
            }),
        ),
    }
}

async fn page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    which: Which,
    back: Returned,
) -> Response {
    let heading = cx.m.get(match which {
        Which::Block => "ts-block-title",
        Which::Unblock => "ts-unblock-title",
    });
    if cx.user.is_none() {
        let href = format!(
            "/wiki/Special:UserLogin?returnto={}",
            title_url(which.title())
        );
        let model = GateModel::login(&cx.m, "ts-block-login", href.clone());
        let body = cx.draw("moderation/gate.html", &model);
        return pages::respond(cx, headers, StatusCode::OK, |_| Page {
            title: heading.clone(),
            returnto: Some(which.title().to_string()),
            body,
            ..Page::default()
        });
    }
    if !cx.holds("block") {
        let text = cx.m.get("ts-block-denied");
        let body = cx.draw(
            "moderation/gate.html",
            GateModel::notice(MessageKind::Error, &text),
        );
        return pages::respond(cx, headers, StatusCode::FORBIDDEN, |_| Page {
            title: heading.clone(),
            returnto: Some(which.title().to_string()),
            body,
            ..Page::default()
        });
    }
    let token = match site.client().token(incoming, "csrf").await {
        Ok(t) => t,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cx.inputs.push(token.value.cache);
    let model = block_model(cx, which, &token.value.value, &back);
    let body = cx.draw("special/block.html", &model);
    let r = pages::respond(cx, headers, StatusCode::OK, |_| Page {
        title: heading.clone(),
        returnto: Some(which.title().to_string()),
        body,
        ..Page::default()
    });
    pages::with_cookies(r, &token.set_cookies)
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    which: Which,
) -> Result<Context, Response> {
    let cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Block) {
        return Err(pages::no_page(&cx, headers, which.title().to_string()));
    }
    Ok(cx)
}

/// `GET Special:Block[/{name}]` or `Special:Unblock[/{name}]`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    title: &str,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, which).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let notice = params.map.get("done").map(|_| {
        (
            MessageKind::Success,
            cx.m.get(match which {
                Which::Block => "ts-block-done",
                Which::Unblock => "ts-unblock-done",
            }),
        )
    });
    let back = Returned {
        notice,
        user: subject_of(title)
            .or_else(|| params.map.get("user").cloned())
            .unwrap_or_default(),
        ..Returned::default()
    };
    page(site, &mut cx, headers, &incoming, which, back).await
}

/// `POST Special:Block` or `Special:Unblock`.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    query: &BTreeMap<String, String>,
    form: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, which).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.map.get(k).map_or("", String::as_str);
    let on = |k: &str| form.map.contains_key(k);
    let user = field("user").trim().to_string();
    let expiry = field("expiry").trim().to_string();
    let reason = field("reason").trim().to_string();
    let back = Returned {
        notice: None,
        user: user.clone(),
        expiry: expiry.clone(),
        reason: reason.clone(),
        nocreate: on("nocreate"),
        all_but_read: on("allbutread"),
        hidename: on("hidename"),
        reblock: on("reblock"),
    };
    let result = match which {
        Which::Block => {
            site.client()
                .block(
                    &incoming,
                    &BlockForm {
                        user: &user,
                        expiry: if expiry.is_empty() {
                            "infinite"
                        } else {
                            &expiry
                        },
                        reason: &reason,
                        nocreate: back.nocreate,
                        all_but_read: back.all_but_read,
                        hidename: back.hidename,
                        reblock: back.reblock,
                        token: field("token"),
                    },
                )
                .await
        }
        Which::Unblock => {
            site.client()
                .unblock(&incoming, &user, &reason, field("token"))
                .await
        }
    };
    match result {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/wiki/{}?done=1&user={}",
                title_url(which.title()),
                url::form_urlencoded::byte_serialize(user.as_bytes()).collect::<String>()
            ),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let first = r.first();
            let text = refusal_text(
                &cx.m,
                first.map_or("", |x| x.code.as_str()),
                first.map_or("", |x| x.message.as_str()),
            );
            let back = Returned {
                notice: Some((MessageKind::Error, text)),
                ..back
            };
            page(site, &mut cx, headers, &incoming, which, back).await
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

/// `GET Special:BlockList`.
pub async fn list(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, &params.map, Which::Block).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let blocks = match site.client().blocks(&incoming).await {
        Ok(b) => b,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    cx.inputs.push(blocks.cache);
    let m = &cx.m;
    let may_unblock = cx.user.is_some() && cx.holds("block");
    let model = BlockListPage {
        nothing: blocks.value.is_empty().then(|| m.get("ts-blocklist-none")),
        columns: [
            "ts-blocklist-col-user",
            "ts-blocklist-col-by",
            "ts-blocklist-col-when",
            "ts-blocklist-col-expiry",
            "ts-blocklist-col-reason",
            "ts-blocklist-col-removes",
        ]
        .iter()
        .map(|k| m.get(k))
        .collect(),
        unblock_column: may_unblock,
        rows: blocks
            .value
            .iter()
            .map(|b| BlockRow {
                user: b.user.as_ref().map(|u| {
                    link_to(
                        format!("/wiki/Special:Contributions/{}", title_url(u)),
                        u.clone(),
                    )
                }),
                hidden: if b.user.is_none() {
                    Some(m.get("ts-changes-hidden-user"))
                } else {
                    b.hidden.then(|| m.get("ts-blocklist-hidden"))
                },
                by: b.by.clone().unwrap_or_default(),
                when: b.timestamp.clone(),
                expiry: if b.expiry == "infinite" || b.expiry == "infinity" {
                    m.get("ts-protect-expiry-never")
                } else {
                    b.expiry.clone()
                },
                reason: b.reason.clone(),
                removes: if b.removes == "all-but-read" {
                    m.get("ts-blocklist-all-but-read")
                } else {
                    b.removes.clone()
                },
                unblock: may_unblock
                    .then(|| {
                        b.user.as_ref().map(|u| {
                            link_to(
                                format!("/wiki/Special:Unblock/{}", title_url(u)),
                                m.get("ts-unblock-submit"),
                            )
                        })
                    })
                    .flatten(),
            })
            .collect(),
    };
    let body = cx.draw("special/blocklist.html", &model);
    pages::respond(&cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-blocklist-title"),
        returnto: Some("Special:BlockList".to_string()),
        body,
        ..Page::default()
    })
}
