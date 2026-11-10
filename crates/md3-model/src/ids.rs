//! Spotify string ids (base62). Serialized as a string.
use serde::{Deserialize, Serialize};

/// the id of a Spotify account (`User.id`), not numeric.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// the id of a playlist (base62, without the `spotify:playlist:` prefix).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlaylistId(pub String);

impl PlaylistId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `spotify:playlist:<id>`
    pub fn uri(&self) -> String {
        format!("spotify:playlist:{}", self.0)
    }

    /// From an id, a `spotify:playlist:<id>` URI or a `https://open.spotify.com/playlist/<id>?…` link.
    pub fn from_uri_or_id(text: &str) -> Self {
        let text = text.trim();
        if let Some((_, rest)) = text.split_once("/playlist/") {
            let id = rest.split(['?', '#', '/']).next().unwrap_or(rest);
            return Self(id.to_owned());
        }
        Self(text.rsplit(':').next().unwrap_or(text).to_owned())
    }
}

impl std::borrow::Borrow<str> for PlaylistId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PlaylistId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
