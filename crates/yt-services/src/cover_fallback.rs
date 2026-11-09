//! Cover fallback stub (ytmusicapi thumbnails are direct).

use std::sync::Arc;

use yt_api::Http;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CoverKind {
    Album,
    Track,
    Artist,
    Playlist,
}

impl CoverKind {
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "album" => Some(Self::Album),
            "track" => Some(Self::Track),
            "artist" => Some(Self::Artist),
            "playlist" => Some(Self::Playlist),
            _ => None,
        }
    }
}

pub struct CoverFallback;

impl CoverFallback {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
    pub async fn find(&self, _http: &Http, _kind: CoverKind, _artist: &str, _title: &str) -> Option<String> {
        None
    }
}
