//! Activity rows as the API serves them (0012 §3; 16 §3.1; 0020 §5): the two shapes of
//! one row, and the pieces every feed module shares.
//!
//! - [`mw_row`]: MediaWiki's shape, the one `list=recentchanges`, `list=usercontribs`
//!   and `list=logevents` return, so that Pywikibot and the bots that read recent
//!   changes see what they expect. Each module names the properties it wants.
//! - [`ts_row`]: the Triplespace shape of 16 §3.1, the one `GET /activity` and the
//!   site's pages read: the record, the target with its title and label, the actor, the
//!   summary, the flags, the sizes and the links.
//! - [`viewer`]: what the caller may see, for the `read` restrictions of 0056 §5 and §14.
//! - [`actor_key_of`]: a user name to its actor key, for `rcuser`, `ucuser` and
//!   `leuser`.
//!
//! Redaction (0012 §8; 0023 §5): a row whose `visibility` hides a part shows the part's
//! hidden flag in place of it, as MediaWiki does, and never the part; a row the viewer's
//! groups do not admit is not returned at all, which the feed query already ensures.

use serde_json::{Map, Value, json};
use triplespace_projections::feed::{self, Row, Viewer, iso8601, mw_timestamp, target_kind};

use crate::api::Ctx;
use crate::response::ApiError;
use crate::tenant::Tenant;

/// The visibility bits (0023 §5), as MediaWiki numbers them.
pub mod visibility {
    /// The content part is hidden (`texthidden`).
    pub const CONTENT: i16 = 1;
    /// The comment is hidden (`commenthidden`).
    pub const COMMENT: i16 = 2;
    /// The actor is hidden (`userhidden`).
    pub const USER: i16 = 4;
    /// Suppressed: hidden from administrators too.
    pub const SUPPRESSED: i16 = 8;
}

/// What the caller may see: its groups, and what confidential restrictions hide from it
/// ([`crate::pages::hidden`]).
pub async fn viewer(ctx: &Ctx) -> Result<Viewer, ApiError> {
    Ok(Viewer::of(
        &ctx.caller.effective,
        crate::pages::hidden(ctx).await?,
    ))
}

/// The actor key of a user name on the tenant, or of a key given as one (`local:7`).
pub async fn actor_key_of(ctx: &Ctx, name: &str) -> Result<Option<String>, tokio_postgres::Error> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let row = ctx
        .db()
        .query_opt(
            "SELECT actor_key, actor_key = $2 FROM view.actor
             WHERE tenant IN ($1, '') AND (lower(name) = lower($2) OR actor_key = $2)
             ORDER BY (tenant = $1) DESC LIMIT 1",
            &[&ctx.tenant.slug, &name],
        )
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let key: String = row.get(0);
    let by_key: bool = row.get(1);
    // A hidden name does not lead to its account for those outside the group (09 §6.3);
    // a key, which never contains a name, still does. Failing to decide hides it.
    if !by_key {
        let effective = &ctx.caller.effective;
        let hidden = triplespace_projections::acl_read::hidden_actors(
            ctx.db(),
            &ctx.tenant.slug,
            std::slice::from_ref(&key),
            |g| effective.reads_in(g),
            scatter_actors::Timestamp(crate::edit::now_micros()),
        )
        .await;
        if hidden.map_or(true, |h| h.contains(&key)) {
            return Ok(None);
        }
    }
    Ok(Some(key))
}

/// The numeric user ID MediaWiki clients expect: the number after the issuer in the
/// actor key, `0` for a surrogate. A surrogate's `actor:{n}` numbers the tenant's
/// surrogates, not accounts (0007 A27), and MediaWiki gives an IP or an imported edit
/// user ID 0.
#[must_use]
pub fn user_id(key: Option<&str>) -> u64 {
    key.and_then(|k| k.rsplit_once(':'))
        .filter(|(issuer, _)| *issuer != scatter_actors::issuer::ACTOR)
        .and_then(|(_, n)| n.parse().ok())
        .unwrap_or(0)
}

