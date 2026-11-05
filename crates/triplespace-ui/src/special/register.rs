//! `Special:CreateAccount` (with its alias `Special:RequestAccount`) and
//! `Special:ConfirmAccounts` (0087 §5, §6; 19 §2.3).
//!
//! - **The form** asks for a name, a password twice and, where the wiki reviews new
//!   accounts, a statement. Where the wiki guards registration with a challenge, the
//!   form carries the challenge the API issued to the browser's session, an empty field
//!   for its solution, a polite live region the site's script reports its progress in,
//!   and a `<noscript>` block that says what is asked and why. The script solves the
//!   challenge in a Web Worker while the person types. Submitted without a solution, the
//!   API asks again, and the page explains that the browser needs JavaScript for this,
//!   and what else the person can do.
//! - **A honeypot field**, off-screen and outside the accessibility tree, is on the form
//!   in every configuration; a submission that fills it is answered as a refused
//!   creation without calling `createaccount`.
//! - **After creating**, a person who was not logged in is logged in with the name and
//!   password they chose and returned to the page they came from. Creating for someone
//!   else says so and stays. Where the wiki reviews accounts, the page says the request
//!   was filed and that its outcome is learned by logging in.
//! - **`Special:ConfirmAccounts`** lists the open requests, oldest first, each with its
//!   statement, its age, and its address for holders of `ts-viewip`, and a form to
//!   approve it or to reject it with a reason.
//!
//! - **External issuers** (0088 §6): the form offers each issuer the wiki offers, a
//!   button posting the login token to the API, which sends the person to the provider
//!   and back. A login whose identity has no account comes back here with a **pending
//!   identity**: the page names the provider, asks only for a name (offering the
//!   provider's username as a button labelled with where it came from), and creates the
//!   account with `createpreservestate`, with no password and no challenge.
//!
//! Every page holding a token is the viewer's alone.

use std::collections::BTreeMap;

use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use serde::Serialize;
use triplespace_client::settings::Saved;
use triplespace_client::{
    CacheInfo, ChallengeInfo, CreateFields, CreateResult, Incoming, LoginResult, Pending,
};

use crate::Site;
use crate::codex::{
    Action, Button, ButtonModel, Field, FieldModel, Message, MessageKind, MessageModel, Weight,
};
use crate::entity::history::Choice;
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Peer};
use crate::special::external::IssuerButtons;

const TITLE: &str = "Special:CreateAccount";
const REVIEW: &str = "Special:ConfirmAccounts";

/// The honeypot's field name: a word a form-filler fills.
pub const HONEYPOT: &str = "wpWebsite";

/// The challenge's part of a form as Plain's `account.html` draws it (19 §5.10): what the
/// site's script reads (`tschallengeid`, `data-ts-challenge-bits`, the status region's
/// `data-` messages) and the `<noscript>` explanation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChallengeModel {
    /// The challenge string.
    pub id: String,
    /// Its difficulty in bits.
    pub bits: u64,
    /// The solution, kept where the form comes back for a reason that did not spend it.
    pub solution: String,
    /// The status while the browser works.
    pub working: String,
    /// The status while it waits for the person.
    pub waiting: String,
    /// The status once solved.
    pub done: String,
    /// The status when solving failed.
    pub failed: String,
    /// What the `<noscript>` block says.
    pub nojs: String,
}

/// The challenge's part of a form, as a model.
#[must_use]
pub fn challenge_model(
    m: &Messages,
    challenge: Option<&ChallengeInfo>,
    solution: &str,
    nojs_key: &str,
) -> Option<ChallengeModel> {
    challenge.map(|c| ChallengeModel {
        id: c.id.clone(),
        bits: c.bits,
        solution: solution.to_string(),
        working: m.get("ts-challenge-working"),
        waiting: m.get("ts-challenge-waiting"),
        done: m.get("ts-challenge-done"),
        failed: m.get("ts-challenge-failed"),
        nojs: m.get(nojs_key),
    })
}

/// The challenge a form came back with, and its solution.
#[must_use]
pub fn posted_challenge(form: &BTreeMap<String, String>) -> Option<(String, String)> {
    let id = form.get("tschallengeid").filter(|s| !s.is_empty())?;
    Some((
        id.clone(),
        form.get("tschallengesolution").cloned().unwrap_or_default(),
    ))
}

/// The challenge a form came back with, as the next form carries it.
fn challenge_of(id: &str) -> Option<ChallengeInfo> {
    let bits = id.split('.').nth(3)?.parse().ok()?;
    Some(ChallengeInfo {
        id: id.to_string(),
        bits,
    })
}

/// What a form is drawn from.
struct FormState {
    token: String,
    back: String,
    username: String,
    /// The statement's mode (`required` or `optional`) and what was typed.
    statement: Option<(String, String)>,
    challenge: Option<ChallengeInfo>,
    solution: String,
    approval: bool,
    for_another: bool,
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what `special/createaccount.html` and
// `special/confirmaccounts.html` draw.
// ---------------------------------------------------------------------------------------

/// The statement a reviewed wiki asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatementModel {
    /// Whether it is required.
    pub required: bool,
    /// What was typed.
    pub value: String,
    /// The label.
    pub label: String,
    /// What it is for.
    pub help: String,
    /// The "optional" flag after the label, where the form shows one.
    pub optional: Option<String>,
}

