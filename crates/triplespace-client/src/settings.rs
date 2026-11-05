//! Settings and preferences through the REST API (0085 §3, §5): the registered settings,
//! their resolved values, a save, and the viewer's preferences, for `Special:WikiSettings`,
//! `Special:InstanceSettings`, `Special:Preferences` and the Appearance menu.
//!
//! A save the API refuses comes back as [`Saved::Refused`] with every reason, which the
//! page shows beside the form it re-renders; only a failure to reach or read the API is a
//! [`ClientError`].

use std::collections::BTreeMap;

use axum::http::{HeaderValue, Method, StatusCode, header};
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::api::{CacheInfo, Fetched};
use crate::transport::{ApiRequest, ApiResponse};
use crate::{Client, ClientError, Incoming, forward};

const BASE: &str = "/w/rest.php/triplespace/v0";

/// A tab of a settings or preferences page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Group {
    /// The group's ID, the section's anchor.
    pub id: String,
    /// The label's message key.
    pub label: String,
}

/// A registered setting, as `GET /settings/schema` describes it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct SettingDef {
    /// The key.
    pub key: String,
    /// `tenant` or `instance`.
    #[serde(default)]
    pub scope: String,
    /// `tenant`, `ceiling` or `instance`.
    #[serde(default)]
    pub bound: String,
    /// The group.
    pub group: String,
    /// The type.
    #[serde(rename = "type")]
    pub ty: String,
    /// For a `choice`: the values.
    #[serde(default)]
    pub choices: Vec<String>,
    /// The registry default.
    #[serde(default)]
    pub default: Option<Value>,
    /// What applies without a default.
    #[serde(default)]
    pub default_text: Option<String>,
    /// The right that writes it, where stronger than `ts-config`.
    #[serde(default)]
    pub right: Option<String>,
    /// The label's message key.
    pub label: String,
    /// The help's message key.
    pub help: String,
    /// The catalogue's description, in English: the help shown while a translation of
    /// the help message is missing.
    #[serde(default)]
    pub meaning: Option<String>,
}

/// The allowed values of a tenancy switch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Allowed {
    /// The values, where the switch has a closed list.
    #[serde(default)]
    pub choices: Vec<String>,
    /// What the value is, where it is free text.
    #[serde(default)]
    pub text: Option<String>,
}

/// A tenancy switch.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Switch {
    /// What it allows.
    pub allowed: Allowed,
    /// Its value; `null` when the instance has not set it.
    #[serde(default)]
    pub value: Option<Value>,
}

/// A tenant setting as the instance sees it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct TenantSetting {
    /// The registry default.
    #[serde(default)]
    pub registry_default: Option<Value>,
    /// The instance's default.
    #[serde(default)]
    pub instance_default: Option<Value>,
    /// Whether it is locked.
    #[serde(default)]
    pub locked: bool,
    /// How many tenants inherit the instance's default.
    #[serde(default)]
    pub inheriting: Option<u64>,
}

/// `GET /settings/schema`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Schema {
    /// `tenant` or `instance`.
    pub scope: String,
    /// The tabs.
    pub groups: Vec<Group>,
    /// The settings.
    pub settings: Vec<SettingDef>,
    /// The tenancy switches.
    #[serde(default)]
    pub tenancy: BTreeMap<String, Switch>,
    /// The `config.template` mode.
    #[serde(default)]
    pub template: String,
    /// The locked keys, `site:{key}`.
    #[serde(default)]
    pub locked: Vec<String>,
    /// The ceilings, by setting.
    #[serde(default)]
    pub ceilings: BTreeMap<String, Value>,
    /// At instance scope: every tenant setting.
    #[serde(default)]
    pub tenant_settings: BTreeMap<String, TenantSetting>,
}

/// A resolved setting, as `GET /settings` reports it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Resolved {
    /// The value.
    #[serde(default)]
    pub value: Option<Value>,
    /// `registry`, `instance`, `tenant` or `locked`.
    pub origin: String,
    /// The ceiling that capped it.
    #[serde(default)]
    pub capped: Option<Value>,
    /// The instance's default.
    #[serde(default)]
    pub instance_default: Option<Value>,
    /// The instance's default names nothing this wiki has.
    #[serde(default)]
    pub unresolved_default: bool,
    /// Locked by the instance.
    #[serde(default)]
    pub locked: bool,
    /// Whether the viewer may change it.
    #[serde(default)]
    pub can_change: bool,
}

