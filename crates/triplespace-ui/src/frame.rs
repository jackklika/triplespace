//! The page frame (0010 §2): the global header (site name, search, the site's links, the
//! **New** menu, the account menu or **Log in**), the page header (identity line, title,
//! tabs), the page's body and the footer. Each part of the header shows only where its
//! feature is offered ([`Features`]).
//!
//! A signed-in viewer's header has the notifications **bell** (19 §6.2): a link to
//! `Special:Notifications` with the unread count, marked while something unread is newer
//! than their last look; and while a `talk` notification is unread, the **orange bar**
//! above the page's title says that they have a new message, until they read it.
//!
//! The frame is drawn by the skin's `frame.html` over the view model [`Model`]
//! (19 §5.10).

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::assets;
use crate::codex::SearchInput;
use crate::features::{Feature, Features};
use crate::i18n::Messages;
use crate::skin::Skin;
use crate::theme::{Sheet, Theme};

/// The characters a title keeps unencoded in a path or a query value, as MediaWiki's
/// `wfUrlencode` leaves them.
const TITLE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':')
    .remove(b'/')
    .remove(b'(')
    .remove(b')')
    .remove(b'!')
    .remove(b',');

/// A title as it appears in a URL: spaces as underscores, the rest percent-encoded.
#[must_use]
pub fn title_url(title: &str) -> String {
    utf8_percent_encode(&title.replace(' ', "_"), TITLE).to_string()
}

/// One of the page's tabs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Tab {
    /// The label.
    pub label: String,
    /// Where it goes.
    pub href: String,
    /// Whether it is the page being shown.
    pub current: bool,
}

/// The signed-in viewer, for the account menu.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Account {
    /// The name.
    pub name: String,
    /// The account menu's accessible name.
    pub label: String,
    /// The menu's links, as label and target; the name alone where there are none.
    pub items: Vec<(String, String)>,
}

/// What a page puts in the frame.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// The title, as shown.
    pub title: String,
    /// The title the login and logout links return to, as shown; none for a page that is
    /// not one (a 404).
    pub returnto: Option<String>,
    /// The title's language, where it differs from the page's (a label in a fallback
    /// language).
    pub title_lang: Option<String>,
    /// The document title, where it differs from the title (`Label (Q6)`).
    pub doc_title: Option<String>,
    /// The identity line, as HTML built from escaped parts.
    pub identity_html: Option<String>,
    /// What goes under the title (an entity's terms), as HTML built from escaped parts.
    pub subtitle_html: Option<String>,
    /// The tabs.
    pub tabs: Vec<Tab>,
    /// The overflow menu's actions, `(label, href)`: the less common ones, Protect… and
    /// Delete… among them (19 §1.3).
    pub actions: Vec<(String, String)>,
    /// The body, as HTML the page built from escaped parts.
    pub body: String,
    /// What the search box already holds.
    pub search: String,
    /// The watch menu, for a signed-in viewer ([`crate::watch`]).
    pub watch: Option<WatchControl>,
}

/// The viewer's watch of the page shown, and the token its forms post.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchControl {
    /// The title the forms post to.
    pub title: String,
    /// Whether the viewer watches it.
    pub watched: bool,
    /// When the watch lapses, ISO 8601.
    pub expires: Option<String>,
    /// Notify me.
    pub notify: bool,
    /// A `watch` token.
    pub token: String,
}

/// What the frame needs of the site.
#[derive(Debug, Clone, Copy)]
pub struct Chrome<'a> {
    /// The site's name.
    pub sitename: &'a str,
    /// The main page's title.
    pub mainpage: &'a str,
    /// What the site offers.
    pub features: &'a Features,
    /// A signed-in viewer's resolved preferences, which the Appearance menu and
    /// `<html>` draw on (0085 §6); `None` for anyone else.
    pub prefs: Option<&'a serde_json::Map<String, serde_json::Value>>,
    /// The open account requests, where the viewer reviews them (0087 §6).
    pub account_requests: Option<u64>,
    /// A signed-in viewer's unread notifications, where the site shows them (19 §6.2).
    pub notifications: Option<&'a triplespace_client::Counts>,
}

