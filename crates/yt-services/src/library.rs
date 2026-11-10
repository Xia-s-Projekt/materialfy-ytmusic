//! Library (playlists, liked songs, albums, artists, podcasts) via ytmusicapi.
//! Sync fire-and-forget API matches sf-services so QML bridges compile.
//!
//! Everything the screen shows comes from a live ytmusicapi call: the remembered/catalog
//! rows only fill a row instantly, the metadata always arrives from the API (never cached).

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use md3_model::{
    AlbumItem, ArtistItem, LibraryEntry, ListChange, PlayContext, PlaylistId, PlaylistItem, ShowItem,
    TrackItem, contexts,
};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use yt_api::YtError;
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
    EntriesChanged,
    LikesChanged,
    SavedChanged,
    ErrorChanged,
    PinsChanged,
    Notice(String),
    Feedback { kind: FeedbackKind, track: TrackItem },
}

/// Ключ кэша плейлиста: ytmusicapi добавляет к id префикс «VL» сам, наружу отдаём голый.
fn playlist_key(id: &str) -> String {
    id.trim().strip_prefix("VL").unwrap_or(id.trim()).to_owned()
}

/// URI → (kind, id): `ytmusic:album:MPREb_…` → ("album", "MPREb_…").
fn uri_parts(uri: &str) -> Option<(&str, &str)> {
    let mut parts = uri.trim().split(':');
    if parts.next()? != "ytmusic" { return None; }
    let kind = parts.next()?;
    let id = parts.next()?;
    if kind.is_empty() || id.is_empty() { return None; }
    Some((kind, id))
}

