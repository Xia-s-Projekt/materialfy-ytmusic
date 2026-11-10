//! The player's saved session (queue.md §9): the resume track and position, the context, the manual queue,
//! the first "Next" rows, shuffle. The file is written atomically (a temporary file + rename).
use std::path::Path;

use md3_model::{PlayContext, TrackItem};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionData {
    pub version: u32,
    pub context_uri: Option<String>,
    pub context: PlayContext,
    /// the resume track (with data to show before the first Play)
    pub track: Option<TrackItem>,
    pub position_ms: u32,
    /// "Next in queue" — URIs in playback order
    pub manual: Vec<String>,
    /// the first "Next" rows (up to 30)
    pub rows: Vec<TrackItem>,
    pub shuffle: bool,
}

impl SessionData {
    pub const VERSION: u32 = 1;

    pub fn load(path: &Path) -> Option<Self> {
        let bytes = std::fs::read(path).ok()?;
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(data) if data.version == Self::VERSION => Some(data),
            Ok(_) => None,
            Err(error) => {
                tracing::warn!("player session not read: {error:#}");
                None
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_survives_a_round_trip() {
        let dir = std::env::temp_dir().join(format!("sf-player-session-{}", std::process::id()));
        let path = dir.join("session.json");
        let data = SessionData {
            version: SessionData::VERSION,
            context_uri: Some("spotify:playlist:x".into()),
            context: PlayContext::with_uri("playlist", "spotify:playlist:x").titled("Playlist"),
            track: Some(TrackItem { uri: "spotify:track:a".into(), title: "A".into(), ..Default::default() }),
            position_ms: 19_566,
            manual: vec!["spotify:track:b".into(), "spotify:track:b".into()],
            rows: vec![TrackItem { uri: "spotify:track:b".into(), ..Default::default() }],
            shuffle: true,
        };
        data.save(&path).unwrap();
        assert_eq!(SessionData::load(&path), Some(data));
        let _ = std::fs::remove_dir_all(dir);
    }
}