/// The namespace number and title of a row's target (namespaces.toml; 0017 §1).
#[must_use]
pub fn title_of(row: &Row) -> Option<(i64, String)> {
    let t = &row.target;
    match t.kind? {
        target_kind::ENTITY => {
            let id = t.id.as_deref()?;
            // A keyed entity has its type's namespace; a source's entity (`mhc:Q1`,
            // 0078 §2) its entity type's, as a foreign one does.
            if let Some((kind, key)) = id.split_once(':') {
                match kind {
                    "domain" => return Some((210, format!("Domain:{key}"))),
                    "keyword" => return Some((212, format!("Keyword:{key}"))),
                    "notation" => return Some((216, format!("Notation:{key}"))),
                    _ => {}
                }
            }
            Some(match t.entity_type.as_deref() {
                Some("property") => (122, format!("Property:{id}")),
                Some("lexeme") => (146, format!("Lexeme:{id}")),
                _ => (120, format!("Item:{id}")),
            })
        }
        target_kind::PAGE => {
            // `view.page` keeps the title without its namespace; a row's title has it,
            // canonically written.
            let (ns, title) = t.page.clone()?;
            let prefixed = match scatter_titles::Namespaces::embedded().get(ns) {
                Some(n) if ns != 0 => {
                    format!("{}:{title}", n.name.as_ref().unwrap_or(&n.canonical))
                }
                _ => title,
            };
            Some((i64::from(ns), prefixed))
        }
        // The account's name where the viewer may see it, else its key (09 §6.3).
        target_kind::ACTOR => t
            .label
            .as_ref()
            .map(|(n, _)| n.as_str())
            .or(t.id.as_deref())
            .map(|k| (2, format!("User:{k}"))),
        target_kind::JOB => t.id.as_ref().map(|j| (-1, format!("Special:Jobs/{j}"))),
        _ => None,
    }
}

/// The user name a row shows: the name; for an `anonymous` or `imported` surrogate the
/// raw value, as the source showed it (07 §1.4: the IP, or `imported>Name`); else the
/// key; nothing for a hidden actor.
pub(crate) fn user_name(row: &Row) -> Option<String> {
    if row.actor.hidden {
        return None;
    }
    let shown_raw = matches!(row.actor.kind.as_deref(), Some("anonymous" | "imported"));
    row.actor
        .name
        .clone()
        .or_else(|| row.actor.raw.clone().filter(|_| shown_raw))
        .or_else(|| row.actor.key.clone())
}

/// The automatic summary (0001 §6) in MediaWiki's comment field: the operation and its
/// arguments, then the user's comment after `: `, as Wikibase composes
/// `/* wbsetlabel-add:1|en */ Six`.
#[must_use]
pub fn comment_text(row: &Row) -> String {
    let mut s = String::new();
    if let Some(op) = &row.summary_op {
        s.push_str("/* ");
        s.push_str(op);
        if let Some(args) = &row.summary_args {
            s.push(':');
            s.push_str(args);
        }
        s.push_str(" */");
    } else if let (Some(t), Some(a)) = (&row.log_type, &row.log_action) {
        s.push_str("/* ");
        s.push_str(t);
        s.push('/');
        s.push_str(a);
        s.push_str(" */");
    }
    if let Some(c) = &row.comment {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(c);
    }
    s
}

/// Whether the properties list names `p`.
fn wants(props: &[String], p: &str) -> bool {
    props.iter().any(|x| x == p)
}

