//! New releases from YouTube Music's own "New releases" browse: the
//! personalized Release-Mix playlist (the radar equivalent) plus the fresh
//! albums/singles grid.

use std::sync::Arc;

use serde::Serialize;

use md3_model::PlaylistItem;
use tokio::runtime::Handle;
use tokio::sync::watch;

use crate::auth::AuthService;
use crate::history::HistoryService;
use crate::library::LibraryService;
use yt_player::PlayerHandle;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewRelease {
    pub id: String,
    pub album_id: String,
    pub uri: String,
    pub title: String,
    pub artist: String,
    pub artists: String,
    pub cover: String,
    pub explicit: bool,
    pub kind: String,
    pub date: String,
    pub date_label: String,
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleasesSnapshot {
    pub loading: bool,
    pub radar: Option<PlaylistItem>,
    pub radar_checked: bool,
    pub releases: Arc<Vec<NewRelease>>,
    pub error_text: String,
}

pub struct ReleasesService {
    rt: Handle,
    auth: Arc<AuthService>,
    _library: Arc<LibraryService>,
    _history: Arc<HistoryService>,
    _player: PlayerHandle,
    snapshot: watch::Sender<ReleasesSnapshot>,
}

impl ReleasesService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        player: PlayerHandle,
    ) -> Arc<Self> {
        let (snapshot, _) = watch::channel(ReleasesSnapshot { radar_checked: true, ..Default::default() });
        Arc::new(Self { rt, auth, _library: library, _history: history, _player: player, snapshot })
    }

    pub fn refresh(&self, force: bool) {
        let (api, snapshot) = (self.auth.api(), self.snapshot.clone());
        self.rt.spawn(async move {
            let _ = force;
            let _ = snapshot.send_replace(ReleasesSnapshot { loading: true, radar_checked: true, ..snapshot.borrow().clone() });
            match api.get_new_releases().await {
                Ok(value) => {
                    let (releases, radar) = map_new_releases(&value);
                    let _ = snapshot.send_replace(ReleasesSnapshot {
                        loading: false,
                        radar_checked: true,
                        radar,
                        releases: Arc::new(releases),
                        error_text: String::new(),
                        ..Default::default()
                    });
                }
                Err(e) => {
                    let _ = snapshot.send_replace(ReleasesSnapshot {
                        loading: false,
                        radar_checked: true,
                        error_text: e.user_text(),
                        ..Default::default()
                    });
                }
            }
        });
    }
    pub fn snapshot(&self) -> ReleasesSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<ReleasesSnapshot> { self.snapshot.subscribe() }
}

/// Bridge `get_new_releases` `{items: [{title, subtitle, browseId, playlistId,
/// cover}]}` → (release rows, radar playlist). Returns the release rows for the
/// grid plus the personalized "New Release Mix" playlist, which is YouTube
/// Music's equivalent of a release radar: it is refreshed with new music from
/// the artists the listener follows. The mix is a playlist, so it is excluded
/// from the album/single grid.
fn map_new_releases(value: &serde_json::Value) -> (Vec<NewRelease>, Option<PlaylistItem>) {
    let items = value.get("items").and_then(|i| i.as_array());
    let Some(items) = items else { return (Vec::new(), None) };
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut radar: Option<PlaylistItem> = None;
    for item in items {
        let text = |k: &str| item.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let browse_id = text("browseId");
        let playlist_id = text("playlistId");
        // A playlist browse id (VL…) is a playlist even without a watch id.
        let is_playlist = !playlist_id.is_empty() || browse_id.starts_with("VL");
        let id = if !browse_id.is_empty() { browse_id.clone() } else { playlist_id.clone() };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let title = text("title");
        if title.is_empty() {
            continue;
        }
        let subtitle = text("subtitle");
        if is_playlist {
            let cover = text("cover");
            // The personalized mix is the radar; any other playlist is kept as
            // a plain release row (it opens as a playlist).
            if radar.is_none() && title.contains("Mix") {
                radar = Some(PlaylistItem {
                    playlist_id: if !playlist_id.is_empty() { playlist_id.clone() } else { browse_id.clone() },
                    uri: format!("ytmusic:playlist:{}", if !playlist_id.is_empty() { playlist_id.clone() } else { browse_id.clone() }),
                    title: title.clone(),
                    description: subtitle.clone(),
                    cover: cover.clone(),
                    ..Default::default()
                });
                continue;
            }
            out.push(NewRelease {
                id,
                album_id: String::new(),
                uri: format!("ytmusic:playlist:{}", if !playlist_id.is_empty() { playlist_id } else { browse_id }),
                title,
                artist: subtitle.clone(),
                artists: subtitle,
                cover,
                explicit: false,
                kind: String::new(),
                date: String::new(),
                date_label: String::new(),
                group: String::new(),
            });
            continue;
        }
        // Badge words inside the subtitle mark the release kind.
        let (kind, group) = if subtitle.contains("Single") || subtitle.contains("EP") {
            ("Single".to_string(), "single")
        } else if subtitle.contains("Album") {
            ("Album".to_string(), "album")
        } else {
            (String::new(), "album")
        };
        let artists = subtitle.replace(" • ", " · ");
        let artists = artists.split(" · ").find(|s| !matches!(s.trim(), "Album" | "Single" | "EP")).unwrap_or(&artists).to_string();
        out.push(NewRelease {
            id,
            album_id: browse_id.clone(),
            uri: format!("ytmusic:album:{browse_id}"),
            title,
            artist: artists.clone(),
            artists,
            cover: text("cover"),
            explicit: false,
            kind,
            date: String::new(),
            date_label: String::new(),
            group: group.to_string(),
        });
    }
    (out, radar)
}
