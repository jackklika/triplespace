//! The site's client of the Triplespace API (0057 §1, §4).
//!
//! The site reaches the instance only through its public API, with the viewer's own
//! credentials, so every page carries exactly the redaction the API applied (0012 §1.4–1.5).
//! The same calls run over two [`transport`]s:
//!
//! - [`transport::HttpTransport`], from the stateless web tier (`triplespace-web`) to the
//!   API over the network: TLS unless the API's host resolves only to internal addresses
//!   (0057 §4); the viewer's headers forwarded, the connecting peer appended to
//!   `X-Forwarded-For` and the web tier's forwarder key to `Triplespace-Forwarder`, and
//!   no judgement of either, which is the API's (0057 §10).
//! - [`transport::ServiceTransport`], from `triplespace-server`'s embedded site to the
//!   API's own router in process: the browser's request passed as it arrived, with its
//!   connection address, so the API judges it exactly as if the browser had called it.
//!
//! - [`replay::Replay`], from recorded exchanges ([`replay::Record`] keeps them), so that
//!   a page renders again without an instance: the parity harness of the skin migration
//!   (0097 §16).
//!
//! [`api`], [`entity`], [`search`], [`account`], [`create`], [`version`], [`activity`], [`revisions`], [`pages`], [`settings`], [`rights`], [`notifications`], [`reports`] and [`watch`] have the typed calls the site makes, each returning the value with the
//! [`api::CacheInfo`] a page needs to compose its own caching (0057 §6).
//!
//! This crate reaches no store and depends on no API crate (0005 §3 rule 10).

#![forbid(unsafe_code)]

pub mod account;
pub mod activity;
pub mod api;
pub mod create;
pub mod entity;
pub mod forward;
pub mod issuers;
pub mod notifications;
pub mod pages;
pub mod register;
pub mod replay;
pub mod reports;
pub mod revisions;
pub mod rights;
pub mod search;
pub mod settings;
pub mod theme;
pub mod threads;
pub mod transport;
pub mod version;
pub mod watch;

pub use account::{LoginResult, WithCookies};
pub use activity::{ActivityPage, ActivityQuery, ActivityRow};
pub use api::{
    CacheInfo, ChipColours, Fetched, MessageQuery, MessageRow, NamespaceRow, ProviderInfo,
    ProviderType, SiteInfo, UserInfo,
};
pub use create::{Created, TermEdit};
pub use entity::{Entities, Provenance};
pub use forward::Incoming;
pub use issuers::{AccountInfo, LoginIssuer, Pending};
pub use notifications::{Counts, Item as Notification, Listing as NotificationPage};
pub use pages::{
    DiffAsk, DiffEnd, DiffSide, EditForm, Lookup, PageCategory, PageDiff, PageInfo, PageList,
    PageListQuery, PageRef, Parsed, Saved, Source, Visibility,
};
pub use register::{
    AccountRequest, ChallengeInfo, CreateFields, CreateForm, CreateResult, LoginForm,
};
pub use reports::{ReportPage, ReportQuery, ReportRow, ReportState};
pub use revisions::{Change, Comparison, EntityDiff, Revision};
pub use rights::{
    BlockForm, BlockRow, DeletedRevision, Deletion, EmptyGroup, GroupInfo, ProtectForm,
    ProtectedRow, Protection, Restriction, RightsForm, SetInfo, TenantVisibility, UserGroups,
};
pub use search::{Hit, Hits};
pub use theme::ThemeInfo;
pub use threads::{
    Attached, Listed, Listing, NewThreadOn, PostForm, PostView, ThreadView, Written,
};
pub use transport::{
    ApiRequest, ApiResponse, ClientError, HttpTransport, ServiceTransport, Transport,
};
pub use watch::{WatchEntry, WatchForm, WatchState};

use std::sync::Arc;

/// The client: a transport and the typed calls over it. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    transport: Arc<dyn Transport>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("embedded", &self.transport.embedded())
            .finish()
    }
}

impl Client {
    /// A client over a transport.
    pub fn new(transport: impl Transport + 'static) -> Self {
        Self {
            transport: Arc::new(transport),
        }
    }

    /// The transport.
    #[must_use]
    pub fn transport(&self) -> &dyn Transport {
        self.transport.as_ref()
    }

    /// Sends a `GET` for the browser's request `incoming`.
    pub async fn get(
        &self,
        incoming: &Incoming,
        path_and_query: &str,
    ) -> Result<ApiResponse, ClientError> {
        let headers = forward::headers(incoming, self.transport.as_ref());
        self.transport
            .send(ApiRequest {
                method: axum::http::Method::GET,
                path_and_query: path_and_query.to_string(),
                headers,
                body: Vec::new(),
                peer: incoming.peer,
            })
            .await
    }
}