/// The bell (19 §6.2).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Bell {
    /// The count shown: empty for none, `99+` past 99.
    count: String,
    /// Its accessible name, with the count in words.
    label: String,
    /// Whether something unread is newer than the viewer's last look.
    unseen: bool,
}

/// The frame's view model (19 §5.10): everything the frame template draws, computed.
/// `*_html` and the pieces named as such are HTML built from escaped parts; the rest is
/// text the template escapes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Model {
    /// The page's language and direction, on `<html>`.
    pub lang: String,
    /// `ltr` or `rtl`.
    pub dir: String,
    /// The site's name.
    pub sitename: String,
    /// The title, as shown.
    pub title: String,
    /// The title's language, where it differs from the page's.
    pub title_lang: Option<String>,
    /// The document title.
    pub doc_title: String,
    /// The identity line, HTML.
    pub identity: Option<String>,
    /// Under the title, HTML.
    pub subtitle: Option<String>,
    /// The tabs.
    pub tabs: Vec<Tab>,
    /// The overflow menu's actions, label and target.
    pub actions: Vec<(String, String)>,
    /// The body, HTML.
    pub body: String,
    /// The search box, where the site offers search: the skin draws it with its
    /// `codex.search` macro.
    pub search: Option<crate::codex::SearchModel>,
    /// The signed-in viewer's account menu.
    pub account: Option<Account>,
    /// The main page's URL.
    pub mainpage_href: String,
    /// Whether a **Log in** link is offered.
    pub login: bool,
    /// The site's links, label and target.
    pub nav: Vec<(String, String)>,
    /// The **New** menu, label and target.
    pub new_menu: Vec<(String, String)>,
    /// Where login and logout return to, as a URL title.
    pub returnto: String,
    /// The stylesheets a page links, in order, before the theme's: the skin chain's.
    pub stylesheets: Vec<String>,
    /// The script.
    pub js: Option<String>,
    /// The blocking script in `<head>`.
    pub head_js: Option<String>,
    /// The palette's stylesheet.
    pub palette_href: Option<String>,
    /// The type set's stylesheet.
    pub typeset_href: Option<String>,
    /// The theme's data block, JSON.
    pub theme_data: String,
    /// The classes on `<html>`.
    pub html_class: String,
    /// The attributes on `<html>`, HTML.
    pub html_attrs: String,
    /// The Appearance panel, which `appearance.html`'s `panel` draws (group 9).
    pub appearance: crate::appearance::AppearancePanel,
    /// The Appearance data block, JSON.
    pub appearance_data: String,
    /// The bell.
    pub bell: Option<Bell>,
    /// The orange bar, HTML.
    pub orange_bar: Option<String>,
    /// The watch control, for a signed-in viewer where the site offers watching.
    pub watch: Option<WatchModel>,
}

/// The watch control as the skin draws it (`watch.html`): every label and URL computed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WatchModel {
    /// Whether the viewer watches the page.
    pub watched: bool,
    /// The button's label: **Watch** or **Watching**.
    pub label: String,
    /// The star's path: outlined, or filled for a watched page.
    pub icon_path: String,
    /// Until when the watch lasts, as a sentence, for a watched page that lapses.
    pub until: Option<String>,
    /// Where the watch form posts.
    pub watch_action: String,
    /// Where the unwatch form posts.
    pub unwatch_action: String,
    /// The `watch` token.
    pub token: String,
    /// The legend over the spans.
    pub legend: String,
    /// The spans offered: value, label and which is chosen.
    pub spans: Vec<WatchSpan>,
    /// Notify me.
    pub notify: bool,
    /// The notify checkbox's label.
    pub notify_label: String,
    /// **Watch** or **Save**.
    pub submit: String,
    /// **Unwatch**.
    pub unwatch_label: String,
}

/// One span of [`WatchModel`].
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WatchSpan {
    /// The radio's value, `infinite` or a duration.
    pub value: String,
    /// The label.
    pub label: String,
    /// Whether it is chosen.
    pub checked: bool,
}

