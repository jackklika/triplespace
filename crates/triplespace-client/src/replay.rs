//! Recorded API exchanges, for rendering pages without an instance (0097 §16; 19 §5.10).
//!
//! The site is a function of the API responses it reads (22 §1.2 rule 6), so a page can
//! be rendered again, anywhere, from the exchanges one rendering made. [`Record`] wraps a
//! transport and keeps every exchange; [`Replay`] answers from a set of them. The parity
//! harness of the skin migration renders its corpus this way, and the recorder captures
//! the corpus from a real instance.
//!
//! An exchange is matched by method, path and query, whether the request carried
//! credentials (a cookie or an `Authorization` header: the API answers a signed-in
//! viewer differently), and the body. The headers the client builds for the transport
//! are not part of the match, so a recording made over HTTP replays in process and the
//! other way round. A query parameter whose value depends on when the page is rendered
//! ([`VOLATILE`]: `since`, which recent changes sets from the clock) is left out of the
//! match, so a recording replays on any day; the list is the only masking there is, and
//! a new entry is a reviewed change.

use std::collections::BTreeMap;
use std::sync::Mutex;

use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::transport::{ApiRequest, ApiResponse, BoxFuture, ClientError, Transport};

/// One request and the answer it got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// The method, `GET` or `POST`.
    pub method: String,
    /// The path and query.
    pub path_and_query: String,
    /// Whether the request carried credentials.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub credentialed: bool,
    /// The request body, for a `POST`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub body: String,
    /// The status.
    pub status: u16,
    /// The response headers the site reads: caching, content type, cookies.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// The response body, as JSON where it is JSON.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub json: serde_json::Value,
    /// The response body where it is not JSON.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// Query parameters whose values depend on the clock, left out of the match.
pub const VOLATILE: &[&str] = &["since"];

/// A path and query with the volatile parameters removed.
fn stable(path_and_query: &str) -> String {
    let Some((path, query)) = path_and_query.split_once('?') else {
        return path_and_query.to_string();
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| {
            let name = kv.split_once('=').map_or(*kv, |(n, _)| n);
            !VOLATILE.contains(&name)
        })
        .collect();
    if kept.is_empty() {
        path.to_string()
    } else {
        format!("{path}?{}", kept.join("&"))
    }
}

/// Whether a request carries credentials.
fn credentialed(headers: &HeaderMap) -> bool {
    headers.contains_key(header::COOKIE) || headers.contains_key(header::AUTHORIZATION)
}

impl Exchange {
    /// What a request and its answer were.
    #[must_use]
    pub fn of(request: &ApiRequest, response: &ApiResponse) -> Self {
        let text = String::from_utf8_lossy(&response.body);
        let json = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        Self {
            method: request.method.to_string(),
            path_and_query: request.path_and_query.clone(),
            credentialed: credentialed(&request.headers),
            body: String::from_utf8_lossy(&request.body).into_owned(),
            status: response.status.as_u16(),
            headers: response
                .headers
                .iter()
                .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
                .collect(),
            text: if json.is_null() {
                text.into_owned()
            } else {
                String::new()
            },
            json,
        }
    }

    /// The key a request is matched by.
    fn key(&self) -> (String, String, bool, String) {
        (
            self.method.clone(),
            stable(&self.path_and_query),
            self.credentialed,
            self.body.clone(),
        )
    }

    /// The answer, as the transport returns it.
    fn response(&self) -> Result<ApiResponse, ClientError> {
        let mut headers = HeaderMap::new();
        for (k, v) in &self.headers {
            if let (Ok(k), Ok(v)) = (HeaderName::try_from(k.as_str()), HeaderValue::try_from(v)) {
                headers.append(k, v);
            }
        }
        let body = if self.json.is_null() {
            self.text.clone().into_bytes()
        } else {
            serde_json::to_vec(&self.json).map_err(|e| ClientError::Unexpected(e.to_string()))?
        };
        Ok(ApiResponse {
            status: StatusCode::from_u16(self.status)
                .map_err(|e| ClientError::Unexpected(e.to_string()))?,
            headers,
            body,
        })
    }
}

/// A transport that keeps every exchange it carries.
pub struct Record<T> {
    inner: T,
    exchanges: Mutex<Vec<Exchange>>,
}

impl<T: Transport> Record<T> {
    /// Records what goes through a transport.
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            exchanges: Mutex::new(Vec::new()),
        }
    }

    /// The exchanges so far, in order.
    pub fn exchanges(&self) -> Vec<Exchange> {
        self.exchanges.lock().map(|e| e.clone()).unwrap_or_default()
    }
}

impl<T: Transport> Transport for Record<T> {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let response = self.inner.send(request.clone()).await?;
            if let Ok(mut e) = self.exchanges.lock() {
                e.push(Exchange::of(&request, &response));
            }
            Ok(response)
        })
    }

    fn embedded(&self) -> bool {
        self.inner.embedded()
    }

    fn forwarder_key(&self) -> Option<&str> {
        self.inner.forwarder_key()
    }
}

