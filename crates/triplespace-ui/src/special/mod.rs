//! Special pages (0047): those the site serves so far, by name.
//!
//! - [`changes`]: `Special:RecentChanges`, `Special:Contributions` and `Special:Log`, three
//!   feeds (0020 §4; 19 §3.9, §6.3).
//! - [`index`]: `Special:SpecialPages`, the list of the others.
//! - [`login`]: `Special:UserLogin` and `Special:UserLogout`.
//! - [`external`]: logging in through external issuers, and `Special:Account`'s Sign-in
//!   methods (0088 §5–§7).
//! - [`register`]: `Special:CreateAccount`, its alias `Special:RequestAccount`, and
//!   `Special:ConfirmAccounts` (0087 §5, §6).
//! - [`new_entity`]: `Special:NewItem` and `Special:NewProperty`, the forms that create
//!   an entity (0047 §5).
//! - [`search`]: `Special:Search`, the full results page, and its **Go to** rule
//!   (0010 §3; 0047 §9).
//! - [`version`]: `Special:Version` and its `Credits` and `License` subpages (0077).
//! - [`preferences`]: `Special:Preferences` and the Appearance menu's form (0085 §5–6).
//! - [`settings`]: `Special:WikiSettings`, `Special:InstanceSettings` and
//!   `Special:Tenancy` (0085 §4), with [`controls`], the form controls drawn from a type.
//! - [`userrights`]: `Special:UserRights`, an account's groups (09 §3.3).
//! - [`block`]: `Special:Block`, `Special:Unblock` and `Special:BlockList` (09 §3.3).
//! - [`undelete`]: `Special:Undelete`, the deletions and the way back (09 §6.2).
//! - [`movepage`]: `Special:MovePage`, the Move form (10 §2.5).
//! - [`protected`]: `Special:ProtectedPages`, the restrictions in force, the sets and the
//!   empty-group report (09 §4.5).
//! - [`notifications`]: `Special:Notifications`, the viewer's inbox (19 §6.2).
//! - [`watchlist`]: `Special:Watchlist` and `Special:EditWatchlist` (19 §6.2).
//! - [`blank`]: `Special:BlankPage`, the frame around nothing.
//! - [`info`]: `Special:NamespaceInfo`, `Special:ListGroupRights`, `Special:ListDatatypes`
//!   and `Special:MyLanguageFallbackChain`, read-only pages about the wiki.
//! - [`allmessages`]: `Special:AllMessages`, the interface messages and their overrides.
//! - [`compare`]: `Special:ComparePages`, any two pages or revisions.
//! - `Special:ProtectedTitles` is [`protected::titles`].
//! - [`set_terms`]: `Special:SetLabel`, `Special:SetDescription`, `Special:SetAliases` and
//!   `Special:SetLabelDescriptionAliases`, Wikibase's term forms.
//! - [`providers`]: `Special:Providers`, the data providers this instance knows.
//! - `Special:NewPages` is a fourth feed in [`changes`].
//! - [`allpages`]: `Special:AllPages` and `Special:PrefixIndex`.
//! - [`whatlinkshere`]: `Special:WhatLinksHere`, the Links here tab.
//! - [`reports`]: the report pages, `Special:WantedPages`, `Special:ShortPages`,
//!   `Special:ListProperties` and the rest, one page from `list=querypage` (0047 §4).
//! - [`redirects`]: MediaWiki's redirecting pages (`MyPage`, `EditPage`, `TalkPage`,
//!   `PermanentLink`, `Diff`, `Redirect`, `EntityPage` and the rest; 21 §6.1).
//!
//! A special page the site does not serve is a `404`, as MediaWiki answers for a special
//! page it does not have.

pub mod allmessages;
pub mod allpages;
pub mod blank;
pub mod block;
pub mod changes;
pub mod compare;
pub mod controls;
pub mod external;
pub mod index;
pub mod info;
pub mod login;
pub mod movepage;
pub mod new_entity;
pub mod notifications;
pub mod preferences;
pub mod protected;
pub mod providers;
pub mod redirects;
pub mod register;
pub mod reports;
pub mod search;
pub mod set_terms;
pub mod settings;
pub mod undelete;
pub mod userrights;
pub mod version;
pub mod watchlist;
pub mod whatlinkshere;

use std::collections::BTreeMap;

use axum::http::HeaderMap;
use axum::response::Response;

use crate::Site;
use crate::pages::{Params, Peer};

/// A special page's name from its title: `Special:Search/foo` → `Search`, with the
/// first letter upper-cased, as MediaWiki's titles have it.
#[must_use]
pub fn name_of(title: &str) -> Option<String> {
    let rest = title.strip_prefix("Special:")?;
    let name = rest.split('/').next().unwrap_or(rest).trim();
    let mut chars = name.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().chain(chars).collect())
}