/// The watch control (19 §6.2): one menu, as forms that work without JavaScript. Its
/// button is the star with **Watch**, or a filled star with **Watching**; it opens how
/// long to watch and *Notify me*, with **Watch** (or **Save** for a watched page) and,
/// for a watched page, **Unwatch**.
#[must_use]
pub fn watch_model(m: &Messages, w: &WatchControl) -> WatchModel {
    let action = |a: &str| format!("/w/index.php?title={}&action={a}", title_url(&w.title));
    let (label, submit, path) = if w.watched {
        (
            m.get("ts-watching"),
            m.get("ts-watch-save"),
            "M20 7h-7L10 0 7 7H0l5.46 5.47-1.64 7 6.18-3.7 6.18 3.73-1.63-7z",
        )
    } else {
        (
            m.get("ts-watch"),
            m.get("ts-watch"),
            "M20 7h-7L10 0 7 7H0l5.46 5.47-1.64 7 6.18-3.7 6.18 3.73-1.63-7zm-10 6.9-3.58 2.15.95-4.07-3.17-3.17h4.14L10 5.15l1.66 3.66h4.14l-3.17 3.17.95 4.07z",
        )
    };
    // A watch that lapses says until when: in the menu, and on the button's hover.
    let until = w
        .expires
        .as_ref()
        .filter(|_| w.watched)
        .map(|e| m.with("ts-watch-until", &[&crate::special::changes::date(m, e)]));
    WatchModel {
        watched: w.watched,
        label,
        icon_path: path.to_string(),
        until,
        watch_action: action("watch"),
        unwatch_action: action("unwatch"),
        token: w.token.clone(),
        legend: m.get("ts-watch-legend"),
        spans: crate::watch::SPANS
            .iter()
            .enumerate()
            .map(|(i, (span, key))| WatchSpan {
                value: (*span).to_string(),
                label: m.get(key),
                checked: i == 0 && (w.expires.is_none() || !w.watched),
            })
            .collect(),
        notify: w.notify,
        notify_label: m.get("ts-watch-notify"),
        submit,
        unwatch_label: m.get("ts-unwatch"),
    }
}

/// The bell and the orange bar for a viewer's counts.
fn notices(
    m: &Messages,
    counts: Option<&triplespace_client::Counts>,
    user_talk: Option<&str>,
) -> (Option<Bell>, Option<String>) {
    let Some(c) = counts else {
        return (None, None);
    };
    let n = c.unread();
    let count = match n {
        0 => String::new(),
        n if n > 99 => "99+".to_string(),
        n => n.to_string(),
    };
    let label = if n == 0 {
        m.get("ts-bell")
    } else {
        m.with("ts-bell-unread", &[&count])
    };
    let bell = Bell {
        count,
        label,
        unseen: c.unseen(),
    };
    let bar = (c.messages > 0).then(|| {
        let link = user_talk.map_or_else(String::new, |t| {
            format!(
                "<a href=\"/wiki/{}\">{}</a>",
                title_url(t),
                crate::html::esc(&m.get("ts-orange-bar-link"))
            )
        });
        crate::html::esc(&m.get("ts-orange-bar")).replace("$1", &link)
    });
    (Some(bell), bar)
}