/// The account creation form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateForm {
    /// The challenge's difficulty, where one is asked for: the form's `data-ts-challenge`.
    pub challenge_bits: Option<u64>,
    /// The create token.
    pub token: String,
    /// The page to return to.
    pub returnto: String,
    /// The name.
    pub username: FieldModel,
    /// The password.
    pub password: FieldModel,
    /// The password again.
    pub retype: FieldModel,
    /// The statement, where the wiki asks for one.
    pub statement: Option<StatementModel>,
    /// The honeypot's label, which no person sees.
    pub honeypot: String,
    /// The challenge's part of the form.
    pub challenge: Option<ChallengeModel>,
    /// **Create your account**, or the request's or another's.
    pub submit: ButtonModel,
}

/// The provider's username offered as a button (0088 §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestModel {
    /// The name.
    pub value: String,
    /// "Use Alice from Wikimedia".
    pub label: String,
    /// What using it means.
    pub note: String,
}

/// The form for a pending identity (0088 §6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingForm {
    /// The create token.
    pub token: String,
    /// The page to return to.
    pub returnto: String,
    /// The name.
    pub username: FieldModel,
    /// The provider's username, where it has one.
    pub suggest: Option<SuggestModel>,
    /// The statement, where the wiki asks for one.
    pub statement: Option<StatementModel>,
    /// **Create account**.
    pub submit: ButtonModel,
}

/// `Special:CreateAccount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreateAccountPage {
    /// A notice: a refusal, the outcome.
    pub notice: Option<MessageModel>,
    /// Paragraphs of text, before the rest.
    pub texts: Vec<String>,
    /// A link, in a paragraph of its own.
    pub link: Option<Choice>,
    /// The creation form.
    pub form: Option<CreateForm>,
    /// The pending identity's form.
    pub pending: Option<PendingForm>,
    /// The external issuers' buttons (0097 A6).
    pub issuers: Option<IssuerButtons>,
}

/// One open request on `Special:ConfirmAccounts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RequestCard {
    /// The request's ID.
    pub id: String,
    /// The name asked for.
    pub name: String,
    /// How long ago it was filed.
    pub age: String,
    /// The address, for holders of `ts-viewip`.
    pub address: Option<String>,
    /// The statement.
    pub statement: Option<String>,
    /// What the card says when there is none.
    pub no_statement: String,
    /// The CSRF token.
    pub token: String,
    /// **Approve**.
    pub approve: ButtonModel,
    /// The reason a rejection needs.
    pub reason: FieldModel,
    /// **Reject**.
    pub reject: ButtonModel,
}

/// `Special:ConfirmAccounts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfirmAccountsPage {
    /// A notice: the outcome, a refusal, that the page needs a login.
    pub notice: Option<MessageModel>,
    /// What the page says when no request is open.
    pub empty: Option<String>,
    /// The open requests, oldest first.
    pub requests: Vec<RequestCard>,
    /// A link, in a paragraph of its own: to log in, or back to the list.
    pub link: Option<Choice>,
}

/// What a creation page shows beneath its notice.
enum CreateBody {
    /// Paragraphs, then a link.
    Text(Vec<String>, Option<Choice>),
    /// The creation form, with the issuers' buttons where they apply.
    Create {
        state: FormState,
        issuers: Option<IssuerButtons>,
    },
    /// The pending identity's form.
    Pending {
        label: String,
        pending: Pending,
        token: String,
        back: String,
        username: String,
        statement: Option<(String, String)>,
    },
}

fn link_choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

fn field_model(
    m: &Messages,
    id: &str,
    name: &str,
    kind: &str,
    value: &str,
    key: &str,
    auto: &str,
) -> FieldModel {
    Field {
        id,
        name,
        kind,
        value,
        label: &m.get(key),
        description: None,
        help: None,
        required: true,
        autocomplete: Some(auto),
    }
    .model()
}

fn primary(label: &str) -> ButtonModel {
    Button {
        label,
        kind: "submit",
        action: Action::Progressive,
        weight: Weight::Primary,
    }
    .model()
}

fn statement_model(
    m: &Messages,
    statement: Option<&(String, String)>,
    flag: bool,
) -> Option<StatementModel> {
    statement.map(|(mode, value)| {
        let required = mode == "required";
        StatementModel {
            required,
            value: value.clone(),
            label: m.get("ts-createaccount-statement"),
            help: m.get("ts-createaccount-statement-help"),
            optional: (flag && !required).then(|| m.get("ts-createaccount-optional")),
        }
    })
}

