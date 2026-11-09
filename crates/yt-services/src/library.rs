//! Library (playlists, liked songs, albums, artists) via ytmusicapi.
//! Sync fire-and-forget API matches sf-services so QML bridges compile.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use md3_model::{AlbumItem, ArtistItem, EpisodeItem, LibraryEntry, ListChange, PlaylistId, PlaylistItem, ShowItem, TrackItem};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use yt_player::PlayerHandle;

use crate::auth::AuthService;
use crate::history::HistoryService;
use crate::lock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackKind {
    Like,
    Unlike,
    Dislike,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryKind {
    #[default]
    Playlists,
    Albums,
    Artists,
    Shows,
}
impl LibraryKind {
    pub fn from_name(name: &str) -> Self {
        match name {
            "albums" => Self::Albums,
            "artists" => Self::Artists,
            "shows" | "podcasts" => Self::Shows,
            _ => Self::Playlists,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PlaylistInfo {
    pub item: PlaylistItem,
    pub tracks: Vec<TrackItem>,
    pub total_duration_ms: i64,
    pub followed: bool,
    pub duration_ms: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LibraryEvent {
    Playlists(ListChange<PlaylistItem>),
    Liked(ListChange<TrackItem>),
    PlaylistTracks { id: PlaylistId, change: ListChange<TrackItem> },
    PlaylistInfo(PlaylistId),
    Albums(ListChange<AlbumItem>),
    Artists(ListChange<ArtistItem>),
    Shows(ListChange<ShowItem>),
    Episodes(ListChange<EpisodeItem>),
    EntriesChanged,
    LikesChanged,
    SavedChanged,
    ErrorChanged,
    PinsChanged,
    Notice(String),
    Feedback { kind: FeedbackKind, track: TrackItem },
}

pub struct LibraryService {
    rt: Handle,
    auth: Arc<AuthService>,
    _player: PlayerHandle,
    _history: Arc<HistoryService>,
    _settings: Arc<mf_platform::SettingsStore>,
    playlists: Mutex<Vec<PlaylistItem>>,
    playlists_info: Mutex<std::collections::HashMap<String, PlaylistInfo>>,
    liked: Mutex<Vec<TrackItem>>,
    liked_set: Mutex<HashSet<String>>,
    saved: Mutex<HashSet<String>>,
    pinned: Mutex<HashSet<String>>,
    error_text: Mutex<String>,
    events: broadcast::Sender<LibraryEvent>,
}

impl LibraryService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        history: Arc<HistoryService>,
        settings: Arc<mf_platform::SettingsStore>,
    ) -> Arc<Self> {
        let (events, _) = broadcast::channel(64);
        let service = Arc::new(Self {
            rt: rt.clone(),
            auth,
            _player: player,
            _history: history,
            _settings: settings,
            playlists: Mutex::new(vec![]),
            playlists_info: Mutex::new(std::collections::HashMap::new()),
            liked: Mutex::new(vec![]),
            liked_set: Mutex::new(HashSet::new()),
            saved: Mutex::new(HashSet::new()),
            pinned: Mutex::new(HashSet::new()),
            error_text: Mutex::new(String::new()),
            events,
        });
        let s2 = service.clone();
        rt.spawn(async move { s2.load_all().await; });
        service
    }

    async fn load_all(self: Arc<Self>) {
        let api = self.auth.api();
        if let Ok(v) = api.get_library_playlists(50).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
            let items: Vec<PlaylistItem> = list.iter().map(yt_api::map::playlist_item).collect();
            *lock(&self.playlists) = items.clone();
            let _ = self.events.send(LibraryEvent::Playlists(ListChange::Reset(items)));
        }
        if let Ok(v) = api.get_liked_songs(100).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned()
                .unwrap_or_else(|| v.get("tracks").and_then(|t| t.as_array()).cloned().unwrap_or_default());
            let items: Vec<TrackItem> = list.iter().map(yt_api::map::track_item).collect();
            let ids: HashSet<String> = items.iter().map(|t| t.track_id.clone()).collect();
            *lock(&self.liked) = items.clone();
            *lock(&self.liked_set) = ids;
            let _ = self.events.send(LibraryEvent::Liked(ListChange::Reset(items)));
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LibraryEvent> {
        self.events.subscribe()
    }

    pub fn refresh(self: &Arc<Self>) {
        let me = self.clone();
        let rt = self.rt.clone();
        rt.spawn(async move { me.load_all().await; });
    }

    // --- state readers used by bridges ---
    pub fn liked(&self) -> Vec<TrackItem> { lock(&self.liked).clone() }
    pub fn liked_loading(&self) -> bool { false }
    pub fn is_liked(&self, track_id_or_uri: &str) -> bool {
        let id = track_id_or_uri.rsplit(':').next().unwrap_or(track_id_or_uri);
        lock(&self.liked_set).contains(id) || lock(&self.liked_set).contains(track_id_or_uri)
    }
    pub fn likes_revision(&self) -> u64 { lock(&self.liked).len() as u64 }
    pub fn playlists(&self) -> Vec<PlaylistItem> { lock(&self.playlists).clone() }
    pub fn playlists_loading(&self) -> bool { false }
    pub fn albums(&self) -> Vec<AlbumItem> { vec![] }
    pub fn artists(&self) -> Vec<ArtistItem> { vec![] }
    pub fn shows(&self) -> Vec<ShowItem> { vec![] }
    pub fn episodes(&self) -> Vec<EpisodeItem> { vec![] }
    pub fn is_saved(&self, uri: &str) -> bool { lock(&self.saved).contains(uri) }
    pub fn saved_revision(&self) -> u64 { lock(&self.saved).len() as u64 }
    pub fn is_pinned(&self, uri: &str) -> bool { lock(&self.pinned).contains(uri) }
    pub fn toggle_pin(&self, uri: String) {
        let mut pins = lock(&self.pinned);
        if pins.contains(&uri) { pins.remove(&uri); } else { pins.insert(uri); }
        drop(pins);
        let _ = self.events.send(LibraryEvent::PinsChanged);
    }
    pub fn pins(&self) -> Vec<String> { lock(&self.pinned).iter().cloned().collect() }
    pub fn pins_revision(&self) -> u64 { lock(&self.pinned).len() as u64 }
    pub fn error_text(&self) -> String { lock(&self.error_text).clone() }
    pub fn loading_pages(&self) -> (u32, u32) { (0, 0) }

    pub fn playlist_info(&self, id: &PlaylistId) -> Option<PlaylistInfo> {
        let info = lock(&self.playlists_info).get(id.as_str()).cloned();
        if info.is_some() { return info; }
        let Some(item) = lock(&self.playlists).iter().find(|p| p.playlist_id == *id.as_str()).cloned() else { return None };
        Some(PlaylistInfo { item, tracks: vec![], total_duration_ms: 0, followed: false, duration_ms: 0 })
    }
    pub fn can_edit(&self, id: &PlaylistId) -> bool {
        let _ = id;
        false
    }
    pub fn playlist_tracks(&self, id: &PlaylistId) -> Option<Vec<TrackItem>> {
        self.playlist_info(id).map(|i| i.tracks)
    }
    pub fn entries(&self, kind: LibraryKind, _owner: &str, _sort: &str) -> Vec<LibraryEntry> {
        match kind {
            LibraryKind::Playlists => self.playlists().into_iter().map(|p| LibraryEntry {
                kind: "playlist".into(),
                id: p.playlist_id.clone(),
                uri: p.uri.clone(),
                title: p.title.clone(),
                subtitle: format!("Playlist · {} tracks", p.track_count),
                cover: p.cover.clone(),
                ..Default::default()
            }).collect(),
            LibraryKind::Albums => vec![],
            LibraryKind::Artists => vec![],
            LibraryKind::Shows => vec![],
        }
    }
    pub fn recent_entries(&self, _limit: usize) -> Vec<LibraryEntry> { vec![] }
    pub fn folder_children(&self, _folder_id: &str) -> Vec<LibraryEntry> { vec![] }

    // --- mutations (async fire-and-forget; ytmusicapi-backed where available) ---
    pub fn load_playlist(&self, _id: PlaylistId) {}
    pub fn refresh_playlist(&self, _id: PlaylistId) {}
    pub fn cancel_playlist(&self, _id: &PlaylistId) {}
    pub fn replay_playlist(&self, _id: &PlaylistId) {}
    pub fn add_to_playlist(&self, _track: TrackItem, _id: PlaylistId) {
        let _ = self.events.send(LibraryEvent::Notice("Adding to playlists needs ytmusicapi playlist editing (TODO)".into()));
    }
    pub fn add_tracks(&self, _id: PlaylistId, _uris: Vec<String>) {}
    pub fn insert_tracks(&self, _id: PlaylistId, _uris: Vec<String>, _position: usize) {}
    pub fn remove_tracks(&self, _id: PlaylistId, _rows: Vec<(String, usize)>) {}
    pub fn move_tracks(&self, _id: PlaylistId, _from: usize, _to: usize) {}
    pub fn create_playlist(&self, _title: String, _track: Option<TrackItem>) {}
    pub fn create_playlist_from(&self, _title: String, _uris: Vec<String>) {}
    pub fn rename_playlist(&self, _id: PlaylistId, _title: String) {}
    pub fn set_description(&self, _id: PlaylistId, _text: String) {}
    pub fn set_public(&self, _id: PlaylistId, _public: bool) {}
    pub fn upload_cover(&self, _id: PlaylistId, _path: std::path::PathBuf) {}
    pub fn follow_playlist(&self, _id: PlaylistId) {}
    pub fn unfollow_playlist(&self, _id: PlaylistId) {}
    pub fn toggle_like(&self, track: TrackItem) {
        let id = track.track_id.clone();
        let mut set = lock(&self.liked_set);
        if set.contains(&id) { set.remove(&id); } else { set.insert(id); }
        drop(set);
        let _ = self.events.send(LibraryEvent::LikesChanged);
    }
    pub fn dislike(&self, _track: TrackItem) {}
    pub fn toggle_saved(&self, uri: String) {
        let mut saved = lock(&self.saved);
        if saved.contains(&uri) { saved.remove(&uri); } else { saved.insert(uri); }
        drop(saved);
        let _ = self.events.send(LibraryEvent::SavedChanged);
    }
}
