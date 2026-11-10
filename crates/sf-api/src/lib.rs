//! Spotify Web API without a UI: client (429/Retry-After limits, quota), routing between the shared/personal application,
//! serde models, HTTP with proxy, links, playlist covers, mapping into md3-model.

use std::fmt;

pub mod client;
pub mod gateway;
pub mod http;
pub mod image_hosts;
pub mod limits;
pub mod link;
pub mod map;
pub mod models;
pub mod playlist_cover;
pub mod util;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ApiSource {
    Shared,
    Personal,
}

impl fmt::Display for ApiSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shared => formatter.write_str("shared"),
            Self::Personal => formatter.write_str("personal"),
        }
    }
}

pub use client::{ApiClient, ApiError, NetActivity, PlayRequest, TokenProvider, WebTokens};
pub use gateway::{AccountId, ApiGateway, Operation, PlaylistAccess, PlaylistId, SessionState};
pub use http::Http;
/// SHA-256 for callers without their own dependency (update checksums in sf-services).
pub use sha2;
pub use limits::{LimitKind, Priority, WebApiLimits};