fn create_form_model(m: &Messages, s: &FormState) -> CreateForm {
    let label = if s.approval {
        m.get("ts-createaccount-request-submit")
    } else if s.for_another {
        m.get("ts-createaccount-other-submit")
    } else {
        m.get("ts-createaccount-submit")
    };
    CreateForm {
        challenge_bits: s.challenge.as_ref().map(|c| c.bits),
        token: s.token.clone(),
        returnto: s.back.clone(),
        username: field_model(
            m,
            "wpName2",
            "username",
            "text",
            &s.username,
            "ts-createaccount-username",
            "username",
        ),
        password: field_model(
            m,
            "wpPassword2",
            "password",
            "password",
            "",
            "ts-createaccount-password",
            "new-password",
        ),
        retype: field_model(
            m,
            "wpRetype",
            "retype",
            "password",
            "",
            "ts-createaccount-retype",
            "new-password",
        ),
        statement: statement_model(m, s.statement.as_ref(), true),
        honeypot: m.get("ts-createaccount-honeypot"),
        challenge: challenge_model(m, s.challenge.as_ref(), &s.solution, "ts-challenge-nojs"),
        submit: primary(&label),
    }
}

/// The creation page's model.
fn create_model(
    m: &Messages,
    notice: Option<(MessageKind, String)>,
    body: &CreateBody,
) -> CreateAccountPage {
    let mut page = CreateAccountPage {
        notice: notice.map(|(k, t)| Message::block(k, &t).model()),
        texts: Vec::new(),
        link: None,
        form: None,
        pending: None,
        issuers: None,
    };
    match body {
        CreateBody::Text(texts, link) => {
            page.texts.clone_from(texts);
            page.link.clone_from(link);
        }
        CreateBody::Create { state, issuers } => {
            if state.approval {
                page.texts.push(m.get("ts-createaccount-reviewed"));
            }
            page.form = Some(create_form_model(m, state));
            page.issuers.clone_from(issuers);
        }
        CreateBody::Pending {
            label,
            pending,
            token,
            back,
            username,
            statement,
        } => {
            page.texts.push(m.with(
                "ts-createaccount-pending-intro",
                &[label, &pending.username],
            ));
            page.pending = Some(PendingForm {
                token: token.clone(),
                returnto: back.clone(),
                username: field_model(
                    m,
                    "wpName2",
                    "username",
                    "text",
                    username,
                    "ts-createaccount-username",
                    "username",
                ),
                suggest: (!pending.username.is_empty()).then(|| SuggestModel {
                    value: pending.username.clone(),
                    label: m.with(
                        "ts-createaccount-pending-suggest",
                        &[label, &pending.username],
                    ),
                    note: m.get("ts-createaccount-pending-suggest-note"),
                }),
                statement: statement_model(m, statement.as_ref(), false),
                submit: primary(&m.get("ts-createaccount-pending-submit")),
            });
        }
    }
    page
}

fn page(
    cx: &Context,
    headers: &HeaderMap,
    status: StatusCode,
    notice: Option<(MessageKind, String)>,
    body: &CreateBody,
    back: &str,
) -> Response {
    let html = cx.draw(
        "special/createaccount.html",
        create_model(&cx.m, notice, body),
    );
    let back = back.to_string();
    crate::special::external::allow_issuer_forms(
        cx,
        pages::respond(cx, headers, status, |m| Page {
            title: m.get("ts-createaccount-title"),
            returnto: Some(back),
            body: html,
            ..Page::default()
        }),
    )
}

/// The page to return to: `returnto` where it is a title, else the main page.
fn returnto(cx: &Context, value: Option<&String>) -> String {
    let t = value.map_or("", |v| v.trim()).replace('_', " ");
    let lower = t.to_ascii_lowercase();
    if t.is_empty()
        || t.contains("://")
        || t.starts_with('/')
        || lower.starts_with("special:userlog")
        || lower.starts_with("special:createaccount")
        || lower.starts_with("special:requestaccount")
    {
        cx.site.mainpage.clone()
    } else {
        t
    }
}

async fn start(
    site: &Site,
    headers: &HeaderMap,
    incoming: &Incoming,
    query: &BTreeMap<String, String>,
    title: &str,
) -> Result<Context, Response> {
    let mut cx = pages::context(site, incoming, query).await?;
    if !cx.features.has(Feature::Register) {
        return Err(pages::no_page(&cx, headers, title.to_string()));
    }
    cx.inputs.push(CacheInfo::default());
    Ok(cx)
}