/// Serves a special page, or `None` for one the site does not serve.
pub async fn serve(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    params: &Params,
) -> Option<Response> {
    let query = &params.map;
    let name = name_of(title)?;
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::serve(site, headers, peer, kind, title, params).await);
    }
    if let Some(which) = redirects::Which::of(&name) {
        return Some(redirects::serve(site, headers, peer, which, title, params).await);
    }
    if let Some(which) = info::Which::of(&name) {
        return Some(info::serve(site, headers, peer, which, params).await);
    }
    if let Some(which) = allpages::Which::of(&name) {
        return Some(allpages::serve(site, headers, peer, which, title, params).await);
    }
    if let Some(which) = set_terms::Which::of(&name) {
        return Some(set_terms::serve(site, headers, peer, which, title, params).await);
    }
    match name.as_str() {
        "BlankPage" => Some(blank::serve(site, headers, peer, params).await),
        "WhatLinksHere" | "Whatlinkshere" => {
            Some(whatlinkshere::serve(site, headers, peer, title, params).await)
        }
        "AllMessages" => Some(allmessages::serve(site, headers, peer, params).await),
        "ComparePages" => Some(compare::serve(site, headers, peer, params).await),
        "ProtectedTitles" => Some(protected::titles(site, headers, peer, params).await),
        "Providers" => Some(providers::serve(site, headers, peer, params).await),
        "NewPages" => Some(changes::new_pages(site, headers, peer, params).await),
        "Search" => Some(search::serve(site, headers, peer, query).await),
        "RecentChanges" => Some(changes::recent_changes(site, headers, peer, params).await),
        "Contributions" => Some(changes::contributions(site, headers, peer, title, params).await),
        "Log" => Some(changes::log(site, headers, peer, title, params).await),
        "SpecialPages" => Some(index::serve(site, headers, peer, query).await),
        "UserLogin" => Some(login::login(site, headers, peer, query).await),
        "UserLogout" => Some(login::logout(site, headers, peer, query).await),
        "Account" => Some(external::serve(site, headers, peer, query).await),
        "CreateAccount" | "RequestAccount" => {
            Some(register::serve(site, headers, peer, query).await)
        }
        "ConfirmAccounts" => Some(register::review(site, headers, peer, query).await),
        "Version" => Some(version::serve(site, headers, peer, title, query).await),
        "Preferences" => Some(preferences::serve(site, headers, peer, params).await),
        "WikiSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Wiki, params).await)
        }
        "InstanceSettings" => {
            Some(settings::serve(site, headers, peer, settings::Scope::Instance, params).await)
        }
        "Tenancy" => Some(settings::tenancy(site, headers, peer, query).await),
        "UserRights" => Some(userrights::serve(site, headers, peer, title, params).await),
        "ProtectedPages" => Some(protected::serve(site, headers, peer, params).await),
        "Block" => {
            Some(block::serve(site, headers, peer, block::Which::Block, title, params).await)
        }
        "Unblock" => {
            Some(block::serve(site, headers, peer, block::Which::Unblock, title, params).await)
        }
        "BlockList" => Some(block::list(site, headers, peer, params).await),
        "Undelete" => Some(undelete::serve(site, headers, peer, title, params).await),
        "MovePage" => Some(movepage::serve(site, headers, peer, title, params).await),
        "Notifications" => Some(notifications::serve(site, headers, peer, params).await),
        "Watchlist" => Some(watchlist::serve(site, headers, peer, params).await),
        "EditWatchlist" => Some(watchlist::edit(site, headers, peer, params).await),
        _ => match reports::Report::of(&name) {
            Some(report) => Some(reports::serve(site, headers, peer, report, params).await),
            None => None,
        },
    }
}

/// Takes a form posted to a special page, or `None` for a page that takes none.
pub async fn post(
    site: &Site,
    headers: &HeaderMap,
    peer: Peer,
    title: &str,
    query: &BTreeMap<String, String>,
    params: &Params,
) -> Option<Response> {
    let form = &params.map;
    let name = name_of(title)?;
    if let Some(kind) = new_entity::Kind::of(&name) {
        return Some(new_entity::post(site, headers, peer, kind, query, params).await);
    }
    if let Some(which) = set_terms::Which::of(&name) {
        return Some(set_terms::post(site, headers, peer, which, query, params).await);
    }
    match name.as_str() {
        "UserLogin" => Some(login::post_login(site, headers, peer, query, form).await),
        "UserLogout" => Some(login::post_logout(site, headers, peer, query, form).await),
        "Account" => Some(external::post(site, headers, peer, query, form).await),
        "CreateAccount" | "RequestAccount" => {
            Some(register::post(site, headers, peer, query, form).await)
        }
        "ConfirmAccounts" => Some(register::post_review(site, headers, peer, query, form).await),
        "Preferences" => Some(preferences::post(site, headers, peer, query, params).await),
        "WikiSettings" => {
            Some(settings::post(site, headers, peer, settings::Scope::Wiki, query, params).await)
        }
        "InstanceSettings" => Some(
            settings::post(
                site,
                headers,
                peer,
                settings::Scope::Instance,
                query,
                params,
            )
            .await,
        ),
        "UserRights" => Some(userrights::post(site, headers, peer, title, query, params).await),
        "ProtectedPages" => Some(protected::post(site, headers, peer, query, params).await),
        "Block" => Some(block::post(site, headers, peer, block::Which::Block, query, params).await),
        "Unblock" => {
            Some(block::post(site, headers, peer, block::Which::Unblock, query, params).await)
        }
        "MovePage" => Some(movepage::post(site, headers, peer, title, params).await),
        "Notifications" => Some(notifications::post(site, headers, peer, params).await),
        "Watchlist" => Some(watchlist::post(site, headers, peer, params).await),
        "EditWatchlist" => Some(watchlist::edit_post(site, headers, peer, params).await),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::name_of;

    #[test]
    fn names() {
        assert_eq!(name_of("Special:Search").as_deref(), Some("Search"));
        assert_eq!(name_of("Special:search/x").as_deref(), Some("Search"));
        assert_eq!(name_of("Special:").as_deref(), None);
        assert_eq!(name_of("Item:Q6").as_deref(), None);
    }
}