/// The frame's view model for a page, with the stylesheets of the skin that draws it.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn model(
    m: &Messages,
    chrome: &Chrome,
    theme: &Theme,
    user: Option<&str>,
    page: &Page,
    skin: &Skin,
) -> Model {
    let f = chrome.features;
    let search = f.has(Feature::Search).then(|| {
        SearchInput {
            name: "search",
            value: &page.search,
            placeholder: &m.get("ts-search-placeholder"),
            label: &m.get("ts-search-label"),
            button: &m.get("ts-search-button"),
        }
        .model()
    });
    let returnto = page
        .returnto
        .as_deref()
        .map_or_else(|| title_url(chrome.mainpage), title_url);
    let account = user.map(|name| {
        let mut items = Vec::new();
        if f.has(Feature::Pages) {
            items.push((
                m.get("ts-account-userpage"),
                format!("/wiki/User:{}", title_url(name)),
            ));
        }
        if f.has(Feature::Watchlist) {
            items.push((
                m.get("ts-watchlist-title"),
                "/wiki/Special:Watchlist".to_string(),
            ));
        }
        if f.has(Feature::Account) {
            items.push((m.get("ts-account"), "/wiki/Special:Account".to_string()));
        }
        if f.has(Feature::Preferences) {
            items.push((
                m.get("ts-preferences"),
                "/wiki/Special:Preferences".to_string(),
            ));
        }
        if let (true, Some(n)) = (f.has(Feature::Register), chrome.account_requests) {
            items.push((
                m.with("ts-account-requests", &[&n.to_string()]),
                "/wiki/Special:ConfirmAccounts".to_string(),
            ));
        }
        if f.has(Feature::Login) {
            items.push((
                m.get("ts-logout"),
                format!("/wiki/Special:UserLogout?returnto={returnto}"),
            ));
        }
        Account {
            name: name.to_string(),
            label: m.with("ts-account-menu", &[name]),
            items,
        }
    });
    let mut nav = Vec::new();
    if f.has(Feature::RecentChanges) {
        nav.push((
            m.get("ts-nav-recentchanges"),
            "/wiki/Special:RecentChanges".to_string(),
        ));
    }
    if f.has(Feature::Jobs) {
        nav.push((m.get("ts-nav-jobs"), "/wiki/Special:Jobs".to_string()));
    }
    nav.push((
        m.get("ts-nav-specialpages"),
        "/wiki/Special:SpecialPages".to_string(),
    ));
    let mut new_menu = Vec::new();
    if f.has(Feature::Create) {
        new_menu.push((m.get("ts-new-item"), "/wiki/Special:NewItem".to_string()));
        new_menu.push((
            m.get("ts-new-property"),
            "/wiki/Special:NewProperty".to_string(),
        ));
    }
    if f.has(Feature::Pages) {
        new_menu.push((m.get("ts-new-page"), "/w/index.php?action=edit".to_string()));
    }
    // A retired palette among the reader's choices reads as what replaced it (0094 §4).
    let resolved = crate::appearance::resolve(chrome.prefs);
    let prefs = resolved.as_ref().or(chrome.prefs);
    let (html_class, html_attrs) = crate::appearance::html_attributes(prefs);
    let appearance = crate::appearance::panel_model(m, theme, prefs, &returnto.replace('_', " "));
    let appearance_data = crate::appearance::data(theme, prefs, None);
    // A signed-in reader's palette and type set are the server's to choose; anyone
    // else is served the wiki's, which the head script swaps for their own (0086 §5).
    let pref = |key: &str| {
        prefs
            .and_then(|p| p.get(key))
            .and_then(serde_json::Value::as_str)
    };
    let palette = theme.chosen(Sheet::Palette, pref("ui.palette"));
    let typeset = theme.chosen(Sheet::Typeset, pref("ui.typeset"));
    let user_talk = user.map(|u| format!("User talk:{u}"));
    let (bell, orange_bar) = notices(m, chrome.notifications, user_talk.as_deref());
    // The orange bar is not drawn on the page it points to.
    let orange_bar = orange_bar.filter(|_| user_talk.as_deref() != Some(page.title.as_str()));
    let entries = assets::entries();
    let site_css = if m.dir() == "rtl" {
        entries.css_rtl.as_deref()
    } else {
        entries.css.as_deref()
    };
    // The stylesheets, in the skin chain's order: `@site` is the built site stylesheet;
    // a file is served from the skin.
    let stylesheets = skin
        .stylesheets()
        .iter()
        .filter_map(|(owner, entry)| {
            if entry == "@site" {
                site_css.map(str::to_string)
            } else {
                skin.asset_url(entry).or_else(|| {
                    eprintln!(
                        "triplespace-ui: the skin `{owner}` names a stylesheet it lacks: {entry}"
                    );
                    None
                })
            }
        })
        .collect();
    Model {
        lang: m.lang().to_string(),
        dir: m.dir().to_string(),
        sitename: chrome.sitename.to_string(),
        title: page.title.clone(),
        title_lang: page.title_lang.clone(),
        doc_title: page.doc_title.clone().unwrap_or_else(|| page.title.clone()),
        identity: page.identity_html.clone(),
        subtitle: page.subtitle_html.clone(),
        tabs: page.tabs.clone(),
        actions: page.actions.clone(),
        body: page.body.clone(),
        search,
        account,
        mainpage_href: format!("/wiki/{}", title_url(chrome.mainpage)),
        login: f.has(Feature::Login),
        nav,
        new_menu,
        returnto,
        stylesheets,
        js: entries.js.clone(),
        head_js: entries.head_js.clone(),
        palette_href: theme.href(Sheet::Palette, &palette),
        typeset_href: theme.href(Sheet::Typeset, &typeset),
        theme_data: theme.data(),
        html_class,
        html_attrs,
        appearance,
        appearance_data,
        bell: bell.filter(|_| user.is_some()),
        orange_bar: orange_bar.filter(|_| user.is_some()),
        watch: page
            .watch
            .as_ref()
            .filter(|_| user.is_some())
            .map(|w| watch_model(m, w)),
    }
}

