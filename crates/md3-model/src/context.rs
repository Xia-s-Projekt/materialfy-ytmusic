//! Playback context of a list: the type (for Player.source) + the Spotify context URI.
use serde::{Deserialize, Serialize};

/// {"type": liked|playlist|album|artist|search|radio|show|queue|…, "uri": "spotify:…"?, "title": "…", "cover": "…"}.
/// `uri` — what goes to librespot/Web API as context_uri (None — play as a list of URIs).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayContext {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "uri", skip_serializing_if = "Option::is_none")]
    pub context_uri: Option<String>,
    /// label for "Playing from …" (the name of the playlist/album/station); empty — unknown
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// context cover ~300 px (the "Playing from …" card); empty — none
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cover: String,
}

impl PlayContext {
    pub fn new(kind: &str) -> Self {
        Self { kind: kind.to_owned(), context_uri: None, title: String::new(), cover: String::new() }
    }

    pub fn with_uri(kind: &str, uri: impl Into<String>) -> Self {
        Self { kind: kind.to_owned(), context_uri: Some(uri.into()), title: String::new(), cover: String::new() }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_cover(mut self, cover: impl Into<String>) -> Self {
        self.cover = cover.into();
        self
    }
}

/// Values of `PlayContext.kind` (= Player.source in QML).
pub mod contexts {
    pub const LIKED: &str = "liked";
    pub const PLAYLIST: &str = "playlist";
    pub const ALBUM: &str = "album";
    pub const ARTIST: &str = "artist";
    pub const SEARCH: &str = "search";
    /// Spotify radio (a spotify:station:… station); formerly "wave"
    pub const RADIO: &str = "radio";
    pub const SHOW: &str = "show";
    pub const EPISODE: &str = "episode";
    /// separate tracks (the queue, history, "any track")
    pub const TRACKS: &str = "tracks";
    /// the context is unknown (the remote device did not report it)
    pub const UNKNOWN: &str = "";

    /// `spotify:user:<id>:collection` — "Liked songs" as a context.
    pub fn liked_uri(account_id: &str) -> String {
        format!("spotify:user:{account_id}:collection")
    }
}