/// MediaWiki's shape of a row (compat §3.3): the fields `rcprop`, `ucprop` or `leprop`
/// name, under MediaWiki's rules — hidden parts as flags, `formatversion` deciding how a
/// boolean is spelt.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn mw_row(ctx: &Ctx, row: &Row, props: &[String], module: &str) -> Value {
    let v2 = ctx.formatversion == 2;
    let flag = |m: &mut Map<String, Value>, name: &str, on: bool| {
        if v2 {
            m.insert(name.into(), json!(on));
        } else if on {
            m.insert(name.into(), json!(""));
        }
    };
    let mut m = Map::new();
    let is_log = row.kind == "log" || row.kind == "job";
    // A log event whose content part is hidden (`delete/event`, 09 §4.3) shows neither
    // its target nor its parameters: MediaWiki's `actionhidden`.
    let action_hidden = row.log_type.is_some() && row.visibility & visibility::CONTENT != 0;
    if module == "recentchanges" {
        m.insert(
            "type".into(),
            json!(match row.kind.as_str() {
                "log" | "job" => "log",
                _ if row.new => "new",
                _ => "edit",
            }),
        );
    }
    // A move's event is titled by the title the page left, as MediaWiki's is (10 §2.5).
    let title = match (&row.log_type, &row.params) {
        (Some(t), Some(p)) if t == "move" => p
            .get("ts-from-title")
            .and_then(Value::as_str)
            .map(|from| {
                (
                    p.get("ts-from-ns").and_then(Value::as_i64).unwrap_or(0),
                    from.to_string(),
                )
            })
            .or_else(|| title_of(row)),
        _ => title_of(row),
    };
    if action_hidden {
        flag(&mut m, "actionhidden", true);
    }
    if (wants(props, "title") || module == "usercontribs")
        && !action_hidden
        && let Some((ns, title)) = &title
    {
        m.insert("ns".into(), json!(ns));
        m.insert("title".into(), json!(title));
    }
    if wants(props, "ids") {
        if let Some(p) = row.target.page_id {
            m.insert("pageid".into(), json!(p));
        } else if row.target.kind == Some(target_kind::PAGE)
            && let Some(id) = row.target.id.as_deref().and_then(|s| s.parse::<u64>().ok())
        {
            m.insert("pageid".into(), json!(id));
        }
        match module {
            "recentchanges" => {
                m.insert("revid".into(), json!(row.revid.unwrap_or(0)));
                m.insert("old_revid".into(), json!(0));
                m.insert("rcid".into(), json!(row.offset));
                if is_log {
                    m.insert("logid".into(), json!(row.logid.unwrap_or(0)));
                }
            }
            "usercontribs" => {
                m.insert("revid".into(), json!(row.revid.unwrap_or(0)));
                m.insert("parentid".into(), json!(0));
            }
            _ => {
                m.insert("logid".into(), json!(row.logid.unwrap_or(0)));
            }
        }
    }
    if wants(props, "user") || wants(props, "userid") || module == "usercontribs" {
        if row.visibility & visibility::USER != 0 || row.actor.hidden {
            flag(&mut m, "userhidden", true);
        } else {
            if wants(props, "user") || module == "usercontribs" {
                m.insert(
                    "user".into(),
                    json!(user_name(row).unwrap_or_else(|| "(erased)".into())),
                );
            }
            if wants(props, "userid") || module == "usercontribs" {
                m.insert("userid".into(), json!(user_id(row.actor.key.as_deref())));
            }
            if row.actor.kind.as_deref() == Some("anonymous") {
                flag(&mut m, "anon", true);
            }
            if row.actor.kind.as_deref() == Some("temporary") {
                flag(&mut m, "temp", true);
            }
        }
    }
    if wants(props, "timestamp") {
        m.insert("timestamp".into(), json!(iso8601(row.time)));
    }
    if wants(props, "comment") || wants(props, "parsedcomment") {
        if row.visibility & visibility::COMMENT != 0 {
            flag(&mut m, "commenthidden", true);
        } else {
            let text = comment_text(row);
            if wants(props, "comment") {
                m.insert("comment".into(), json!(text));
            }
            if wants(props, "parsedcomment") {
                m.insert("parsedcomment".into(), json!(crate::html_escape(&text)));
            }
        }
    }
    if wants(props, "flags") {
        flag(&mut m, "bot", row.bot);
        flag(&mut m, "minor", row.minor);
        flag(&mut m, "new", row.new);
    }
    if wants(props, "sizes") || wants(props, "size") {
        if module == "recentchanges" {
            m.insert("oldlen".into(), json!(size_before(row)));
            m.insert("newlen".into(), json!(row.size.unwrap_or(0)));
        } else {
            m.insert("size".into(), json!(row.size.unwrap_or(0)));
        }
    }
    if wants(props, "sizediff") {
        m.insert("sizediff".into(), json!(row.delta.unwrap_or(0)));
    }
    if wants(props, "tags") {
        m.insert("tags".into(), json!(row.tags));
    }
    if wants(props, "patrolled") && module == "recentchanges" {
        flag(&mut m, "patrolled", row.patrolled);
        flag(&mut m, "unpatrolled", !row.patrolled);
    }
    if wants(props, "loginfo") && is_log || module == "logevents" {
        if wants(props, "type") || wants(props, "loginfo") {
            m.insert("logtype".into(), json!(row.log_type));
            m.insert("logaction".into(), json!(row.log_action));
            if module == "logevents" {
                m.insert("type".into(), json!(row.log_type));
                m.insert("action".into(), json!(row.log_action));
            }
        }
        if (wants(props, "details") || wants(props, "loginfo")) && !action_hidden {
            m.insert(
                "logparams".into(),
                row.params.clone().unwrap_or_else(|| json!({})),
            );
            if module == "logevents" {
                m.insert(
                    "params".into(),
                    row.params.clone().unwrap_or_else(|| json!({})),
                );
            }
        }
    }
    if row.kind == "erased" {
        flag(&mut m, "erased", true);
    }
    // Triplespace's own fields, under the `ts` prefix MediaWiki clients ignore.
    if wants(props, "source") {
        m.insert("source".into(), json!(row.source));
    }
    if wants(props, "job")
        && let Some(j) = row.job
    {
        m.insert("job".into(), json!(j));
    }
    if wants(props, "operator")
        && let Some(o) = &row.operator
    {
        m.insert("operator".into(), json!(o));
    }
    Value::Object(m)
}