/// A fresh form, or the page that says the viewer may not create an account.
#[allow(clippy::too_many_arguments)]
async fn fresh(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &str,
    username: &str,
    statement: &str,
    notice: Option<(MessageKind, String)>,
    status: StatusCode,
    mut cookies: Vec<HeaderValue>,
) -> Response {
    let f = match site.client().create_form(incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cookies.extend(f.set_cookies);
    let f = f.value;
    if !f.can_create {
        let body = CreateBody::Text(
            vec![cx.m.get(if cx.user.is_some() {
                "ts-createaccount-not-allowed-user"
            } else {
                "ts-createaccount-not-allowed"
            })],
            None,
        );
        return pages::with_cookies(
            page(cx, headers, StatusCode::FORBIDDEN, None, &body, back),
            &cookies,
        );
    }
    let approval = cx.site.account_approval && !holds_confirm(cx);
    let state = FormState {
        token: f.token.clone(),
        back: back.to_string(),
        username: username.to_string(),
        statement: f
            .statement
            .as_deref()
            .map(|mode| (mode.to_string(), statement.to_string())),
        challenge: f.challenge,
        solution: String::new(),
        approval,
        for_another: cx.user.is_some(),
    };
    // The issuers' buttons carry the login token of the session the form's token is
    // bound to: a second session would replace the first in the browser.
    let issuers =
        (cx.user.is_none() && !cx.site.login_issuers.is_empty() && !f.login_token.is_empty())
            .then(|| {
                crate::special::external::issuer_buttons(
                    &cx.m,
                    &cx.site.login_issuers,
                    "logintoken",
                    &f.login_token,
                    "login",
                    back,
                    "ts-createaccount-external-heading",
                    "ts-createaccount-external-button",
                )
            })
            .flatten();
    let body = CreateBody::Create { state, issuers };
    pages::with_cookies(page(cx, headers, status, notice, &body, back), &cookies)
}

/// What a button names the issuer of a pending identity.
fn issuer_label(cx: &Context, p: &Pending) -> String {
    cx.site
        .login_issuers
        .iter()
        .find(|i| i.issuer == p.issuer)
        .map_or_else(|| p.issuer_name.clone(), |i| i.label.clone())
}

/// The page for a pending identity, with a fresh token.
#[allow(clippy::too_many_arguments)]
async fn pending_page(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &str,
    p: &Pending,
    username: &str,
    statement: &str,
    notice: Option<(MessageKind, String)>,
    mut cookies: Vec<HeaderValue>,
) -> Response {
    let f = match site.client().create_form(incoming).await {
        Ok(f) => f,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    cookies.extend(f.set_cookies);
    let body = CreateBody::Pending {
        label: issuer_label(cx, p),
        pending: p.clone(),
        token: f.value.token.clone(),
        back: back.to_string(),
        username: username.to_string(),
        statement: f
            .value
            .statement
            .as_deref()
            .map(|mode| (mode.to_string(), statement.to_string())),
    };
    pages::with_cookies(
        page(cx, headers, StatusCode::OK, notice, &body, back),
        &cookies,
    )
}

/// `POST Special:CreateAccount` for a pending identity.
async fn post_pending(
    site: &Site,
    cx: &mut Context,
    headers: &HeaderMap,
    incoming: &Incoming,
    back: &str,
    form: &BTreeMap<String, String>,
) -> Response {
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let Some(p) = (match site.client().pending_identity(incoming).await {
        Ok(p) => p,
        Err(e) => return pages::api_failed(cx, headers, &e),
    }) else {
        // The pending identity expired: start again from the login page.
        return pages::see_other(
            &format!(
                "/wiki/Special:UserLogin?tserror=ts-oauth-state&returnto={}",
                title_url(back)
            ),
            &[],
        );
    };
    let suggested = field("tssuggested").trim();
    let name = if suggested.is_empty() {
        field("username").trim().to_string()
    } else {
        suggested.to_string()
    };
    let statement = field("tsstatement").to_string();
    let return_url = format!(
        "{}/wiki/{}",
        cx.site.server.trim_end_matches('/'),
        title_url(back)
    );
    let r = match site
        .client()
        .create_preserved(
            incoming,
            &name,
            field("createtoken"),
            &return_url,
            form.get("tsstatement").map(String::as_str),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => return pages::api_failed(cx, headers, &e),
    };
    match r.value {
        CreateResult::Pass { .. } => {
            pages::see_other(&format!("/wiki/{}", title_url(back)), &r.set_cookies)
        }
        CreateResult::Requested => {
            let body = CreateBody::Text(
                vec![cx.m.get("ts-createaccount-requested-next-external")],
                None,
            );
            let notice = Some((MessageKind::Success, cx.m.get("ts-createaccount-requested")));
            pages::with_cookies(
                page(cx, headers, StatusCode::OK, notice, &body, back),
                &r.set_cookies,
            )
        }
        CreateResult::Fail { code, message } => {
            let text = match code.as_str() {
                "ts-oauth-bound" => cx.m.get("ts-oauth-bound"),
                _ => refusal(&cx.m, &code, &message),
            };
            pending_page(
                site,
                cx,
                headers,
                incoming,
                back,
                &p,
                &name,
                &statement,
                Some((MessageKind::Error, text)),
                r.set_cookies,
            )
            .await
        }
        CreateResult::BadToken | CreateResult::Challenge(_) => {
            pending_page(
                site,
                cx,
                headers,
                incoming,
                back,
                &p,
                &name,
                &statement,
                Some((MessageKind::Warning, cx.m.get("ts-login-expired"))),
                r.set_cookies,
            )
            .await
        }
    }
}

/// Whether the viewer holds `confirmaccount`, which the site reads from the open-request
/// count the API reports to holders alone; a holder creates accounts at once.
fn holds_confirm(cx: &Context) -> bool {
    cx.user.is_some() && cx.account_requests.is_some()
}

/// `GET Special:CreateAccount`.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, TITLE).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, query.get("returnto"));
    if cx.user.is_none() && !cx.site.login_issuers.is_empty() {
        match site.client().pending_identity(&incoming).await {
            Ok(Some(p)) => {
                return pending_page(
                    site,
                    &mut cx,
                    headers,
                    &incoming,
                    &back,
                    &p,
                    "",
                    "",
                    None,
                    Vec::new(),
                )
                .await;
            }
            Ok(None) => {}
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    let notice = cx
        .user
        .clone()
        .map(|name| {
            (
                MessageKind::Notice,
                cx.m.with("ts-createaccount-as-other", &[&name]),
            )
        })
        .or_else(|| crate::special::external::answer(&cx.m, query));
    fresh(
        site,
        &mut cx,
        headers,
        &incoming,
        &back,
        "",
        "",
        notice,
        StatusCode::OK,
        Vec::new(),
    )
    .await
}

/// The message for a refused creation.
fn refusal(m: &Messages, code: &str, message: &str) -> String {
    match code {
        "userexists" => m.get("ts-createaccount-userexists"),
        "noname" => m.get("ts-createaccount-noname"),
        "badretype" => m.get("ts-createaccount-badretype"),
        "passwordtooshort" => m.get("ts-createaccount-passwordtooshort"),
        "acct_creation_throttle_hit" => m.get("ts-createaccount-throttle"),
        "ts-account-statement-required" => m.get("ts-createaccount-statement-required"),
        "ts-account-statement-long" => m.get("ts-createaccount-statement-long"),
        "permissiondenied" => m.get("ts-createaccount-not-allowed"),
        _ => message.to_string(),
    }
}

/// `POST Special:CreateAccount`.
#[allow(clippy::too_many_lines)]
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match start(site, headers, &incoming, query, TITLE).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let back = returnto(&cx, form.get("returnto"));
    if form.contains_key("tspending") && cx.user.is_none() {
        return post_pending(site, &mut cx, headers, &incoming, &back, form).await;
    }
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let username = field("username").trim().to_string();
    let statement = field("tsstatement").to_string();
    // A filled honeypot is answered as a refused creation, without asking the API.
    if !field(HONEYPOT).is_empty() {
        let notice = Some((MessageKind::Error, cx.m.get("ts-createaccount-failed")));
        return fresh(
            site,
            &mut cx,
            headers,
            &incoming,
            &back,
            &username,
            &statement,
            notice,
            StatusCode::OK,
            Vec::new(),
        )
        .await;
    }
    let posted = posted_challenge(form);
    let return_url = format!(
        "{}/wiki/{}",
        cx.site.server.trim_end_matches('/'),
        title_url(&back)
    );
    let fields = CreateFields {
        username: &username,
        password: field("password"),
        retype: field("retype"),
        token: field("createtoken"),
        return_url: &return_url,
        challenge: posted.as_ref().map(|(i, s)| (i.as_str(), s.as_str())),
        statement: form.get("tsstatement").map(String::as_str),
    };
    let result = match site.client().create_account(&incoming, &fields).await {
        Ok(r) => r,
        Err(e) => return pages::api_failed(&cx, headers, &e),
    };
    let mut cookies = result.set_cookies;
    let approval = cx.site.account_approval && !holds_confirm(&cx);
    let again = |cx: &Context,
                 challenge: Option<ChallengeInfo>,
                 solution: &str,
                 notice: (MessageKind, String)| {
        let state = FormState {
            token: field("createtoken").to_string(),
            back: back.clone(),
            username: username.clone(),
            statement: form
                .get("tsstatementmode")
                .map(|mode| (mode.clone(), statement.clone())),
            challenge,
            solution: solution.to_string(),
            approval,
            for_another: cx.user.is_some(),
        };
        page(
            cx,
            headers,
            StatusCode::OK,
            Some(notice),
            &CreateBody::Create {
                state,
                issuers: None,
            },
            &back,
        )
    };
    match result.value {
        CreateResult::Pass { username: name } => {
            if cx.user.is_some() {
                let body = CreateBody::Text(
                    Vec::new(),
                    Some(link_choice(
                        format!("/wiki/User:{}", title_url(&name)),
                        cx.m.with("ts-createaccount-userpage", &[&name]),
                    )),
                );
                let notice = Some((
                    MessageKind::Success,
                    cx.m.with("ts-createaccount-created-other", &[&name]),
                ));
                return pages::with_cookies(
                    page(&cx, headers, StatusCode::OK, notice, &body, &back),
                    &cookies,
                );
            }
            // Log the new account in, with the name and password just chosen.
            let token = match site.client().token(&incoming, "login").await {
                Ok(t) => t,
                Err(e) => return pages::api_failed(&cx, headers, &e),
            };
            cookies.extend(token.set_cookies);
            match site
                .client()
                .client_login(
                    &incoming,
                    &name,
                    field("password"),
                    &token.value.value,
                    &return_url,
                    None,
                )
                .await
            {
                Ok(r) if matches!(r.value, LoginResult::Pass { .. }) => {
                    cookies.extend(r.set_cookies);
                    pages::see_other(&format!("/wiki/{}", title_url(&back)), &cookies)
                }
                Ok(r) => {
                    cookies.extend(r.set_cookies);
                    pages::see_other(
                        &format!("/wiki/Special:UserLogin?returnto={}", title_url(&back)),
                        &cookies,
                    )
                }
                Err(e) => pages::api_failed(&cx, headers, &e),
            }
        }
        CreateResult::Requested => {
            let body = CreateBody::Text(
                vec![cx.m.get("ts-createaccount-requested-next")],
                Some(link_choice(
                    format!("/wiki/{}", title_url(&back)),
                    cx.m.with("ts-login-continue", &[&back]),
                )),
            );
            let notice = Some((MessageKind::Success, cx.m.get("ts-createaccount-requested")));
            pages::with_cookies(
                page(&cx, headers, StatusCode::OK, notice, &body, &back),
                &cookies,
            )
        }
        CreateResult::Challenge(c) => {
            // No solution came: the browser ran no script. One came and was refused: it
            // went stale or was spent, and the script works on the new one.
            let key = if posted.as_ref().is_none_or(|(_, s)| s.is_empty()) {
                "ts-challenge-nojs"
            } else {
                "ts-challenge-retry"
            };
            let notice = (MessageKind::Error, cx.m.get(key));
            pages::with_cookies(again(&cx, Some(c), "", notice), &cookies)
        }
        CreateResult::Fail { code, message } => {
            let notice = (MessageKind::Error, refusal(&cx.m, &code, &message));
            // The challenge was not spent: the form keeps it, and its solution.
            let (challenge, solution) = posted
                .as_ref()
                .map_or((None, String::new()), |(i, s)| (challenge_of(i), s.clone()));
            pages::with_cookies(again(&cx, challenge, &solution, notice), &cookies)
        }
        CreateResult::BadToken => {
            let notice = Some((MessageKind::Warning, cx.m.get("ts-login-expired")));
            fresh(
                site,
                &mut cx,
                headers,
                &incoming,
                &back,
                &username,
                &statement,
                notice,
                StatusCode::OK,
                cookies,
            )
            .await
        }
    }
}

// ---- Special:ConfirmAccounts ---------------------------------------------------------

fn age(m: &Messages, filed: i64) -> String {
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    )
    .unwrap_or(i64::MAX);
    let days = (now - filed).max(0) / 86_400;
    if days == 0 {
        m.get("ts-confirmaccounts-today")
    } else {
        m.with("ts-confirmaccounts-days", &[&days.to_string()])
    }
}

/// What the review page shows beneath its notice.
enum ReviewBody<'a> {
    /// A link: to log in, or back to the list.
    Link(Choice),
    /// The open requests, with the token their forms carry.
    List(&'a [triplespace_client::AccountRequest], &'a str),
}