/// `GET /settings`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Settings {
    /// `tenant` or `instance`.
    pub scope: String,
    /// The `config` partition's next offset, the base of a save.
    pub base: u64,
    /// By key.
    pub settings: BTreeMap<String, Resolved>,
}

/// One reason a save was refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Refusal {
    /// The code: `ts-invalid-setting`, `editconflict`, `badtoken`, …
    pub code: String,
    /// The key it concerns.
    #[serde(default)]
    pub key: Option<String>,
    /// Why, in English.
    #[serde(default)]
    pub message: String,
}

/// How a save went.
#[derive(Debug, Clone, PartialEq)]
pub enum Saved<T> {
    /// Saved; the API's answer.
    Ok(T),
    /// Refused, with every reason, and nothing written.
    Refused(Vec<Refusal>),
}

/// A preference, as `GET /account/preferences` reports it.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Pref {
    /// The resolved value.
    #[serde(default)]
    pub value: Option<Value>,
    /// `account`, `request`, `tenant`, `instance` or `registry`.
    pub origin: String,
    /// Whether the account set it.
    #[serde(default)]
    pub own: bool,
    /// The type.
    #[serde(rename = "type")]
    pub ty: String,
    /// The group.
    pub group: String,
    /// `account`, `farm` or `choice`.
    #[serde(default)]
    pub follows: String,
    /// The label's message key.
    pub label: String,
    /// For a `choice`: the values.
    #[serde(default)]
    pub choices: Vec<String>,
    /// The registry default.
    #[serde(default)]
    pub default: Option<Value>,
    /// Whether a reader who is not logged in may keep it in the browser.
    #[serde(default)]
    pub browser: bool,
}

/// `GET /account/preferences`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Preferences {
    /// The tabs.
    pub groups: Vec<Group>,
    /// By key.
    pub preferences: BTreeMap<String, Pref>,
}

fn scoped(path: &str, instance: bool) -> String {
    if instance {
        format!("{BASE}{path}?scope=instance")
    } else {
        format!("{BASE}{path}")
    }
}

