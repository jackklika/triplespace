//! Protection, group membership, blocks and the tenant's visibility (09 §3–§4; 0016 §7;
//! 0056 §13; 19 §6.3): what the Protect form, `Special:UserRights`,
//! `Special:ProtectedPages`, `Special:Block` and the Visibility tab read and write.
//!
//! Writes go through the Action API's modules (`protect`, `userrights`, `block`,
//! `unblock`) and the REST `/acl` routes, with the viewer's own credentials and token, so
//! every refusal is the API's own, returned as [`Saved::Refused`] with its code for the
//! page to explain; only a failure to reach or read the API is a [`ClientError`].

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::api::{CacheInfo, Fetched, check};
use crate::settings::{Refusal, Saved, saved};
use crate::{Client, ClientError, Incoming};

const REST: &str = "/w/rest.php/triplespace/v0";

/// One restriction in force on a page or entity, as `inprop=protection` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Restriction {
    /// `edit`, `move`, `read`, `create`, …
    pub kind: String,
    /// The group.
    pub level: String,
    /// `infinity`, or an ISO 8601 time.
    pub expiry: String,
    /// The enclosing target that set it (`acl:namespace:4`), where not the page's own.
    pub source: Option<String>,
    /// For the page's own `read`: whether it took the talk page (0056 A24).
    pub talk: bool,
}

impl Restriction {
    pub(crate) fn of(v: &Value) -> Self {
        Self {
            kind: v["type"].as_str().unwrap_or_default().to_string(),
            level: v["level"].as_str().unwrap_or_default().to_string(),
            expiry: v["expiry"].as_str().unwrap_or("infinity").to_string(),
            source: v["source"].as_str().map(str::to_string),
            talk: v["ts-talk"].as_bool().unwrap_or(false),
        }
    }
}

/// A page's or entity's protection, for the Protect form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Protection {
    /// The title, as the API normalized it.
    pub title: String,
    /// Whether the page exists; a missing title can still be create-protected.
    pub exists: bool,
    /// The restrictions in force, the enclosing targets' among them.
    pub restrictions: Vec<Restriction>,
    /// The actions that may be restricted here (`restrictiontypes`).
    pub types: Vec<String>,
}

/// What the Protect form submits: one `action=protect` request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectForm<'a> {
    /// The title.
    pub title: &'a str,
    /// `(action, group)` pairs; `all` lifts one.
    pub protections: Vec<(String, String)>,
    /// The expiry, `infinite` or a time, for every protection.
    pub expiry: &'a str,
    /// The reason.
    pub reason: &'a str,
    /// Whether a `read` restriction takes the talk page (`tstalk`).
    pub talk: bool,
    /// The CSRF token.
    pub token: &'a str,
}

/// A group, as `siprop=usergroups` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupInfo {
    /// The name.
    pub name: String,
    /// Its permissions.
    pub rights: Vec<String>,
    /// Whether the API marks it implicit (`*`, `user`, …).
    pub implicit: bool,
}

/// An account, as `list=users` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UserGroups {
    /// The name.
    pub name: String,
    /// The user ID.
    pub id: u64,
    /// `(group, expiry)` for each explicit membership.
    pub memberships: Vec<(String, String)>,
    /// The expiry of a block in force, where there is one.
    pub block_expiry: Option<String>,
}

/// What `Special:UserRights` submits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RightsForm<'a> {
    /// The account.
    pub user: &'a str,
    /// Groups to add.
    pub add: Vec<String>,
    /// Groups to remove.
    pub remove: Vec<String>,
    /// The expiry of every addition, `infinite` or a time.
    pub expiry: &'a str,
    /// The reason.
    pub reason: &'a str,
    /// The `userrights` token.
    pub token: &'a str,
}

/// What `Special:MovePage` submits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveForm<'a> {
    /// The page, by title.
    pub from: &'a str,
    /// The new title.
    pub to: &'a str,
    /// The reason.
    pub reason: &'a str,
    /// Leave no redirect (needs `suppressredirect`).
    pub noredirect: bool,
    /// Carry the subpages along (needs `move-subpages`).
    pub subpages: bool,
}