fn request_model(m: &Messages, r: &triplespace_client::AccountRequest, token: &str) -> RequestCard {
    RequestCard {
        id: r.id.clone(),
        name: r.name.clone(),
        age: age(m, r.filed),
        address: r.address.clone(),
        statement: r.statement.clone(),
        no_statement: m.get("ts-confirmaccounts-no-statement"),
        token: token.to_string(),
        approve: primary(&m.with("ts-confirmaccounts-approve", &[&r.name])),
        reason: Field {
            id: &format!("reason-{}", r.id),
            name: "reason",
            kind: "text",
            value: "",
            label: &m.get("ts-confirmaccounts-reason"),
            description: None,
            help: Some(&m.get("ts-confirmaccounts-reason-help")),
            required: true,
            autocomplete: Some("off"),
        }
        .model(),
        reject: Button {
            label: &m.with("ts-confirmaccounts-reject", &[&r.name]),
            kind: "submit",
            action: Action::Destructive,
            weight: Weight::Normal,
        }
        .model(),
    }
}

/// The review page's model.
fn review_model(
    m: &Messages,
    notice: Option<(MessageKind, String)>,
    body: &ReviewBody<'_>,
) -> ConfirmAccountsPage {
    let notice = notice.map(|(k, t)| Message::block(k, &t).model());
    match body {
        ReviewBody::Link(l) => ConfirmAccountsPage {
            notice,
            empty: None,
            requests: Vec::new(),
            link: Some(l.clone()),
        },
        ReviewBody::List(list, token) => ConfirmAccountsPage {
            notice,
            empty: list.is_empty().then(|| m.get("ts-confirmaccounts-none")),
            requests: list.iter().map(|r| request_model(m, r, token)).collect(),
            link: None,
        },
    }
}