/// A transport that answers from recorded exchanges, and refuses what was not recorded.
#[derive(Debug, Clone, Default)]
pub struct Replay {
    by_key: BTreeMap<(String, String, bool, String), Exchange>,
    embedded: bool,
}

impl Replay {
    /// Answers from a set of exchanges. A request recorded more than once is answered
    /// with its last recording.
    #[must_use]
    pub fn new(exchanges: impl IntoIterator<Item = Exchange>) -> Self {
        Self {
            by_key: exchanges.into_iter().map(|e| (e.key(), e)).collect(),
            embedded: false,
        }
    }

    /// Replays as an in-process transport would answer, so that the browser's request
    /// is passed as it arrived (0057 §1.4).
    #[must_use]
    pub fn in_process(mut self) -> Self {
        self.embedded = true;
        self
    }

    /// The exchanges, in key order.
    pub fn exchanges(&self) -> impl Iterator<Item = &Exchange> {
        self.by_key.values()
    }
}

impl Transport for Replay {
    fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
        Box::pin(async move {
            let key = (
                request.method.to_string(),
                stable(&request.path_and_query),
                credentialed(&request.headers),
                String::from_utf8_lossy(&request.body).into_owned(),
            );
            match self.by_key.get(&key) {
                Some(e) => e.response(),
                None => Err(ClientError::Unreachable(format!(
                    "not recorded: {} {}{}",
                    request.method,
                    request.path_and_query,
                    if key.2 { " (credentialed)" } else { "" }
                ))),
            }
        })
    }

    fn embedded(&self) -> bool {
        self.embedded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Method;

    struct Echo;

    impl Transport for Echo {
        fn send(&self, request: ApiRequest) -> BoxFuture<'_, Result<ApiResponse, ClientError>> {
            Box::pin(async move {
                let mut headers = HeaderMap::new();
                headers.insert(header::ETAG, HeaderValue::from_static("\"e\""));
                Ok(ApiResponse {
                    status: StatusCode::OK,
                    headers,
                    body: serde_json::to_vec(&serde_json::json!({
                        "path": request.path_and_query,
                        "signed_in": credentialed(&request.headers),
                    }))
                    .unwrap(),
                })
            })
        }

        fn embedded(&self) -> bool {
            true
        }
    }

    fn request(path: &str, cookie: bool) -> ApiRequest {
        let mut headers = HeaderMap::new();
        if cookie {
            headers.insert(header::COOKIE, HeaderValue::from_static("s=1"));
        }
        ApiRequest {
            method: Method::GET,
            path_and_query: path.into(),
            headers,
            body: vec![],
            peer: None,
        }
    }

    #[tokio::test]
    async fn recorded_exchanges_replay_and_the_rest_is_refused() {
        let record = Record::new(Echo);
        let a = record.send(request("/w/api.php?x=1", false)).await.unwrap();
        let b = record.send(request("/w/api.php?x=1", true)).await.unwrap();
        assert_ne!(a.body, b.body);
        let exchanges = record.exchanges();
        assert_eq!(exchanges.len(), 2);
        assert!(!exchanges[0].credentialed && exchanges[1].credentialed);
        assert_eq!(exchanges[0].headers["etag"], "\"e\"");
        // Round trip through JSON, as the corpus files are.
        let text = serde_json::to_string(&exchanges).unwrap();
        let back: Vec<Exchange> = serde_json::from_str(&text).unwrap();
        let replay = Replay::new(back).in_process();
        assert!(Transport::embedded(&replay));
        let r = replay.send(request("/w/api.php?x=1", true)).await.unwrap();
        assert_eq!(r.body, b.body);
        assert_eq!(r.headers[header::ETAG], "\"e\"");
        let r = replay.send(request("/w/api.php?x=1", false)).await.unwrap();
        assert_eq!(r.body, a.body);
        assert!(matches!(
            replay.send(request("/w/api.php?x=2", false)).await,
            Err(ClientError::Unreachable(m)) if m.contains("not recorded")
        ));
    }

    #[test]
    fn volatile_parameters_are_not_matched() {
        assert_eq!(
            stable("/w/rest.php/x?kind=edit&since=2026-09-30T21:24:24Z&limit=50"),
            "/w/rest.php/x?kind=edit&limit=50"
        );
        assert_eq!(stable("/w/rest.php/x?since=1"), "/w/rest.php/x");
        assert_eq!(stable("/w/rest.php/x"), "/w/rest.php/x");
    }

    #[test]
    fn a_body_that_is_not_json_is_kept_as_text() {
        let e = Exchange::of(
            &request("/x", false),
            &ApiResponse {
                status: StatusCode::NOT_FOUND,
                headers: HeaderMap::new(),
                body: b"<html>".to_vec(),
            },
        );
        assert_eq!(e.text, "<html>");
        assert!(e.json.is_null());
        assert_eq!(e.response().unwrap().body, b"<html>");
    }
}