/// What `Special:Block` submits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "the form's four checkboxes")]
pub struct BlockForm<'a> {
    /// The account.
    pub user: &'a str,
    /// `infinite` or a time.
    pub expiry: &'a str,
    /// The reason.
    pub reason: &'a str,
    /// Also remove `createaccount`.
    pub nocreate: bool,
    /// Block everything but `read`.
    pub all_but_read: bool,
    /// Hide the username (needs `hideuser`; indefinite blocks only).
    pub hidename: bool,
    /// Replace a block in force.
    pub reblock: bool,
    /// The CSRF token.
    pub token: &'a str,
}

/// One row of `list=protectedpages` or a reservation of `list=protectedtitles`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProtectedRow {
    /// The title.
    pub title: String,
    /// The namespace.
    pub ns: i64,
    /// `edit`, `read`, `create`, …
    pub kind: String,
    /// The group.
    pub level: String,
    /// `infinity` or a time.
    pub expiry: String,
}

/// The rows of `list=protectedpages` or `list=protectedtitles`.
fn protected_rows(v: &Value, key: &str) -> Vec<ProtectedRow> {
    v["query"][key]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|p| ProtectedRow {
                    title: p["title"].as_str().unwrap_or_default().to_string(),
                    ns: p["ns"].as_i64().unwrap_or(0),
                    kind: p["type"].as_str().unwrap_or("create").to_string(),
                    level: p["level"].as_str().unwrap_or_default().to_string(),
                    expiry: p["expiry"].as_str().unwrap_or("infinity").to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A set, as `list=protectedsets` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetInfo {
    /// The set's ID.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// `(kind, level, expiry)` per restriction.
    pub protection: Vec<(String, String, String)>,
    /// The members the viewer may read: `(page_id, ns, title)`.
    pub members: Vec<(u64, i64, String)>,
}

/// A group a confidential restriction names that has no current member (09 §4.5).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EmptyGroup {
    /// The group.
    pub group: String,
    /// How many restrictions in force name it.
    pub restrictions: u64,
}

/// A deletion, as the delete log reports it for a title.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Deletion {
    /// Who deleted it; `None` where the name is hidden.
    pub by: Option<String>,
    /// When, ISO 8601.
    pub time: String,
    /// The reason.
    pub reason: String,
    /// The log ID.
    pub logid: u64,
    /// Whether the talk page went with it.
    pub talk: bool,
}

/// The latest deleted revision of a title, for a member of its deletion group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedRevision {
    /// The revision ID.
    pub revid: u64,
    /// Its author; `None` where hidden.
    pub user: Option<String>,
    /// When, ISO 8601.
    pub time: String,
    /// Its summary.
    pub comment: String,
    /// Its content, where the viewer may read it.
    pub content: Option<String>,
}

/// A block in force, as `list=blocks` reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlockRow {
    /// The block's log ID.
    pub id: Option<u64>,
    /// The blocked account; `None` where its name is hidden from the viewer.
    pub user: Option<String>,
    /// Whether the name is hidden at all (shown to the suppression group).
    pub hidden: bool,
    /// Who blocked.
    pub by: Option<String>,
    /// When, ISO 8601.
    pub timestamp: String,
    /// `infinite` or a time.
    pub expiry: String,
    /// The reason.
    pub reason: String,
    /// What it removes: `all-but-read`, or the permissions joined.
    pub removes: String,
}

/// The tenant's own ACL, as `GET /acl/tenant/{slug}` reports it: the `read` group, if
/// restricted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TenantVisibility {
    /// The group the tenant is restricted to; `None` for a public tenant.
    pub read_group: Option<String>,
}

/// An Action API write's answer: the body, or the API's refusal as one [`Refusal`].
fn action_result(v: Value) -> Saved<Value> {
    if let Some(e) = v.get("error") {
        return Saved::Refused(vec![Refusal {
            code: e["code"].as_str().unwrap_or("unknown").to_string(),
            key: None,
            message: e["info"].as_str().unwrap_or_default().to_string(),
        }]);
    }
    Saved::Ok(v)
}