fn review_page(
    cx: &Context,
    headers: &HeaderMap,
    notice: Option<(MessageKind, String)>,
    body: &ReviewBody<'_>,
) -> Response {
    let html = cx.draw(
        "special/confirmaccounts.html",
        review_model(&cx.m, notice, body),
    );
    pages::respond(cx, headers, StatusCode::OK, |m| Page {
        title: m.get("ts-confirmaccounts-title"),
        returnto: Some(REVIEW.to_string()),
        body: html,
        ..Page::default()
    })
}

/// `GET Special:ConfirmAccounts`.
pub async fn review(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, REVIEW).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    if cx.user.is_none() {
        let body = ReviewBody::Link(link_choice(
            format!("/wiki/Special:UserLogin?returnto={}", title_url(REVIEW)),
            cx.m.get("ts-login"),
        ));
        let notice = Some((MessageKind::Notice, cx.m.get("ts-confirmaccounts-login")));
        return review_page(&cx, headers, notice, &body);
    }
    let (list, token) = tokio::join!(
        site.client().account_requests(&incoming),
        site.client().token(&incoming, "csrf")
    );
    let (list, token) = match (list, token) {
        (Ok(l), Ok(t)) => (l, t),
        (Err(e), _) | (_, Err(e)) => return pages::api_failed(&cx, headers, &e),
    };
    let m = &cx.m;
    let notice = match query.get("done").map(String::as_str) {
        Some("approved") => Some((MessageKind::Success, m.get("ts-confirmaccounts-approved"))),
        Some("rejected") => Some((MessageKind::Success, m.get("ts-confirmaccounts-rejected"))),
        _ => None,
    };
    let r = review_page(
        &cx,
        headers,
        notice,
        &ReviewBody::List(&list, &token.value.value),
    );
    pages::with_cookies(r, &token.set_cookies)
}

