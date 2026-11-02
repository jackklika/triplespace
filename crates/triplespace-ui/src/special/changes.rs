//! `Special:RecentChanges`, `Special:Contributions`, `Special:Log` and `Special:NewPages`
//! (0010 §7; 0020 §4; 19 §3.9; 0047 §4.4): feeds over one list, readable without
//! JavaScript. New pages are the feed's edits that created their target (`show=new`):
//! entities and document pages alike, as Wikidata's new pages are its new items.
//!
//! Both read `GET /activity` ([`triplespace_client::activity`]): recent changes is the
//! everything set, contributions the actor set. The rows are the same, drawn the same
//! way (§3.4's history rows, less the diff links the diff page will bring): the time,
//! a chip for where the record came from, the target with its label, the actor, the
//! summary, the size change and the tags, grouped by day.
//!
//! - **Filters** (`GET` form): the row kinds (edits, jobs, log actions; mirror syncs,
//!   off by default, with a strip above the list that says so and offers to show them),
//!   hiding bot edits, a namespace, the window in days (30 by default, as
//!   MediaWiki's default ⊂ `$wgRCMaxAge`) and the rows per page.
//! - **Paging** is the feed's cursor: **Older** carries `continue`, **Newest** drops it.
//! - **Contributions** take the user from the subpage (`Special:Contributions/Alice`) or
//!   `target=`, with a form for the name when neither is given. An unknown user is said
//!   to be one.
//!
//! What waits for later phases: the diff and history links (Phase 4's diff page), "group
//! by page", patrol marks, the Atom and stream forms, and user pages, which is why an
//! actor links to their contributions.

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Serialize;
use triplespace_client::activity::{ActivityPage, ActivityQuery, ActivityRow};
use triplespace_client::{ClientError, SiteInfo};

use crate::Site;
use crate::codex::{Chip, ChipKind, ChipModel, Message, MessageKind, MessageModel};
use crate::entity::history::{Choice, Pager};
use crate::features::Feature;
use crate::frame::{Page, title_url};
use crate::i18n::Messages;
use crate::pages::{self, Context, Params, Peer};

/// The row kinds a reader can switch, with the message of each and whether it is on
/// when the form says nothing.
const KINDS: [(&str, &str, bool); 4] = [
    ("edit", "ts-changes-kind-edits", true),
    ("job", "ts-changes-kind-jobs", true),
    ("log", "ts-changes-kind-log", true),
    ("sync", "ts-changes-kind-syncs", false),
];

/// The namespaces the filter offers: number and message.
const NAMESPACES: [(&str, &str); 3] = [
    ("120", "ts-search-group-item"),
    ("122", "ts-search-group-property"),
    ("210", "ts-search-group-domain"),
];

const DAYS: [u32; 6] = [1, 3, 7, 14, 30, 90];
const DEFAULT_DAYS: u32 = 30;
const LIMITS: [u32; 4] = [50, 100, 250, 500];
const DEFAULT_LIMIT: u32 = 50;

/// Which page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Which {
    RecentChanges,
    Contributions,
    /// `Special:Log` (19 §6.3): the log rows, by type, actor and page.
    Log,
    /// `Special:NewPages` (0047 §4.4): the edits that created a page or an entity, by
    /// MediaWiki's `username`, `namespace` and `hidebots`.
    NewPages,
}

/// The log types `Special:Log` offers, each a message key; any other type typed into
/// the address is passed through.
const LOG_TYPES: [(&str, &str); 9] = [
    ("delete", "ts-log-type-delete"),
    ("move", "ts-log-type-move"),
    ("suppress", "ts-log-type-suppress"),
    ("protect", "ts-log-type-protect"),
    ("rights", "ts-log-type-rights"),
    ("block", "ts-log-type-block"),
    ("create", "ts-log-type-create"),
    ("thread", "ts-log-type-thread"),
    ("reclaim", "ts-log-type-reclaim"),
];

/// What the query asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Ask {
    which: Which,
    /// The user, for contributions.
    user: Option<String>,
    kinds: Vec<String>,
    hide_bots: bool,
    namespace: Option<String>,
    /// The log type, for the log.
    log_type: Option<String>,
    /// A page's title, for the log.
    page: Option<String>,
    days: u32,
    limit: u32,
    cursor: Option<String>,
}

impl Ask {
    fn of(which: Which, params: &Params, subpage: Option<&str>) -> Self {
        let q = &params.map;
        let asked: Vec<String> = params.all("kind").map(str::to_string).collect();
        let filtered = q.contains_key("filtered");
        let kinds = if asked.is_empty() && !filtered {
            KINDS
                .iter()
                .filter(|(_, _, on)| *on)
                .map(|(k, _, _)| (*k).to_string())
                .collect()
        } else {
            KINDS
                .iter()
                .filter(|(k, _, _)| asked.iter().any(|a| a == k))
                .map(|(k, _, _)| (*k).to_string())
                .collect()
        };
        let pick = |key: &str, choices: &[u32], default: u32| {
            q.get(key)
                .and_then(|v| v.trim().parse::<u32>().ok())
                .filter(|n| choices.contains(n))
                .unwrap_or(default)
        };
        let clean = |u: &str| u.replace('_', " ").trim().to_string();
        // `Special:Log/delete` names a type, `Special:Log/Alice` a user, as MediaWiki's.
        let sub_is_type =
            which == Which::Log && subpage.is_some_and(|t| LOG_TYPES.iter().any(|(k, _)| *k == t));
        let user = match which {
            Which::Contributions => subpage
                .map(str::to_string)
                .or_else(|| q.get("target").cloned())
                .map(|u| clean(&u))
                .filter(|u| !u.is_empty()),
            Which::Log => subpage
                .filter(|_| !sub_is_type)
                .map(str::to_string)
                .or_else(|| q.get("user").cloned())
                .map(|u| clean(&u))
                .filter(|u| !u.is_empty()),
            Which::NewPages => q
                .get("username")
                .map(|u| clean(u))
                .filter(|u| !u.is_empty()),
            Which::RecentChanges => None,
        };
        let log_type = match which {
            Which::Log => subpage
                .filter(|_| sub_is_type)
                .map(str::to_string)
                .or_else(|| q.get("type").cloned())
                .map(|t| t.trim().to_string())
                .filter(|t| {
                    !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                }),
            _ => None,
        };
        let page = match which {
            Which::Log => q.get("page").map(|p| clean(p)).filter(|p| !p.is_empty()),
            _ => None,
        };
        Self {
            which,
            user,
            kinds: match which {
                Which::Log => vec!["log".into()],
                Which::NewPages => vec!["edit".into()],
                _ => kinds,
            },
            log_type,
            page,
            // The form says what to show (`bots`, on by default); the URL keeps
            // MediaWiki's `hidebots=1` too.
            hide_bots: q.get("hidebots").is_some_and(|v| v != "0")
                || (filtered && q.get("bots").is_none()),
            namespace: q
                .get("namespace")
                .filter(|n| NAMESPACES.iter().any(|(k, _)| k == n))
                .cloned(),
            days: pick("days", &DAYS, DEFAULT_DAYS),
            limit: pick("limit", &LIMITS, DEFAULT_LIMIT),
            cursor: q.get("continue").cloned().filter(|c| !c.is_empty()),
        }
    }