fn q(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// A multi-value parameter, `|`-joined.
fn joined(list: &[String]) -> String {
    list.join("|")
}

impl Client {
    /// An Action API write, its refusal read from the body.
    pub(crate) async fn action(
        &self,
        incoming: &Incoming,
        fields: &[(&str, &str)],
    ) -> Result<Saved<Value>, ClientError> {
        let mut all: Vec<(&str, &str)> = vec![("format", "json"), ("formatversion", "2")];
        all.extend_from_slice(fields);
        let r = self.post_form(incoming, "/w/api.php", &all).await?;
        if r.status != StatusCode::OK {
            return Err(ClientError::Unexpected(format!("HTTP {}", r.status)));
        }
        let v: Value = serde_json::from_slice(&r.body)
            .map_err(|e| ClientError::Unexpected(format!("not JSON: {e}")))?;
        Ok(action_result(v))
    }

    /// `prop=info&inprop=protection` of a title: what the Protect form shows.
    pub async fn protection(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Protection>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=info&inprop=protection&titles={}&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let page = v["query"]["pages"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        let value = Protection {
            title: page["title"].as_str().unwrap_or(title).to_string(),
            exists: page.get("missing").is_none() && page.get("invalid").is_none(),
            restrictions: page["protection"]
                .as_array()
                .map(|a| a.iter().map(Restriction::of).collect())
                .unwrap_or_default(),
            types: page["restrictiontypes"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=protect`.
    pub async fn protect(
        &self,
        incoming: &Incoming,
        form: &ProtectForm<'_>,
    ) -> Result<Saved<Value>, ClientError> {
        let protections: Vec<String> = form
            .protections
            .iter()
            .map(|(a, g)| format!("{a}={g}"))
            .collect();
        let protections = joined(&protections);
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "protect"),
            ("title", form.title),
            ("protections", &protections),
            ("expiry", form.expiry),
            ("reason", form.reason),
            ("token", form.token),
        ];
        if form.talk {
            fields.push(("tstalk", "1"));
        }
        self.action(incoming, &fields).await
    }

    /// `siprop=usergroups`: the tenant's groups with their rights.
    pub async fn groups(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Vec<GroupInfo>>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=siteinfo&siprop=usergroups&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["usergroups"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|g| GroupInfo {
                        name: g["name"].as_str().unwrap_or_default().to_string(),
                        rights: g["rights"]
                            .as_array()
                            .map(|r| {
                                r.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        implicit: g.get("implicit").is_some(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=users&usprop=groupmemberships|blockinfo` of one name; `None` for an
    /// account that does not exist, or whose name is hidden from the viewer.
    pub async fn user_groups(
        &self,
        incoming: &Incoming,
        name: &str,
    ) -> Result<Fetched<Option<UserGroups>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&list=users&ususers={}&usprop=groupmemberships%7Cblockinfo&format=json&formatversion=2",
                    q(name)
                ),
            )
            .await?;
        let v = check(&r)?;
        let u = v["query"]["users"]
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .unwrap_or(Value::Null);
        let value = if u.get("missing").is_some() || u.is_null() {
            None
        } else {
            Some(UserGroups {
                name: u["name"].as_str().unwrap_or(name).to_string(),
                id: u["userid"].as_u64().unwrap_or(0),
                memberships: u["groupmemberships"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|m| {
                                (
                                    m["group"].as_str().unwrap_or_default().to_string(),
                                    m["expiry"].as_str().unwrap_or("infinity").to_string(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                block_expiry: u["blockexpiry"].as_str().map(str::to_string),
            })
        };
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `meta=userinfo&uiprop=changeablegroups`: which groups the viewer may add to or
    /// remove from, for others and for itself: `(add, remove, add-self, remove-self)`.
    pub async fn changeable_groups(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<[Vec<String>; 4]>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&meta=userinfo&uiprop=changeablegroups&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let c = &v["query"]["userinfo"]["changeablegroups"];
        let list = |k: &str| -> Vec<String> {
            c[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(Fetched {
            value: [
                list("add"),
                list("remove"),
                list("add-self"),
                list("remove-self"),
            ],
            cache: CacheInfo::of(&r),
        })
    }

    /// `action=userrights`.
    pub async fn userrights(
        &self,
        incoming: &Incoming,
        form: &RightsForm<'_>,
    ) -> Result<Saved<Value>, ClientError> {
        let add = joined(&form.add);
        let remove = joined(&form.remove);
        let fields: Vec<(&str, &str)> = vec![
            ("action", "userrights"),
            ("user", form.user),
            ("add", &add),
            ("remove", &remove),
            ("expiry", form.expiry),
            ("reason", form.reason),
            ("token", form.token),
        ];
        self.action(incoming, &fields).await
    }

    /// `action=block`.
    pub async fn block(
        &self,
        incoming: &Incoming,
        form: &BlockForm<'_>,
    ) -> Result<Saved<Value>, ClientError> {
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "block"),
            ("user", form.user),
            ("expiry", form.expiry),
            ("reason", form.reason),
            ("token", form.token),
        ];
        for (flag, on) in [
            ("nocreate", form.nocreate),
            ("tsallbutread", form.all_but_read),
            ("hidename", form.hidename),
            ("reblock", form.reblock),
        ] {
            if on {
                fields.push((flag, "1"));
            }
        }
        self.action(incoming, &fields).await
    }

    /// `action=unblock`.
    pub async fn unblock(
        &self,
        incoming: &Incoming,
        user: &str,
        reason: &str,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        self.action(
            incoming,
            &[
                ("action", "unblock"),
                ("user", user),
                ("reason", reason),
                ("token", token),
            ],
        )
        .await
    }

    /// `list=protectedpages` (every type, `read` included) and `list=protectedtitles`,
    /// as `Special:ProtectedPages` lists them; `(pages, titles)`, up to 500 each.
    pub async fn protected_pages(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<(Vec<ProtectedRow>, Vec<ProtectedRow>)>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&list=protectedpages%7Cprotectedtitles&prtype=edit%7Cmove%7Cread%7Ccreate&prlimit=500&ptlimit=500&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        Ok(Fetched {
            value: (
                protected_rows(&v, "protectedpages"),
                protected_rows(&v, "protectedtitles"),
            ),
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=protectedtitles`: the reserved titles (09 §4.8), up to 500, as
    /// `Special:ProtectedTitles` lists them.
    ///
    /// # Errors
    ///
    /// The API's refusal, or the API away or unreadable.
    pub async fn protected_titles(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Vec<ProtectedRow>>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&list=protectedtitles&ptlimit=500&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        Ok(Fetched {
            value: protected_rows(&v, "protectedtitles"),
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=protectedsets`: the sets the viewer may read.
    pub async fn protected_sets(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Vec<SetInfo>>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&list=protectedsets&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["protectedsets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| SetInfo {
                        id: s["id"].as_u64().unwrap_or(0),
                        name: s["name"].as_str().unwrap_or_default().to_string(),
                        protection: s["protection"]
                            .as_array()
                            .map(|p| {
                                p.iter()
                                    .map(|x| {
                                        (
                                            x["type"].as_str().unwrap_or_default().to_string(),
                                            x["level"].as_str().unwrap_or_default().to_string(),
                                            x["expiry"].as_str().unwrap_or("infinity").to_string(),
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                        members: s["members"]
                            .as_array()
                            .map(|m| {
                                m.iter()
                                    .map(|x| {
                                        (
                                            x["pageid"].as_u64().unwrap_or(0),
                                            x["ns"].as_i64().unwrap_or(0),
                                            x["title"].as_str().unwrap_or_default().to_string(),
                                        )
                                    })
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /acl/empty-groups`, for a holder of `userrights`; `None` where the viewer
    /// is not one.
    pub async fn empty_groups(
        &self,
        incoming: &Incoming,
    ) -> Result<Option<Vec<EmptyGroup>>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/acl/empty-groups"))
            .await?;
        match saved::<Value>(&r, "the empty-group report")? {
            Saved::Ok(v) => Ok(Some(
                v["groups"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|g| EmptyGroup {
                                group: g["group"].as_str().unwrap_or_default().to_string(),
                                restrictions: g["restrictions"].as_u64().unwrap_or(0),
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            )),
            Saved::Refused(_) => Ok(None),
        }
    }

    /// `POST /acl/set`, `POST /acl/set/{id}/members`, `DELETE /acl/set/{id}/members/…`
    /// and `DELETE /acl/set/{id}`, as the Sets tab's forms send them.
    pub async fn set_write(
        &self,
        incoming: &Incoming,
        method: Method,
        path: &str,
        body: &Value,
    ) -> Result<Saved<Value>, ClientError> {
        let r = self
            .send_json(incoming, method, &format!("{REST}/acl/set{path}"), body)
            .await?;
        saved(&r, "the set")
    }

    /// `GET /acl/tenant/{slug}`: whether the tenant is restricted, and to which group.
    pub async fn tenant_visibility(
        &self,
        incoming: &Incoming,
        slug: &str,
    ) -> Result<Fetched<TenantVisibility>, ClientError> {
        let r = self
            .get(incoming, &format!("{REST}/acl/tenant/{}", q(slug)))
            .await?;
        let v = match saved::<Value>(&r, "the tenant's visibility")? {
            Saved::Ok(v) => v,
            Saved::Refused(e) => {
                return Err(ClientError::Api {
                    code: e.first().map(|x| x.code.clone()).unwrap_or_default(),
                    info: e.first().map(|x| x.message.clone()).unwrap_or_default(),
                });
            }
        };
        let read_group = v["acl"]["restrictions"]["read"]["group"]
            .as_str()
            .map(str::to_string);
        Ok(Fetched {
            value: TenantVisibility { read_group },
            cache: CacheInfo::of(&r),
        })
    }

    /// `PUT /acl/tenant/{slug}` with a `read` group, or `DELETE` to make it public.
    pub async fn set_tenant_visibility(
        &self,
        incoming: &Incoming,
        slug: &str,
        read_group: Option<&str>,
        reason: &str,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let path = format!("{REST}/acl/tenant/{}", q(slug));
        let r = match read_group {
            Some(g) => {
                self.send_json(
                    incoming,
                    Method::PUT,
                    &path,
                    &json!({"restrictions": {"read": {"group": g}}, "reason": reason, "token": token}),
                )
                .await?
            }
            None => {
                self.send_json(
                    incoming,
                    Method::DELETE,
                    &path,
                    &json!({"reason": reason, "token": token}),
                )
                .await?
            }
        };
        saved(&r, "the tenant's visibility")
    }
}

impl Client {
    /// `action=delete`, with `deletetalk` where asked.
    pub async fn delete_page(
        &self,
        incoming: &Incoming,
        title: &str,
        reason: &str,
        talk: bool,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "delete"),
            ("title", title),
            ("reason", reason),
            ("token", token),
        ];
        if talk {
            fields.push(("deletetalk", "1"));
        }
        self.action(incoming, &fields).await
    }

    /// `action=move` (10 §2.5).
    pub async fn move_page(
        &self,
        incoming: &Incoming,
        form: &MoveForm<'_>,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        let mut fields: Vec<(&str, &str)> = vec![
            ("action", "move"),
            ("from", form.from),
            ("to", form.to),
            ("reason", form.reason),
            ("token", token),
        ];
        if form.noredirect {
            fields.push(("noredirect", "1"));
        }
        if form.subpages {
            fields.push(("movesubpages", "1"));
        }
        self.action(incoming, &fields).await
    }

    /// `action=undelete`.
    pub async fn undelete_page(
        &self,
        incoming: &Incoming,
        title: &str,
        reason: &str,
        token: &str,
    ) -> Result<Saved<Value>, ClientError> {
        self.action(
            incoming,
            &[
                ("action", "undelete"),
                ("title", title),
                ("reason", reason),
                ("token", token),
            ],
        )
        .await
    }

    /// The newest `delete` log event of a title (`list=logevents&letype=delete&letitle=`),
    /// if the viewer may see one: what a deleted page shows in place of its content.
    pub async fn deletion_of(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Option<Deletion>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&list=logevents&letype=delete&letitle={}&lelimit=1&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["logevents"]
            .as_array()
            .and_then(|a| a.first())
            .filter(|e| e["action"] == "delete")
            .map(|e| Deletion {
                by: e["user"].as_str().map(str::to_string),
                time: e["timestamp"].as_str().unwrap_or_default().to_string(),
                reason: e["comment"]
                    .as_str()
                    .unwrap_or_default()
                    .trim_start_matches("/* delete/delete */")
                    .trim()
                    .to_string(),
                logid: e["logid"].as_u64().unwrap_or(0),
                talk: e["params"]["talk"].as_bool().unwrap_or(false),
            });
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// The feed target a title's log events are under (`list=logevents&letitle=`): a page
    /// by its ID, deleted or not, or an entity by its ID; `None` where the viewer may see
    /// no event of it.
    pub async fn log_target(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Option<String>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&list=logevents&letitle={}&lelimit=1&leprop=ids%7Ctitle&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["logevents"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|e| {
                let ns = e["ns"].as_i64().unwrap_or(0);
                if matches!(ns, 120 | 122 | 146) {
                    e["title"]
                        .as_str()
                        .map(|t| format!("entity:{}", t.rsplit(':').next().unwrap_or(t)))
                } else {
                    e["pageid"].as_u64().map(|id| format!("page:{id}"))
                }
            });
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `prop=deletedrevisions` of a title: its newest deleted revision, for a member of the
    /// deletion group; `None` for anyone else.
    pub async fn deleted_revision(
        &self,
        incoming: &Incoming,
        title: &str,
    ) -> Result<Fetched<Option<DeletedRevision>>, ClientError> {
        let r = self
            .get(
                incoming,
                &format!(
                    "/w/api.php?action=query&prop=deletedrevisions&titles={}&drvprop=ids%7Cuser%7Ctimestamp%7Ccomment%7Ccontent&drvslots=main&drvlimit=1&format=json&formatversion=2",
                    q(title)
                ),
            )
            .await?;
        // A viewer without `deletedhistory` is refused: nothing to show them.
        let v = match check(&r) {
            Ok(v) => v,
            Err(ClientError::Api { code, .. }) if code == "permissiondenied" => {
                return Ok(Fetched {
                    value: None,
                    cache: CacheInfo::of(&r),
                });
            }
            Err(e) => return Err(e),
        };
        let value = v["query"]["pages"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|p| p["deletedrevisions"].as_array())
            .and_then(|a| a.first())
            .map(|d| DeletedRevision {
                revid: d["revid"].as_u64().unwrap_or(0),
                user: d["user"].as_str().map(str::to_string),
                time: d["timestamp"].as_str().unwrap_or_default().to_string(),
                comment: d["comment"].as_str().unwrap_or_default().to_string(),
                content: d["slots"]["main"]["content"].as_str().map(str::to_string),
            });
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }

    /// `list=blocks`: the blocks in force.
    pub async fn blocks(&self, incoming: &Incoming) -> Result<Fetched<Vec<BlockRow>>, ClientError> {
        let r = self
            .get(
                incoming,
                "/w/api.php?action=query&list=blocks&bklimit=500&format=json&formatversion=2",
            )
            .await?;
        let v = check(&r)?;
        let value = v["query"]["blocks"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|b| BlockRow {
                        id: b["id"].as_u64(),
                        user: b["user"].as_str().map(str::to_string),
                        hidden: b["ts-hidden"].as_bool().unwrap_or(false)
                            || b["userhidden"].as_bool().unwrap_or(false),
                        by: b["by"].as_str().map(str::to_string),
                        timestamp: b["timestamp"].as_str().unwrap_or_default().to_string(),
                        expiry: b["expiry"].as_str().unwrap_or("infinite").to_string(),
                        reason: b["reason"].as_str().unwrap_or_default().to_string(),
                        removes: match &b["ts-removes"] {
                            Value::Array(p) => p
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join(", "),
                            other => other.as_str().unwrap_or_default().to_string(),
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Fetched {
            value,
            cache: CacheInfo::of(&r),
        })
    }
}