/// `POST Special:ConfirmAccounts`: approves or rejects one request.
pub async fn post_review(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    query: &BTreeMap<String, String>,
    form: &BTreeMap<String, String>,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let cx = match start(site, headers, &incoming, query, REVIEW).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let field = |k: &str| form.get(k).map_or("", String::as_str);
    let approve = field("decision") == "approve";
    let result = site
        .client()
        .decide_request(
            &incoming,
            field("id"),
            approve,
            field("token"),
            field("reason").trim(),
        )
        .await;
    match result {
        Ok(Saved::Ok(_)) => pages::see_other(
            &format!(
                "/wiki/{}?done={}",
                title_url(REVIEW),
                if approve { "approved" } else { "rejected" }
            ),
            &[],
        ),
        Ok(Saved::Refused(r)) => {
            let text = match r.first().map(|x| x.code.as_str()) {
                Some("ts-reason-required") => cx.m.get("ts-confirmaccounts-reason-required"),
                Some("ts-no-request") => cx.m.get("ts-confirmaccounts-gone"),
                Some("userexists") => cx.m.get("ts-createaccount-userexists"),
                Some("badtoken") => cx.m.get("ts-login-expired"),
                _ => r.first().map(|x| x.message.clone()).unwrap_or_default(),
            };
            review_page(
                &cx,
                headers,
                Some((MessageKind::Error, text)),
                &ReviewBody::Link(link_choice(
                    format!("/wiki/{}", title_url(REVIEW)),
                    cx.m.get("ts-confirmaccounts-back"),
                )),
            )
        }
        Err(e) => pages::api_failed(&cx, headers, &e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(approval: bool, statement: Option<(&str, &str)>) -> FormState {
        FormState {
            token: "t".into(),
            back: "Main Page".into(),
            username: "Bob".into(),
            statement: statement.map(|(a, b)| (a.to_string(), b.to_string())),
            challenge: None,
            solution: String::new(),
            approval,
            for_another: false,
        }
    }

    /// The creation page's model (19 §5.10): a reviewed wiki's intro and request button,
    /// the statement's optional flag on the creation form but not the pending form, the
    /// pending identity's suggestion, and text pages.
    #[test]
    fn creation_is_a_model() {
        let m = Messages::for_language("en");
        let p = create_model(
            &m,
            None,
            &CreateBody::Create {
                state: state(true, Some(("optional", "Hi"))),
                issuers: None,
            },
        );
        assert_eq!(p.texts, vec![m.get("ts-createaccount-reviewed")]);
        let f = p.form.unwrap();
        assert_eq!(f.submit.label, m.get("ts-createaccount-request-submit"));
        let st = f.statement.unwrap();
        assert!(!st.required && st.optional.is_some() && st.value == "Hi");
        let p = create_model(
            &m,
            None,
            &CreateBody::Create {
                state: state(false, Some(("required", ""))),
                issuers: None,
            },
        );
        assert!(p.texts.is_empty());
        assert!(p.form.unwrap().statement.unwrap().required);

        let pending = Pending {
            issuer: "wikimedia".into(),
            issuer_name: "Wikimedia".into(),
            username: "Harej".into(),
        };
        let p = create_model(
            &m,
            Some((MessageKind::Error, "Taken".into())),
            &CreateBody::Pending {
                label: "Wikimedia".into(),
                pending,
                token: "t".into(),
                back: "Main Page".into(),
                username: String::new(),
                statement: Some(("optional".into(), String::new())),
            },
        );
        assert!(p.notice.unwrap().classes.contains("cdx-message--error"));
        let pf = p.pending.unwrap();
        assert_eq!(pf.suggest.unwrap().value, "Harej");
        assert!(
            pf.statement.unwrap().optional.is_none(),
            "the pending form shows no flag"
        );

        let text = create_model(
            &m,
            None,
            &CreateBody::Text(
                vec!["Filed.".into()],
                Some(link_choice("/wiki/X".into(), "On".into())),
            ),
        );
        assert!(text.form.is_none() && text.pending.is_none());
        assert_eq!(
            (text.texts.len(), text.link.unwrap().href.as_str()),
            (1, "/wiki/X")
        );
    }

    /// The review page's model: a card per request, or the empty notice; a link otherwise.
    #[test]
    fn review_is_a_model() {
        let m = Messages::for_language("en");
        let r = triplespace_client::AccountRequest {
            id: "r1".into(),
            name: "Dave".into(),
            statement: None,
            filed: 0,
            expires: 0,
            address: Some("192.0.2.1".into()),
        };
        let p = review_model(&m, None, &ReviewBody::List(&[r], "csrf"));
        assert!(p.empty.is_none());
        let card = &p.requests[0];
        assert_eq!((card.id.as_str(), card.token.as_str()), ("r1", "csrf"));
        assert_eq!(card.reason.id, "reason-r1");
        assert!(card.statement.is_none() && card.address.is_some());
        let p = review_model(&m, None, &ReviewBody::List(&[], "csrf"));
        assert_eq!(p.empty, Some(m.get("ts-confirmaccounts-none")));
        let p = review_model(
            &m,
            None,
            &ReviewBody::Link(link_choice(
                "/wiki/Special:UserLogin".into(),
                "Log in".into(),
            )),
        );
        assert!(p.requests.is_empty() && p.link.is_some() && p.empty.is_none());
    }

    /// Plain's drawing of a model, as the site serves it.
    fn plain(template: &str, model: impl serde::Serialize) -> String {
        let skins = crate::skin::Skins::load(None);
        let plain = skins.get("plain").expect("plain");
        plain
            .render(template, &Messages::for_language("en"), model)
            .unwrap()
    }

    /// The challenge's part of a form: what the site's script reads, escaped (0087 §5).
    #[test]
    fn the_challenge_part_of_a_form() {
        let m = Messages::for_language("en");
        let c = ChallengeInfo {
            id: "1.w.createaccount.12.9.s\"x.m".into(),
            bits: 12,
        };
        let model = challenge_model(&m, Some(&c), "", "ts-challenge-nojs");
        let skins = crate::skin::Skins::load(None);
        let html = skins
            .get("plain")
            .unwrap()
            .render_str(
                "{% import \"account.html\" as account %}{{ account.challenge(c) }}",
                &m,
                serde_json::json!({ "c": model }),
            )
            .unwrap();
        assert!(
            html.contains("name=\"tschallengeid\" value=\"1.w.createaccount.12.9.s&quot;x.m\"")
        );
        assert!(html.contains("data-ts-challenge-bits=\"12\""));
        assert!(html.contains("role=\"status\" aria-live=\"polite\""));
        assert!(html.contains("<noscript>"));
        assert!(challenge_model(&m, None, "", "ts-challenge-nojs").is_none());
        assert_eq!(
            challenge_of("1.w.badlogin.9.1.s.m").map(|c| c.bits),
            Some(9)
        );
    }

    /// The honeypot is the field the handler checks, out of the tab order and the
    /// accessibility tree; a required statement is required; no challenge, no marker.
    #[test]
    fn the_form_has_its_honeypot() {
        let m = Messages::for_language("en");
        let body = CreateBody::Create {
            state: FormState {
                token: "t+\\".into(),
                back: "Main Page".into(),
                username: "Bob".into(),
                statement: Some(("required".into(), "Hi".into())),
                challenge: None,
                solution: String::new(),
                approval: true,
                for_another: false,
            },
            issuers: None,
        };
        let html = plain("special/createaccount.html", create_model(&m, None, &body));
        assert!(html.contains(&format!("name=\"{HONEYPOT}\"")));
        assert!(html.contains("tabindex=\"-1\" autocomplete=\"off\""));
        assert!(html.contains("aria-hidden=\"true\""));
        assert!(html.contains("name=\"tsstatement\""));
        assert!(html.contains(" required>Hi</textarea>"));
        assert!(!html.contains("data-ts-challenge="));
    }
}