    /// The feed to fetch, from `now`; `target` is the log's page, resolved.
    fn query(&self, now: u64, lang: &str, target: Option<String>) -> ActivityQuery {
        let since = now.saturating_sub(u64::from(self.days) * 86_400 * 1_000_000);
        // An erased record was an edit: its gap row goes with the edits (19 §3.4).
        let mut kinds = self.kinds.clone();
        if kinds.iter().any(|k| k == "edit") {
            kinds.push("erased".into());
        }
        ActivityQuery {
            target,
            actor: self.user.clone(),
            log_type: self.log_type.clone(),
            kinds,
            namespaces: self.namespace.iter().cloned().collect(),
            show: self
                .hide_bots
                .then(|| "!bot".to_string())
                .into_iter()
                .chain((self.which == Which::NewPages).then(|| "new".to_string()))
                .collect(),
            since: Some(iso8601(since)),
            limit: self.limit,
            cursor: self.cursor.clone(),
            lang: lang.to_string(),
            ..ActivityQuery::default()
        }
    }

    /// The page's own URL with these filters and a cursor.
    fn href(&self, cursor: Option<&str>) -> String {
        let mut s = match (self.which, &self.user) {
            (Which::RecentChanges, _) => "/w/index.php?title=Special:RecentChanges".to_string(),
            (Which::Log, _) => "/w/index.php?title=Special:Log".to_string(),
            (Which::Contributions, Some(u)) => format!(
                "/w/index.php?title=Special:Contributions/{}",
                utf8_percent_encode(&u.replace(' ', "_"), NON_ALPHANUMERIC)
            ),
            (Which::Contributions, None) => "/w/index.php?title=Special:Contributions".to_string(),
            (Which::NewPages, _) => "/w/index.php?title=Special:NewPages".to_string(),
        };
        s.push_str("&filtered=1");
        if self.which == Which::NewPages
            && let Some(u) = &self.user
        {
            let _ = write!(s, "&username={}", utf8_percent_encode(u, NON_ALPHANUMERIC));
        }
        for k in &self.kinds {
            let _ = write!(s, "&kind={k}");
        }
        if self.hide_bots {
            s.push_str("&hidebots=1");
        }
        if let Some(n) = &self.namespace {
            let _ = write!(s, "&namespace={n}");
        }
        if self.which == Which::Log {
            if let Some(t) = &self.log_type {
                let _ = write!(s, "&type={}", utf8_percent_encode(t, NON_ALPHANUMERIC));
            }
            if let Some(u) = &self.user {
                let _ = write!(s, "&user={}", utf8_percent_encode(u, NON_ALPHANUMERIC));
            }
            if let Some(p) = &self.page {
                let _ = write!(s, "&page={}", utf8_percent_encode(p, NON_ALPHANUMERIC));
            }
        }
        let _ = write!(s, "&days={}&limit={}", self.days, self.limit);
        if let Some(c) = cursor {
            let _ = write!(s, "&continue={}", utf8_percent_encode(c, NON_ALPHANUMERIC));
        }
        s
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` from µs since the epoch.
fn iso8601(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// A date from an ISO 8601 timestamp, as the reader's language writes it.
pub(crate) fn date(m: &Messages, iso: &str) -> String {
    let d = iso.split('T').next().unwrap_or(iso);
    let mut it = d.split('-');
    match (
        it.next(),
        it.next().and_then(|x| x.parse::<u32>().ok()),
        it.next().and_then(|x| x.parse::<u32>().ok()),
    ) {
        (Some(y), Some(mo), Some(day)) if (1..=12).contains(&mo) => m.with(
            "ts-time-day",
            &[&day.to_string(), &m.get(&format!("ts-month-{mo}")), y],
        ),
        _ => d.to_string(),
    }
}

/// `HH:MM` from an ISO 8601 timestamp.
pub(crate) fn clock(iso: &str) -> &str {
    iso.split('T').nth(1).and_then(|t| t.get(..5)).unwrap_or("")
}

/// The provider's code for a source such as `mirror/wikidata`.
fn provider_code<'a>(site: &'a SiteInfo, source: &str) -> Option<&'a str> {
    let slug = source.strip_prefix("mirror/")?;
    site.providers
        .iter()
        .find(|p| p.slug == slug)
        .map(|p| p.code.as_str())
}

// ---------------------------------------------------------------------------------------
// The view models (19 §5.10): what the skin's `changes.html` macros and the
// `special/changes.html`, `special/watchlist.html` and `special/notifications.html`
// templates draw.
// ---------------------------------------------------------------------------------------

/// An account, as a target or as the actor (0095 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum AccountModel {
    /// The instance's own key: the Server Operator badge.
    Operator {
        /// The badge.
        badge: ChipModel,
    },
    /// A person: their name linked to their contributions.
    Named {
        /// The person icon's hidden text, where the account is a target.
        icon: Option<String>,
        /// The key, as the name's tooltip, where the wiki named the key.
        title: Option<String>,
        /// The contributions page.
        href: String,
        /// The name shown.
        shown: String,
        /// Whether it is the target (`ts-change__target`) or the actor.
        target: bool,
        /// The key beside the name, for another wiki's account the wiki has not named.
        key: Option<String>,
    },
}

/// A row's target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TargetModel {
    /// An account.
    Account {
        /// The account.
        account: AccountModel,
    },
    /// An ID with no title.
    Id {
        /// The ID.
        id: String,
    },
    /// A job, named and not linked.
    Job {
        /// "job n".
        text: String,
    },
    /// A title with its label.
    Labelled {
        /// The page.
        href: String,
        /// The label's language, where it differs from the page's.
        lang: Option<String>,
        /// The label.
        label: String,
        /// The ID beside it.
        id: String,
    },
    /// A title alone.
    Titled {
        /// The page.
        href: String,
        /// The title as shown.
        text: String,
    },
}

/// A row's actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ActorModel {
    /// Hidden.
    Hidden {
        /// What the row says.
        text: String,
    },
    /// An account.
    Account {
        /// The account.
        account: AccountModel,
    },
    /// None known.
    None,
}

/// A row's summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SummaryModel {
    /// What the row says of a hidden summary; the rest is empty then.
    pub hidden: Option<String>,
    /// The automatic part.
    pub auto: Option<String>,
    /// Where a move went.
    pub moved: Option<String>,
    /// The user's comment.
    pub comment: Option<String>,
}

/// A flag: new, minor, bot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flag {
    /// `new`, `minor` or `bot`.
    pub class: &'static str,
    /// The tooltip.
    pub title: String,
    /// The letter.
    pub text: String,
}

/// The size change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Size {
    /// `pos` or `neg`, for a change.
    pub class: Option<&'static str>,
    /// `+20`, `−5`, `0` or `(400)`.
    pub text: String,
}

/// One row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RowModel {
    /// The kind: `edit`, `sync`, `job`, `log`, `erased`.
    pub kind: String,
    /// Whether the viewer has not seen it (the watchlist).
    pub unseen: bool,
    /// ISO 8601.
    pub datetime: String,
    /// `HH:MM`.
    pub time: String,
    /// The diff page, where the row has one.
    pub diff: Option<String>,
    /// The chip that says where it came from.
    pub origin: ChipModel,
    /// What an erased row says, spanning the other columns.
    pub erased: Option<(usize, String)>,
    /// The flags.
    pub flags: Vec<Flag>,
    /// The target.
    pub target: Option<TargetModel>,
    /// The size change.
    pub size: Option<Size>,
    /// The actor.
    pub actor: ActorModel,
    /// The summary.
    pub summary: SummaryModel,
    /// The tags, but for `bot`.
    pub tags: Vec<String>,
}

/// A day's rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Day {
    /// The date, as the reader's language writes it.
    pub date: String,
    /// The rows.
    pub rows: Vec<RowModel>,
}

/// The changes table: the columns and the rows by day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListModel {
    /// The columns: class and heading.
    pub columns: Vec<(&'static str, String)>,
    /// The days.
    pub days: Vec<Day>,
}

/// One control of the filter form: the shared vocabulary of `controls` (0097 A8).
pub use crate::special::controls::FormItem;

/// The filter form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FormModel {
    /// The accessible name.
    pub label: String,
    /// The page it filters.
    pub title: String,
    /// The controls.
    pub items: Vec<FormItem>,
    /// **Apply**.
    pub apply: String,
}

/// The sync strip of 19 §3.9.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Strip {
    /// "Mirror syncs are hidden."
    pub text: String,
    /// **Show them**.
    pub show: Choice,
}

/// A feed page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangesPage {
    /// The filter form.
    pub form: FormModel,
    /// The sync strip, when syncs are off.
    pub strip: Option<Strip>,
    /// What the API refused.
    pub error: Option<MessageModel>,
    /// What the page says when it has no rows.
    pub empty: Option<String>,
    /// The rows.
    pub list: Option<ListModel>,
    /// The pager.
    pub pager: Option<Pager>,
    /// The introduction of contributions asked for no one.
    pub intro: Option<String>,
}

fn choice(href: String, label: String) -> Choice {
    Choice {
        href,
        label,
        current: false,
    }
}

/// The chip that says where a row came from.
fn origin_chip(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> ChipModel {
    match row.kind.as_str() {
        "sync" => match provider_code(site, &row.source) {
            Some(code) => Chip::new(ChipKind::Provider(code.to_string()), code).model(),
            None => Chip::new(ChipKind::Local, &row.source).model(),
        },
        "job" => Chip::new(ChipKind::Job, &m.get("ts-changes-chip-job")).model(),
        "log" => Chip::new(ChipKind::Log, &m.get("ts-changes-chip-log")).model(),
        _ => Chip::new(ChipKind::Local, &m.get("ts-changes-chip-local")).model(),
    }
}

/// The instance's own actor key, `instance:{farm code}`, which the site shows as the
/// **Server Operator** (0095 §3).
fn operator_key(site: &SiteInfo) -> Option<String> {
    (!site.farm_code.is_empty()).then(|| format!("instance:{}", site.farm_code))
}

/// An account, as a target or as the actor (0095 §3): the instance's own key as the
/// Server Operator badge; else a person icon, marking it as the account and not its user
/// page, and its name linked to its contributions. The name is the account's username
/// where the row gives it, else the wiki's name for the key (`Interface:Name/{key}`), with
/// the key as its tooltip, else the key. The key is shown beside the name only for an
/// account of another wiki that the wiki has not named. A key of this wiki's issuer, or
/// `local`, is this wiki's.
fn account_model(
    m: &Messages,
    site: &SiteInfo,
    key: Option<&str>,
    username: Option<&str>,
    icon: bool,
) -> Option<AccountModel> {
    if let (Some(k), Some(op)) = (key, operator_key(site))
        && k == op
    {
        return Some(AccountModel::Operator {
            badge: Chip {
                kind: ChipKind::Operator,
                text: &m.get("ts-server-operator"),
                title: Some(k),
            }
            .model(),
        });
    }
    let named = key.and_then(|k| m.name_of(k));
    let shown = username
        .map(str::to_string)
        .or(named.clone())
        .or(key.map(str::to_string))?;
    // An API that reports no issuer code (a stock Wikibase) is taken to be naming its own.
    let ours = site.issuer.is_empty()
        || key
            .and_then(|k| k.split_once(':'))
            .is_none_or(|(issuer, _)| issuer == "local" || issuer == site.issuer);
    let show_key = key.filter(|_| !ours && named.is_none() && username.is_some());
    // A name the wiki gave the key carries the key as its tooltip.
    let title = key
        .filter(|_| username.is_none() && named.is_some())
        .map(str::to_string);
    let who = username.or(key).unwrap_or(&shown);
    Some(AccountModel::Named {
        icon: icon.then(|| m.get("ts-changes-account")),
        title,
        href: format!("/wiki/Special:Contributions/{}", title_url(who)),
        shown: shown.clone(),
        target: icon,
        key: show_key.map(str::to_string),
    })
}

/// The target: its title linked, with its label where one is known. An account is
/// [`account_model`]'s. In a discussion's history (`subject_only`), a thread is named by
/// its subject, the date and namespace being the same on every row.
fn target_model(
    m: &Messages,
    site: &SiteInfo,
    row: &ActivityRow,
    subject_only: bool,
) -> Option<TargetModel> {
    let lang = m.lang();
    let t = row.target.as_ref()?;
    if t.kind == "actor" {
        return account_model(
            m,
            site,
            Some(t.id.as_str()).filter(|k| !k.is_empty()),
            t.label.as_ref().map(|l| l.value.as_str()),
            true,
        )
        .map(|account| TargetModel::Account { account });
    }
    let Some(title) = &t.title else {
        return Some(TargetModel::Id { id: t.id.clone() });
    };
    // A job's page (`Special:Jobs/{n}`) is not built yet: the job is named, not linked.
    if let Some(n) = title.strip_prefix("Special:Jobs/") {
        return Some(TargetModel::Job {
            text: m.with("ts-changes-job", &[n]),
        });
    }
    let href = format!("/wiki/{}", title_url(title));
    if let Some(label) = &t.label {
        return Some(TargetModel::Labelled {
            href,
            lang: (!label.language.is_empty() && label.language != lang)
                .then(|| label.language.clone()),
            label: label.value.clone(),
            id: t.id.clone(),
        });
    }
    let shown = if subject_only && is_thread_row(row) {
        title.split_once('/').map_or(title.as_str(), |(_, s)| s)
    } else {
        title.as_str()
    };
    Some(TargetModel::Titled {
        href,
        text: shown.to_string(),
    })
}

/// The actor: a link to their contributions, the Server Operator badge for the instance,
/// or the hidden marker.
fn actor_model(m: &Messages, site: &SiteInfo, row: &ActivityRow) -> ActorModel {
    if row.actor.hidden {
        return ActorModel::Hidden {
            text: m.get("ts-changes-hidden-user"),
        };
    }
    account_model(
        m,
        site,
        row.actor.key.as_deref(),
        row.actor.name.as_deref(),
        false,
    )
    .map_or(ActorModel::None, |account| ActorModel::Account { account })
}

/// Whether a row is a thread's: its target is a page in the `Thread` namespace.
fn is_thread_row(row: &ActivityRow) -> bool {
    row.target
        .as_ref()
        .and_then(|t| t.title.as_deref())
        .is_some_and(|t| t.starts_with("Thread:"))
}

/// A thread record's automatic summary in words (16 §6.1): what the record did, and for a
/// listing or a move, relative to the talk page whose history this is (`here`), so that a
/// history reads "listed the thread here" or "moved the thread away".
fn thread_summary(m: &Messages, row: &ActivityRow, here: Option<u64>) -> Option<String> {
    let op = row.summary.op.as_deref()?;
    let param = |k: &str| {
        row.log
            .as_ref()
            .and_then(|l| l.params.as_ref())
            .and_then(|p| p.get(k))
            .and_then(serde_json::Value::as_u64)
    };
    let is_here = |k: &str| here.is_some() && param(k) == here;
    let key = match op {
        "create" => "ts-changes-thread-create",
        "post" => "ts-changes-thread-post",
        "edit" => "ts-changes-thread-edit",
        "rename" => {
            return Some(m.with(
                "ts-changes-thread-rename",
                &[row.summary.args.as_deref().unwrap_or_default()],
            ));
        }
        "move" if is_here("from_talk") => "ts-changes-thread-move-away",
        "move" if is_here("talk") => "ts-changes-thread-move-here",
        "move" => "ts-changes-thread-move",
        "attach" if is_here("talk") => "ts-changes-thread-attach-here",
        "attach" => "ts-changes-thread-attach",
        "detach" if is_here("talk") => "ts-changes-thread-detach-here",
        "detach" => "ts-changes-thread-detach",
        "pin" => "ts-changes-thread-pin",
        "unpin" => "ts-changes-thread-unpin",
        _ => return None,
    };
    Some(m.get(key))
}

/// The summary: the user's comment, and the automatic part muted.
fn summary_model(m: &Messages, row: &ActivityRow, here: Option<u64>) -> SummaryModel {
    let mut out = SummaryModel {
        hidden: None,
        auto: None,
        moved: None,
        comment: None,
    };
    if row.summary.hidden {
        out.hidden = Some(m.get("ts-changes-hidden-comment"));
        return out;
    }
    let auto = match (&row.summary.op, &row.summary.args) {
        (Some(op), Some(args)) => Some(format!("{op}: {args}")),
        (Some(op), None) => Some(op.clone()),
        _ => None,
    };
    out.auto = if row.kind == "sync" {
        let provider = row.source.strip_prefix("mirror/").unwrap_or(&row.source);
        Some(m.with("ts-changes-synced", &[provider]))
    } else if let Some(t) = is_thread_row(row)
        .then(|| thread_summary(m, row, here))
        .flatten()
    {
        Some(t)
    } else {
        auto
    };
    // A move names where the page went (10 §2.5).
    out.moved = row
        .log
        .as_ref()
        .filter(|l| l.r#type == "move" && !is_thread_row(row))
        .and_then(|l| {
            let p = l.params.as_ref()?;
            Some((
                p.get("ts-from-title")?.as_str()?,
                p.get("target_title")?.as_str()?,
            ))
        })
        .map(|(from, to)| m.with("ts-changes-moved-to", &[from, to]));
    out.comment = row.comment.clone().filter(|c| !c.is_empty());
    out
}

/// The size change: `+20`, `−5`, or the size when nothing came before.
fn size_model(row: &ActivityRow) -> Option<Size> {
    match (row.delta, row.size) {
        (Some(d), _) if d != 0 => Some(Size {
            class: Some(if d > 0 { "pos" } else { "neg" }),
            text: format!("{}{}", if d > 0 { "+" } else { "\u{2212}" }, d.abs()),
        }),
        (Some(_), _) => Some(Size {
            class: None,
            text: "0".into(),
        }),
        (None, Some(size)) => Some(Size {
            class: None,
            text: format!("({size})"),
        }),
        (None, None) => None,
    }
}

/// The diff page of a row (19 §3.8): `?diff={revid}` for a local edit, `?diff={partition}:{offset}`
/// for a mirror sync, which has no local revision ID; nothing for the other kinds.
fn diff_href(row: &ActivityRow) -> Option<String> {
    let title = row.target.as_ref()?.title.as_deref()?;
    // A thread's rows go to the post they wrote, or to the thread: a thread has no diffs
    // yet (`compare` refuses its content).
    if is_thread_row(row) {
        let href = format!("/wiki/{}", title_url(title));
        return Some(match (row.summary.op.as_deref(), row.revid) {
            (Some("create" | "post"), Some(r)) => format!("{href}#post-{r}"),
            _ => href,
        });
    }
    let arg = match row.kind.as_str() {
        "edit" if row.source == "local" || row.source == "pages" => row.revid?.to_string(),
        "sync" => format!("{}:{}", row.record.partition, row.record.offset),
        _ => return None,
    };
    Some(format!("/wiki/{}?diff={arg}", title_url(title)))
}

/// The columns of the changes table, by the message that heads each. Every row has a
/// cell in each, empty where the row has nothing for it, so the fields line up.
const COLUMNS: [(&str, &str); 7] = [
    ("ts-changes-col-time", "time"),
    ("ts-changes-col-origin", "origin"),
    ("ts-changes-col-flags", "flags"),
    ("ts-changes-col-target", "target"),
    ("ts-changes-col-size", "size"),
    ("ts-changes-col-user", "user"),
    ("ts-changes-col-summary", "summary"),
];

fn row_model(m: &Messages, site: &SiteInfo, row: &ActivityRow, here: Option<u64>) -> RowModel {
    let origin = origin_chip(m, site, row);
    if row.kind == "erased" {
        return RowModel {
            kind: row.kind.clone(),
            unseen: row.unseen,
            datetime: row.time.clone(),
            time: clock(&row.time).to_string(),
            diff: diff_href(row),
            origin,
            erased: Some((COLUMNS.len() - 2, m.get("ts-changes-erased"))),
            flags: Vec::new(),
            target: None,
            size: None,
            actor: ActorModel::None,
            summary: SummaryModel {
                hidden: None,
                auto: None,
                moved: None,
                comment: None,
            },
            tags: Vec::new(),
        };
    }
    let mut flags = Vec::new();
    for (on, msg, class) in [
        (row.flags.new, "ts-changes-flag-new", "new"),
        (row.flags.minor, "ts-changes-flag-minor", "minor"),
        (row.flags.bot, "ts-changes-flag-bot", "bot"),
    ] {
        if on {
            flags.push(Flag {
                class,
                title: m.get(&format!("{msg}-title")),
                text: m.get(msg),
            });
        }
    }
    let target = target_model(m, site, row, here.is_some()).or_else(|| {
        row.job.map(|j| TargetModel::Job {
            text: m.with("ts-changes-job", &[&j.to_string()]),
        })
    });
    RowModel {
        kind: row.kind.clone(),
        unseen: row.unseen,
        datetime: row.time.clone(),
        time: clock(&row.time).to_string(),
        diff: diff_href(row),
        origin,
        erased: None,
        flags,
        target,
        size: size_model(row),
        actor: actor_model(m, site, row),
        summary: summary_model(m, row, here),
        tags: row.tags.iter().filter(|t| *t != "bot").cloned().collect(),
    }
}

/// The rows, grouped by day.
pub(crate) fn list_model(m: &Messages, site: &SiteInfo, rows: &[ActivityRow]) -> Option<ListModel> {
    list_model_here(m, site, rows, None)
}

/// The rows as one table, a row group per day headed by its date, as the history of the
/// talk page `here` reads them: a thread listed on it or moved to or from it says so
/// (16 §6.1).
pub(crate) fn list_model_here(
    m: &Messages,
    site: &SiteInfo,
    rows: &[ActivityRow],
    here: Option<u64>,
) -> Option<ListModel> {
    if rows.is_empty() {
        return None;
    }
    let mut days: Vec<Day> = Vec::new();
    let mut day: Option<&str> = None;
    for row in rows {
        let d = row.time.split('T').next().unwrap_or("");
        if day != Some(d) {
            days.push(Day {
                date: date(m, &row.time),
                rows: Vec::new(),
            });
            day = Some(d);
        }
        if let Some(last) = days.last_mut() {
            last.rows.push(row_model(m, site, row, here));
        }
    }
    Some(ListModel {
        columns: COLUMNS
            .iter()
            .map(|(msg, col)| (*col, m.get(msg)))
            .collect(),
        days,
    })
}

/// The filter form.
#[allow(clippy::too_many_lines)]
fn form_model(m: &Messages, ask: &Ask) -> FormModel {
    let title = match (ask.which, &ask.user) {
        (Which::RecentChanges, _) => "Special:RecentChanges".to_string(),
        (Which::Log, _) => "Special:Log".to_string(),
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        (Which::Contributions, None) => "Special:Contributions".to_string(),
        (Which::NewPages, _) => "Special:NewPages".to_string(),
    };
    let text = |label: String, name: &str, value: Option<&str>| {
        FormItem::text(label, name, value.unwrap_or(""))
    };
    let bots = || FormItem::check("bots", "1", !ask.hide_bots, m.get("ts-changes-bots"));
    let select = |label: String, name: &str, options: Vec<(String, String)>, current: &str| {
        FormItem::select(
            label,
            name,
            options
                .into_iter()
                .map(|(v, t)| {
                    let selected = v == current;
                    (v, t, selected)
                })
                .collect(),
        )
    };
    let days = || {
        select(
            m.get("ts-changes-days"),
            "days",
            DAYS.iter()
                .map(|d| {
                    (
                        d.to_string(),
                        m.with("ts-changes-days-n", &[&d.to_string()]),
                    )
                })
                .collect(),
            &ask.days.to_string(),
        )
    };
    let limit = || {
        select(
            m.get("ts-changes-limit"),
            "limit",
            LIMITS
                .iter()
                .map(|l| (l.to_string(), l.to_string()))
                .collect(),
            &ask.limit.to_string(),
        )
    };
    let namespace = || {
        let mut namespaces = vec![(String::new(), m.get("ts-changes-namespace-all"))];
        namespaces.extend(
            NAMESPACES
                .iter()
                .map(|(n, msg)| ((*n).to_string(), m.get(msg))),
        );
        select(
            m.get("ts-changes-namespace"),
            "namespace",
            namespaces,
            ask.namespace.as_deref().unwrap_or(""),
        )
    };
    let mut items = Vec::new();
    match ask.which {
        Which::Contributions => {
            items.push(text(
                m.get("ts-contributions-user"),
                "target",
                ask.user.as_deref(),
            ));
        }
        Which::NewPages => {
            items.push(text(
                m.get("ts-newpages-user"),
                "username",
                ask.user.as_deref(),
            ));
            items.push(bots());
        }
        Which::Log => {
            let mut options = vec![(String::new(), m.get("ts-log-type-all"))];
            options.extend(
                LOG_TYPES
                    .iter()
                    .map(|(k, msg)| ((*k).to_string(), m.get(msg))),
            );
            items.push(select(
                m.get("ts-log-type"),
                "type",
                options,
                ask.log_type.as_deref().unwrap_or(""),
            ));
            items.push(text(m.get("ts-log-user"), "user", ask.user.as_deref()));
            items.push(text(m.get("ts-log-page"), "page", ask.page.as_deref()));
        }
        Which::RecentChanges => {}
    }
    if ask.which == Which::Log {
        items.push(days());
        items.push(limit());
    } else {
        if ask.which != Which::NewPages {
            let mut kinds: Vec<FormItem> = KINDS
                .iter()
                .map(|(kind, msg, _)| {
                    FormItem::check(
                        "kind",
                        kind,
                        ask.kinds.iter().any(|k| k == kind),
                        m.get(msg),
                    )
                })
                .collect();
            kinds.push(bots());
            items.push(FormItem::group(m.get("ts-changes-kinds"), kinds));
        }
        items.push(namespace());
        items.push(days());
        items.push(limit());
    }
    FormModel {
        label: m.get("ts-changes-filter-label"),
        title,
        items,
        apply: m.get("ts-changes-apply"),
    }
}

/// The pager: **Newest** when on a later page, **Older** when there is one.
fn pager_model(m: &Messages, ask: &Ask, next: Option<&str>) -> Option<Pager> {
    if ask.cursor.is_none() && next.is_none() {
        return None;
    }
    Some(Pager {
        label: m.get("ts-search-pages-label"),
        newest: ask
            .cursor
            .as_ref()
            .map(|_| choice(ask.href(None), m.get("ts-changes-newest"))),
        older: next.map(|n| choice(ask.href(Some(n)), m.get("ts-changes-older"))),
    })
}

/// Fetches the page, adding its caching to the context.
async fn fetch(
    site: &Site,
    cx: &mut Context,
    incoming: &triplespace_client::Incoming,
    ask: &Ask,
) -> Result<ActivityPage, ClientError> {
    let lang = cx.m.lang().to_string();
    // The log's page, as a target: an entity by its ID, a page by the ID `prop=info`
    // gives it; a title with no page the viewer may see has no rows.
    let target = match &ask.page {
        None => None,
        Some(p) => {
            let t = site.client().log_target(incoming, p).await?;
            cx.inputs.push(t.cache);
            match t.value {
                Some(t) => Some(t),
                None => {
                    return Ok(ActivityPage {
                        rows: Vec::new(),
                        next: None,
                    });
                }
            }
        }
    };
    let f = site
        .client()
        .activity(incoming, &ask.query(now_micros(), &lang, target))
        .await?;
    cx.inputs.push(f.cache);
    Ok(f.value)
}

/// The sync strip of 19 §3.9: when syncs are off, says so and offers them.
fn strip_model(m: &Messages, ask: &Ask) -> Option<Strip> {
    if ask.kinds.iter().any(|k| k == "sync") {
        return None;
    }
    let mut with = ask.clone();
    with.kinds.push("sync".into());
    with.cursor = None;
    Some(Strip {
        text: m.get("ts-changes-syncs-hidden"),
        show: choice(with.href(None), m.get("ts-changes-syncs-show")),
    })
}

async fn serve_page(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    which: Which,
    subpage: Option<&str>,
    params: &Params,
) -> Response {
    let incoming = pages::incoming(headers, peer);
    let mut cx = match pages::context(site, &incoming, &params.map).await {
        Ok(cx) => cx,
        Err(r) => return r,
    };
    let title_key = match which {
        Which::RecentChanges => "Special:RecentChanges",
        Which::Contributions => "Special:Contributions",
        Which::Log => "Special:Log",
        Which::NewPages => "Special:NewPages",
    };
    if !cx.features.has(Feature::RecentChanges) {
        return pages::no_page(&cx, headers, title_key.into());
    }
    let ask = Ask::of(which, params, subpage);
    let mut page = None;
    let mut error = None;
    if which != Which::Contributions || ask.user.is_some() {
        match fetch(site, &mut cx, &incoming, &ask).await {
            Ok(p) => page = Some(p),
            Err(ClientError::Api { code, info }) if code == "bad-parameter" => {
                error = Some(info);
            }
            Err(e) => return pages::api_failed(&cx, headers, &e),
        }
    }
    let m = &cx.m;
    let empty = match (&page, &ask.user, which) {
        (Some(p), Some(u), Which::Contributions) if p.rows.is_empty() => {
            Some(m.with("ts-contributions-none", &[u]))
        }
        (Some(p), _, Which::NewPages) if p.rows.is_empty() => Some(m.get("ts-newpages-none")),
        (Some(p), _, _) if p.rows.is_empty() => Some(m.get("ts-changes-none")),
        _ => None,
    };
    let body = cx.draw(
        "special/changes.html",
        ChangesPage {
            form: form_model(m, &ask),
            strip: (which == Which::RecentChanges)
                .then(|| strip_model(m, &ask))
                .flatten(),
            error: error
                .as_deref()
                .map(|e| Message::block(MessageKind::Error, e).model()),
            empty,
            list: page.as_ref().and_then(|p| list_model(m, &cx.site, &p.rows)),
            pager: page
                .as_ref()
                .filter(|p| !p.rows.is_empty())
                .and_then(|p| pager_model(m, &ask, p.next.as_deref())),
            intro: (page.is_none() && ask.user.is_none() && which == Which::Contributions)
                .then(|| m.get("ts-contributions-intro")),
        },
    );
    let title = match (which, &ask.user) {
        (Which::RecentChanges, _) => m.get("ts-recentchanges-title"),
        (Which::Log, _) => m.get("ts-log-title"),
        (Which::Contributions, Some(u)) => m.with("ts-contributions-title", &[u]),
        (Which::Contributions, None) => m.get("ts-contributions-title-none"),
        (Which::NewPages, _) => m.get("ts-newpages-title"),
    };
    let returnto = match (which, &ask.user) {
        (Which::Contributions, Some(u)) => format!("Special:Contributions/{u}"),
        _ => title_key.to_string(),
    };
    pages::respond(&cx, headers, StatusCode::OK, |_| Page {
        title,
        returnto: Some(returnto),
        body,
        ..Page::default()
    })
}

/// `Special:Log`, `Special:Log/{type}`, `Special:Log/{user}`.
pub async fn log(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let subpage = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest)
        .filter(|s| !s.is_empty());
    serve_page(site, headers, peer, Which::Log, subpage, params).await
}

/// `Special:RecentChanges`.
pub async fn recent_changes(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    params: &Params,
) -> Response {
    serve_page(site, headers, peer, Which::RecentChanges, None, params).await
}

/// `Special:NewPages`.
pub async fn new_pages(site: &Site, headers: &HeaderMap, peer: Peer, params: &Params) -> Response {
    serve_page(site, headers, peer, Which::NewPages, None, params).await
}

/// `Special:Contributions`, `Special:Contributions/{user}`.
pub async fn contributions(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Response {
    let subpage = title
        .strip_prefix("Special:")
        .and_then(|t| t.split_once('/'))
        .map(|(_, rest)| rest)
        .filter(|s| !s.is_empty());
    serve_page(site, headers, peer, Which::Contributions, subpage, params).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(pairs: &[(&str, &str)]) -> Params {
        Params::new(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn the_defaults_leave_syncs_out() {
        let ask = Ask::of(Which::RecentChanges, &params(&[]), None);
        assert_eq!(ask.kinds, vec!["edit", "job", "log"]);
        assert_eq!(ask.days, DEFAULT_DAYS);
        assert_eq!(ask.limit, DEFAULT_LIMIT);
        assert!(!ask.hide_bots);
        let q = ask.query(90 * 86_400 * 1_000_000, "en", None);
        assert_eq!(q.since.as_deref(), Some("1970-03-02T00:00:00Z"));
        assert_eq!(q.kinds, vec!["edit", "job", "log", "erased"]);
    }

    #[test]
    fn a_filtered_form_is_taken_as_given() {
        let ask = Ask::of(
            Which::RecentChanges,
            &params(&[
                ("filtered", "1"),
                ("kind", "edit"),
                ("kind", "sync"),
                ("kind", "nope"),
                ("hidebots", "1"),
                ("namespace", "122"),
                ("days", "7"),
                ("limit", "999"),
                ("continue", "1|2|3"),
            ]),
            None,
        );
        assert_eq!(ask.kinds, vec!["edit", "sync"]);
        assert!(ask.hide_bots);
        assert_eq!(ask.namespace.as_deref(), Some("122"));
        assert_eq!(ask.days, 7);
        assert_eq!(ask.limit, DEFAULT_LIMIT, "an unknown limit falls back");
        assert_eq!(
            ask.href(Some("4|5|6")),
            "/w/index.php?title=Special:RecentChanges&filtered=1&kind=edit&kind=sync&hidebots=1&namespace=122&days=7&limit=50&continue=4%7C5%7C6"
        );
        let empty = Ask::of(Which::RecentChanges, &params(&[("filtered", "1")]), None);
        assert!(empty.kinds.is_empty(), "every kind unticked is none");
    }

    #[test]
    fn contributions_take_the_user_from_the_subpage_or_the_form() {
        let a = Ask::of(Which::Contributions, &params(&[]), Some("Alice_bot"));
        assert_eq!(a.user.as_deref(), Some("Alice bot"));
        assert_eq!(
            a.href(None),
            "/w/index.php?title=Special:Contributions/Alice%5Fbot&filtered=1&kind=edit&kind=job&kind=log&days=30&limit=50"
        );
        let b = Ask::of(Which::Contributions, &params(&[("target", " Bob ")]), None);
        assert_eq!(b.user.as_deref(), Some("Bob"));
        let c = Ask::of(Which::Contributions, &params(&[("target", "")]), None);
        assert_eq!(c.user, None);
    }

    #[test]
    fn new_pages_are_the_creating_edits() {
        let a = Ask::of(
            Which::NewPages,
            &params(&[("username", " Bob "), ("hidebots", "1"), ("kind", "job")]),
            None,
        );
        let q = a.query(10_000_000, "en", None);
        assert_eq!(
            q.kinds,
            ["edit", "erased"],
            "edits only, whatever the form says"
        );
        assert_eq!(q.show, ["!bot", "new"]);
        assert_eq!(q.actor.as_deref(), Some("Bob"));
        assert!(
            a.href(None)
                .starts_with("/w/index.php?title=Special:NewPages&filtered=1&username=Bob")
        );
        let b = Ask::of(Which::NewPages, &params(&[]), None);
        assert_eq!(b.query(10_000_000, "en", None).show, ["new"]);
    }

    #[test]
    fn times() {
        assert_eq!(iso8601(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601(1_758_760_000_000_000), "2025-09-25T00:26:40Z");
        assert_eq!(clock("2025-09-25T00:26:40Z"), "00:26");
    }

    #[test]
    fn sizes() {
        let mut r = ActivityRow {
            delta: Some(20),
            size: Some(400),
            ..ActivityRow::default()
        };
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("+20"));
        r.delta = Some(-5);
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("\u{2212}5"));
        r.delta = None;
        assert_eq!(size_model(&r).map(|s| s.text).as_deref(), Some("(400)"));
    }

    #[test]
    fn a_threads_rows_read_from_the_talk_page_they_are_shown_on() {
        // 16 §6.1: listings and moves relative to the page whose history this is.
        let m = Messages::for_language("en");
        let row = |op: &str, params: serde_json::Value| ActivityRow {
            kind: "edit".into(),
            time: "2026-10-25T20:27:00Z".into(),
            source: "pages".into(),
            revid: Some(930),
            target: Some(triplespace_client::activity::ActivityTarget {
                kind: "page".into(),
                id: "29".into(),
                title: Some("Thread:2026-10-25/Why is P31 wrong here?".into()),
                ..Default::default()
            }),
            summary: triplespace_client::activity::Summary {
                op: Some(op.into()),
                ..Default::default()
            },
            log: Some(triplespace_client::activity::LogInfo {
                r#type: "thread".into(),
                action: op.into(),
                params: Some(params),
            }),
            ..ActivityRow::default()
        };
        let say = |r: &ActivityRow, here| thread_summary(&m, r, here).unwrap();
        let attach = row("attach", serde_json::json!({"talk": 7}));
        assert_eq!(say(&attach, Some(7)), "listed the thread here");
        assert_eq!(say(&attach, Some(8)), "listed the thread on another page");
        let mv = row("move", serde_json::json!({"talk": 9, "from_talk": 7}));
        assert_eq!(say(&mv, Some(7)), "moved the thread to another page");
        assert_eq!(say(&mv, Some(9)), "moved the thread here");
        assert_eq!(say(&row("post", serde_json::Value::Null), None), "replied");

        // A post links to itself, on the thread page; the subject names the thread.
        let post = row("post", serde_json::Value::Null);
        assert_eq!(
            diff_href(&post).as_deref(),
            Some("/wiki/Thread:2026-10-25/Why_is_P31_wrong_here%3F#post-930")
        );
        let shown = |here| match row_model(&m, &SiteInfo::default(), &post, here).target {
            Some(TargetModel::Titled { text, .. }) => text,
            other => panic!("{other:?}"),
        };
        assert_eq!(shown(Some(7)), "Why is P31 wrong here?");
        assert_eq!(shown(None), "Thread:2026-10-25/Why is P31 wrong here?");
    }

    /// A log row whose target is an account, as `rights/rights` writes them.
    fn rights_row(
        target_key: &str,
        label: &str,
        actor_key: &str,
        actor: Option<&str>,
    ) -> ActivityRow {
        serde_json::from_value(serde_json::json!({
            "kind": "log", "time": "2026-10-28T05:31:00Z", "source": "local",
            "record": {"partition": 1, "offset": 1},
            "target": {"kind": "actor", "id": target_key, "ns": 2, "title": format!("User:{label}"),
                       "label": {"value": label, "language": "en"}},
            "actor": {"key": actor_key, "name": actor},
            "summary": {"op": "rights/rights"},
        }))
        .unwrap()
    }

    #[test]
    fn accounts_are_marked_and_the_instance_is_the_server_operator() {
        let m = Messages::for_language("en");
        let site = SiteInfo {
            issuer: "wwq".into(),
            farm_code: "gpu".into(),
            ..SiteInfo::default()
        };
        let target = |m: &Messages, row: &ActivityRow| match row_model(m, &site, row, None).target {
            Some(TargetModel::Account { account }) => account,
            other => panic!("{other:?}"),
        };
        // An account of this wiki: the icon, its name to its contributions, no key.
        let a = target(
            &m,
            &rights_row("wwq:25", "Harej bot 2", "wwq:7", Some("Harej")),
        );
        let AccountModel::Named {
            icon, href, key, ..
        } = &a
        else {
            panic!("{a:?}");
        };
        assert_eq!(icon.as_deref(), Some("Account"));
        assert_eq!(
            href, "/wiki/Special:Contributions/Harej_bot_2",
            "not its user page"
        );
        assert_eq!(key, &None, "no key for this wiki's account");
        let actor = |m: &Messages, row: &ActivityRow| match row_model(m, &site, row, None).actor {
            ActorModel::Account { account } => account,
            other => panic!("{other:?}"),
        };
        // The instance acting: the badge, the key only as its tooltip.
        let a = actor(
            &m,
            &rights_row("wwq:1", "Librarybase", "instance:gpu", None),
        );
        let AccountModel::Operator { badge } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(badge.text, "Server Operator");
        assert_eq!(badge.title.as_deref(), Some("instance:gpu"));
        // Another wiki's account keeps its key beside its name; a named one does not.
        let a = target(&m, &rights_row("other:3", "Ann", "wwq:7", Some("Harej")));
        let AccountModel::Named { key, .. } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(key.as_deref(), Some("other:3"));
        let named = Messages::for_language("en").with_overrides(
            serde_json::from_value(serde_json::json!({"name/instance:far": {"": "Far Farm"}}))
                .unwrap(),
            "en",
        );
        let a = actor(&named, &rights_row("wwq:1", "X", "instance:far", None));
        let AccountModel::Named { shown, title, .. } = &a else {
            panic!("{a:?}");
        };
        assert_eq!(shown, "Far Farm");
        assert_eq!(title.as_deref(), Some("instance:far"));
    }

    #[test]
    fn entities_keep_their_ids() {
        let m = Messages::for_language("en");
        let row: ActivityRow = serde_json::from_value(serde_json::json!({
            "kind": "edit", "time": "2026-10-28T05:50:00Z", "source": "local",
            "record": {"partition": 1, "offset": 2},
            "target": {"kind": "entity", "id": "Q100001", "ns": 120, "title": "Item:Q100001",
                       "label": {"value": "The Supreme Court", "language": "en"}},
            "actor": {"key": "wwq:7", "name": "Harej"},
            "summary": {},
        }))
        .unwrap();
        let t = row_model(&m, &SiteInfo::default(), &row, None).target;
        let Some(TargetModel::Labelled { id, label, .. }) = t else {
            panic!("{t:?}");
        };
        assert_eq!(id, "Q100001");
        assert_eq!(label, "The Supreme Court");
    }
}