fn size_before(row: &Row) -> i64 {
    match (row.size, row.delta) {
        (Some(s), Some(d)) => i64::from(s) - i64::from(d),
        _ => 0,
    }
}

/// The page title's URL under the tenant's base.
fn page_url(tenant: &Tenant, title: &str) -> String {
    let base = tenant.public_base.trim_end_matches('/');
    format!(
        "{base}/wiki/{}",
        percent_encoding::utf8_percent_encode(&title.replace(' ', "_"), crate::TITLE_SEGMENT)
    )
}

/// The Triplespace shape of a row (16 §3.1).
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn ts_row(tenant: &Tenant, row: &Row) -> Value {
    let hidden_user = row.visibility & visibility::USER != 0 || row.actor.hidden;
    let hidden_comment = row.visibility & visibility::COMMENT != 0;
    let mut m = Map::new();
    m.insert("kind".into(), json!(row.kind));
    m.insert("time".into(), json!(iso8601(row.time)));
    m.insert("source".into(), json!(row.source));
    m.insert(
        "record".into(),
        json!({"partition": row.partition, "offset": row.offset}),
    );
    if let Some(r) = row.revid {
        m.insert("revid".into(), json!(r));
    }
    if let Some(l) = row.logid {
        m.insert("logid".into(), json!(l));
    }
    let title = title_of(row);
    if let Some(kind) = row.target.kind {
        let mut t = Map::new();
        t.insert(
            "kind".into(),
            json!(match kind {
                target_kind::ENTITY => "entity",
                target_kind::PAGE => "page",
                target_kind::ACTOR => "actor",
                target_kind::JOB => "job",
                _ => "record",
            }),
        );
        if let Some(id) = &row.target.id {
            t.insert("id".into(), json!(id));
        }
        if let Some(m) = &row.target.member {
            t.insert("member".into(), json!(m));
        }
        if let Some(et) = &row.target.entity_type {
            t.insert("entity_type".into(), json!(et));
        }
        if let Some(p) = row.target.page_id {
            t.insert("page_id".into(), json!(p));
        }
        if let Some((ns, title)) = &title {
            t.insert("ns".into(), json!(ns));
            t.insert("title".into(), json!(title));
            t.insert("url".into(), json!(page_url(tenant, title)));
        }
        if let Some((text, lang)) = &row.target.label {
            t.insert("label".into(), json!({"value": text, "language": lang}));
        }
        m.insert("target".into(), Value::Object(t));
    }
    let mut a = Map::new();
    if hidden_user {
        a.insert("hidden".into(), json!(true));
    } else {
        if let Some(k) = &row.actor.key {
            a.insert("key".into(), json!(k));
        }
        if let Some(n) = &row.actor.name {
            a.insert("name".into(), json!(n));
        }
        if let Some(k) = &row.actor.kind {
            a.insert("kind".into(), json!(k));
        }
        if let Some(i) = &row.actor.iri {
            a.insert("iri".into(), json!(i));
        }
    }
    m.insert("actor".into(), Value::Object(a));
    if let Some(o) = &row.operator {
        m.insert("operator".into(), json!(o));
    }
    if let Some(r) = &row.requested_by {
        m.insert("requested_by".into(), json!(r));
    }
    if let Some(j) = row.job {
        m.insert("job".into(), json!(j));
    }
    if hidden_comment {
        m.insert("summary".into(), json!({"hidden": true}));
    } else {
        let mut s = Map::new();
        if let Some(op) = &row.summary_op {
            s.insert("op".into(), json!(op));
        }
        if let Some(args) = &row.summary_args {
            s.insert("args".into(), json!(args));
        }
        // A log row's operation is its event; a revision that is also a log event (a
        // thread `create`) keeps its own and carries the event under `log`.
        if let (Some(t), Some(a)) = (&row.log_type, &row.log_action)
            && (row.kind == "log" || row.kind == "job")
        {
            s.insert("op".into(), json!(format!("{t}/{a}")));
        }
        s.insert("text".into(), json!(comment_text(row)));
        m.insert("summary".into(), Value::Object(s));
        if let Some(c) = &row.comment {
            m.insert("comment".into(), json!(c));
        }
    }
    m.insert("tags".into(), json!(row.tags));
    m.insert(
        "flags".into(),
        json!({"minor": row.minor, "bot": row.bot, "new": row.new}),
    );
    m.insert("patrolled".into(), json!(row.patrolled));
    if let Some(s) = row.size {
        m.insert("size".into(), json!(s));
    }
    if let Some(d) = row.delta {
        m.insert("delta".into(), json!(d));
    }
    if let Some(c) = &row.changes {
        m.insert("changes".into(), c.clone());
    }
    if let (Some(t), Some(a)) = (&row.log_type, &row.log_action) {
        let mut l = Map::new();
        l.insert("type".into(), json!(t));
        l.insert("action".into(), json!(a));
        if let Some(p) = &row.params {
            l.insert("params".into(), p.clone());
        }
        m.insert("log".into(), Value::Object(l));
    }
    let mut vis = Vec::new();
    if row.visibility & visibility::CONTENT != 0 {
        vis.push("content");
    }
    if hidden_comment {
        vis.push("comment");
    }
    if hidden_user {
        vis.push("user");
    }
    if row.visibility & visibility::SUPPRESSED != 0 {
        vis.push("suppressed");
    }
    m.insert("visibility".into(), json!(vis));
    let mut links = Map::new();
    if let Some((_, title)) = &title {
        let base = tenant.public_base.trim_end_matches('/');
        let t = percent_encoding::utf8_percent_encode(&title.replace(' ', "_"), crate::QUERY)
            .to_string();
        links.insert(
            "history".into(),
            json!(format!("{base}/w/index.php?title={t}&action=history")),
        );
        if (row.kind == "edit" || row.kind == "sync")
            && let Some(r) = row.revid
        {
            links.insert(
                "diff".into(),
                json!(format!("{base}/w/index.php?title={t}&diff={r}")),
            );
        }
    }
    if let Some(j) = row.job {
        links.insert(
            "job".into(),
            json!(format!(
                "{}/wiki/Special:Jobs/{j}",
                tenant.public_base.trim_end_matches('/')
            )),
        );
    }
    m.insert("links".into(), Value::Object(links));
    Value::Object(m)
}