/// Renders a page in the frame: the skin's `frame.html` over the page's model and `msg`.
///
/// # Errors
///
/// When the skin's template fails.
pub fn render(
    skin: &Skin,
    m: &Messages,
    chrome: &Chrome,
    theme: &Theme,
    user: Option<&str>,
    page: &Page,
) -> Result<String, minijinja::Error> {
    skin.render("frame.html", m, model(m, chrome, theme, user, page, skin))
}

/// The **Restricted** pill's tooltip: the groups a reader must be in, and the sets the
/// target is in.
#[must_use]
pub fn restricted_tip(m: &Messages, groups: &[String], sets: &[String]) -> String {
    let mut tip = m.with("ts-restricted-groups", &[&groups.join(", ")]);
    if !sets.is_empty() {
        tip.push(' ');
        tip.push_str(&m.with("ts-restricted-sets", &[&sets.join(", ")]));
    }
    tip
}

/// The **Restricted** pill of 19 §1.3: a lock, the groups a reader must be in (named
/// in its tooltip), and the sets the target is in, for a page or entity whose visibility
/// is not public (0056 §13).
#[must_use]
pub fn restricted_pill(m: &Messages, groups: &[String], sets: &[String]) -> String {
    use crate::html::esc;
    let tip = restricted_tip(m, groups, sets);
    format!(
        " <span aria-hidden=\"true\">·</span> <span class=\"ts-chip ts-chip--restricted\" title=\"{}\"><span class=\"ts-chip__lock\" aria-hidden=\"true\">🔒</span> {}<span class=\"ts-visually-hidden\">: {}</span></span>",
        esc(&tip),
        esc(&m.get("ts-restricted")),
        esc(&tip),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page {
            title: "Main Page".into(),
            returnto: Some("Main Page".into()),
            tabs: vec![Tab {
                label: "Read".into(),
                href: "/wiki/Main_Page".into(),
                current: true,
            }],
            body: "<p>x</p>".into(),
            ..Page::default()
        }
    }

    fn chrome(features: &Features) -> Chrome<'_> {
        Chrome {
            sitename: "Librarybase",
            mainpage: "Project:Home",
            features,
            prefs: None,
            account_requests: None,
            notifications: None,
        }
    }

    fn all() -> Features {
        Features::all_built()
    }

    fn worksheet() -> std::sync::Arc<Skin> {
        crate::skin::Skins::load(None)
            .get("worksheet")
            .expect("worksheet")
            .clone()
    }

    #[test]
    fn anonymous_frame() {
        let m = Messages::for_language("en");
        let f = all();
        let html = render(
            &worksheet(),
            &m,
            &chrome(&f),
            &Theme::shipped(),
            None,
            &page(),
        )
        .unwrap();
        assert!(html.starts_with(
            "<!DOCTYPE html>\n<html class=\"client-nojs skin-theme-clientpref-os\" lang=\"en\" dir=\"ltr\">"
        ));
        assert!(html.contains("class=\"ts-appearance__nojs\""));
        assert!(!html.contains("data-appearance"));
        assert!(html.contains("<title>Main Page – Librarybase</title>"));
        assert!(html.contains(">Librarybase</a>"));
        assert!(html.contains("name=\"search\""));
        assert!(html.contains("href=\"/wiki/Project:Home\">Librarybase</a>"));
        assert!(html.contains("aria-current=\"page\">Read</a>"));
        assert!(html.contains(&Theme::shipped().href(Sheet::Palette, "parchment").unwrap()));
        assert!(html.contains("<p>x</p>"));
    }

    #[test]
    fn account_menu_escapes_the_name() {
        let m = Messages::for_language("en");
        let f = all();
        let html = render(
            &worksheet(),
            &m,
            &chrome(&f),
            &Theme::shipped(),
            Some("A <b> C"),
            &page(),
        )
        .unwrap();
        assert!(html.contains("A &lt;b&gt; C"));
        assert!(!html.contains("Special:UserLogin"));
    }

    #[test]
    fn the_bell_and_the_orange_bar() {
        let m = Messages::for_language("en");
        let f = all();
        let mut counts = triplespace_client::Counts {
            alerts: 2,
            messages: 1,
            newest_unread: Some("2026-10-25T10:00:00Z".into()),
            seen: Some("2026-10-24T10:00:00Z".into()),
        };
        let mut c = chrome(&f);
        c.notifications = Some(&counts);
        let html = render(
            &worksheet(),
            &m,
            &c,
            &Theme::shipped(),
            Some("Alice"),
            &page(),
        )
        .unwrap();
        assert!(html.contains("ts-bell ts-bell--unseen"), "{html}");
        assert!(html.contains("aria-label=\"Notifications, 3 unread\""));
        assert!(html.contains("<span class=\"ts-bell__count\" aria-hidden=\"true\">3</span>"));
        assert!(html.contains(
            "You have a new message on <a href=\"/wiki/User_talk:Alice\">your talk page</a>."
        ));
        // Not on the talk page itself, and gone once the message is read.
        let mut on_talk = page();
        on_talk.title = "User talk:Alice".into();
        let html = render(
            &worksheet(),
            &m,
            &c,
            &Theme::shipped(),
            Some("Alice"),
            &on_talk,
        )
        .unwrap();
        assert!(!html.contains("ts-orange-bar"));
        counts.messages = 0;
        counts.alerts = 0;
        counts.seen.clone_from(&counts.newest_unread);
        let mut c = chrome(&f);
        c.notifications = Some(&counts);
        let html = render(
            &worksheet(),
            &m,
            &c,
            &Theme::shipped(),
            Some("Alice"),
            &page(),
        )
        .unwrap();
        assert!(!html.contains("ts-orange-bar"));
        assert!(!html.contains("ts-bell--unseen"));
        assert!(!html.contains("ts-bell__count"));
        assert!(html.contains("aria-label=\"Notifications\""));
        // Nobody signed in: no bell.
        let html = render(&worksheet(), &m, &c, &Theme::shipped(), None, &page()).unwrap();
        assert!(!html.contains("ts-bell"));
    }

    #[test]
    fn features_not_offered_are_not_shown() {
        let m = Messages::for_language("en");
        let none = Features::default();
        let html = render(
            &worksheet(),
            &m,
            &chrome(&none),
            &Theme::shipped(),
            None,
            &page(),
        )
        .unwrap();
        assert!(
            !html.contains("name=\"search\""),
            "no search box without search"
        );
        for absent in [
            "RecentChanges",
            "Special:Jobs",
            "Special:NewItem",
            "action=edit",
            "Special:UserLogin",
        ] {
            assert!(!html.contains(absent), "{absent}");
        }
        assert!(html.contains("Special:SpecialPages"));
        let signed_in = render(
            &worksheet(),
            &m,
            &chrome(&none),
            &Theme::shipped(),
            Some("Alice"),
            &page(),
        )
        .unwrap();
        assert!(signed_in.contains("Alice"));
        assert!(!signed_in.contains("Special:Account"));
        assert!(!signed_in.contains("User:Alice"));
    }

    #[test]
    fn titles_in_urls() {
        assert_eq!(title_url("Main Page"), "Main_Page");
        assert_eq!(title_url("Item:Q6"), "Item:Q6");
        assert_eq!(title_url("A&B?c"), "A%26B%3Fc");
        assert_eq!(title_url("Ünïcode"), "%C3%9Cn%C3%AFcode");
    }
}