pub struct LibraryService {
    rt: Handle,
    auth: Arc<AuthService>,
    _player: PlayerHandle,
    history: Arc<HistoryService>,
    _settings: Arc<mf_platform::SettingsStore>,
    playlists: Mutex<Vec<PlaylistItem>>,
    /// прочитанные плейлисты; ключ — id без «VL»
    playlists_info: Mutex<HashMap<String, PlaylistInfo>>,
    /// какие плейлисты читаются прямо сейчас: повторный запрос тот же результат не изменит
    reading: Mutex<HashSet<String>>,
    liked: Mutex<Vec<TrackItem>>,
    liked_set: Mutex<HashSet<String>>,
    albums: Mutex<Vec<AlbumItem>>,
    artists: Mutex<Vec<ArtistItem>>,
    shows: Mutex<Vec<ShowItem>>,
    /// uri альбомов/шоу/исполнителей/плейлистов, добавленных в библиотеку
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
            history,
            _settings: settings,
            playlists: Mutex::new(vec![]),
            playlists_info: Mutex::new(HashMap::new()),
            reading: Mutex::new(HashSet::new()),
            liked: Mutex::new(vec![]),
            liked_set: Mutex::new(HashSet::new()),
            albums: Mutex::new(vec![]),
            artists: Mutex::new(vec![]),
            shows: Mutex::new(vec![]),
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
        if let Ok(v) = api.get_library_playlists(100).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
            let items: Vec<PlaylistItem> = list.iter().map(yt_api::map::playlist_item).collect();
            *lock(&self.playlists) = items.clone();
            let _ = self.events.send(LibraryEvent::Playlists(ListChange::Reset(items)));
            self.clear_error();
        }
        if let Ok(v) = api.get_liked_songs(200).await {
            let list: Vec<serde_json::Value> = v
                .get("tracks")
                .and_then(|t| t.as_array())
                .cloned()
                .unwrap_or_else(|| v.as_array().cloned().unwrap_or_default());
            let items: Vec<TrackItem> = list.iter().map(yt_api::map::track_item).collect();
            let ids: HashSet<String> = items.iter().map(|t| t.track_id.clone()).collect();
            *lock(&self.liked) = items.clone();
            *lock(&self.liked_set) = ids;
            let _ = self.events.send(LibraryEvent::Liked(ListChange::Reset(items)));
        }
        if let Ok(v) = api.get_library_albums(100).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
            let items: Vec<AlbumItem> = list.iter().map(yt_api::map::album_item).collect();
            for item in &items {
                self.note_saved(&item.uri, item.saved);
            }
            *lock(&self.albums) = items.clone();
            let _ = self.events.send(LibraryEvent::Albums(ListChange::Reset(items)));
        }
        if let Ok(v) = api.get_library_artists(100).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
            let items: Vec<ArtistItem> = list.iter().map(yt_api::map::artist_item).collect();
            for item in &items {
                self.note_saved(&item.uri, true);
            }
            *lock(&self.artists) = items.clone();
            let _ = self.events.send(LibraryEvent::Artists(ListChange::Reset(items)));
        }
        if let Ok(v) = api.get_library_podcasts(100).await {
            let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
            let items: Vec<ShowItem> = list
                .iter()
                // «New Episodes» — автоплейлист, а не подкаст: страницы у него нет
                .filter(|v| v.get("podcastId").and_then(Value::as_str) != Some("RDPN"))
                .map(yt_api::map::show_item)
                .collect();
            for item in &items {
                self.note_saved(&item.uri, true);
            }
            *lock(&self.shows) = items.clone();
            let _ = self.events.send(LibraryEvent::Shows(ListChange::Reset(items)));
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

    // --- ошибки и заметки ---

    fn set_error(&self, text: String) {
        let current = lock(&self.error_text);
        if *current != text {
            *lock(&self.error_text) = text;
            drop(current);
            let _ = self.events.send(LibraryEvent::ErrorChanged);
        }
    }
    fn clear_error(&self) {
        self.set_error(String::new());
    }
    fn fail(&self, error: YtError) {
        self.set_error(error.user_text());
    }
    pub(crate) fn notice(&self, text: impl Into<String>) {
        let _ = self.events.send(LibraryEvent::Notice(text.into()));
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
    pub fn albums(&self) -> Vec<AlbumItem> { lock(&self.albums).clone() }
    pub fn artists(&self) -> Vec<ArtistItem> { lock(&self.artists).clone() }
    pub fn shows(&self) -> Vec<ShowItem> { lock(&self.shows).clone() }
    pub fn is_saved(&self, uri: &str) -> bool { lock(&self.saved).contains(uri) }
    pub fn saved_revision(&self) -> u64 { lock(&self.saved).len() as u64 }

    /// Страница каталога сообщила живой флаг: альбом в библиотеке, исполнитель подписан,
    /// подкаст сохранён. Без этого кнопка на странице врала бы до перезагрузки библиотеки.
    pub fn note_saved(&self, uri: &str, saved: bool) {
        if uri.is_empty() { return; }
        let mut set = lock(&self.saved);
        let changed = if saved { set.insert(uri.to_owned()) } else { set.remove(uri) };
        drop(set);
        if changed {
            let _ = self.events.send(LibraryEvent::SavedChanged);
        }
    }

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
        let key = playlist_key(id.as_str());
        let in_library = lock(&self.playlists).iter().any(|p| playlist_key(&p.playlist_id) == key);
        if let Some(mut info) = lock(&self.playlists_info).get(&key).cloned() {
            // «В библиотеке» = «в подписках»: свой плейлист подписки не требует
            info.followed = in_library && !info.item.can_edit;
            return Some(info);
        }
        let item = lock(&self.playlists).iter().find(|p| playlist_key(&p.playlist_id) == key).cloned()?;
        Some(PlaylistInfo { item, tracks: vec![], total_duration_ms: 0, followed: in_library, duration_ms: 0 })
    }
    pub fn can_edit(&self, id: &PlaylistId) -> bool {
        self.playlist_info(id).is_some_and(|i| i.item.can_edit)
    }
    pub fn playlist_tracks(&self, id: &PlaylistId) -> Option<Vec<TrackItem>> {
        self.playlist_info(id).map(|i| i.tracks)
    }

    // --- чтение плейлиста ---

    async fn fetch_playlist(&self, key: &str) -> Result<PlaylistInfo, YtError> {
        let api = self.auth.api();
        let v = api.get_playlist(key, 500).await?;
        let tracks: Vec<TrackItem> = v
            .get("tracks")
            .and_then(|t| t.as_array())
            .map(|a| a.iter().map(yt_api::map::track_item).collect())
            .unwrap_or_default();
        let total = tracks.iter().map(|t| t.duration_ms).sum();
        Ok(PlaylistInfo {
            item: yt_api::map::playlist_item(&v),
            tracks,
            total_duration_ms: total,
            followed: false,
            duration_ms: total,
        })
    }

    /// Перечитать плейлист и разослать строки + шапку. false — чтение уже идёт.
    async fn read_playlist(&self, id: &PlaylistId) -> bool {
        let key = playlist_key(id.as_str());
        if key.is_empty() { return false; }
        if !lock(&self.reading).insert(key.clone()) {
            return false;
        }
        let result = self.fetch_playlist(&key).await;
        lock(&self.reading).remove(&key);
        match result {
            Ok(info) => {
                lock(&self.playlists_info).insert(key.clone(), info.clone());
                let id = PlaylistId::new(key.as_str());
                let _ = self.events.send(LibraryEvent::PlaylistTracks {
                    id: id.clone(),
                    change: ListChange::Reset(info.tracks.clone()),
                });
                let _ = self.events.send(LibraryEvent::PlaylistInfo(id));
                self.clear_error();
                true
            }
            Err(e) => {
                self.fail(e);
                false
            }
        }
    }

    pub fn load_playlist(self: &Arc<Self>, id: PlaylistId) {
        let me = self.clone();
        self.rt.spawn(async move { me.read_playlist(&id).await; });
    }

    pub fn refresh_playlist(self: &Arc<Self>, id: PlaylistId) {
        self.load_playlist(id);
    }

    pub fn cancel_playlist(&self, _id: &PlaylistId) {}
    pub fn replay_playlist(self: &Arc<Self>, id: &PlaylistId) {
        self.load_playlist(id.clone());
    }

    /// После записи: перечитать затронутый плейлист и разделы (создание/удаление меняют список).
    fn after_write(self: &Arc<Self>, id: Option<PlaylistId>) {
        let me = self.clone();
        self.rt.spawn(async move {
            if let Some(id) = id {
                me.read_playlist(&id).await;
            }
            me.load_all().await;
        });
    }

    /// Строки плейлиста, если он уже прочитан (для перестановки и удаления).
    fn rows_of(&self, id: &PlaylistId) -> Vec<TrackItem> {
        self.playlist_info(id).map(|i| i.tracks).unwrap_or_default()
    }

    // --- экран «Библиотека» ---

    pub fn entries(&self, kind: LibraryKind, owner: &str, sort: &str) -> Vec<LibraryEntry> {
        let mut out = match kind {
            LibraryKind::Playlists => {
                let liked_total = lock(&self.liked).len() as i64;
                let mut rows: Vec<LibraryEntry> = lock(&self.playlists)
                    .iter()
                    .map(|p| LibraryEntry {
                        kind: "playlist".into(),
                        id: p.playlist_id.clone(),
                        uri: p.uri.clone(),
                        title: p.title.clone(),
                        subtitle: subtitle_playlist(p),
                        cover: p.cover.clone(),
                        owner: p.owner.clone(),
                        owner_id: p.owner_id.clone(),
                        can_edit: p.can_edit,
                        read_only: !p.can_edit,
                        pinned: self.is_pinned(&p.uri),
                        track_count: p.track_count,
                        ..Default::default()
                    })
                    .collect();
                if owner == "all" {
                    rows.insert(
                        0,
                        LibraryEntry {
                            kind: "liked".into(),
                            id: String::new(),
                            uri: String::new(),
                            title: "Liked Songs".to_owned(),
                            subtitle: format!("Playlist · {liked_total} tracks"),
                            cover: String::new(),
                            can_edit: false,
                            read_only: true,
                            track_count: liked_total,
                            ..Default::default()
                        },
                    );
                }
                rows
            }
            LibraryKind::Albums => lock(&self.albums)
                .iter()
                .map(|a| LibraryEntry {
                    kind: "album".into(),
                    id: a.album_id.clone(),
                    uri: a.uri.clone(),
                    title: a.title.clone(),
                    subtitle: subtitle_album(a),
                    cover: a.cover.clone(),
                    can_edit: false,
                    read_only: false,
                    ..Default::default()
                })
                .collect(),
            LibraryKind::Artists => lock(&self.artists)
                .iter()
                .map(|a| LibraryEntry {
                    kind: "artist".into(),
                    id: a.artist_id.clone(),
                    uri: a.uri.clone(),
                    title: a.name.clone(),
                    subtitle: I18n::artist(),
                    cover: a.cover.clone(),
                    can_edit: false,
                    read_only: false,
                    ..Default::default()
                })
                .collect(),
            LibraryKind::Shows => lock(&self.shows)
                .iter()
                .map(|s| LibraryEntry {
                    kind: "show".into(),
                    id: s.show_id.clone(),
                    uri: s.uri.clone(),
                    title: s.name.clone(),
                    subtitle: s.publisher.clone(),
                    cover: s.cover.clone(),
                    can_edit: false,
                    read_only: false,
                    ..Default::default()
                })
                .collect(),
        };
        if kind == LibraryKind::Playlists {
            out.retain(|e| match owner {
                "mine" => e.kind == "liked" || e.can_edit,
                "followed" => e.kind != "liked" && !e.can_edit,
                _ => true,
            });
        }
        sort_entries(&mut out, sort);
        out
    }

    /// Последние прослушанные контексты (локальная история): плейлист, альбом, исполнитель, подкаст.
    pub fn recent_entries(&self, limit: usize) -> Vec<LibraryEntry> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for record in self.history.recent(limit.saturating_mul(4).max(40)) {
            let uri = record.context.context_uri.clone().unwrap_or_default();
            if uri.is_empty() || !seen.insert(uri.clone()) {
                continue;
            }
            let Some((kind, id)) = uri_parts(&uri) else { continue };
            if !matches!(kind, "playlist" | "album" | "artist" | "show" | "track") {
                continue;
            }
            let entry_kind = if kind == "track" { "album" } else { kind };
            out.push(LibraryEntry {
                kind: entry_kind.to_owned(),
                id: id.to_owned(),
                uri: uri.clone(),
                title: if record.context.title.is_empty() { record.track.title.clone() } else { record.context.title.clone() },
                subtitle: if record.context.title.is_empty() { "Artist".to_owned() } else { record.context.title.clone() },
                cover: record.context.cover.clone(),
                can_edit: false,
                read_only: false,
                played_at: record.played_at.clone(),
                ..Default::default()
            });
            if out.len() >= limit {
                break;
            }
        }
        out
    }

    // --- мутации: плейлисты ---

    pub fn add_to_playlist(self: &Arc<Self>, track: TrackItem, id: PlaylistId) {
        if track.track_id.is_empty() {
            self.notice("This track has no YouTube Music id");
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.add_playlist_items(&playlist_key(id.as_str()), &[track.track_id.clone()], false, false).await {
                Ok(_) => {
                    me.notice(format!("Added to “{}”", title_of(&*me, &id)));
                    me.after_write(Some(id));
                }
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn add_tracks(self: &Arc<Self>, id: PlaylistId, uris: Vec<String>) {
        let ids = video_ids(&uris);
        if ids.is_empty() {
            self.notice("No YouTube Music tracks selected");
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.add_playlist_items(&playlist_key(id.as_str()), &ids, false, false).await {
                Ok(_) => me.after_write(Some(id)),
                Err(e) => me.fail(e),
            }
        });
    }

    /// Вернуть удалённое на свои места: ytmusicapi умеет только дописывать в конец,
    /// поэтому добавленные строки переводятся на позицию перестановкой.
    pub fn insert_tracks(self: &Arc<Self>, id: PlaylistId, uris: Vec<String>, position: usize) {
        let ids = video_ids(&uris);
        if ids.is_empty() { return; }
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            let key = playlist_key(id.as_str());
            let added = match api.add_playlist_items(&key, &ids, false, true).await {
                Ok(v) => added_set_video_ids(&v),
                Err(e) => return me.fail(e),
            };
            if added.len() != ids.len() {
                // ответ без setVideoId — позицию не выразить, строки остаются в конце
                return me.after_write(Some(id));
            }
            let Ok(_) = me.fetch_playlist(&key).await else { return me.after_write(Some(id)) };
            let rows = me.rows_of(&id);
            let count = rows.len();
            if position >= count {
                return me.after_write(Some(id));
            }
            let placed = place_rows(&me, &id, &rows, &added, position).await;
            me.after_write(if placed { None } else { Some(id) });
        });
    }

    /// rows: [(videoId, pos, setVideoId)] — удалению нужны все три.
    pub fn remove_tracks(self: &Arc<Self>, id: PlaylistId, rows: Vec<(String, usize, String)>) {
        let videos: Vec<serde_json::Value> = rows
            .iter()
            .filter(|(video_id, _, set)| !video_id.is_empty() && !set.is_empty())
            .map(|(video_id, _, set)| serde_json::json!({"videoId": video_id, "setVideoId": set}))
            .collect();
        if videos.is_empty() {
            self.notice("Only your own playlists can be edited on YouTube Music");
            return;
        }
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.remove_playlist_items(&playlist_key(id.as_str()), &videos).await {
                Ok(_) => me.after_write(Some(id)),
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn move_tracks(self: &Arc<Self>, id: PlaylistId, from: usize, to: usize) {
        let me = self.clone();
        self.rt.spawn(async move {
            let rows = me.rows_of(&id);
            if from >= rows.len() || to >= rows.len() || from == to { return; }
            let item = &rows[from];
            if item.set_video_id.is_empty() {
                me.notice("Only your own playlists can be reordered on YouTube Music");
                return;
            }
            let api = me.auth.api();
            let key = playlist_key(id.as_str());
            // moveItem ставит строку перед другой; «в самый конец» он не умеет — там
            // остаётся удалить и дописать в конец
            let before: Option<&str> = if from < to {
                if to + 1 < rows.len() { Some(rows[to + 1].set_video_id.as_str()) } else { None }
            } else {
                Some(rows[to].set_video_id.as_str())
            };
            match before {
                Some(before) => {
                    if let Err(e) = api.move_playlist_item(&key, &item.set_video_id, before).await {
                        me.fail(e);
                        return;
                    }
                }
                None => {
                    let video = serde_json::json!({"videoId": item.track_id, "setVideoId": item.set_video_id});
                    if let Err(e) = api.remove_playlist_items(&key, &[video]).await {
                        me.fail(e);
                        return;
                    }
                    if let Err(e) = api.add_playlist_items(&key, &[item.track_id.clone()], false, false).await {
                        me.fail(e);
                        return;
                    }
                }
            }
            me.read_playlist(&id).await;
        });
    }

    pub fn create_playlist(self: &Arc<Self>, title: String, track: Option<TrackItem>) {
        let me = self.clone();
        let video_ids = track
            .filter(|t| !t.track_id.is_empty())
            .map(|t| vec![t.track_id])
            .unwrap_or_default();
        self.rt.spawn(async move {
            let api = me.auth.api();
            let ids = if video_ids.is_empty() { None } else { Some(video_ids.as_slice()) };
            match api.create_playlist(&title, "", "PRIVATE", ids.unwrap_or(&[])).await {
                Ok(v) => {
                    let id = playlist_id_of(&v);
                    me.notice(format!("Created “{title}”"));
                    me.after_write(id.map(PlaylistId::new));
                }
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn create_playlist_from(self: &Arc<Self>, title: String, uris: Vec<String>) {
        let me = self.clone();
        let ids = video_ids(&uris);
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.create_playlist(&title, "", "PRIVATE", &ids).await {
                Ok(v) => {
                    me.notice(format!("Created “{title}”"));
                    me.after_write(playlist_id_of(&v).map(PlaylistId::new));
                }
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn rename_playlist(self: &Arc<Self>, id: PlaylistId, title: String) {
        self.edit(Some(id), title, None, None);
    }

    pub fn set_description(self: &Arc<Self>, id: PlaylistId, text: String) {
        self.edit(Some(id), String::new(), Some(text), None);
    }

    pub fn set_public(self: &Arc<Self>, id: PlaylistId, public: bool) {
        let privacy = if public { "PUBLIC" } else { "PRIVATE" };
        self.edit(Some(id), String::new(), None, Some(privacy));
    }

    fn edit(self: &Arc<Self>, id: Option<PlaylistId>, title: String, description: Option<String>, privacy: Option<&str>) {
        let Some(id) = id else { return };
        let me = self.clone();
        let privacy = privacy.map(str::to_owned);
        self.rt.spawn(async move {
            let api = me.auth.api();
            let title = if title.is_empty() { None } else { Some(title.as_str()) };
            match api.edit_playlist(&playlist_key(id.as_str()), title, description.as_deref(), privacy.as_deref()).await {
                Ok(_) => me.after_write(Some(id)),
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn follow_playlist(self: &Arc<Self>, id: PlaylistId) {
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.rate_playlist(&playlist_key(id.as_str()), "LIKE").await {
                Ok(_) => {
                    me.note_saved(&id.uri(), true);
                    me.after_write(Some(id));
                }
                Err(e) => me.fail(e),
            }
        });
    }

    pub fn unfollow_playlist(self: &Arc<Self>, id: PlaylistId) {
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.rate_playlist(&playlist_key(id.as_str()), "INDIFFERENT").await {
                Ok(_) => {
                    me.note_saved(&id.uri(), false);
                    me.after_write(Some(id));
                }
                Err(e) => me.fail(e),
            }
        });
    }

    // --- лайки ---

    pub fn toggle_like(self: &Arc<Self>, track: TrackItem) {
        if track.track_id.is_empty() { return; }
        let was = self.is_liked(&track.track_id);
        {
            let mut set = lock(&self.liked_set);
            if was { set.remove(&track.track_id); } else { set.insert(track.track_id.clone()); }
        }
        let _ = self.events.send(LibraryEvent::LikesChanged);
        let me = self.clone();
        let rating = if was { "INDIFFERENT" } else { "LIKE" };
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.rate_song(&track.track_id, rating).await {
                Ok(_) => {
                    if was {
                        lock(&me.liked).retain(|t| t.track_id != track.track_id);
                        let _ = me.events.send(LibraryEvent::Liked(ListChange::Reset(me.liked())));
                    } else {
                        lock(&me.liked).insert(0, track.clone());
                        let _ = me.events.send(LibraryEvent::Liked(ListChange::Reset(me.liked())));
                    }
                    me.clear_error();
                }
                Err(e) => {
                    // откат: API отказало, локальный набор вернули
                    let mut set = lock(&me.liked_set);
                    if was { set.insert(track.track_id.clone()); } else { set.remove(&track.track_id); }
                    drop(set);
                    let _ = me.events.send(LibraryEvent::LikesChanged);
                    me.fail(e);
                }
            }
        });
    }

    pub fn dislike(self: &Arc<Self>, track: TrackItem) {
        if track.track_id.is_empty() { return; }
        let was = self.is_liked(&track.track_id);
        if was {
            lock(&self.liked_set).remove(&track.track_id);
            let _ = self.events.send(LibraryEvent::LikesChanged);
        }
        let me = self.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            match api.rate_song(&track.track_id, "DISLIKE").await {
                Ok(_) => {
                    if was {
                        lock(&me.liked).retain(|t| t.track_id != track.track_id);
                        let _ = me.events.send(LibraryEvent::Liked(ListChange::Reset(me.liked())));
                    }
                    me.clear_error();
                }
                Err(e) => me.fail(e),
            }
        });
    }

    /// Альбом/подкаст/плейлист — в библиотеку (rate_playlist), исполнитель — подписка.
    pub fn toggle_saved(self: &Arc<Self>, uri: String) {
        let Some((kind, id)) = uri_parts(&uri) else { return };
        let kind = kind.to_owned();
        let id = id.to_owned();
        if id.is_empty() { return; }
        let was = self.is_saved(&uri);
        self.note_saved(&uri, !was);
        let me = self.clone();
        let uri = uri.clone();
        self.rt.spawn(async move {
            let api = me.auth.api();
            let result = match kind.as_str() {
                "artist" => {
                    if was {
                        api.unsubscribe_artist(&id).await
                    } else {
                        api.subscribe_artist(&id).await
                    }
                }
                // у выпусков подкастов своего API «в библиотеку» в ytmusicapi нет
                "album" | "show" | "playlist" => {
                    api.rate_playlist(&id, if was { "INDIFFERENT" } else { "LIKE" }).await
                }
                _ => return,
            };
            match result {
                Ok(_) => {
                    me.clear_error();
                    if kind == "artist" || kind == "show" {
                        me.load_all().await;
                    }
                }
                Err(e) => {
                    me.note_saved(&uri, was);
                    me.fail(e);
                }
            }
        });
    }
}

/// Подпись строки библиотеки: «Плейлист · N треков» (+ владелец, если он не вы).
fn subtitle_playlist(p: &PlaylistItem) -> String {
    let mut parts = vec!["Playlist".to_owned(), plural_tracks(p.track_count)];
    if !p.owner.is_empty() {
        parts.push(p.owner.clone());
    }
    parts.join(" · ")
}

fn subtitle_album(a: &AlbumItem) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !a.artists.is_empty() { parts.push(a.artists.clone()); }
    if a.year > 0 { parts.push(a.year.to_string()); }
    if parts.is_empty() { "Album".to_owned() } else { parts.join(" · ") }
}

fn plural_tracks(count: i64) -> String {
    if count <= 0 { "0 tracks".to_owned() } else { format!("{count} tracks") }
}

/// Локализованные подписи (mf_i18n), без зависимости bridges → services.
struct I18n;
impl I18n {
    fn artist() -> String { "Artist".to_owned() }
}

/// URI треков → videoId (`ytmusic:track:ID`).
fn video_ids(uris: &[String]) -> Vec<String> {
    uris.iter()
        .filter_map(|uri| uri_parts(uri).map(|(_, id)| id.to_owned()))
        .filter(|id| !id.is_empty())
        .collect()
}

fn title_of(service: &LibraryService, id: &PlaylistId) -> String {
    service.playlist_info(id).map(|i| i.item.title).unwrap_or_default()
}

/// setVideoId добавленных строк из ответа add_playlist_items.
fn added_set_video_ids(response: &serde_json::Value) -> Vec<String> {
    response
        .get("playlistEditResults")
        .and_then(|r| r.as_array())
        .map(|list| {
            list.iter()
                .filter_map(|r| r.get("setVideoId").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Поставить только что добавленные строки (в конце списка) на позицию `position`.
/// Возвращает false, если выразить перестановку не удалось.
async fn place_rows(
    me: &Arc<LibraryService>,
    id: &PlaylistId,
    rows: &[TrackItem],
    added: &[String],
    position: usize,
) -> bool {
    let api = me.auth.api();
    let key = playlist_key(id.as_str());
    let count = rows.len();
    // строки добавлены в конец в том же порядке, что и в added
    let start = count.saturating_sub(added.len());
    for (offset, set) in added.iter().enumerate() {
        let target = position + offset;
        if start + offset == target {
            continue;
        }
        let Some(before) = rows.get(target).map(|r| r.set_video_id.as_str()).filter(|s| !s.is_empty()) else {
            return false;
        };
        if api.move_playlist_item(&key, set, before).await.is_err() {
            return false;
        }
    }
    let _ = id;
    true
}

/// id нового плейлиста из ответа create_playlist (строка или словарь).
fn playlist_id_of(response: &serde_json::Value) -> Option<String> {
    if let Some(id) = response.as_str() {
        return (!id.is_empty()).then(|| id.to_owned());
    }
    response
        .get("playlistId")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|id| !id.is_empty())
}

use serde_json::Value;

/// Сортировка раздела библиотеки. Порядок YouTube Music («ytmusic») сохраняется как пришёл.
fn sort_entries(rows: &mut [LibraryEntry], sort: &str) {
    match sort {
        "alpha" => rows.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        "author" => rows.sort_by(|a, b| {
            a.owner.to_lowercase().cmp(&b.owner.to_lowercase()).then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }),
        "added" | "recent" => rows.sort_by(|a, b| {
            let key = |e: &LibraryEntry| if sort == "recent" { e.played_at.clone() } else { e.added_at.clone() };
            key(b).cmp(&key(a))
        }),
        _ => {}
    }
}

/// Контекст модели «Любимые треки» — тот же, что у моста (см. bridges::library).
pub fn liked_context() -> PlayContext {
    PlayContext::new(contexts::LIKED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_strips_vl_prefix() {
        assert_eq!(playlist_key("VLPLabc"), "PLabc");
        assert_eq!(playlist_key("PLabc"), "PLabc");
        assert_eq!(playlist_key(" PLabc "), "PLabc");
    }

    #[test]
    fn uri_parts_reads_ytmusic_uris() {
        assert_eq!(uri_parts("ytmusic:album:MPREb_1"), Some(("album", "MPREb_1")));
        assert_eq!(uri_parts("ytmusic:artist:UC123"), Some(("artist", "UC123")));
        assert_eq!(uri_parts("spotify:playlist:x"), None);
        assert_eq!(uri_parts("ytmusic:"), None);
    }

    #[test]
    fn video_ids_extracts_ids() {
        let uris = vec!["ytmusic:track:abc".to_owned(), "ytmusic:episode:xyz".to_owned(), "".to_owned()];
        assert_eq!(video_ids(&uris), vec!["abc".to_owned(), "xyz".to_owned()]);
    }

    #[test]
    fn added_ids_come_from_edit_results() {
        let v = serde_json::json!({"status": "STATUS_SUCCEEDED",
            "playlistEditResults": [{"setVideoId": "s1"}, {"setVideoId": "s2"}]});
        assert_eq!(added_set_video_ids(&v), vec!["s1".to_owned(), "s2".to_owned()]);
        assert!(added_set_video_ids(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn playlist_id_reads_string_and_dict() {
        assert_eq!(playlist_id_of(&serde_json::json!("PLnew")).as_deref(), Some("PLnew"));
        assert_eq!(playlist_id_of(&serde_json::json!({"playlistId": "PLnew"})).as_deref(), Some("PLnew"));
        assert_eq!(playlist_id_of(&serde_json::json!({})), None);
    }

    #[test]
    fn tracks_plural_is_plain() {
        assert_eq!(plural_tracks(1), "1 tracks");
        assert_eq!(plural_tracks(0), "0 tracks");
    }
}