/// The API's refusal of a REST request: its `code` and `message`.
fn refused(status: StatusCode, body: &[u8]) -> ClientError {
    let v: Map<String, Value> = serde_json::from_slice(body).unwrap_or_default();
    ClientError::Api {
        code: v
            .get("code")
            .and_then(Value::as_str)
            .map_or_else(|| format!("http-{}", status.as_u16()), str::to_string),
        info: v
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

fn read<T: serde::de::DeserializeOwned>(r: &ApiResponse, what: &str) -> Result<T, ClientError> {
    if r.status != StatusCode::OK {
        return Err(refused(r.status, &r.body));
    }
    serde_json::from_slice(&r.body).map_err(|e| ClientError::Unexpected(format!("{what}: {e}")))
}

/// A save's answer: the value, or the refusals; a status the API never answers a save
/// with is an error. `404` is a refusal too: what the save named is gone or was never
/// there (a set retired, a request already decided, a member the set does not hold), and
/// the form says so.
pub(crate) fn saved<T: serde::de::DeserializeOwned>(
    r: &ApiResponse,
    what: &str,
) -> Result<Saved<T>, ClientError> {
    match r.status {
        StatusCode::OK => Ok(Saved::Ok(read(r, what)?)),
        StatusCode::BAD_REQUEST
        | StatusCode::FORBIDDEN
        | StatusCode::NOT_FOUND
        | StatusCode::CONFLICT
        | StatusCode::UNAUTHORIZED => {
            let v: Value = serde_json::from_slice(&r.body).unwrap_or(Value::Null);
            let mut errors: Vec<Refusal> = v
                .get("errors")
                .and_then(|e| serde_json::from_value(e.clone()).ok())
                .unwrap_or_default();
            if errors.is_empty() {
                errors.push(Refusal {
                    code: v["code"]
                        .as_str()
                        .map_or_else(|| format!("http-{}", r.status.as_u16()), str::to_string),
                    key: v["key"].as_str().map(str::to_owned),
                    message: v["message"].as_str().unwrap_or_default().to_string(),
                });
            }
            Ok(Saved::Refused(errors))
        }
        s => Err(refused(s, &r.body)),
    }
}

impl Client {
    /// Sends a request with a JSON body for the browser's request `incoming`.
    pub async fn send_json(
        &self,
        incoming: &Incoming,
        method: Method,
        path_and_query: &str,
        body: &Value,
    ) -> Result<ApiResponse, ClientError> {
        let mut headers = forward::headers(incoming, self.transport());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        self.transport()
            .send(ApiRequest {
                method,
                path_and_query: path_and_query.to_string(),
                headers,
                body: body.to_string().into_bytes(),
                peer: incoming.peer,
            })
            .await
    }

    /// `GET /settings/schema`, a tenant's or with `instance` the instance's.
    pub async fn settings_schema(
        &self,
        incoming: &Incoming,
        instance: bool,
    ) -> Result<Fetched<Schema>, ClientError> {
        let r = self
            .get(incoming, &scoped("/settings/schema", instance))
            .await?;
        Ok(Fetched {
            value: read(&r, "settings schema")?,
            cache: CacheInfo::of(&r),
        })
    }

    /// `GET /settings`.
    pub async fn settings(
        &self,
        incoming: &Incoming,
        instance: bool,
    ) -> Result<Fetched<Settings>, ClientError> {
        let r = self.get(incoming, &scoped("/settings", instance)).await?;
        Ok(Fetched {
            value: read(&r, "settings")?,
            cache: CacheInfo::of(&r),
        })
    }

    /// `PATCH /settings` with a body as the route takes it (`set`, `unset`, `summary`,
    /// `base`, `token`, and at instance scope `defaults`, `locks`, `tenancy`).
    pub async fn save_settings(
        &self,
        incoming: &Incoming,
        instance: bool,
        body: &Value,
    ) -> Result<Saved<Value>, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::PATCH,
                &scoped("/settings", instance),
                body,
            )
            .await?;
        saved(&r, "settings saved")
    }

    /// `GET /account/preferences`.
    pub async fn preferences(
        &self,
        incoming: &Incoming,
    ) -> Result<Fetched<Preferences>, ClientError> {
        let r = self
            .get(incoming, &format!("{BASE}/account/preferences"))
            .await?;
        Ok(Fetched {
            value: read(&r, "preferences")?,
            cache: CacheInfo::of(&r),
        })
    }

    /// `PATCH /account/preferences` with `set`, `reset`, `reset_all` and `token`.
    pub async fn save_preferences(
        &self,
        incoming: &Incoming,
        body: &Value,
    ) -> Result<Saved<Preferences>, ClientError> {
        let r = self
            .send_json(
                incoming,
                Method::PATCH,
                &format!("{BASE}/account/preferences"),
                body,
            )
            .await?;
        saved(&r, "preferences saved")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    fn response(status: StatusCode, body: &Value) -> ApiResponse {
        ApiResponse {
            status,
            headers: HeaderMap::new(),
            body: body.to_string().into_bytes(),
        }
    }

    #[test]
    fn refusals_are_read_whole() {
        let r = response(
            StatusCode::BAD_REQUEST,
            &serde_json::json!({"code": "ts-locked", "message": "locked", "errors": [
                {"code": "ts-locked", "key": "site.language", "message": "locked"},
                {"code": "ts-unknown-setting", "key": "x", "message": "unknown"}
            ]}),
        );
        match saved::<Value>(&r, "t").unwrap() {
            Saved::Refused(e) => {
                assert_eq!(e.len(), 2);
                assert_eq!(e[0].key.as_deref(), Some("site.language"));
            }
            Saved::Ok(_) => panic!("refused"),
        }
        let r = response(
            StatusCode::FORBIDDEN,
            &serde_json::json!({"code": "badtoken", "message": "no"}),
        );
        assert!(
            matches!(saved::<Value>(&r, "t").unwrap(), Saved::Refused(e) if e[0].code == "badtoken")
        );
        // What the save named is gone: a refusal the form shows, not an unreadable answer.
        let r = response(
            StatusCode::NOT_FOUND,
            &serde_json::json!({"code": "ts-no-target", "message": "The set does not hold it."}),
        );
        assert!(
            matches!(saved::<Value>(&r, "t").unwrap(), Saved::Refused(e) if e[0].code == "ts-no-target")
        );
        let r = response(StatusCode::INTERNAL_SERVER_ERROR, &serde_json::json!({}));
        assert!(saved::<Value>(&r, "t").is_err());
    }
}