/// The `continue` entry for a page with more rows: `{prefix}continue` and the
/// `continue` sentinel, as MediaWiki's query modules emit them (compat §2.6).
pub fn continuation(body: &mut Map<String, Value>, prefix: &str, next: Option<feed::Cursor>) {
    if let Some(c) = next {
        body.insert(
            "continue".into(),
            json!({format!("{prefix}continue"): c.token(), "continue": "-||"}),
        );
    }
}

/// The MediaWiki form of a row's time, for tests and the `rcstart` echo.
#[must_use]
pub fn mw_time(row: &Row) -> String {
    mw_timestamp(row.time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use triplespace_projections::feed::{Actor, Target};

    fn row() -> Row {
        Row {
            time: 1_758_760_000_000_000,
            partition: 0x1002,
            offset: 9,
            kind: "edit".into(),
            source: "local".into(),
            revid: Some(901),
            logid: None,
            target: Target {
                kind: Some(target_kind::ENTITY),
                id: Some("Q6".into()),
                member: None,
                entity_type: Some("item".into()),
                page_id: Some(1),
                page: None,
                label: Some(("Six".into(), "en".into())),
            },
            actor: Actor {
                key: Some("librarybase:7".into()),
                name: Some("Alice".into()),
                kind: Some("registered".into()),
                raw: None,
                iri: Some("https://librarybase.org/actor/librarybase:7".into()),
                hidden: false,
            },
            operator: None,
            requested_by: None,
            job: None,
            summary_op: Some("add".into()),
            summary_args: Some("claims:P4 labels:fr".into()),
            comment: Some("test edit".into()),
            tags: vec!["bot".into()],
            minor: false,
            bot: true,
            new: false,
            size: Some(400),
            delta: Some(20),
            visibility: 0,
            patrolled: false,
            log_type: None,
            log_action: None,
            params: None,
            changes: None,
        }
    }

    fn tenant() -> Tenant {
        Tenant {
            slug: "librarybase".into(),
            issuer: "librarybase".into(),
            base: "https://librarybase.org".into(),
            public_base: "https://librarybase.org".into(),
            host: "librarybase.org".into(),
        }
    }

    #[test]
    fn titles_follow_the_namespace_registry() {
        let mut r = row();
        assert_eq!(title_of(&r), Some((120, "Item:Q6".into())));
        r.target.entity_type = Some("property".into());
        r.target.id = Some("P4".into());
        assert_eq!(title_of(&r), Some((122, "Property:P4".into())));
        r.target.id = Some("domain:wikipedia.org".into());
        assert_eq!(title_of(&r), Some((210, "Domain:wikipedia.org".into())));
        r.target.kind = Some(target_kind::PAGE);
        r.target.page = Some((4, "Home".into()));
        assert_eq!(title_of(&r), Some((4, "Project:Home".into())));
    }

    #[test]
    fn the_comment_carries_the_automatic_summary() {
        assert_eq!(
            comment_text(&row()),
            "/* add:claims:P4 labels:fr */ test edit"
        );
        let mut r = row();
        r.summary_op = None;
        r.summary_args = None;
        r.log_type = Some("job".into());
        r.log_action = Some("finish".into());
        r.comment = None;
        assert_eq!(comment_text(&r), "/* job/finish */");
    }

    #[test]
    fn the_triplespace_row_has_the_shape_of_16_3_1() {
        let v = ts_row(&tenant(), &row());
        assert_eq!(v["kind"], "edit");
        assert_eq!(v["time"], "2025-09-25T00:26:40Z");
        assert_eq!(v["record"]["offset"], 9);
        assert_eq!(v["target"]["title"], "Item:Q6");
        assert_eq!(v["target"]["url"], "https://librarybase.org/wiki/Item:Q6");
        assert_eq!(v["target"]["label"]["value"], "Six");
        assert_eq!(v["actor"]["name"], "Alice");
        assert_eq!(v["summary"]["op"], "add");
        assert_eq!(v["flags"]["bot"], true);
        assert_eq!(v["delta"], 20);
        assert_eq!(v["visibility"], json!([]));
        assert_eq!(
            v["links"]["diff"],
            "https://librarybase.org/w/index.php?title=Item%3AQ6&diff=901"
        );
        let mut r = row();
        r.actor.hidden = true;
        r.visibility = visibility::COMMENT;
        let v = ts_row(&tenant(), &r);
        assert_eq!(v["actor"], json!({"hidden": true}));
        assert_eq!(v["summary"], json!({"hidden": true}));
        assert!(v.get("comment").is_none());
        assert_eq!(v["visibility"], json!(["comment", "user"]));
    }

    #[test]
    fn user_ids_are_the_number_in_the_key() {
        assert_eq!(user_id(Some("librarybase:7")), 7);
        assert_eq!(user_id(Some("ip:127.0.0.1")), 0);
        assert_eq!(user_id(Some("actor:3")), 0);
        assert_eq!(user_id(None), 0);
    }

    #[test]
    fn a_surrogate_shows_as_the_source_showed_it() {
        let mut r = row();
        r.actor = Actor {
            key: Some("actor:1".into()),
            kind: Some("imported".into()),
            raw: Some("imported>Harej".into()),
            ..Actor::default()
        };
        assert_eq!(user_name(&r).as_deref(), Some("imported>Harej"));
        r.actor.kind = Some("federated".into());
        assert_eq!(user_name(&r).as_deref(), Some("actor:1"));
        r.actor.hidden = true;
        assert_eq!(user_name(&r), None);
    }
}
